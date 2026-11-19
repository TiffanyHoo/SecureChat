using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using SecureChat.Core;
using SecureChat.Core.Crypto;
using SecureChat.Core.Protocol;
using Xunit;

namespace SecureChat.Core.Tests;

/// <summary>测试辅助:在回环端口上建立一对已握手的 SecureConnection。</summary>
internal static class TestPair
{
    public static async Task<(SecureConnection Server, SecureConnection Client, int Port)> CreateAsync(
        string psk = "test-psk", SecureChatOptions? options = null)
    {
        options ??= new SecureChatOptions { RotationMinutes = 10, KeepAliveSeconds = 15, DeadSeconds = 45 };

        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        int port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();

        var server = new SecureConnection(options);
        var client = new SecureConnection(options);

        var serverTask = server.ListenAsync(port, psk, "监听方", CancellationToken.None);
        // 确保监听先就绪
        await Task.Delay(200, CancellationToken.None);
        var clientTask = client.ConnectAsync("127.0.0.1", port, psk, "连接方", CancellationToken.None);
        await Task.WhenAll(serverTask, clientTask);
        return (server, client, port);
    }
}

public class EndToEndTests
{
    [Fact]
    public async Task 握手后双方互发消息_明文一致()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        Assert.Equal(ConnState.Established, server.State);
        Assert.Equal(ConnState.Established, client.State);

        var recv = new TaskCompletionSource<(string From, string Content)>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.MessageReceived += (from, content, _) => recv.TrySetResult((from, content));

        await client.SendAsync("你好,这是密文通道里的明文");
        var (from, content) = await recv.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal("连接方", from);
        Assert.Equal("你好,这是密文通道里的明文", content);

        // 反向
        var recv2 = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        client.MessageReceived += (_, content2, _) => recv2.TrySetResult(content2);
        await server.SendAsync("reversed");
        Assert.Equal("reversed", await recv2.Task.WaitAsync(TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public async Task 姓名仅经加密通道交换_对端可见()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        // Profile 在握手完成时即已交换,事件可能早于本测试订阅,改为轮询属性
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (server.PeerName is null && DateTime.UtcNow < deadline)
            await Task.Delay(50);
        Assert.Equal("连接方", server.PeerName);
        Assert.Equal("监听方", client.PeerName);
    }

    [Fact]
    public async Task PSK口令不匹配_握手失败()
    {
        int port;
        {
            var l = new TcpListener(IPAddress.Loopback, 0);
            l.Start();
            port = ((IPEndPoint)l.LocalEndpoint).Port;
            l.Stop();
        }
        var server = new SecureConnection();
        var client = new SecureConnection();
        await using var _1 = server; await using var _2 = client;

        var serverTask = server.ListenAsync(port, "正确口令", "S", CancellationToken.None);
        await Task.Delay(200, CancellationToken.None);
        // 客户端可能先收到对端断开(IO.IOException)或自己校验失败(CryptographicException),二者皆可
        await Assert.ThrowsAnyAsync<Exception>(
            () => client.ConnectAsync("127.0.0.1", port, "错误口令", "C", CancellationToken.None));
        // 服务端必须报出口令不匹配
        var ex = await Assert.ThrowsAnyAsync<Exception>(
            () => serverTask.WaitAsync(TimeSpan.FromSeconds(5)));
        Assert.IsType<CryptographicException>(ex);
    }

    [Fact]
    public async Task 手动轮换_纪元递增_消息继续可达()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        var reasons = new List<string>();
        server.Disconnected += r => { lock (reasons) reasons.Add("S: " + r); };
        client.Disconnected += r => { lock (reasons) reasons.Add("C: " + r); };

        uint oldEpoch = client.Epoch;
        string oldFp = client.Fingerprint;

        await client.RekeyNowAsync();

        try
        {
            Assert.Equal(oldEpoch + 1, client.Epoch);
            Assert.Equal(oldEpoch + 1, server.Epoch);
            Assert.Equal(client.Fingerprint, server.Fingerprint);
            Assert.NotEqual(oldFp, client.Fingerprint);
        }
        finally
        {
            lock (reasons) Assert.True(reasons.Count == 0,
                $"连接意外断开: {string.Join("; ", reasons)}; epoch C={client.Epoch} S={server.Epoch}");
        }

        var recv = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.MessageReceived += (_, content, _) => recv.TrySetResult(content);
        await client.SendAsync("换钥后仍然可达");
        Assert.Equal("换钥后仍然可达", await recv.Task.WaitAsync(TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public async Task 双方同时轮换_碰撞也能收敛()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        var reasons = new List<string>();
        server.Disconnected += r => { lock (reasons) reasons.Add("S: " + r); };
        client.Disconnected += r => { lock (reasons) reasons.Add("C: " + r); };

        await Task.WhenAll(client.RekeyNowAsync(), server.RekeyNowAsync());

        Assert.Equal(client.Epoch, server.Epoch);
        Assert.Equal(client.Fingerprint, server.Fingerprint);

        var recv = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.MessageReceived += (_, content, _) => recv.TrySetResult(content);
        await client.SendAsync("碰撞后消息正常");
        try
        {
            var got = await recv.Task.WaitAsync(TimeSpan.FromSeconds(5));
            Assert.Equal("碰撞后消息正常", got);
        }
        finally
        {
            lock (reasons) Assert.True(reasons.Count == 0,
                $"连接意外断开: {string.Join("; ", reasons)}; epoch C={client.Epoch} S={server.Epoch} fpEqual={client.Fingerprint == server.Fingerprint}");
        }
    }

    [Fact]
    public void 密文被篡改_GCM认证失败()
    {
        byte[] key = RandomNumberGenerator.GetBytes(32);
        byte[] nonceBase = RandomNumberGenerator.GetBytes(8);
        byte[] sealedBuf = AesGcmBox.Seal(key, nonceBase, 1, FrameType.Data, 0, "attack at dawn"u8.ToArray());

        byte[] decrypted = AesGcmBox.Open(key, nonceBase, FrameType.Data, 0, sealedBuf);
        Assert.Equal("attack at dawn"u8.ToArray(), decrypted);

        sealedBuf[^1] ^= 0xFF; // 篡改密文末字节(即认证标签)
        Assert.ThrowsAny<CryptographicException>(() => AesGcmBox.Open(key, nonceBase, FrameType.Data, 0, sealedBuf));
    }

    [Fact]
    public void AAD绑定帧头_篡改纪元或类型导致解密失败()
    {
        byte[] key = RandomNumberGenerator.GetBytes(32);
        byte[] nonceBase = RandomNumberGenerator.GetBytes(8);
        byte[] sealedBuf = AesGcmBox.Seal(key, nonceBase, 1, FrameType.Data, 3, "payload"u8.ToArray());

        // 用不同 epoch/type 解密(等同帧头被篡改)
        Assert.ThrowsAny<CryptographicException>(() => AesGcmBox.Open(key, nonceBase, FrameType.Data, 4, sealedBuf));
        Assert.ThrowsAny<CryptographicException>(() => AesGcmBox.Open(key, nonceBase, FrameType.Profile, 3, sealedBuf));
    }

    [Fact]
    public void 不同纪元派生不同密钥_前向隔离()
    {
        byte[] z = RandomNumberGenerator.GetBytes(32);
        byte[] t0 = SHA256.HashData("t0"u8.ToArray());
        byte[] t1 = SHA256.HashData("t1"u8.ToArray());

        var m0 = Handshake.DeriveMaterial(z, "psk", t0, 0);
        var m1 = Handshake.DeriveMaterial(z, "psk", t1, 1);
        var m0Again = Handshake.DeriveMaterial(z, "psk", t0, 0);

        Assert.Equal(m0.KeyA2B, m0Again.KeyA2B);          // 确定性
        Assert.NotEqual(m0.KeyA2B, m1.KeyA2B);            // 纪元隔离
        Assert.NotEqual(m0.KeyA2B, m0.KeyB2A);            // 双向密钥独立
    }

    [Fact]
    public async Task 对端正常断开_触发Disconnected()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        var gone = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.Disconnected += reason => gone.TrySetResult(reason);

        await client.DisconnectAsync();
        await gone.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(ConnState.Closed, server.State);
    }

    [Fact]
    public async Task Over请求_接受_协商帧双向可达()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        var clientGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        var serverGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        client.OverReceived += kind => clientGot.TrySetResult(kind);
        server.OverReceived += kind => serverGot.TrySetResult(kind);

        // 客户端请求 → 服务端收到 request
        await client.SendOverAsync("request");
        Assert.Equal("request", await serverGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));

