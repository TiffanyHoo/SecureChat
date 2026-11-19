using System.Buffers.Binary;
using System.Security.Cryptography;
using SecureChat.Core.Protocol;

namespace SecureChat.Core.Crypto;

/// <summary>
/// 一个密钥纪元(epoch)的双向会话密钥。
/// A = 发起方, B = 监听方;每方向独立 AES-256-GCM 密钥与 8 字节 nonce 基值。
/// </summary>
public sealed record SessionKeys
{
    public required uint Epoch { get; init; }
    public required byte[] KeyA2B { get; init; }       // 32B
    public required byte[] NonceBaseA2B { get; init; } // 8B
    public required byte[] KeyB2A { get; init; }       // 32B
    public required byte[] NonceBaseB2A { get; init; } // 8B
    public required bool IAmA { get; init; }
    public required byte[] MyTiebreak { get; init; }    // 8B,同时重入 Rekey 时的确定性仲裁
    public required byte[] PeerTiebreak { get; init; }
    public required byte[] Transcript { get; init; }    // 当前纪元链的哈希,用于轮换派生
    public required byte[] ConfirmKey { get; init; }    // 握手确认 MAC 密钥

    public byte[] MyKey => IAmA ? KeyA2B : KeyB2A;
    public byte[] MyNonceBase => IAmA ? NonceBaseA2B : NonceBaseB2A;
    public byte[] PeerKey => IAmA ? KeyB2A : KeyA2B;
    public byte[] PeerNonceBase => IAmA ? NonceBaseB2A : NonceBaseA2B;

    /// <summary>密钥指纹(SHA256 前 8 字节 hex),用于 UI 展示比对。</summary>
    public string Fingerprint
    {
        get
        {
            var material = new byte[KeyA2B.Length + KeyB2A.Length];
            KeyA2B.CopyTo(material, 0);
            KeyB2A.CopyTo(material, KeyA2B.Length);
            return Convert.ToHexString(SHA256.HashData(material).AsSpan(0, 8)).ToLowerInvariant();
        }
    }

    /// <summary>任意一方是否为"仲裁低位"(双方同时发起 Rekey 时由它主导)。</summary>
    public bool IAmLowTiebreak =>
        MyTiebreak.AsSpan().SequenceCompareTo(PeerTiebreak.AsSpan()) < 0;
}

/// <summary>AES-256-GCM 封装/解封。密文自含 4B 计数器,nonce = 8B 基值 + 4B 计数器。</summary>
public static class AesGcmBox
{
    private const int TagSize = 16;
    private const int CounterSize = 4;

    public static byte[] Seal(byte[] key, byte[] nonceBase8, ulong counter,
                              FrameType type, uint epoch, ReadOnlySpan<byte> plaintext)
    {
        Span<byte> nonce = stackalloc byte[12];
        nonceBase8.AsSpan(0, 8).CopyTo(nonce);
        BinaryPrimitives.WriteUInt32BigEndian(nonce[8..], (uint)counter);

        byte[] aad = BuildAad(type, epoch, (uint)counter, plaintext.Length);
        var sealedBuf = new byte[CounterSize + plaintext.Length + TagSize];
        BinaryPrimitives.WriteUInt32BigEndian(sealedBuf, (uint)counter);

        using var gcm = new AesGcm(key, TagSize);
        var ciphertext = sealedBuf.AsSpan(CounterSize, plaintext.Length);
        var tag = sealedBuf.AsSpan(CounterSize + plaintext.Length, TagSize);
        gcm.Encrypt(nonce, plaintext, ciphertext, tag, aad);
        return sealedBuf;
    }

    /// <summary>解封;密文/AAD 被篡改或密钥不符时抛 CryptographicException。</summary>
    public static byte[] Open(byte[] key, byte[] nonceBase8,
                              FrameType type, uint epoch, byte[] payload)
    {
        if (payload.Length < CounterSize + TagSize)
            throw new CryptographicException("密文载荷过短");

        uint counter = BinaryPrimitives.ReadUInt32BigEndian(payload);
        Span<byte> nonce = stackalloc byte[12];
        nonceBase8.AsSpan(0, 8).CopyTo(nonce);
        BinaryPrimitives.WriteUInt32BigEndian(nonce[8..], counter);

        int plainLen = payload.Length - CounterSize - TagSize;
        byte[] aad = BuildAad(type, epoch, counter, plainLen);

        var plaintext = new byte[plainLen];
        using var gcm = new AesGcm(key, TagSize);
        var ciphertext = payload.AsSpan(CounterSize, plainLen);
        var tag = payload.AsSpan(CounterSize + plainLen, TagSize);
        gcm.Decrypt(nonce, ciphertext, tag, plaintext, aad);
        return plaintext;
    }

    /// <summary>AAD 绑定帧头:篡改类型/纪元/计数器/长度均导致认证失败。</summary>
    private static byte[] BuildAad(FrameType type, uint epoch, uint counter, int plainLen)
    {
        var aad = new byte[13];
        aad[0] = (byte)type;
        BinaryPrimitives.WriteUInt32BigEndian(aad.AsSpan(1), epoch);
        BinaryPrimitives.WriteUInt32BigEndian(aad.AsSpan(5), counter);
        BinaryPrimitives.WriteInt32BigEndian(aad.AsSpan(9), plainLen);
        return aad;
    }
}
