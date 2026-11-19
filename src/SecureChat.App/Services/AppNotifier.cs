using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;

namespace SecureChat.App.Services;

/// <summary>
/// 系统 Toast 通知:仅提示"有新消息",绝不携带消息内容(内容只存在于解密后的应用内)。
/// 非打包(unpackaged)应用若无开始菜单快捷方式,系统会静默不显示 Toast——
/// 此时由任务栏闪烁 + 标题未读计数兜底,本类失败均静默降级。
/// </summary>
internal static class AppNotifier
{
    private static bool _available;

    /// <summary>应用启动时调用一次:注册通知平台并挂接点击回调(点击 Toast 恢复窗口)。</summary>
    public static void Initialize()
    {
        try
        {
            AppNotificationManager.Default.NotificationInvoked += (_, _) =>
                App.MainAppWindow?.DispatcherQueue.TryEnqueue(() => App.ShowMainWindow());
            AppNotificationManager.Default.Register();
            _available = true;
        }
        catch
        {
            _available = false;
        }
    }

    public static void Shutdown()
    {
        if (!_available) return;
        try { AppNotificationManager.Default.Unregister(); } catch { /* 退出尽力而为 */ }
    }

    /// <summary>弹"有新消息"通知。peerName 为对端显示名,通知中不含任何消息正文。</summary>
    public static void ShowNewMessage(string peerName) =>
        ShowCore("SecureChat",
            $"收到来自 {peerName} 的新消息(内容已加密,请进入应用查看)", "newmsg", "chat");

    /// <summary>非消息类系统提示(如"已最小化到托盘")。</summary>
    public static void ShowPlain(string title, string body) =>
        ShowCore(title, body, "sys", "sys");

    private static void ShowCore(string title, string body, string tag, string group)
    {
        if (!_available) return;
        try
        {
            var toast = new AppNotificationBuilder()
                .AddText(title)
                .AddText(body)
                .BuildNotification();
            toast.Tag = tag;   // 相同 Tag/Group 的新通知替换旧的,连收多条不刷屏
            toast.Group = group;
            AppNotificationManager.Default.Show(toast);
        }
        catch { /* 显示失败不阻塞消息接收 */ }
    }
}