        // 服务端同意 → 客户端收到 accept
        await server.SendOverAsync("accept");
        Assert.Equal("accept", await clientGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));

        // 协商后连接仍然存活,普通消息继续可达
        var recv = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.MessageReceived += (_, content, _) => recv.TrySetResult(content);
        await client.SendAsync("协商后仍在线");
        Assert.Equal("协商后仍在线", await recv.Task.WaitAsync(TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public async Task Over拒绝与撤回_对端事件类型正确()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        // 每次发送用独立 TCS:同一 TCS 第二次 SetResult 会被忽略
        var rejectGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        var cancelGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.OverReceived += kind =>
        {
            if (kind == "reject") rejectGot.TrySetResult(kind);
            if (kind == "cancel") cancelGot.TrySetResult(kind);
        };

        await client.SendOverAsync("reject");
        Assert.Equal("reject", await rejectGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));

        await client.SendOverAsync("cancel");
        Assert.Equal("cancel", await cancelGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public async Task Over双方同时请求_双方均收到request_连接不中断()
    {
        var (server, client, _) = await TestPair.CreateAsync();
        await using var _1 = server; await using var _2 = client;

        var sGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        var cGot = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        server.OverReceived += kind => sGot.TrySetResult(kind);
        client.OverReceived += kind => cGot.TrySetResult(kind);

        // 默契场景:双方同时发出请求(UI 层收到对方 request 时自动 accept 结束)
        await Task.WhenAll(server.SendOverAsync("request"), client.SendOverAsync("request"));

        Assert.Equal("request", await sGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));
        Assert.Equal("request", await cGot.Task.WaitAsync(TimeSpan.FromSeconds(5)));
        Assert.Equal(ConnState.Established, server.State);
        Assert.Equal(ConnState.Established, client.State);
    }
}
