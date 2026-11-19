using Microsoft.UI.Xaml;

namespace SecureChat.App;

public partial class App : Application
{
    public static Window? MainAppWindow { get; private set; }

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(Microsoft.UI.Xaml.LaunchActivatedEventArgs args)
    {
        Services.AppNotifier.Initialize();
        MainAppWindow = new MainWindow();
        MainAppWindow.Activate();
        Services.TrayIcon.Initialize(); // 托盘驻留:叉掉窗口后台继续收消息
    }

    /// <summary>从托盘/Toast 通知恢复主窗口(隐藏状态下也能带回前台)。</summary>
    public static void ShowMainWindow()
    {
        var w = MainAppWindow;
        if (w is null) return;
        w.AppWindow.Show(false);
        w.Activate();
    }

    /// <summary>托盘"退出":真正结束程序(区别于叉掉=最小化到托盘)。</summary>
    public static void ExitApp() => (MainAppWindow as MainWindow)?.RequestExit();
}
