namespace SecureChat.Core.Protocol;

/// <summary>线路帧类型。</summary>
public enum FrameType : byte
{
    Hello = 1,     // 握手临时公钥
    Confirm = 2,   // 握手确认(HMAC over transcript)
    Data = 3,      // 加密聊天消息
    Profile = 4,   // 加密显示名
    Rekey = 5,     // 密钥轮换 Hello(仅由 tiebreak 较低一方=driver 发送)
    Ping = 6,      // 保活
    Bye = 7,       // 对端主动结束
    RekeyRequest = 8, // 请求对端( driver )发起轮换(仅由较高一方发送)
}

/// <summary>线路帧:头部之后的载荷。</summary>
public sealed record Frame(FrameType Type, uint Epoch, byte[] Payload);

/// <summary>
/// 帧读写。线路格式: [4B 总长][1B type][4B epoch][payload]。
/// 大端序;总长含 9 字节头。TCP 粘包/半包由 ReadExact 处理。
/// </summary>
public static class FrameIO
{
    public const int HeaderSize = 9;
    public const int MaxPayloadSize = 1 << 20; // 1 MiB

    /// <summary>读取一帧。流关闭返回 null;头部非法抛 IOException。</summary>
    public static async Task<Frame?> ReadAsync(Stream stream, CancellationToken ct = default)
    {
        var header = new byte[HeaderSize];
        if (!await ReadExactAsync(stream, header, ct).ConfigureAwait(false)) return null;

        int total = (header[0] << 24) | (header[1] << 16) | (header[2] << 8) | header[3];
        var type = (FrameType)header[4];
        uint epoch = ((uint)header[5] << 24) | ((uint)header[6] << 16) | ((uint)header[7] << 8) | header[8];

        int payloadLen = total - HeaderSize;
        if (payloadLen < 0 || payloadLen > MaxPayloadSize)
            throw new IOException($"非法帧长度: {total}");
        if (!Enum.IsDefined(type))
            throw new IOException($"非法帧类型: {header[4]}");

        var payload = new byte[payloadLen];
        if (payloadLen > 0 && !await ReadExactAsync(stream, payload, ct).ConfigureAwait(false))
            return null;

        return new Frame(type, epoch, payload);
    }

    public static async Task WriteAsync(Stream stream, Frame frame, CancellationToken ct = default)
    {
        int total = HeaderSize + frame.Payload.Length;
        if (frame.Payload.Length > MaxPayloadSize)
            throw new IOException("载荷超过上限");

        var buf = new byte[total];
        buf[0] = (byte)(total >> 24);
        buf[1] = (byte)(total >> 16);
        buf[2] = (byte)(total >> 8);
        buf[3] = (byte)total;
        buf[4] = (byte)frame.Type;
        buf[5] = (byte)(frame.Epoch >> 24);
        buf[6] = (byte)(frame.Epoch >> 16);
        buf[7] = (byte)(frame.Epoch >> 8);
        buf[8] = (byte)frame.Epoch;
        Buffer.BlockCopy(frame.Payload, 0, buf, HeaderSize, frame.Payload.Length);

        await stream.WriteAsync(buf, ct).ConfigureAwait(false);
    }

    private static async Task<bool> ReadExactAsync(Stream stream, byte[] buf, CancellationToken ct)
    {
        int off = 0;
        while (off < buf.Length)
        {
            int n = await stream.ReadAsync(buf.AsMemory(off), ct).ConfigureAwait(false);
            if (n <= 0) return false;
            off += n;
        }
        return true;
    }
}
