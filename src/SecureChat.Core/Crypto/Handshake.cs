using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using SecureChat.Core.Protocol;

namespace SecureChat.Core.Crypto;

/// <summary>某纪元的密钥材料(尚未绑定 tiebreak)。</summary>
public sealed record DerivedMaterial(
    byte[] KeyA2B, byte[] NonceBaseA2B,
    byte[] KeyB2A, byte[] NonceBaseB2A,
    byte[] ConfirmKey);

/// <summary>握手/轮换密钥派生。ECDH P-256 + PSK 认证 + HKDF-SHA256。</summary>
public static class Handshake
{
    public const byte ProtocolVersion = 1;
    public const int NonceSize = 16;
    public const int TiebreakSize = 8;
    public const int ConfirmMacSize = 32;

    /// <summary>
    /// 组装 Hello/Rekey 载荷: ver(1) || pubLen(2) || pubKey(SPKI) || nonce(16)。
    /// 握手与轮换复用同一格式,链路上只出现临时公钥与随机数。
    /// </summary>
    public static byte[] BuildHelloPayload(ECDiffieHellman ecdh)
    {
        byte[] pub = ecdh.ExportSubjectPublicKeyInfo();
        var payload = new byte[3 + pub.Length + NonceSize];
        payload[0] = ProtocolVersion;
        BinaryPrimitives.WriteUInt16BigEndian(payload.AsSpan(1), (ushort)pub.Length);
        pub.CopyTo(payload, 3);
        RandomNumberGenerator.Fill(payload.AsSpan(3 + pub.Length));
        return payload;
    }

    /// <summary>解析对端 Hello/Rekey 载荷,返回 (公钥SPKI, nonce)。</summary>
    public static (byte[] PubKey, byte[] Nonce) ParseHelloPayload(ReadOnlySpan<byte> payload)
    {
        if (payload.Length < 3 + NonceSize || payload[0] != ProtocolVersion)
            throw new IOException("协议版本或载荷格式不符");
        int pubLen = BinaryPrimitives.ReadUInt16BigEndian(payload[1..]);
        if (payload.Length != 3 + pubLen + NonceSize)
            throw new IOException("载荷长度不符");
        return (payload.Slice(3, pubLen).ToArray(), payload.Slice(3 + pubLen).ToArray());
    }

    /// <summary>ECDH 原始共享秘密。</summary>
    public static byte[] EcdhSharedSecret(ECDiffieHellman mine, byte[] peerSpkiPubKey)
    {
        using var peer = ECDiffieHellman.Create();
        peer.ImportSubjectPublicKeyInfo(peerSpkiPubKey, out _);
        return mine.DeriveRawSecretAgreement(peer.PublicKey);
    }

    /// <summary>transcript = SHA256(pubA ‖ nonceA ‖ pubB ‖ nonceB),按角色排序防反射。</summary>
    public static byte[] BuildTranscript(byte[] myPub, byte[] myNonce, byte[] peerPub, byte[] peerNonce, bool iAmA)
    {
        var (pubA, nonceA, pubB, nonceB) = iAmA
            ? (myPub, myNonce, peerPub, peerNonce)
            : (peerPub, peerNonce, myPub, myNonce);
        var buf = new byte[pubA.Length + nonceA.Length + pubB.Length + nonceB.Length];
        int off = 0;
        pubA.CopyTo(buf, off); off += pubA.Length;
        nonceA.CopyTo(buf, off); off += nonceA.Length;
        pubB.CopyTo(buf, off); off += pubB.Length;
        nonceB.CopyTo(buf, off);
        return SHA256.HashData(buf);
    }

