using System.Collections.Specialized;
using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using SecureChat.App.ViewModels;
using Windows.Graphics;

namespace SecureChat.App;

/// <summary>
/// 主窗口。聊天区始终显示解密后的明文;线路数据为密文。
/// 初始/最小尺寸 500x680(DIP,按 DPI 换算物理像素),布局自适应。
/// 窗口在后台时收到消息:弹 Toast(不含消息内容)+ 任务栏闪烁 + 标题未读计数 + 提示音。
/// </summary>
public sealed partial class MainWindow : Window
{
    public const int MinWidthDip = 500;
    public const int MinHeightDip = 680;

    public MainViewModel Vm { get; } = new();

    private readonly string _baseTitle = "SecureChat 安全通话";
    private IntPtr _hwnd;
    private int _unread;
    private bool _reallyExit;     // 托盘"退出"时置 true;叉掉默认是最小化到托盘
    private bool _trayTipShown;

    // 聊天列表:仅"刚新增的气泡"播一次入场动画,播过即移出集合,滚动回看不重播
    private readonly HashSet<ChatMessage> _pendingBubbleAnim = new();
    private ScrollViewer? _chatScroll;
    private ScrollViewer? ChatScroll => _chatScroll ??= FindDescendant<ScrollViewer>(ChatList);

    public MainWindow()
    {
        this.InitializeComponent();
        Title = _baseTitle;
        _hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        AppWindow.SetIcon("Assets/app.ico"); // 标题栏/Alt+Tab 图标(exe 图标由 csproj 管,窗口要单独设)
        Vm.Initialize(DispatcherQueue);

        ApplyWindowSize();
        NativeMethods.SetWindowSubclass(_hwnd, MinSizeWndProc, 1, 0); // 拦 WM_GETMINMAXINFO 锁最小尺寸

        StartOverWaitPulse();

        Vm.Messages.CollectionChanged += OnMessagesChanged;
        ChatList.ContainerContentChanging += OnContainerContentChanging;

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

    /// <summary>x:Bind 函数绑定:通用布尔 → 显隐。</summary>
    public Visibility VisibleIf(bool b) => b ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>x:Bind 函数绑定:房间码非空时才允许复制/高亮码框。</summary>
    public bool CanCopyCode(string? code) => !string.IsNullOrWhiteSpace(code);

    // ─────────────────────────── OVER 交互 ───────────────────────────

    /// <summary>撤回已发出的 OVER 请求。</summary>
    private void CancelOver_Click(object sender, RoutedEventArgs e) => Vm.CancelOverCommand.Execute(null);

    /// <summary>接受对方 OVER 请求:挂断并清空会话。</summary>
    private void OverAccept_Click(object sender, RoutedEventArgs e) => _ = Vm.RespondOverAsync(true);

    /// <summary>拒绝对方 OVER 请求:继续聊。</summary>
    private void OverReject_Click(object sender, RoutedEventArgs e) => _ = Vm.RespondOverAsync(false);

    /// <summary>"OVER 已发出"文字呼吸脉冲,营造电台呼叫氛围。</summary>
    private void StartOverWaitPulse()
    {
        var pulse = new DoubleAnimation
        {
            From = 1.0,
            To = 0.35,
            Duration = new Duration(TimeSpan.FromMilliseconds(700)),
            AutoReverse = true,
            RepeatBehavior = RepeatBehavior.Forever,
        };
        Storyboard.SetTarget(pulse, OverWaitText);
        Storyboard.SetTargetProperty(pulse, "Opacity");
        var sb = new Storyboard();
        sb.Children.Add(pulse);
        sb.Begin();
    }

    // ─────────────────────────── 窗口尺寸 ───────────────────────────

    /// <summary>初始窗口 500x680(DIP),按当前窗口 DPI 换算为物理像素。</summary>
    private void ApplyWindowSize()
    {
        double scale = NativeMethods.GetDpiForWindow(_hwnd) / 96.0;
        AppWindow.Resize(new SizeInt32(
            (int)Math.Round(MinWidthDip * scale),
            (int)Math.Round(MinHeightDip * scale)));
    }

    /// <summary>WM_GETMINMAXINFO:把最小追踪尺寸钉在 500x680(DIP × 当前 DPI),拖拽缩不下去。</summary>
    private static IntPtr MinSizeWndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, UIntPtr id, UIntPtr refData)
    {
        const uint WM_GETMINMAXINFO = 0x0024;
        if (msg == WM_GETMINMAXINFO)
        {
            var mmi = Marshal.PtrToStructure<NativeMethods.MINMAXINFO>(lParam);
            double scale = NativeMethods.GetDpiForWindow(hWnd) / 96.0;
            mmi.ptMinTrackSize.X = (int)Math.Round(MinWidthDip * scale);
            mmi.ptMinTrackSize.Y = (int)Math.Round(MinHeightDip * scale);
            Marshal.StructureToPtr(mmi, lParam, false);
        }
        return NativeMethods.DefSubclassProc(hWnd, msg, wParam, lParam);
    }


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

    // ─────────────────── 聊天列表:无闪烁滚动 + 新气泡入场动画 ───────────────────

