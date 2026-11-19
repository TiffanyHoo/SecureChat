using System.Text.Json;

namespace SecureChat.Core.Protocol;

/// <summary>载荷序列化(JSON,仅存在于加密通道内部,链路不可见)。</summary>
public static class Payloads
{
    private static readonly JsonSerializerOptions Options = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

    public sealed record ChatMessage(string Content);
    public sealed record ProfileMessage(string Name);
    /// <summary>OVER 挂断协商:请求 / 同意 / 拒绝 / 撤回。</summary>
    public sealed record OverMessage(string Kind);

    public static byte[] Serialize<T>(T value) =>
        JsonSerializer.SerializeToUtf8Bytes(value, Options);

    public static T? Deserialize<T>(byte[] bytes) where T : class
    {
        try { return JsonSerializer.Deserialize<T>(bytes, Options); }
        catch (JsonException) { return null; }
    }
}
