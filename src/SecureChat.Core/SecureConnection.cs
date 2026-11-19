using System.Buffers.Binary;
using System.Net.Sockets;
using System.Security.Cryptography;
using SecureChat.Core.Crypto;
using SecureChat.Core.Protocol;
using SecureChat.Core.Transport;

namespace SecureChat.Core;

public enum ConnState { Idle, Listening, Connecting, Handshaking, Established, Closed }

public sealed class SecureChatOptions
{
    /// <summary>会话密钥自动轮换周期(分钟),最小 1。</summary>
    public int RotationMinutes { get; init; } = 10;
    /// <summary>保活 Ping 间隔(秒)。</summary>
    public int KeepAliveSeconds { get; init; } = 25;
    /// <summary>无收包判定掉线的超时(秒)。</summary>
    public int DeadSeconds { get; init; } = 90;
}

/// <summary>
/// 一条端到端加密连接:TCP 直连 → ECDH+PSK 握手 → AES-GCM 加密会话 → 周期 Rekey。
/// 事件可能在后台线程触发,UI 层需自行封送。
/// </summary>
public sealed class SecureConnection : IAsyncDisposable
{
    public event Action? Connected;
    public event Action<string>? PeerNameReceived;                    // 对端显示名
    public event Action<string, string, DateTimeOffset>? MessageReceived; // (发送者, 明文, 时间)
    public event Action<uint, string>? KeyRotated;                    // (纪元, 指纹)
    public event Action<string>? Disconnected;                        // 原因

    public ConnState State { get; private set; } = ConnState.Idle;
    public uint Epoch => _keys?.Epoch ?? 0;
    public string Fingerprint => _keys?.Fingerprint ?? string.Empty;
    public string? PeerName => _peerName;

    public int SecondsUntilRotation => State == ConnState.Established
        ? Math.Max(0, (int)(_nextRotationAt - DateTimeOffset.UtcNow).TotalSeconds)
        : 0;

    private readonly SecureChatOptions _opt;
    private readonly SemaphoreSlim _sendLock = new(1, 1);
    private readonly TaskCompletionSource<bool> _startedTcs =
        new(TaskCreationOptions.RunContinuationsAsynchronously);

    private CancellationTokenSource? _cts;
    private Stream? _stream;
    private string _psk = string.Empty;
    private string _myName = string.Empty;
    private string? _peerName;
    private SessionKeys? _keys;
    private SessionKeys? _prevRecvKeys;         // 轮换瞬间在途帧仍可解
    private ulong _sendCounter;
    private readonly Dictionary<uint, ulong> _recvCounters = new();
    private ECDiffieHellman? _myRekeyEcdh;
    private byte[]? _myRekeyHello;
    private TaskCompletionSource<bool>? _rekeyDoneTcs;
    private int _rekeyBusy;
    private DateTimeOffset _nextRotationAt;
    private DateTimeOffset _lastReceive;
    private Timer? _rotationTimer;
    private Timer? _keepAliveTimer;
    private Exception? _startupError;
    private volatile bool _disposed;

    public SecureConnection(SecureChatOptions? options = null) => _opt = options ?? new();

    /// <summary>监听模式:等待对端连入并完成握手。返回即已建立(或抛异常)。</summary>
    public Task ListenAsync(int port, string psk, string displayName, CancellationToken ct = default)
        => StartAsync(true, port, null, psk, displayName, ct);

    /// <summary>连接模式:主动连入对端并完成握手。返回即已建立(或抛异常)。</summary>
    public Task ConnectAsync(string host, int port, string psk, string displayName, CancellationToken ct = default)
        => StartAsync(false, port, host, psk, displayName, ct);

    private async Task StartAsync(bool listen, int port, string? host,
                                  string psk, string displayName, CancellationToken ct)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (State != ConnState.Idle)
            throw new InvalidOperationException("连接已在使用中");
        if (string.IsNullOrEmpty(psk))
            throw new ArgumentException("PSK 口令不能为空", nameof(psk));

