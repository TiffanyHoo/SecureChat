using System.Runtime.InteropServices;

namespace SecureChat.App.Services;

/// <summary>
/// 系统托盘图标:窗口叉掉后最小化到托盘,程序后台继续接收加密消息并弹通知。
/// 双击托盘 = 恢复窗口;右键 = 显示 / 退出。基于 Shell_NotifyIcon P/Invoke(WinUI 3 无内置托盘)。
/// </summary>
internal static class TrayIcon
{
    private const uint TrayId = 1;
    private const uint WmTrayCallback = 0x8001;   // WM_APP + 1
    private const uint NimAdd = 0, NimDelete = 2;
    private const uint NifMessage = 0x1, NifIcon = 0x2, NifTip = 0x4;
    private const uint WmLbuttondblclk = 0x203, WmRbuttonup = 0x205;
    private const uint IdShow = 1, IdExit = 2;
    private const uint MfSeparator = 0x800;
    private const uint TpmReturncmd = 0x0100, TpmRightbutton = 0x0002, TpmBottomalign = 0x0020;

    private static uint _wmTaskbarCreated;
    private static IntPtr _msgHwnd;
    private static Native.WndProc? _wndProc;     // 静态字段保活,防止委托被 GC
    private static bool _created;

    /// <summary>托盘是否可用。不可用时窗口叉掉应正常退出(避免程序困在后台无入口)。</summary>
    public static bool Available => _msgHwnd != IntPtr.Zero;

    public static void Initialize()
    {
        if (_msgHwnd != IntPtr.Zero) return;
        _wmTaskbarCreated = Native.RegisterWindowMessageW("TaskbarCreated");
        _wndProc = WndProc;
        var wc = new Native.WNDCLASSW
        {
            lpfnWndProc = _wndProc,
            hInstance = Native.GetModuleHandleW(null),
            lpszClassName = Marshal.StringToHGlobalUni("SecureChatTrayWnd"),
        };
        if (Native.RegisterClassW(ref wc) == 0) return;
        // message-only 窗口:仅接收托盘回调,不可见
        _msgHwnd = Native.CreateWindowExW(0, "SecureChatTrayWnd", string.Empty, 0,
            0, 0, 0, 0, new IntPtr(-3) /*HWND_MESSAGE*/, IntPtr.Zero,
            Native.GetModuleHandleW(null), IntPtr.Zero);
        Add();
    }

    public static void Remove()
    {
        if (!_created) return;
        _created = false;
        var nid = BuildNid();
        Native.Shell_NotifyIconW(NimDelete, ref nid);
    }

    private static void Add()
    {
        var nid = BuildNid();
        _created = Native.Shell_NotifyIconW(NimAdd, ref nid);
    }

    private static Native.NOTIFYICONDATAW BuildNid() => new()
    {
        cbSize = (uint)Marshal.SizeOf<Native.NOTIFYICONDATAW>(),
        hWnd = _msgHwnd,
        uID = TrayId,
        uFlags = NifMessage | NifIcon | NifTip,
        uCallbackMessage = WmTrayCallback,
        hIcon = LoadAppIcon(),
        szTip = "SecureChat:双击显示窗口,右键可退出",
        szInfo = string.Empty,
        szInfoTitle = string.Empty,
    };

    private static IntPtr _appIcon;

    /// <summary>加载应用图标(小尺寸,适配托盘);文件缺失时回退系统默认,保证托盘不空白。</summary>
    private static IntPtr LoadAppIcon()
    {
        if (_appIcon != IntPtr.Zero) return _appIcon;
        int size = Native.GetSystemMetrics(49); // SM_CXSMICON
        _appIcon = Native.LoadImageW(IntPtr.Zero, "Assets/app.ico", 1 /*IMAGE_ICON*/, size, size, 0x0010 /*LR_LOADFROMFILE*/);
        if (_appIcon == IntPtr.Zero)
            _appIcon = Native.LoadIconW(IntPtr.Zero, (IntPtr)32512 /*IDI_APPLICATION*/);
        return _appIcon;
    }