    /// <summary>
    /// 新消息到来:仅当原本就贴着底部时才自动滚到底,且用无动画 ChangeView,
    /// 取代旧的 ScrollIntoView(带动画、会强制整段虚拟化列表重排,导致整页闪烁)。
    /// 同时把"真正的聊天气泡"登记为待入场动画;系统消息/OVER 卡片不参与(OVER 自带入场)。
    /// </summary>
    private void OnMessagesChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (e.Action == NotifyCollectionChangedAction.Reset)
        {
            _pendingBubbleAnim.Clear();
            return;
        }
        if (e.Action != NotifyCollectionChangedAction.Add || e.NewItems is null) return;

        // 此刻布局尚未纳入新条目,读到的仍是"新增前"的滚动状态,据此判断是否贴底
        bool stickToBottom = IsChatAtBottom();
        foreach (var item in e.NewItems)
            if (item is ChatMessage { IsSystem: false, IsOverCard: false } msg)
                _pendingBubbleAnim.Add(msg);

        // 待本帧布局完成后再滚,确保 ScrollableHeight 已含新条目;无动画,避免闪烁
        DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            if (stickToBottom && ChatScroll is { } sv)
                sv.ChangeView(null, sv.ScrollableHeight, null, disableAnimation: true);
        });
    }

    /// <summary>容器首次承载"待动画的新气泡"时播一次入场动画;播过即移出集合,回收/滚动回看不再重播。</summary>
    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.Item is not ChatMessage msg || !_pendingBubbleAnim.Remove(msg)) return;
        if (args.ItemContainer is not FrameworkElement fe) return;

        fe.Opacity = 0; // 先隐藏,避免容器入树瞬间以全不透明闪现一下
        // 待容器入树后再启动 Storyboard,确保动画必然生效,不会停在透明态导致气泡"消失"
        DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () => AnimateBubbleEntrance(fe));
    }

    /// <summary>是否已贴底(内容不足一屏视为贴底;留 32px 容差)。</summary>
    private bool IsChatAtBottom()
    {
        if (ChatScroll is not { } sv) return true;
        return sv.ScrollableHeight <= 0 || sv.VerticalOffset >= sv.ScrollableHeight - 32;
    }

    /// <summary>淡入 + 轻微上滑,一次性作用于新气泡容器。</summary>
    private static void AnimateBubbleEntrance(UIElement container)
    {
        if (container is not FrameworkElement fe) return;
        var slide = new TranslateTransform { Y = 16 };
        fe.RenderTransform = slide;
        fe.Opacity = 0;

        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        var duration = new Duration(TimeSpan.FromMilliseconds(240));
        var fade = new DoubleAnimation { From = 0, To = 1, Duration = duration, EasingFunction = ease };
        Storyboard.SetTarget(fade, fe);
        Storyboard.SetTargetProperty(fade, "Opacity");
        var move = new DoubleAnimation { From = 16, To = 0, Duration = duration, EasingFunction = ease };
        Storyboard.SetTarget(move, slide);
        Storyboard.SetTargetProperty(move, "Y");

        var sb = new Storyboard();
        sb.Children.Add(fade);
        sb.Children.Add(move);
        sb.Begin();
    }

    /// <summary>深度优先查找首个指定类型的可视树子节点(用于取 ListView 内置 ScrollViewer)。</summary>
    private static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject
    {
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < count; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T match) return match;
            if (FindDescendant<T>(child) is { } deeper) return deeper;
        }
        return null;
    }
}

/// <summary>按消息方向/系统类型选择气泡模板。</summary>
public class ChatTemplateSelector : DataTemplateSelector
{
    public DataTemplate? MineTemplate { get; set; }
    public DataTemplate? TheirsTemplate { get; set; }
    public DataTemplate? SystemTemplate { get; set; }
    public DataTemplate? OverCardTemplate { get; set; }

    protected override DataTemplate? SelectTemplateCore(object item, DependencyObject container) =>
        SelectTemplateCore(item);

    protected override DataTemplate? SelectTemplateCore(object item)
    {
        if (item is not ChatMessage msg) return null;
        if (msg.IsOverCard) return OverCardTemplate;
        if (msg.IsSystem) return SystemTemplate;
        return msg.IsMine ? MineTemplate : TheirsTemplate;
    }
}

/// <summary>任务栏闪烁、提示音与窗口最小尺寸所需的 Win32 调用。</summary>
internal static class NativeMethods
{
    public const uint FlashAll = 3;              // FLASHW_CAPTION | FLASHW_TRAY
    public const uint MbSimpleBeep = 0xFFFFFFFF; // 简单提示音

    public delegate IntPtr SubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam,
                                        UIntPtr uIdSubclass, UIntPtr dwRefData);

    [StructLayout(LayoutKind.Sequential)]
    public struct FLASHWINFO
    {
        public uint cbSize;
        public IntPtr hwnd;
        public uint dwFlags;
        public uint uCount;
        public uint dwTimeout;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MINMAXINFO
    {
        public POINT ptReserved;
        public POINT ptMaxSize;
        public POINT ptMaxPosition;
        public POINT ptMinTrackSize;
        public POINT ptMaxTrackSize;
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool FlashWindowEx(ref FLASHWINFO pwfi);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool MessageBeep(uint uType);

    [DllImport("user32.dll")]
    internal static extern uint GetDpiForWindow(IntPtr hwnd);

    [DllImport("comctl32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetWindowSubclass(IntPtr hwnd, SubclassProc proc, uint uIdSubclass, uint dwRefData);

    [DllImport("comctl32.dll")]
    internal static extern IntPtr DefSubclassProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
}
