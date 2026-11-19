using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using SecureChat.App.ViewModels;

namespace SecureChat.App;

/// <summary>
/// 主窗口。聊天区始终显示解密后的明文;线路数据为密文。
/// 窗口在后台时收到消息:弹 Toast(不含消息内容)+ 任务栏闪烁 + 标题未读计数 + 提示音。
/// </summary>
public sealed partial class MainWindow : Window
{
    public MainViewModel Vm { get; } = new();

    private readonly string _baseTitle = "SecureChat 安全通话";
    private IntPtr _hwnd;
    private int _unread;
    private bool _reallyExit;     // 托盘"退出"时置 true;叉掉默认是最小化到托盘
    private bool _trayTipShown;

    public MainWindow()
    {
        this.InitializeComponent();
        Title = _baseTitle;
        _hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        Vm.Initialize(DispatcherQueue);

        Vm.Messages.CollectionChanged += (_, _) =>
        {
            if (ChatList.Items.Count > 0)
                ChatList.ScrollIntoView(ChatList.Items[^1]);
        };

        Vm.IncomingNoticed += peer => OnIncomingWhileHidden(peer);

        // 跟踪前后台:回前台时清未读计数
        Activated += (_, args) =>
        {
            bool foreground = args.WindowActivationState != WindowActivationState.Deactivated;
            Vm.IsWindowForeground = foreground;
            if (foreground)
            {
                _unread = 0;
                Title = _baseTitle;
            }
        };

        // 叉掉窗口 = 最小化到托盘,后台继续接收加密消息并弹通知;
        // 托盘右键"退出"(或托盘不可用时)才真正关闭
        AppWindow.Closing += OnAppWindowClosing;
        Closed += (_, _) =>
        {
            Services.TrayIcon.Remove();
            Services.AppNotifier.Shutdown();
            Vm.Shutdown();
        };
    }

    /// <summary>托盘"退出"入口:跳过拦截,真正结束程序。</summary>
    public void RequestExit()
    {
        _reallyExit = true;
        Close();
    }

    private void OnAppWindowClosing(Microsoft.UI.Windowing.AppWindow sender,
        Microsoft.UI.Windowing.AppWindowClosingEventArgs args)
    {
        if (_reallyExit || !Services.TrayIcon.Available) return;
        args.Cancel = true;               // 拦截关闭 → 后台驻留托盘
        Vm.IsWindowForeground = false;
        AppWindow.Hide();
        if (!_trayTipShown)
        {
            _trayTipShown = true;
            Services.AppNotifier.ShowPlain("SecureChat 已最小化到托盘",
                "程序在后台继续接收加密消息并弹通知;双击托盘图标恢复窗口,右键可选择退出。");
        }
    }

    /// <summary>后台收到消息:通知不带内容,只提示来源。</summary>
    private void OnIncomingWhileHidden(string peerName)
    {
        _unread++;
        Title = _unread <= 1 ? $"{_baseTitle} - 1 条未读" : $"{_baseTitle} - {_unread} 条未读";
        Services.AppNotifier.ShowNewMessage(peerName);
        FlashTaskbar();
        NativeMethods.MessageBeep(NativeMethods.MbSimpleBeep);
    }

    private void FlashTaskbar()
    {
        var info = new NativeMethods.FLASHWINFO
        {
            cbSize = (uint)Marshal.SizeOf<NativeMethods.FLASHWINFO>(),
            hwnd = _hwnd,
            dwFlags = NativeMethods.FlashAll, // 闪烁标题栏 + 任务栏按钮
            uCount = 3,
            dwTimeout = 0,
        };
        NativeMethods.FlashWindowEx(ref info);
    }

    // x:Bind 函数绑定:角色切换时显隐对应输入框
    public Visibility ListenVisibility(bool isListen) => isListen ? Visibility.Visible : Visibility.Collapsed;
    public Visibility ConnectVisibility(bool isListen) => isListen ? Visibility.Collapsed : Visibility.Visible;

    /// <summary>x:Bind 函数绑定:房间码非空时才允许复制/高亮码框。</summary>
    public bool CanCopyCode(string? code) => !string.IsNullOrWhiteSpace(code);

    private void CopyCode_Click(object sender, RoutedEventArgs e)
    {
        if (string.IsNullOrWhiteSpace(Vm.GeneratedCode)) return;
        try
        {
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(Vm.GeneratedCode);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            Vm.StatusText = "房间码已复制,通过你信任的渠道发给对方即可";
        }
        catch
        {
            Vm.StatusText = "复制失败,请手动选中房间码复制";
        }
    }

    private void InputBox_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            Vm.InputText = InputBox.Text; // 两way绑定在失焦才回写,回车前手动同步
            Vm.SendCommand.Execute(null);
            InputBox.Focus(FocusState.Programmatic);
            e.Handled = true;
        }
    }
}

/// <summary>按消息方向/系统类型选择气泡模板。</summary>
public class ChatTemplateSelector : DataTemplateSelector
{
    public DataTemplate? MineTemplate { get; set; }
    public DataTemplate? TheirsTemplate { get; set; }
    public DataTemplate? SystemTemplate { get; set; }

    protected override DataTemplate? SelectTemplateCore(object item, DependencyObject container) =>
        SelectTemplateCore(item);

    protected override DataTemplate? SelectTemplateCore(object item)
    {
        if (item is not ChatMessage msg) return null;
        if (msg.IsSystem) return SystemTemplate;
        return msg.IsMine ? MineTemplate : TheirsTemplate;
    }
}

/// <summary>任务栏闪烁与提示音所需的 Win32 调用。</summary>
internal static class NativeMethods
{
    public const uint FlashAll = 3;              // FLASHW_CAPTION | FLASHW_TRAY
    public const uint MbSimpleBeep = 0xFFFFFFFF; // 简单提示音

    [StructLayout(LayoutKind.Sequential)]
    public struct FLASHWINFO
    {
        public uint cbSize;
        public IntPtr hwnd;
        public uint dwFlags;
        public uint uCount;
        public uint dwTimeout;
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool FlashWindowEx(ref FLASHWINFO pwfi);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool MessageBeep(uint uType);
}