    private static IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        // explorer 重启后托盘图标丢失,重新注册
        if (msg == _wmTaskbarCreated && _created)
        {
            _created = false;
            Add();
            return IntPtr.Zero;
        }
        if (msg == WmTrayCallback)
        {
            switch ((uint)lParam)
            {
                case WmLbuttondblclk: App.ShowMainWindow(); break;
                case WmRbuttonup: ShowContextMenu(); break;
            }
            return IntPtr.Zero;
        }
        return Native.DefWindowProcW(hwnd, msg, wParam, lParam);
    }

    private static void ShowContextMenu()
    {
        Native.SetForegroundWindow(_msgHwnd); // 让菜单在失焦时自动关闭
        IntPtr menu = Native.CreatePopupMenu();
        Native.AppendMenuW(menu, 0, (IntPtr)IdShow, "显示 SecureChat");
        Native.AppendMenuW(menu, MfSeparator, IntPtr.Zero, null);
        Native.AppendMenuW(menu, 0, (IntPtr)IdExit, "退出 SecureChat");
        Native.GetCursorPos(out var pt);
        uint cmd = Native.TrackPopupMenu(menu, TpmReturncmd | TpmRightbutton | TpmBottomalign,
            pt.X, pt.Y, 0, _msgHwnd, IntPtr.Zero);
        Native.PostMessageW(_msgHwnd, 0 /*WM_NULL*/, IntPtr.Zero, IntPtr.Zero);
        Native.DestroyMenu(menu);
        if (cmd == IdShow) App.ShowMainWindow();
        else if (cmd == IdExit) App.ExitApp();
    }

    private static class Native
    {
        internal delegate IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

        [StructLayout(LayoutKind.Sequential)]
        internal struct WNDCLASSW
        {
            public uint style;
            public WndProc lpfnWndProc;
            public int cbClsExtra;
            public int cbWndExtra;
            public IntPtr hInstance;
            public IntPtr hIcon;
            public IntPtr hCursor;
            public IntPtr hbrBackground;
            public IntPtr lpszMenuName;
            public IntPtr lpszClassName;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct POINT
        {
            public int X;
            public int Y;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        internal struct NOTIFYICONDATAW
        {
            public uint cbSize;
            public IntPtr hWnd;
            public uint uID;
            public uint uFlags;
            public uint uCallbackMessage;
            public IntPtr hIcon;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
            public uint dwState;
            public uint dwStateMask;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
            public uint uVersion;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
            public uint dwInfoFlags;
            public Guid guidItem;
            public IntPtr hBalloonIcon;
        }

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        internal static extern ushort RegisterClassW(ref WNDCLASSW lpWndClass);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        internal static extern uint RegisterWindowMessageW(string lpString);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        internal static extern IntPtr CreateWindowExW(uint dwExStyle, string lpClassName,
            string lpWindowName, uint dwStyle, int x, int y, int nWidth, int nHeight,
            IntPtr hWndParent, IntPtr hMenu, IntPtr hInstance, IntPtr lpParam);

        [DllImport("user32.dll")]
        internal static extern IntPtr DefWindowProcW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

        [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool Shell_NotifyIconW(uint dwMessage, ref NOTIFYICONDATAW lpData);

        [DllImport("user32.dll")]
        internal static extern IntPtr LoadIconW(IntPtr hInstance, IntPtr lpIconName);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        internal static extern IntPtr LoadImageW(IntPtr hInst, string lpszName, uint uType, int cx, int cy, uint fuLoad);

        [DllImport("user32.dll")]
        internal static extern int GetSystemMetrics(int nIndex);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetForegroundWindow(IntPtr hWnd);

        [DllImport("user32.dll")]
        internal static extern IntPtr CreatePopupMenu();

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool AppendMenuW(IntPtr hMenu, uint uFlags, IntPtr uIDNewItem, string? lpNewItem);

        [DllImport("user32.dll")]
        internal static extern uint TrackPopupMenu(IntPtr hMenu, uint uFlags, int x, int y,
            int nReserved, IntPtr hWnd, IntPtr prcRect);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool DestroyMenu(IntPtr hMenu);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("user32.dll")]
        internal static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        internal static extern IntPtr GetModuleHandleW(string? lpModuleName);
    }
}