        State = listen ? ConnState.Listening : ConnState.Connecting;
        _psk = psk;
        _myName = displayName;
        _cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        _ = Task.Run(() => RunAsync(listen, port, host, _cts.Token), CancellationToken.None);
        await _startedTcs.Task.ConfigureAwait(false);
        if (_startupError is not null)
        {
            State = ConnState.Closed;
            throw _startupError;
        }
    }

    // ─────────────────────────── 主流程 ───────────────────────────

    private async Task RunAsync(bool listen, int port, string? host, CancellationToken ct)
    {
        try
        {
            _stream = listen
                ? (await TcpChannel.AcceptAsync(port, ct).ConfigureAwait(false)).Stream
                : await TcpChannel.ConnectAsync(host!, port, ct).ConfigureAwait(false);

            State = ConnState.Handshaking;
            StateWasInitiator = !listen; // 连接方为 A,监听方为 B
            _keys = await HandshakeAsync(ct).ConfigureAwait(false);

            State = ConnState.Established;
            _lastReceive = DateTimeOffset.UtcNow;
            _nextRotationAt = _lastReceive.AddMinutes(_opt.RotationMinutes);
            _rotationTimer = new Timer(_ => _ = RekeyInternalAsync(ct), null,
                TimeSpan.FromMinutes(_opt.RotationMinutes),
                TimeSpan.FromMinutes(_opt.RotationMinutes));
            _keepAliveTimer = new Timer(_ => _ = KeepAliveTickAsync(ct), null,
                TimeSpan.FromSeconds(_opt.KeepAliveSeconds),
                TimeSpan.FromSeconds(_opt.KeepAliveSeconds));

            if (_myName.Length > 0)
                await SendEncryptedAsync(FrameType.Profile,
                    Payloads.Serialize(new Payloads.ProfileMessage(_myName)), ct).ConfigureAwait(false);

            Connected?.Invoke();
            _startedTcs.TrySetResult(true);
            await ReceiveLoopAsync(ct).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            _startupError ??= ex;
            _startedTcs.TrySetResult(false);
            State = ConnState.Closed;
            Disconnected?.Invoke(ReasonOf(ex));
        }
        catch (OperationCanceledException)
        {
            _startedTcs.TrySetResult(false);
            State = ConnState.Closed;
            Disconnected?.Invoke("已取消");
        }
        finally
        {
            await CleanupAsync().ConfigureAwait(false);
        }
    }

    /// <summary>完整握手:Hello 交换 → ECDH/HKDF → Confirm 双向确认。</summary>
    private async Task<SessionKeys> HandshakeAsync(CancellationToken ct)
    {
        bool iAmA = StateWasInitiator;
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(TimeSpan.FromSeconds(10));

        using var ecdh = ECDiffieHellman.Create(ECCurve.NamedCurves.nistP256);
        byte[] myPub = ecdh.ExportSubjectPublicKeyInfo();
        byte[] hello = Handshake.BuildHelloPayload(ecdh);

        Frame peerFrame;
        if (iAmA)
        {
            await FrameIO.WriteAsync(_stream!, new Frame(FrameType.Hello, 0, hello), timeout.Token).ConfigureAwait(false);
            peerFrame = await ExpectAsync(FrameType.Hello, timeout.Token).ConfigureAwait(false);
        }
        else
        {
            peerFrame = await ExpectAsync(FrameType.Hello, timeout.Token).ConfigureAwait(false);
            await FrameIO.WriteAsync(_stream!, new Frame(FrameType.Hello, 0, hello), timeout.Token).ConfigureAwait(false);
        }

        (byte[] peerPub, byte[] peerNonce) = Handshake.ParseHelloPayload(peerFrame.Payload);
        byte[] myNonce = hello.AsSpan(^Handshake.NonceSize).ToArray();
        byte[] transcript = Handshake.BuildTranscript(myPub, myNonce, peerPub, peerNonce, iAmA);
        byte[] z = Handshake.EcdhSharedSecret(ecdh, peerPub);
        var material = Handshake.DeriveMaterial(z, _psk, transcript, 0);

        // Confirm: mac(32) || tiebreak(8)
        byte[] myTiebreak = RandomNumberGenerator.GetBytes(Handshake.TiebreakSize);
        byte[] myMac = Handshake.BuildConfirmMac(material.ConfirmKey, transcript, iAmA);
        var confirmPayload = new byte[Handshake.ConfirmMacSize + Handshake.TiebreakSize];
        myMac.CopyTo(confirmPayload, 0);
        myTiebreak.CopyTo(confirmPayload, Handshake.ConfirmMacSize);

        if (iAmA)
        {
            await FrameIO.WriteAsync(_stream!, new Frame(FrameType.Confirm, 0, confirmPayload), timeout.Token).ConfigureAwait(false);
            var p = await ExpectAsync(FrameType.Confirm, timeout.Token).ConfigureAwait(false);
            VerifyPeerConfirm(p.Payload, material.ConfirmKey, transcript, iAmA, out var peerTiebreak);
            return Handshake.BuildSessionKeys(material, 0, transcript, iAmA, myTiebreak, peerTiebreak);
        }
        else
        {
            var p = await ExpectAsync(FrameType.Confirm, timeout.Token).ConfigureAwait(false);
            VerifyPeerConfirm(p.Payload, material.ConfirmKey, transcript, iAmA, out var peerTiebreak);
            await FrameIO.WriteAsync(_stream!, new Frame(FrameType.Confirm, 0, confirmPayload), timeout.Token).ConfigureAwait(false);
            return Handshake.BuildSessionKeys(material, 0, transcript, iAmA, myTiebreak, peerTiebreak);
        }
    }

    private void VerifyPeerConfirm(byte[] payload, byte[] confirmKey, byte[] transcript,
                                   bool iAmA, out byte[] peerTiebreak)
    {
        if (payload.Length != Handshake.ConfirmMacSize + Handshake.TiebreakSize)
            throw new IOException("Confirm 载荷长度非法");
        Handshake.VerifyConfirmMac(confirmKey, transcript, !iAmA, payload.AsSpan(0, Handshake.ConfirmMacSize).ToArray());
        peerTiebreak = payload.AsSpan(Handshake.ConfirmMacSize).ToArray();
    }

    private bool StateWasInitiator { get; set; }

    private async Task<Frame> ExpectAsync(FrameType type, CancellationToken ct)
    {
        var f = await FrameIO.ReadAsync(_stream!, ct).ConfigureAwait(false)
                ?? throw new IOException("对端在握手期间关闭了连接");
        if (f.Type != type)
            throw new IOException($"握手期间收到意外帧: {f.Type}");
        return f;
    }

    // ─────────────────────────── 收包循环 ───────────────────────────

    private async Task ReceiveLoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var frame = await FrameIO.ReadAsync(_stream!, ct).ConfigureAwait(false)
                        ?? throw new IOException("对端关闭了连接");
            _lastReceive = DateTimeOffset.UtcNow;

            switch (frame.Type)
            {
                case FrameType.Data:
                    HandleData(frame);
                    break;
                case FrameType.Profile:
                    HandleProfile(frame);
                    break;
                case FrameType.Rekey:
                    await HandleRekeyAsync(frame, ct).ConfigureAwait(false);
                    break;
                case FrameType.RekeyRequest:
                    // 我是 driver:对端请求轮换 → 由我发起(已在轮换中则忽略)
                    if (_keys is { IAmLowTiebreak: true } && Volatile.Read(ref _rekeyBusy) == 0)
                        _ = RekeyNowAsync(ct);
                    break;
                case FrameType.Ping:
                    DecryptIncoming(frame); // 保活帧同样要解密并推进防重放计数器
                    break;
                case FrameType.Bye:
                    throw new IOException("对端已结束会话");
                default:
                    throw new IOException($"会话中收到意外帧: {frame.Type}");
            }
        }
    }

    private void HandleData(Frame frame)
    {
        byte[] pt = DecryptIncoming(frame);
        var msg = Payloads.Deserialize<Payloads.ChatMessage>(pt)
                  ?? throw new IOException("消息载荷格式错误");
        MessageReceived?.Invoke(_peerName ?? "对方", msg.Content, DateTimeOffset.Now);
    }

    private void HandleProfile(Frame frame)
    {
        byte[] pt = DecryptIncoming(frame);
        var profile = Payloads.Deserialize<Payloads.ProfileMessage>(pt)
                      ?? throw new IOException("Profile 载荷格式错误");
        _peerName = profile.Name;
        PeerNameReceived?.Invoke(profile.Name);
    }

    /// <summary>按纪元取密钥解密 + 防重放(计数器严格递增)。</summary>
    private byte[] DecryptIncoming(Frame frame)
    {
        var keys = _keys ?? throw new InvalidOperationException();
        byte[] key, nonceBase;
        if (frame.Epoch == keys.Epoch) { key = keys.PeerKey; nonceBase = keys.PeerNonceBase; }
        else if (_prevRecvKeys is { } old && frame.Epoch == old.Epoch)
        { key = old.PeerKey; nonceBase = old.PeerNonceBase; }
        else throw new IOException($"未知密钥纪元: {frame.Epoch}");

        byte[] pt = AesGcmBox.Open(key, nonceBase, frame.Type, frame.Epoch, frame.Payload);

        uint counter = BinaryPrimitives.ReadUInt32BigEndian(frame.Payload);
        ulong last = _recvCounters.TryGetValue(frame.Epoch, out var v) ? v : 0;
        if (counter <= last)
            throw new IOException("检测到重放或乱序帧,连接已中止");
        _recvCounters[frame.Epoch] = counter;
        return pt;
    }

    // ─────────────────────────── 密钥轮换 ───────────────────────────

    /// <summary>
    /// 立即轮换密钥(自动定时与手动按钮共用)。已在轮换中则直接返回。
    /// 确定性角色:tiebreak 较低一方(driver)发送 RekeyHello,较高一方只发 RekeyRequest,
    /// 从根本上避免双方同时发起的竞态。
    /// </summary>
    public async Task RekeyNowAsync(CancellationToken ct = default)
    {
        if (State != ConnState.Established)
            throw new InvalidOperationException("连接未建立");
        uint startEpoch = _keys!.Epoch;

        if (Interlocked.CompareExchange(ref _rekeyBusy, 1, 0) != 0)
        {
            // 已有轮换在进行(可能正是对端请求触发的):等待纪元推进即可
            await WaitEpochAdvanceAsync(startEpoch, ct).ConfigureAwait(false);
            return;
        }

        try
        {
            _rekeyDoneTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            bool iAmDriver = _keys.IAmLowTiebreak;

            if (iAmDriver)
            {
                BeginMyRekey(); // 所有权归连接,由 HandleRekeyAsync 派生后释放
                byte[] hello = _myRekeyHello!;
                await SendEncryptedAsync(FrameType.Rekey, hello, ct).ConfigureAwait(false);
            }
            else
            {
                await SendEncryptedAsync(FrameType.RekeyRequest, Array.Empty<byte>(), ct).ConfigureAwait(false);
            }

            await _rekeyDoneTcs.Task.WaitAsync(TimeSpan.FromSeconds(10), ct).ConfigureAwait(false);
            if (_keys!.Epoch == startEpoch)
                throw new TimeoutException("对端未在期限内完成密钥轮换");

            if (iAmDriver)
            {
                // driver 最后切换:本方切换即代表对端已完成
                await SendEncryptedAsync(FrameType.Ping, Array.Empty<byte>(), ct).ConfigureAwait(false);
            }
            else
            {
                // 高方先切换:等待 driver 新纪元回执(Ping),确保双方都完成
                await WaitEpochEchoAsync(startEpoch + 1, ct).ConfigureAwait(false);
            }
        }
        finally
        {
            Interlocked.Exchange(ref _rekeyBusy, 0);
            _rekeyDoneTcs = null;
        }
    }

    /// <summary>轮询等待纪元从 from 前进(上限 10 秒)。</summary>
    private async Task WaitEpochAdvanceAsync(uint from, CancellationToken ct)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(10);
        while (_keys!.Epoch == from)
        {
            if (DateTimeOffset.UtcNow > deadline)
                throw new TimeoutException("等待密钥轮换超时");
            await Task.Delay(50, ct).ConfigureAwait(false);
        }
    }

    /// <summary>轮询等待收到新纪元的对端回执帧(上限 10 秒)。</summary>
    private async Task WaitEpochEchoAsync(uint targetEpoch, CancellationToken ct)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(10);
        while (!_recvCounters.ContainsKey(targetEpoch))
        {
            if (DateTimeOffset.UtcNow > deadline)
                throw new TimeoutException("等待对端密钥轮换回执超时");
            await Task.Delay(20, ct).ConfigureAwait(false);
        }
    }

    private async Task RekeyInternalAsync(CancellationToken ct)
    {
        try { if (State == ConnState.Established) await RekeyNowAsync(ct).ConfigureAwait(false); }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            await KillAsync("密钥轮换失败: " + ex.Message).ConfigureAwait(false);
        }
    }

    private ECDiffieHellman BeginMyRekey()
    {
        if (_myRekeyEcdh is null)
        {
            _myRekeyEcdh = ECDiffieHellman.Create(ECCurve.NamedCurves.nistP256);
            _myRekeyHello = Handshake.BuildHelloPayload(_myRekeyEcdh);
        }
        return _myRekeyEcdh;
    }

    /// <summary>
    /// 处理对端 RekeyHello。
    /// driver(低 tiebreak)收到的是高方的"响应 hello"→ 派生并切换,不再回发;
    /// 高方收到的是 driver 的"发起 hello"→ 派生后用旧钥回发自己的 hello 再切换。
    /// TCP FIFO 保证响应 hello 先于任何新纪元帧到达。
    /// </summary>
    private async Task HandleRekeyAsync(Frame frame, CancellationToken ct)
    {
        var current = _keys ?? throw new InvalidOperationException();
        bool iAmDriver = current.IAmLowTiebreak;

        // 已切换后迟到的旧纪元重复帧 → 忽略
        if (frame.Epoch + 1 == current.Epoch)
            return;
        if (frame.Epoch != current.Epoch)
            throw new IOException($"Rekey 纪元非法: {frame.Epoch}");

        (byte[] peerPub, byte[] peerNonce) = Handshake.ParseHelloPayload(DecryptIncoming(frame));

        BeginMyRekey();
        byte[] myHello = _myRekeyHello!;
        (byte[] myPub, byte[] myNonce) = Handshake.ParseHelloPayload(myHello);

        using var ecdh = _myRekeyEcdh!;
        byte[] z = Handshake.EcdhSharedSecret(ecdh, peerPub);
        byte[] nextTranscript = Handshake.BuildNextTranscript(current, myPub, myNonce, peerPub, peerNonce);
        var material = Handshake.DeriveMaterial(z, _psk, nextTranscript, current.Epoch + 1);
        var next = Handshake.BuildSessionKeys(material, current.Epoch + 1, nextTranscript,
                                              current.IAmA, current.MyTiebreak, current.PeerTiebreak);

        // 高方:先在旧钥下回发自己的 hello(driver 按旧钥解),随后再切换
        if (!iAmDriver)
            await SendEncryptedAsync(FrameType.Rekey, myHello, ct).ConfigureAwait(false);

        await _sendLock.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            _prevRecvKeys = current;   // 轮换瞬间在途的旧纪元帧仍可解
            _keys = next;
            _sendCounter = 0;          // nonce 基值已换,计数器归零
            _nextRotationAt = DateTimeOffset.UtcNow.AddMinutes(_opt.RotationMinutes);
        }
        finally { _sendLock.Release(); }

        _myRekeyEcdh = null; // ecdh 由上方 using 负责释放
        _myRekeyHello = null;

        KeyRotated?.Invoke(next.Epoch, next.Fingerprint);
        _rekeyDoneTcs?.TrySetResult(true);
    }

    // ─────────────────────────── 发送 ───────────────────────────

    public async Task SendAsync(string content, CancellationToken ct = default)
    {
        if (State != ConnState.Established)
            throw new InvalidOperationException("连接未建立");
        await SendEncryptedAsync(FrameType.Data,
            Payloads.Serialize(new Payloads.ChatMessage(content)), ct).ConfigureAwait(false);
    }

    /// <summary>更新我的显示名;已建立时立即经加密通道告知对端。</summary>
    public async Task SetDisplayNameAsync(string name, CancellationToken ct = default)
    {
        _myName = name ?? string.Empty;
        if (State == ConnState.Established && _myName.Length > 0)
            await SendEncryptedAsync(FrameType.Profile,
                Payloads.Serialize(new Payloads.ProfileMessage(_myName)), ct).ConfigureAwait(false);
    }

    /// <summary>帧级加密发送。计数器与线路顺序在同一把发送锁下保证一致。</summary>
    private async Task SendEncryptedAsync(FrameType type, byte[] plaintext, CancellationToken ct)
    {
        await _sendLock.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            await SealSendUnderLockAsync(type, plaintext, ct).ConfigureAwait(false);
        }
        finally { _sendLock.Release(); }
    }

    /// <summary>在已持有发送锁的前提下封帧发送(供 RekeyNow 的"检查+发送"原子操作复用)。</summary>
    private async Task SealSendUnderLockAsync(FrameType type, byte[] plaintext, CancellationToken ct)
    {
        var keys = _keys ?? throw new InvalidOperationException();
        ulong counter = ++_sendCounter;
        byte[] sealedPayload = AesGcmBox.Seal(keys.MyKey, keys.MyNonceBase, counter,
                                              type, keys.Epoch, plaintext);
        await FrameIO.WriteAsync(_stream!, new Frame(type, keys.Epoch, sealedPayload), ct).ConfigureAwait(false);
    }

    // ─────────────────────────── 保活 / 断开 ───────────────────────────

    private async Task KeepAliveTickAsync(CancellationToken ct)
    {
        try
        {
            if (State != ConnState.Established) return;
            var idle = DateTimeOffset.UtcNow - _lastReceive;
            if (idle.TotalSeconds > _opt.DeadSeconds)
            {
                await KillAsync("连接超时").ConfigureAwait(false);
                return;
            }
            if (idle.TotalSeconds > _opt.KeepAliveSeconds - 1)
                await SendEncryptedAsync(FrameType.Ping, Array.Empty<byte>(), ct).ConfigureAwait(false);
        }
        catch { /* 保活失败交给接收循环/超时判定处理 */ }
    }

    /// <summary>主动断开:尽力告知对端后关闭。</summary>
    public async Task DisconnectAsync(string reason = "本地主动断开")
    {
        await KillAsync(reason).ConfigureAwait(false);
    }

    private async Task KillAsync(string reason)
    {
        if (State == ConnState.Closed) return;
        try
        {
            if (State == ConnState.Established && _stream is not null)
            {
                using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                await SendEncryptedAsync(FrameType.Bye, Array.Empty<byte>(), timeout.Token).ConfigureAwait(false);
            }
        }
        catch { /* 尽力而为 */ }
        State = ConnState.Closed;
        _startedTcs.TrySetResult(false);
        try { _cts?.Cancel(); } catch { }
        Disconnected?.Invoke(reason);
    }

    private static string ReasonOf(Exception ex) => ex switch
    {
        IOException io => io.Message,
        SocketException se => $"网络错误: {se.Message}",
        CryptographicException ce => ce.Message,
        _ => ex.Message,
    };

    private Task CleanupAsync()
    {
        _rotationTimer?.Dispose(); _rotationTimer = null;
        _keepAliveTimer?.Dispose(); _keepAliveTimer = null;
        try { _stream?.Dispose(); } catch { }
        _stream = null;
        if (_keys is not null)
        {
            CryptographicOperations.ZeroMemory(_keys.KeyA2B);
            CryptographicOperations.ZeroMemory(_keys.KeyB2A);
        }
        _myRekeyEcdh?.Dispose();
        _myRekeyEcdh = null;
        _myRekeyHello = null;
        return Task.CompletedTask;
    }

    public async ValueTask DisposeAsync()
    {
        if (_disposed) return;
        _disposed = true;
        await KillAsync("已释放").ConfigureAwait(false);
        _sendLock.Dispose();
        _cts?.Dispose();
    }
}
