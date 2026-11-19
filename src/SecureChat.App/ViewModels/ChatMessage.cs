namespace SecureChat.App.ViewModels;

/// <summary>聊天区显示条目。</summary>
public class ChatMessage
{
    public string Sender { get; init; } = string.Empty;
    public string Content { get; init; } = string.Empty;
    public DateTimeOffset Time { get; init; } = DateTimeOffset.Now;
    public bool IsMine { get; init; }
    public bool IsSystem { get; init; }
    /// <summary>OVER 互动卡片(接收方确认是否挂断),由专属模板渲染。</summary>
    public bool IsOverCard { get; init; }

    public string TimeText => Time.ToString("HH:mm:ss");
    public string Header => $"{Sender}  {TimeText}";
}
