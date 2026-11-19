using System.Collections.ObjectModel;
using System.Net.Sockets;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.UI.Dispatching;
using SecureChat.Core;
using SecureChat.Core.Protocol;

namespace SecureChat.App.ViewModels;

/// <summary>
/// 主界面视图模型:房间创建/加入(房间码流程)、消息收发、密钥状态展示。
/// 所有 Core 事件经 DispatcherQueue 封送回 UI 线程。
/// </summary>
public partial class MainViewModel : ObservableObject
{
    public ObservableCollection<ChatMessage> Messages { get; } = new();

    [ObservableProperty] private int _listenModeIndex; // 0=创建房间(监听方) 1=加入房间(发起方)
    [ObservableProperty] private string _listenPort = "9001";
    [ObservableProperty] private string _displayName = "我";
    [ObservableProperty] private string _rotationMinutes = "10";
    [ObservableProperty] private string _statusText = "未连接";
    [ObservableProperty] private bool _isConnected;
    [ObservableProperty] private bool _isBusy;
    [ObservableProperty] private string _keyInfo = "尚未建立安全会话";
    [ObservableProperty] private string _peerLabel = "";
    [ObservableProperty] private string _inputText = "";
    [ObservableProperty] private string _hostIp = "";        // 创建房间使用的本机 IP
    [ObservableProperty] private string _generatedCode = ""; // 创建房间后生成的房间码
    [ObservableProperty] private string _roomCodeInput = ""; // 加入方粘贴的房间码

    private SecureConnection? _connection;
    private DispatcherQueue? _dispatcher;
    private DispatcherQueueTimer? _tickTimer;
    private bool _rekeying;

    /// <summary>本机候选监听 IP(私网优先),多网卡场景由用户在下拉框选择。</summary>
    public IReadOnlyList<string> HostIps { get; }

    public MainViewModel()
    {
        HostIps = RoomCode.GetCandidateHostIps();
        _hostIp = HostIps[0];
    }

    /// <summary>窗口是否在前台(由 MainWindow 维护)。失焦时收到消息才发通知,前台聊天不打扰。</summary>
    public bool IsWindowForeground { get; set; } = true;

    /// <summary>窗口失焦时收到新消息触发(参数=发送者显示名)。UI 层据此弹通知/闪烁/计数。</summary>
    public event Action<string>? IncomingNoticed;

    /// <summary>是否处于"创建房间"模式(与"加入房间"相对)。</summary>
    public bool IsListenMode => ListenModeIndex == 0;

    partial void OnListenModeIndexChanged(int value) => OnPropertyChanged(nameof(IsListenMode));

    public bool CanConfigure => !IsConnected && !IsBusy;
    public bool CanChat => IsConnected && !IsBusy;
    /// <summary>断开在"监听等待中(可取消)"或"已连接"时可用——等待对方加入期间也必须能取消。</summary>
    public bool CanDisconnect => IsConnected || IsBusy;

    partial void OnIsConnectedChanged(bool value)
    {
        OnPropertyChanged(nameof(CanConfigure));
        OnPropertyChanged(nameof(CanChat));
        OnPropertyChanged(nameof(CanDisconnect));
    }

    partial void OnIsBusyChanged(bool value)
    {
        OnPropertyChanged(nameof(CanConfigure));
        OnPropertyChanged(nameof(CanChat));
        OnPropertyChanged(nameof(CanDisconnect));
    }

    public void Initialize(DispatcherQueue dispatcher)
    {
        _dispatcher = dispatcher;
        _tickTimer = dispatcher.CreateTimer();
        _tickTimer.Interval = TimeSpan.FromSeconds(1);
        _tickTimer.Tick += (_, _) => RefreshCountdown();
        _tickTimer.Start();
    }

    public void Shutdown()
    {
        _tickTimer?.Stop();
        _connection?.DisposeAsync().AsTask().GetAwaiter().GetResult();
    }

    private void Marshal(Action action)
    {
        if (_dispatcher is { } dq) dq.TryEnqueue(() => action());
        else action();
    }

    private void AddSystem(string text) =>
        Messages.Add(new ChatMessage { Content = text, IsSystem = true });

    // ─────────────────────────── 房间创建 / 加入 ───────────────────────────

    /// <summary>创建房间:生成 256 位随机口令 → 编码房间码(本机IP+端口+周期+口令) → 开始监听。</summary>
    [RelayCommand]
    private async Task CreateRoomAsync()
    {
        if (IsBusy || IsConnected) return;
        if (!int.TryParse(ListenPort, out var port) || port is < 1 or > 65535)
        {
            StatusText = "本地端口需为 1-65535";
            return;
        }
        if (!int.TryParse(RotationMinutes, out var rotation) || rotation < 1 || rotation > 720)
        {
            StatusText = "轮换周期需为 1-720 分钟";
            return;
        }

        IsBusy = true;
        Messages.Clear();
        SecureConnection? conn = null;
        try
        {
            byte[] psk = RoomCode.GeneratePsk();
            GeneratedCode = RoomCode.Create(HostIp, port, rotation, psk);

            conn = new SecureConnection(new SecureChatOptions { RotationMinutes = rotation });
            _connection = conn;
            WireEvents(conn);

            StatusText = $"房间已创建,正在监听端口 {port},等待对方加入...";
            AddSystem($"房间码已生成(含随机口令)。请复制后经你信任的渠道发给对方,对方粘贴即可加入。");

            await conn.ListenAsync(port, RoomCode.PskToText(psk), DisplayName.Trim());

            if (conn.State != ConnState.Established)
            {
                // 监听被取消/中断(如点了断开),不得误报"已建立"
                if (ReferenceEquals(_connection, conn)) _connection = null;
                GeneratedCode = string.Empty;
                await conn.DisposeAsync();
                return;
            }

            IsConnected = true;
            StatusText = "安全会话已建立(端到端加密)";
            AddSystem("握手完成,通信已加密。对方名称不会在线路上明文出现。");
        }
        catch (Exception ex)
        {
            StatusText = "创建房间失败: " + ReasonOf(ex);
            _connection = null;
            if (conn is not null) await conn.DisposeAsync();
        }
        finally
        {
            IsBusy = false;
        }
    }

