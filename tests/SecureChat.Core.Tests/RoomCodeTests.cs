using SecureChat.Core.Protocol;
using Xunit;

namespace SecureChat.Core.Tests;

public class RoomCodeTests
{
    [Fact]
    public void 编解码roundtrip_字段一致()
    {
        byte[] psk = RoomCode.GeneratePsk();
        string code = RoomCode.Create("192.168.1.100", 9001, 15, psk);

        Assert.StartsWith("SC1-", code);
        Assert.True(RoomCode.TryParse(code, out RoomInfo? room));
        Assert.NotNull(room);
        Assert.Equal("192.168.1.100", room!.Host);
        Assert.Equal(9001, room.Port);
        Assert.Equal(15, room.RotationMinutes);
        Assert.Equal(RoomCode.PskToText(psk), room.PskText);
        Assert.Equal(43, room.PskText.Length); // 32B → Base64Url 43 字符,强口令
    }

    [Fact]
    public void PSK为256位随机_两次生成不相等()
    {
        byte[] a = RoomCode.GeneratePsk();
        byte[] b = RoomCode.GeneratePsk();
        Assert.Equal(32, a.Length);
        Assert.Equal(32, b.Length);
        Assert.False(a.SequenceEqual(b));
    }

    [Theory]
    [InlineData("")]                                   // 空串
    [InlineData("随便一段文本")]                        // 无前缀
    [InlineData("SC1-!!!!")]                          // 非法 Base64
    [InlineData("SC1-YQ")]                             // 载荷过短
    [InlineData("SC1-YQ==")]                           // 1 字节载荷
    public void 非法输入_拒绝解析(string code)
    {
        Assert.False(RoomCode.TryParse(code, out _));
    }

    [Fact]
    public void 篡改载荷_拒绝解析()
    {
        byte[] psk = RoomCode.GeneratePsk();
        string code = RoomCode.Create("10.0.0.5", 7000, 30, psk);

        // 截断房间码
        Assert.False(RoomCode.TryParse(code[..(code.Length - 4)], out _));
        // 追加垃圾
        Assert.False(RoomCode.TryParse(code + "abcd", out _));
        // 原始码应仍可解析
        Assert.True(RoomCode.TryParse(code, out _));
    }

    [Fact]
    public void 前后空白与大小写前缀_可容忍解析()
    {
        byte[] psk = RoomCode.GeneratePsk();
        string code = RoomCode.Create("172.20.1.8", 65535, 720, psk);

        Assert.True(RoomCode.TryParse("  " + code + "\r\n", out RoomInfo? room));
        Assert.Equal("172.20.1.8", room!.Host);
        Assert.Equal(65535, room.Port);
        Assert.Equal(720, room.RotationMinutes);

        Assert.True(RoomCode.TryParse("sc1-" + code[4..], out _)); // 小写前缀
    }

    [Fact]
    public void 本机IP枚举_至少返回一个候选()
    {
        var ips = RoomCode.GetCandidateHostIps();
        Assert.NotEmpty(ips);
        Assert.All(ips, ip => Assert.Matches(@"^\d+\.\d+\.\d+\.\d+$", ip));
        Assert.Equal(ips.Count, ips.Distinct().Count()); // 无重复
    }
}
