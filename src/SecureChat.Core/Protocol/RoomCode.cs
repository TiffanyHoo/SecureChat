using System.Buffers.Binary;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Security.Cryptography;

namespace SecureChat.Core.Protocol;

/// <summary>解析房间码得到的连接信息。PskText 为口令文本(Base64Url,43 字符),直接作为 SecureConnection 的 PSK 使用。</summary>
public sealed record RoomInfo(string Host, int Port, int RotationMinutes, string PskText);

/// <summary>
/// 房间码:把"创建方 IP + 监听端口 + 轮换周期 + 256 位随机口令"打包为一段可复制的文本,
/// 让对方粘贴即可加入,免手动传递 IP/端口/口令。
/// 格式: "SC1-" + Base64Url( ver(1B)=1 | ipLen(1B) | ip | port(2B BE) | rotation(2B BE) | psk(32B) )
/// 注意:口令编码在房间码内,房间码必须经由用户信任的渠道(当面/电话等)交给对方。
/// </summary>
public static class RoomCode
{
    public const string Prefix = "SC1-";
    private const int PskSize = 32;

    /// <summary>生成 256 位 CSPRNG 会话口令——暴力穷举不可行,彻底规避弱口令风险。</summary>
    public static byte[] GeneratePsk() => RandomNumberGenerator.GetBytes(PskSize);

    /// <summary>PSK 的文本形式(43 字符 Base64Url)。Core 握手接口以 string 传口令。</summary>
    public static string PskToText(byte[] psk) => Base64UrlEncode(psk);

    public static string Create(string host, int port, int rotationMinutes, byte[] psk)
    {
        byte[] ip = IPAddress.Parse(host).GetAddressBytes();
        if (psk.Length != PskSize) throw new ArgumentException($"PSK 必须为 {PskSize} 字节", nameof(psk));
        byte[] payload = new byte[2 + ip.Length + 2 + 2 + psk.Length];
        payload[0] = 1;                          // 版本
        payload[1] = (byte)ip.Length;
        ip.CopyTo(payload, 2);
        BinaryPrimitives.WriteUInt16BigEndian(payload.AsSpan(2 + ip.Length), (ushort)port);
        BinaryPrimitives.WriteUInt16BigEndian(payload.AsSpan(4 + ip.Length), (ushort)rotationMinutes);
        psk.CopyTo(payload, 6 + ip.Length);
        return Prefix + Base64UrlEncode(payload);
    }

    public static bool TryParse(string code, out RoomInfo? room)
    {
        room = null;
        code = code.Trim();
        if (!code.StartsWith(Prefix, StringComparison.OrdinalIgnoreCase)) return false;
        byte[]? payload = TryBase64UrlDecode(code[Prefix.Length..]);
        if (payload is null || payload.Length < 2 + 4 + 2 + 2 + PskSize) return false;
        if (payload[0] != 1) return false;
        int ipLen = payload[1];
        if (ipLen is not (4 or 16)) return false;
        if (payload.Length != 2 + ipLen + 2 + 2 + PskSize) return false;

        byte[] ipBytes = payload[2..(2 + ipLen)];
        int port = BinaryPrimitives.ReadUInt16BigEndian(payload.AsSpan(2 + ipLen));
        int rotation = BinaryPrimitives.ReadUInt16BigEndian(payload.AsSpan(4 + ipLen));
        byte[] psk = payload[(6 + ipLen)..];
        if (port is < 1 or > 65535 || rotation is < 1 or > 720) return false;

        room = new RoomInfo(new IPAddress(ipBytes).ToString(), port, rotation, PskToText(psk));
        return true;
    }

    /// <summary>枚举本机可作为监听地址的 IPv4(私网优先排序,排除回环/链路本地/未启用网卡)。多网卡(如 VPN)时由用户在界面选择。</summary>
    public static IReadOnlyList<string> GetCandidateHostIps()
    {
        var list = new List<string>();
        try
        {
            foreach (var ni in NetworkInterface.GetAllNetworkInterfaces())
            {
                if (ni.OperationalStatus != OperationalStatus.Up) continue;
                if (ni.NetworkInterfaceType == NetworkInterfaceType.Loopback) continue;
                foreach (var addr in ni.GetIPProperties().UnicastAddresses)
                {
                    if (addr.Address.AddressFamily != AddressFamily.InterNetwork) continue;
                    string ip = addr.Address.ToString();
                    if (ip.StartsWith("169.254.")) continue; // 链路本地(未获 DHCP)
                    if (!list.Contains(ip)) list.Add(ip);
                }
            }
        }
        catch { /* 网卡枚举失败时回退,至少给出回环地址 */ }
        list.Sort((a, b) => Rank(a).CompareTo(Rank(b))); // 私网网段排前面
        if (list.Count == 0) list.Add("127.0.0.1");
        return list;

        static int Rank(string ip) =>
            ip.StartsWith("192.168.") || ip.StartsWith("10.") || IsPrivate172(ip) ? 0 : 1;

        static bool IsPrivate172(string ip)
        {
            // 172.16.0.0 – 172.31.255.255
            if (!ip.StartsWith("172.")) return false;
            var second = ip.Split('.')[1];
            return int.TryParse(second, out int n) && n is >= 16 and <= 31;
        }
    }

    private static string Base64UrlEncode(byte[] data) =>
        Convert.ToBase64String(data).TrimEnd('=').Replace('+', '-').Replace('/', '_');

    private static byte[]? TryBase64UrlDecode(string text)
    {
        try
        {
            string s = text.Replace('-', '+').Replace('_', '/');
            int rem = s.Length % 4;
            if (rem == 1) return null;               // 1 余数不可能来自合法 Base64
            if (rem == 2) s += "==";
            else if (rem == 3) s += "=";
            return Convert.FromBase64String(s);
        }
        catch { return null; }
    }
}