    /// <summary>由临时 ECDH 共享秘密 + PSK 派生某纪元密钥材料。</summary>
    public static DerivedMaterial DeriveMaterial(byte[] sharedSecret, string psk,
                                                 byte[] transcript, uint epoch)
    {
        byte[] pskBytes = Encoding.UTF8.GetBytes(psk);
        var ikm = new byte[sharedSecret.Length + pskBytes.Length];
        sharedSecret.CopyTo(ikm, 0);
        pskBytes.CopyTo(ikm, sharedSecret.Length);

        byte[] okm = HKDF.DeriveKey(HashAlgorithmName.SHA256, ikm, 96, transcript, InfoFor(epoch));
        byte[] keyA2B = okm.AsSpan(0, 32).ToArray();
        byte[] keyB2A = okm.AsSpan(32, 32).ToArray();
        byte[] confirmKey = okm.AsSpan(64, 32).ToArray();

        // nonce 基值由确认密钥再派生
        var nonceSeedA = HMACSHA256.HashData(confirmKey, "nonce-base-a"u8);
        var nonceSeedB = HMACSHA256.HashData(confirmKey, "nonce-base-b"u8);

        return new DerivedMaterial(
            keyA2B,
            nonceSeedA.AsSpan(0, 8).ToArray(),
            keyB2A,
            nonceSeedB.AsSpan(0, 8).ToArray(),
            confirmKey);
    }

    /// <summary>由密钥材料构建会话密钥纪元。</summary>
    public static SessionKeys BuildSessionKeys(DerivedMaterial m, uint epoch, byte[] transcript,
                                               bool iAmA, byte[] myTiebreak, byte[] peerTiebreak) => new()
    {
        Epoch = epoch,
        KeyA2B = m.KeyA2B,
        NonceBaseA2B = m.NonceBaseA2B,
        KeyB2A = m.KeyB2A,
        NonceBaseB2A = m.NonceBaseB2A,
        IAmA = iAmA,
        MyTiebreak = myTiebreak,
        PeerTiebreak = peerTiebreak,
        Transcript = transcript,
        ConfirmKey = m.ConfirmKey,
    };

    /// <summary>纪元信息串:不同纪元派生不同密钥。</summary>
    public static byte[] InfoFor(uint epoch) =>
        Encoding.ASCII.GetBytes($"SecureChat/v1/epoch{epoch}");

    /// <summary>确认 MAC: HMAC(confirmKey, transcript ‖ role)。role 绑定方向防反射。</summary>
    public static byte[] BuildConfirmMac(byte[] confirmKey, byte[] transcript, bool iAmA) =>
        HMACSHA256.HashData(confirmKey, ConfirmTranscript(transcript, iAmA));

    public static void VerifyConfirmMac(byte[] confirmKey, byte[] transcript, bool iAmA, byte[] mac)
    {
        byte[] expected = BuildConfirmMac(confirmKey, transcript, iAmA);
        if (!CryptographicOperations.FixedTimeEquals(expected, mac))
            throw new CryptographicException("PSK 口令不匹配,或连接正被中间人劫持");
    }

    /// <summary>Rekey:旧 transcript 链拼接双方新临时材料,按 tiebreak 排序,派生下一纪元 transcript。</summary>
    public static byte[] BuildNextTranscript(SessionKeys current,
                                             byte[] myPub, byte[] myNonce,
                                             byte[] peerPub, byte[] peerNonce)
    {
        bool iAmLow = current.MyTiebreak.AsSpan().SequenceCompareTo(current.PeerTiebreak.AsSpan()) < 0;
        var (pubLow, nonceLow, pubHigh, nonceHigh) = iAmLow
            ? (myPub, myNonce, peerPub, peerNonce)
            : (peerPub, peerNonce, myPub, myNonce);

        var buf = new byte[current.Transcript.Length + pubLow.Length + nonceLow.Length
                                                  + pubHigh.Length + nonceHigh.Length];
        int off = 0;
        current.Transcript.CopyTo(buf, off); off += current.Transcript.Length;
        pubLow.CopyTo(buf, off); off += pubLow.Length;
        nonceLow.CopyTo(buf, off); off += nonceLow.Length;
        pubHigh.CopyTo(buf, off); off += pubHigh.Length;
        nonceHigh.CopyTo(buf, off);
        return SHA256.HashData(buf);
    }

    private static byte[] ConfirmTranscript(byte[] transcript, bool iAmA)
    {
        var buf = new byte[transcript.Length + 1];
        transcript.CopyTo(buf, 0);
        buf[^1] = iAmA ? (byte)1 : (byte)2;
        return buf;
    }
}
