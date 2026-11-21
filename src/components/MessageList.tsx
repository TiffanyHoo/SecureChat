import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import {
  autoUpdate,
  FloatingPortal,
  offset,
  useDismiss,
  useFloating,
  useInteractions,
} from '@floating-ui/react';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { buttonOver, buttonSecondary } from '../ui/ui';
import { emojiMaxScale, chipExit, emojiPulse, springEnter, springPop } from '../ui/motion';
import type { ChatMessage, ReactionKey } from '../store';

interface MessageListProps {
  messages: ChatMessage[];
  onRespondOver: (accept: boolean) => void;
  onToggleReaction: (uid: string, reaction: ReactionKey) => void;
}

const REACTION_EMOJI: Record<ReactionKey, string> = {
  like: '👍',
  heart: '❤️',
  copythat: '📻',
};

const REACTION_ARIA: Record<ReactionKey, string> = {
  like: '回应：赞',
  heart: '回应：爱心',
  copythat: '回应：Copy that',
};

const REACTION_ORDER: ReactionKey[] = ['like', 'heart', 'copythat'];
const STAMP_DURATION_MS = 1400;
const GROUP_WINDOW_MS = 5 * 60 * 1000;

const timeFormatter = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
});

function isSameGroup(previous: ChatMessage | undefined, current: ChatMessage) {
  if (!previous) return false;
  if (previous.kind === 'system' || previous.kind === 'over-card') return false;
  if (current.kind === 'system' || current.kind === 'over-card') return false;
  if (previous.kind !== current.kind || previous.sender !== current.sender) return false;
  if (previous.sentAtUnixMs === undefined || current.sentAtUnixMs === undefined) return false;

  const interval = current.sentAtUnixMs - previous.sentAtUnixMs;
  return interval >= 0 && interval <= GROUP_WINDOW_MS;
}

const hasCopythat = (message: ChatMessage) =>
  (message.reactions ?? []).some((entry) => entry.reaction === 'copythat');

const hasSelfReaction = (message: ChatMessage, reaction: ReactionKey) =>
  (message.reactions ?? []).some((entry) => entry.reaction === reaction && entry.self);

interface ReactionPanelProps {
  message: ChatMessage | null;
  referenceElement: HTMLDivElement | null;
  onClose: () => void;
  onReaction: (message: ChatMessage, reaction: ReactionKey) => void;
}

function ReactionPanel({ message, referenceElement, onClose, onReaction }: ReactionPanelProps) {
  const open = message !== null;
  const { refs, floatingStyles, context } = useFloating({
    open,
    elements: { reference: referenceElement },
    onOpenChange: (nextOpen, _event, reason) => {
      if (nextOpen) return;
      onClose();
      if (reason === 'escape-key') referenceElement?.focus();
    },
    placement: 'top-start',
    strategy: 'fixed',
    transform: false,
    middleware: [offset(8)],
    whileElementsMounted: autoUpdate,
  });
  const dismiss = useDismiss(context, { enabled: open });
  const { getFloatingProps } = useInteractions([dismiss]);

  return (
    <FloatingPortal>
      <AnimatePresence>
        {message && (
          <motion.div
            key="message-reaction-panel"
            id="message-reaction-panel"
            ref={refs.setFloating}
            style={{ ...floatingStyles, transformOrigin: 'top left' }}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={springPop}
            role="group"
            aria-label="添加回应"
            className="z-50 flex origin-top-left gap-0.5 rounded-full border border-line bg-surface p-0.5 shadow-lg"
            {...getFloatingProps()}
          >
            {REACTION_ORDER.map((reaction) => {
              const active = hasSelfReaction(message, reaction);
              return (
                <button
                  key={reaction}
                  type="button"
                  autoFocus={reaction === 'like'}
                  aria-pressed={active}
                  aria-label={REACTION_ARIA[reaction]}
                  title={REACTION_ARIA[reaction]}
                  onClick={() => onReaction(message, reaction)}
                  className={`min-h-7 min-w-7 cursor-pointer rounded-full border px-1.5 py-0.5 text-xs whitespace-nowrap motion-safe:transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus ${
                    active
                      ? 'border-accent bg-accent-soft font-medium text-accent'
                      : 'border-line bg-surface text-ink hover:border-accent/60 hover:bg-surface-muted'
                  }`}
                >
                  <span aria-hidden="true">{REACTION_EMOJI[reaction]}</span>
                </button>
              );
            })}
          </motion.div>
        )}
      </AnimatePresence>
    </FloatingPortal>
  );
}