    /// <summary>加入房间:解析房间码(对方IP+端口+周期+口令) → 直接连入。</summary>
    [RelayCommand]
    private async Task JoinRoomAsync()
    {
        if (IsBusy || IsConnected) return;
        if (!RoomCode.TryParse(RoomCodeInput, out RoomInfo? room) || room is null)
        {
            StatusText = "房间码无效:请粘贴对方发来的完整房间码(SC1- 开头)";
            return;
        }

        IsBusy = true;
        Messages.Clear();
        SecureConnection? conn = null;
        try
        {
            conn = new SecureConnection(new SecureChatOptions { RotationMinutes = room.RotationMinutes });
            _connection = conn;
            WireEvents(conn);

            StatusText = $"正在连接 {room.Host}:{room.Port}...";
            AddSystem($"正在连接 {room.Host}:{room.Port} ...");

            await conn.ConnectAsync(room.Host, room.Port, room.PskText, DisplayName.Trim());

            if (conn.State != ConnState.Established)
            {
                // 连接被取消/中断(如点了断开),不得误报"已建立"
                if (ReferenceEquals(_connection, conn)) _connection = null;
                await conn.DisposeAsync();
                return;
            }

            IsConnected = true;
            StatusText = "安全会话已建立(端到端加密)";
            AddSystem("握手完成,通信已加密。对方名称不会在线路上明文出现。");
        }
        catch (Exception ex)
        {
            StatusText = "加入房间失败: " + ReasonOf(ex);
            _connection = null;
            if (conn is not null) await conn.DisposeAsync();
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task DisconnectAsync()
    {
        if (_connection is null) return;
        var conn = _connection;
        _connection = null;
        IsConnected = false;
        GeneratedCode = string.Empty; // 作废房间码,避免复制出去的码已无法连接
        await conn.DisconnectAsync("本地主动断开");
        await conn.DisposeAsync();
    }

    private void WireEvents(SecureConnection conn)
    {
        conn.Connected += () => Marshal(() =>
        {
            if (!ReferenceEquals(_connection, conn)) return;
            IsConnected = true;
        });

        conn.PeerNameReceived += name => Marshal(() =>
        {
            if (!ReferenceEquals(_connection, conn)) return;
            PeerLabel = $" · 对方:{name}";
        });

        conn.MessageReceived += (from, content, time) => Marshal(() =>
        {
            if (!ReferenceEquals(_connection, conn)) return;
            Messages.Add(new ChatMessage { Sender = from, Content = content, Time = time, IsMine = false });
            if (!IsWindowForeground)
                IncomingNoticed?.Invoke(from); // 通知只带发送者名,不带消息内容
        });

        conn.KeyRotated += (epoch, fp) => Marshal(() =>
        {
            if (!ReferenceEquals(_connection, conn)) return;
            AddSystem($"会话密钥已自动轮换 → 纪元 {epoch},新指纹 {fp}");
        });

        conn.Disconnected += reason => Marshal(() =>
        {
            if (!ReferenceEquals(_connection, conn)) return;
            IsConnected = false;
            StatusText = "已断开: " + reason;
            AddSystem("连接已断开。");
        });
    }

    // ─────────────────────────── 收发 ───────────────────────────

    [RelayCommand]
    private async Task SendAsync()
    {
        var conn = _connection;
        if (conn is null || !IsConnected) return;
        var text = InputText.Trim();
        if (text.Length == 0) return;

        InputText = string.Empty;
        Messages.Add(new ChatMessage
        {
            Sender = DisplayName.Trim(),
            Content = text,
            IsMine = true,
        });

        try
        {
            await conn.SendAsync(text);
        }
        catch (Exception ex)
        {
            AddSystem("发送失败: " + ReasonOf(ex));
        }
    }

    [RelayCommand]
    private async Task RekeyNowAsync()
    {
        var conn = _connection;
        if (conn is null || !IsConnected || _rekeying) return;
        _rekeying = true;
        try
        {
            await conn.RekeyNowAsync();
        }
        catch (Exception ex)
        {
            AddSystem("手动换钥失败: " + ReasonOf(ex));
        }
        finally
        {
            _rekeying = false;
        }
    }

    // ─────────────────────────── 状态刷新 ───────────────────────────

    private void RefreshCountdown()
    {
        var conn = _connection;
        if (conn is null || !IsConnected)
        {
            if (KeyInfo != "尚未建立安全会话")
                KeyInfo = "尚未建立安全会话";
            return;
        }
        var remain = TimeSpan.FromSeconds(conn.SecondsUntilRotation);
        KeyInfo = $"加密纪元 {conn.Epoch} · 密钥指纹 {conn.Fingerprint}" +
                  $" · {remain:hh\\:mm\\:ss} 后自动换钥";
    }

    private static string ReasonOf(Exception ex) => ex switch
    {
        ArgumentException => ex.Message,
        SocketException se => $"网络错误({se.SocketErrorCode})",
        TimeoutException => "连接超时",
        _ => ex.Message,
    };
}
