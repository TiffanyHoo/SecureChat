using System.Net;
using System.Net.Sockets;

namespace SecureChat.Core.Transport;

/// <summary>TCP 通道:监听方与连接方两种角色,建立后返回全双工流。</summary>
public static class TcpChannel
{
    /// <summary>监听指定端口并接受一条连接。返回 (流, 对端地址)。</summary>
    public static async Task<(Stream Stream, IPEndPoint Remote)> AcceptAsync(
        int port, CancellationToken ct)
    {
        using var listener = new TcpListener(IPAddress.Any, port);
        listener.Server.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
        listener.Start();
        using var reg = ct.Register(() => listener.Stop());
        Socket socket;
        try
        {
            socket = await listener.AcceptSocketAsync(ct).ConfigureAwait(false);
        }
        catch (SocketException) when (ct.IsCancellationRequested)
        {
            throw new OperationCanceledException(ct);
        }
        var remote = (IPEndPoint)socket.RemoteEndPoint!;
        return (new NetworkStream(socket, ownsSocket: true), remote);
    }

    /// <summary>连接远端。返回流。</summary>
    public static async Task<Stream> ConnectAsync(string host, int port, CancellationToken ct)
    {
        var client = new TcpClient();
        try
        {
            await client.ConnectAsync(host, port, ct).ConfigureAwait(false);
            return client.GetStream();
        }
        catch
        {
            client.Dispose();
            throw;
        }
    }
}
