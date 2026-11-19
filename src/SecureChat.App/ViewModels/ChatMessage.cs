namespace SecureChat.App.ViewModels;

/// <summary>聊天区显示条目。</summary>
public class ChatMessage
{
    public string Sender { get; init; } = string.Empty;
    public string Content { get; init; } = string.Empty;
    public DateTimeOffset Time { get; init; } = DateTimeOffset.Now;
    public bool IsMine { get; init; }
    public bool IsSystem { get; init; }

    public string TimeText => Time.ToString("HH:mm:ss");
    public string Header => $"{Sender}  {TimeText}";
}
