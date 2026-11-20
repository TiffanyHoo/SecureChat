import { useEffect, useRef, useState } from 'react';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { buttonOver, buttonSecondary } from '../ui/ui';
import type { ChatMessage, ReactionKey } from '../store';

interface MessageListProps {
  messages: ChatMessage[];
  onRespondOver: (accept: boolean) => void;
  onToggleReaction: (uid: string, reaction: ReactionKey) => void;
}

const REACTION_LABELS: Record<ReactionKey, string> = {
  like: '👍',
  heart: '❤️',
  copythat: '📻 COPY THAT',
};

const REACTION_ARIA: Record<ReactionKey, string> = {
  like: '回应：赞',
  heart: '回应：爱心',
  copythat: '回应：Copy that',
};

const REACTION_ORDER: ReactionKey[] = ['like', 'heart', 'copythat'];
const STAMP_DURATION_MS = 1400;

const timeFormatter = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
});

const hasCopythat = (message: ChatMessage) =>
  (message.reactions ?? []).some((entry) => entry.reaction === 'copythat');

const hasSelfReaction = (message: ChatMessage, reaction: ReactionKey) =>
  (message.reactions ?? []).some((entry) => entry.reaction === reaction && entry.self);

function ReactionChips({ message, mine }: { message: ChatMessage; mine: boolean }) {
  const entries = message.reactions ?? [];
  if (entries.length === 0) return null;
  const counts = new Map<ReactionKey, { count: number; self: boolean }>();
  for (const entry of entries) {
    if (!(entry.reaction in REACTION_LABELS)) continue;
    const key = entry.reaction as ReactionKey;
    const current = counts.get(key) ?? { count: 0, self: false };
    counts.set(key, { count: current.count + 1, self: current.self || entry.self });
  }
  if (counts.size === 0) return null;
  return (
    <div
      className={`absolute top-full z-10 flex max-w-[min(23rem,80vw)] -translate-y-1/2 flex-wrap gap-1 ${
        mine ? 'right-0' : 'left-0'
      }`}
      aria-label="消息回应"
    >
      {REACTION_ORDER.filter((key) => counts.has(key)).map((key) => {
        const { count, self } = counts.get(key)!;
        return (
          <span
            key={key}
            className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs shadow-md ${
              self ? 'border-accent/70 bg-accent-soft text-accent' : 'border-line bg-surface text-ink'
            }`}
          >
            {REACTION_LABELS[key]}
            {count > 1 && <b className="font-semibold text-ink-muted">×{count}</b>}
          </span>
        );
      })}
    </div>
  );
}

export function MessageList({ messages, onRespondOver, onToggleReaction }: MessageListProps) {
  const listRef = useRef<HTMLOListElement>(null);
  const stickToBottom = useRef(true);
  const [stamped, setStamped] = useState<string[]>([]);
  // 记录上一帧每条消息的 copythat 状态：新增时触发印章（本地/远端一致），取消不触发
  const copythatState = useRef(new Map<string, boolean>());

  useEffect(() => {
    const list = listRef.current;
    if (list && stickToBottom.current) list.scrollTop = list.scrollHeight;
  }, [messages.length]);

  useEffect(() => {
    const current = new Map<string, boolean>();
    for (const message of messages) {
      if (message.uid === undefined) continue;
      current.set(message.uid, hasCopythat(message));
    }
    const previous = copythatState.current;
    const newlyStamped: string[] = [];
    for (const [uid, active] of current) {
      if (active && !previous.get(uid)) newlyStamped.push(uid);
    }
    copythatState.current = current;
    if (newlyStamped.length === 0) return;
    for (const uid of newlyStamped) {
      window.setTimeout(
        () => setStamped((list) => list.filter((entry) => entry !== uid)),
        STAMP_DURATION_MS,
      );
    }
    setStamped((list) => [...new Set([...list, ...newlyStamped])]);
  }, [messages]);

  const handleCopyThat = (message: ChatMessage) => {
    if (!message.uid) return;
    if (!hasSelfReaction(message, 'copythat')) {
      void writeText(message.content).catch(() => undefined);
    }
    onToggleReaction(message.uid, 'copythat');
  };

  return (
    <ol
      ref={listRef}
      className="m-0 flex min-h-0 list-none flex-col gap-2.5 overflow-y-auto px-3 py-2.5 [scrollbar-gutter:stable]"
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
            <li
              key={message.id}
              className="mx-auto max-w-[90%] px-2 py-1 text-center text-xs italic leading-relaxed text-ink-muted [overflow-wrap:anywhere]"
            >
              {message.content}
            </li>
          );
        }
        if (message.kind === 'over-card') {
          return (
            <li
              key={message.id}
              className="mx-auto grid w-[min(22rem,90%)] gap-2.5 rounded-2xl border border-over/40 bg-surface p-4 text-center shadow-lg animate-card-in"
            >
              <strong className="text-sm font-semibold text-over">OVER！要结束这次通话吗？</strong>
              <span className="text-xs text-ink-muted">双方都同意才会挂断</span>
              <div className="flex justify-center gap-2.5">
                <button type="button" className={buttonOver} onClick={() => onRespondOver(true)}>
                  OVER，拜拜
                </button>
                <button type="button" className={buttonSecondary} onClick={() => onRespondOver(false)}>
                  不 OVER，继续聊
                </button>
              </div>
            </li>
          );
        }
        const mine = message.kind === 'mine';
        const stampedNow = message.uid !== undefined && stamped.includes(message.uid);
        return (
          <li
            key={message.id}
            className={`group flex w-fit max-w-[min(23rem,80%)] flex-col animate-bubble-in ${
              mine ? 'self-end items-end' : 'self-start items-start'
            }`}
          >
            <div className="mx-2 mb-0.5 text-[0.68rem] text-ink-muted">
              {message.sender} · {timeFormatter.format(message.sentAtUnixMs ?? Date.now())}
            </div>
            <div className="relative inline-flex max-w-full">
              <div
                className={`px-3 py-2 text-sm leading-relaxed whitespace-pre-wrap select-text shadow-sm [overflow-wrap:anywhere] ${
                  mine
                    ? 'rounded-2xl rounded-tr-sm bg-gradient-to-br from-accent to-accent-strong text-white'
                    : 'rounded-2xl rounded-tl-sm border border-line bg-surface text-ink'
                }`}
              >
                {message.content}
              </div>
              {stampedNow && (
                <span
                  className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-2xl border-2 border-over bg-surface/80 font-mono text-sm font-bold tracking-[0.12em] text-over animate-radio-stamp"
                  role="img"
                  aria-label="Copy that"
                >
                  COPY THAT ✓
                </span>
              )}
              <ReactionChips message={message} mine={mine} />
            </div>
            {!mine && message.uid !== undefined && (
              <div
                className="pointer-events-none mt-4 flex translate-y-0.5 gap-1 opacity-0 transition duration-150 ease-out group-hover:pointer-events-auto group-hover:opacity-100 group-hover:translate-y-0 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-focus-within:translate-y-0"
                aria-label="添加回应"
              >
                {REACTION_ORDER.map((reaction) => {
                  const active = hasSelfReaction(message, reaction);
                  return (
                    <button
                      key={reaction}
                      type="button"
                      aria-pressed={active}
                      aria-label={REACTION_ARIA[reaction]}
                      onClick={() => {
                        if (reaction === 'copythat') handleCopyThat(message);
                        else onToggleReaction(message.uid!, reaction);
                      }}
                      className={`min-h-7 cursor-pointer rounded-full border px-2.5 py-0.5 text-xs transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus ${
                        active
                          ? 'border-accent bg-accent-soft font-medium text-accent'
                          : 'border-line bg-surface text-ink hover:border-accent/60 hover:bg-surface-muted'
                      }`}
                    >
                      {reaction === 'copythat' ? 'COPY THAT' : REACTION_LABELS[reaction]}
                    </button>
                  );
                })}
              </div>
            )}
          </li>
        );
      })}
    </ol>
  );
}
