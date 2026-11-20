import { useEffect, useRef } from 'react';
import type { ChatMessage } from '../store';

interface MessageListProps {
  messages: ChatMessage[];
  onRespondOver: (accept: boolean) => void;
}

const timeFormatter = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
});

export function MessageList({ messages, onRespondOver }: MessageListProps) {
  const listRef = useRef<HTMLOListElement>(null);
  const stickToBottom = useRef(true);

  useEffect(() => {
    const list = listRef.current;
    if (list && stickToBottom.current) list.scrollTop = list.scrollHeight;
  }, [messages.length]);

  return (
    <ol
      ref={listRef}
      className="message-list"
      aria-label="聊天记录"
      aria-live="polite"
      aria-relevant="additions"
      role="list"
      onScroll={(event) => {
        const list = event.currentTarget;
        stickToBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < 32;
      }}
    >
      {messages.map((message) => {
        if (message.kind === 'system') {
          return (
            <li key={message.id} className="system-message">
              {message.content}
            </li>
          );
        }
        if (message.kind === 'over-card') {
          return (
            <li key={message.id} className="over-card">
              <strong>OVER！要结束这次通话吗？</strong>
              <span>双方都同意才会挂断</span>
              <div>
                <button type="button" className="primary-button" onClick={() => onRespondOver(true)}>
                  OVER，拜拜
                </button>
                <button type="button" className="secondary-button" onClick={() => onRespondOver(false)}>
                  不 OVER，继续聊
                </button>
              </div>
            </li>
          );
        }
        const mine = message.kind === 'mine';
        return (
          <li key={message.id} className={`chat-message ${mine ? 'mine' : 'theirs'}`}>
            <div className="message-meta">
              {message.sender} · {timeFormatter.format(message.sentAtUnixMs)}
            </div>
            <div className="message-bubble">{message.content}</div>
          </li>
        );
      })}
    </ol>
  );
}