function ReactionChip({
  reactionKey,
  count,
  self,
}: {
  reactionKey: ReactionKey;
  count: number;
  self: boolean;
}) {
  // 仅在计数增加时触发表情跳动，取消（计数递减）时保持原样不跳动
  const prevCount = useRef(0);
  const increments = useRef(0);
  if (count > prevCount.current) increments.current += 1;
  prevCount.current = count;
  return (
    // layout="position" 只补间位置不碰 scale，避免与下方缩放动画冲突
    <motion.span
      layout="position"
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, y: 4, transition: chipExit }}
      transition={springPop}
      className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs shadow-md ${
        self ? 'border-accent/70 bg-accent-soft text-accent' : 'border-line bg-surface text-ink'
      }`}
    >
      <motion.span
        key={`pulse-${increments.current}`}
        initial={{ scale: 1, y: 0 }}
        animate={{ scale: [1, emojiMaxScale, 1], y: [0, -2, 0] }}
        transition={emojiPulse}
        className="inline-block"
      >
        {REACTION_EMOJI[reactionKey]}
      </motion.span>
      {count > 1 && <b className="inline-block font-semibold text-ink-muted">×{count}</b>}
    </motion.span>
  );
}

function ReactionChips({ message, mine }: { message: ChatMessage; mine: boolean }) {
  const entries = message.reactions ?? [];
  if (entries.length === 0) return null;
  const counts = new Map<ReactionKey, { count: number; self: boolean }>();
  for (const entry of entries) {
    if (!(entry.reaction in REACTION_EMOJI)) continue;
    const key = entry.reaction as ReactionKey;
    const current = counts.get(key) ?? { count: 0, self: false };
    counts.set(key, { count: current.count + 1, self: current.self || entry.self });
  }
  if (counts.size === 0) return null;
  return (
    <div
      className={`mt-1 flex max-w-[min(23rem,80vw)] flex-wrap gap-1 ${
        mine ? 'justify-end' : 'justify-start'
      }`}
      aria-label="消息回应"
    >
      {/* popLayout 让退出元素立即脱离文档流，补位重排与进出场动画同时进行 */}
      <AnimatePresence mode="popLayout">
        {REACTION_ORDER.filter((key) => counts.has(key)).map((key) => (
          <ReactionChip
            key={key}
            reactionKey={key}
            count={counts.get(key)!.count}
            self={counts.get(key)!.self}
          />
        ))}
      </AnimatePresence>
    </div>
  );
}

export function MessageList({ messages, onRespondOver, onToggleReaction }: MessageListProps) {
  const listRef = useRef<HTMLOListElement>(null);
  const bubbleRefs = useRef(new Map<string, HTMLDivElement>());
  const stickToBottom = useRef(true);
  const [stamped, setStamped] = useState<string[]>([]);
  const [activeReactionId, setActiveReactionId] = useState<string | null>(null);
  // 记录上一帧每条消息的 copythat 状态：新增时触发印章（本地/远端一致），取消不触发
  const copythatState = useRef(new Map<string, boolean>());
  const activeReactionMessage =
    messages.find(
      (message) =>
        message.id === activeReactionId && message.kind === 'theirs' && message.uid !== undefined,
    ) ?? null;
  const activeReferenceElement = activeReactionId
    ? (bubbleRefs.current.get(activeReactionId) ?? null)
    : null;

  useEffect(() => {
    const list = listRef.current;
    if (list && stickToBottom.current) list.scrollTop = list.scrollHeight;
  }, [messages]);

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

  useEffect(() => {
    if (activeReactionId && !activeReactionMessage) setActiveReactionId(null);
  }, [activeReactionId, activeReactionMessage]);

  const handleCopyThat = (message: ChatMessage) => {
    if (!message.uid) return;
    if (!hasSelfReaction(message, 'copythat')) {
      void writeText(message.content).catch(() => undefined);
    }
    onToggleReaction(message.uid, 'copythat');
  };

  const handleReaction = (message: ChatMessage, reaction: ReactionKey) => {
    if (reaction === 'copythat') handleCopyThat(message);
    else if (message.uid) onToggleReaction(message.uid, reaction);
    setActiveReactionId(null);
    bubbleRefs.current.get(message.id)?.focus();
  };

  return (
    <>
      <ol
        ref={listRef}
        className="m-0 flex min-h-0 list-none flex-col overflow-x-clip overflow-y-auto px-3 pb-2.5 pt-4 [scrollbar-gutter:stable] [scroll-behavior:smooth]"
        aria-label="聊天记录"
        aria-live="polite"
        aria-relevant="additions"
        role="list"
        onScroll={(event) => {
          const list = event.currentTarget;
          stickToBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < 32;
        }}
      >
      {messages.map((message, index) => {
        if (message.kind === 'system') {
          return (
            <motion.li
              key={message.id}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="mx-auto mt-4 max-w-[90%] px-2 py-1 text-center text-xs italic leading-relaxed text-ink-muted [overflow-wrap:anywhere] first:mt-0"
            >
              {message.content}
            </motion.li>
          );
        }
        if (message.kind === 'over-card') {
          return (
            <motion.li
              key={message.id}
              initial={{ opacity: 0, y: 18, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              transition={springEnter}
              className="mx-auto mt-4 grid w-[min(22rem,90%)] gap-2.5 rounded-2xl border border-over/40 bg-surface p-4 text-center shadow-lg first:mt-0"
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
            </motion.li>
          );
        }
        const mine = message.kind === 'mine';
        const isGroupStart = !isSameGroup(messages[index - 1], message);
        const sentAt = message.sentAtUnixMs ?? Date.now();
        const formattedTime = timeFormatter.format(sentAt);
        const stampedNow = message.uid !== undefined && stamped.includes(message.uid);
        return (
          <motion.li
            key={message.id}
            initial={{ opacity: 0, y: -14, x: mine ? 10 : -10, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, x: 0, scale: 1 }}
            transition={springEnter}
            className={`group relative flex w-full flex-col ${
              isGroupStart ? 'mt-4' : 'mt-1.5'
            } ${mine ? 'items-end' : 'items-start'} first:mt-0`}
          >
            {isGroupStart && (
              <div className="mx-2 mb-0.5 text-[0.68rem] text-ink-muted">
                {message.sender} ·{' '}
                <time dateTime={new Date(sentAt).toISOString()}>{formattedTime}</time>
              </div>
            )}
            <div
              ref={(element) => {
                if (element) bubbleRefs.current.set(message.id, element);
                else bubbleRefs.current.delete(message.id);
              }}
              tabIndex={!mine && message.uid ? 0 : undefined}
              role={!mine && message.uid ? 'group' : undefined}
              aria-label={!mine && message.uid ? `消息：${message.content}` : undefined}
              aria-keyshortcuts={!mine && message.uid ? 'Shift+F10' : undefined}
              aria-expanded={!mine && message.uid ? activeReactionId === message.id : undefined}
              aria-controls={activeReactionId === message.id ? 'message-reaction-panel' : undefined}
              onContextMenu={(event) => {
                if (mine || !message.uid) return;
                event.preventDefault();
                setActiveReactionId(message.id);
              }}
              onKeyDown={(event) => {
                if (mine || !message.uid) return;
                if (event.key !== 'ContextMenu' && !(event.key === 'F10' && event.shiftKey)) return;
                event.preventDefault();
                setActiveReactionId(message.id);
              }}
              className="relative inline-flex max-w-[min(23rem,80%)] rounded-2xl focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
            >
              <div
                className={`px-3 py-2 text-sm leading-relaxed whitespace-pre-wrap select-text shadow-sm [overflow-wrap:anywhere] ${
                  mine
                    ? 'rounded-2xl rounded-tr-sm bg-gradient-to-br from-accent to-accent-strong text-white'
                    : 'rounded-2xl rounded-tl-sm border border-line bg-surface text-ink'
                }`}
              >
                {message.content}
              </div>
              {!isGroupStart && (
                <time
                  dateTime={new Date(sentAt).toISOString()}
                  className={`pointer-events-none absolute top-1/2 -translate-y-1/2 text-[0.68rem] whitespace-nowrap text-ink-muted opacity-0 motion-safe:transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 [@media(hover:none)]:opacity-100 ${
                    mine ? 'right-full mr-2' : 'left-full ml-2'
                  }`}
                >
                  {formattedTime}
                </time>
              )}
              {stampedNow && (
                <motion.span
                  initial={{ opacity: 0, scale: 2.4, rotate: -10 }}
                  animate={{
                    opacity: [0, 1, 1, 0],
                    scale: [2.4, 1, 1.08, 1],
                    rotate: [-10, -3, -1.8, -3],
                  }}
                  transition={{
                    duration: STAMP_DURATION_MS / 1000,
                    times: [0, 0.18, 0.3, 1],
                    ease: 'easeOut',
                  }}
                  className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-2xl border-2 border-over bg-surface/80 font-mono text-sm font-bold tracking-[0.12em] text-over"
                  role="img"
                  aria-label="Copy that"
                >
                  COPY THAT ✓
                </motion.span>
              )}
            </div>
            <ReactionChips message={message} mine={mine} />
          </motion.li>
        );
      })}
      </ol>
      <ReactionPanel
        message={activeReactionMessage}
        referenceElement={activeReferenceElement}
        onClose={() => setActiveReactionId(null)}
        onReaction={handleReaction}
      />
    </>
  );
}
