import type { ConnectionState, SessionEvent, SessionSnapshot } from './types';

export type OverPhase = 'idle' | 'waiting' | 'peer-asking';
export type MessageKind = 'mine' | 'theirs' | 'system' | 'over-card';

export type ReactionKey = 'like' | 'heart' | 'copythat';

export const REACTION_KEYS: ReactionKey[] = ['like', 'heart', 'copythat'];

export interface MessageReaction {
  reaction: string;
  self: boolean;
}

export interface ChatMessage {
  id: string;
  kind: MessageKind;
  sender?: string;
  content: string;
  sentAtUnixMs?: number;
  uid?: string;
  reactions?: MessageReaction[];
}

export interface SessionState {
  sessionId: number | null;
  lastSeq: number;
  localSeq: number;
  connectionState: ConnectionState;
  status: string;
  peerName: string;
  epoch: number;
  fingerprint: string;
  nextRotationUnixMs: number | null;
  generatedCode: string;
  messages: ChatMessage[];
  overPhase: OverPhase;
  overEnding: boolean;
  rekeying: boolean;
}

export const initialSessionState: SessionState = {
  sessionId: null,
  lastSeq: 0,
  localSeq: 0,
  connectionState: 'idle',
  status: '未连接',
  peerName: '',
  epoch: 0,
  fingerprint: '',
  nextRotationUnixMs: null,
  generatedCode: '',
  messages: [],
  overPhase: 'idle',
  overEnding: false,
  rekeying: false,
};

export type SessionAction =
  | {
      type: 'session-started';
      sessionId: number;
      status: string;
      systemMessage: string;
      roomCode?: string;
    }
  | { type: 'event'; event: SessionEvent }
  | { type: 'snapshot'; snapshot: SessionSnapshot }
  | { type: 'local-message'; sender: string; content: string; uid: string }
  | { type: 'command-error'; message: string; status?: string }
  | { type: 'over-requested' }
  | { type: 'over-cancelled' }
  | { type: 'over-failed' }
  | { type: 'over-rejected' }
  | { type: 'over-finish'; reason: string }
  | { type: 'over-complete' }
  | { type: 'rekeying'; value: boolean }
  | { type: 'copied'; ok: boolean }
  | { type: 'toggle-reaction'; uid: string; reaction: string }
  | { type: 'local-disconnected' };

const toggleReaction = (messages: ChatMessage[], uid: string, reaction: string, self: boolean) =>
  messages.map((message) => {
    if (message.uid !== uid) return message;
    const reactions = message.reactions ?? [];
    const index = reactions.findIndex((entry) => entry.reaction === reaction && entry.self === self);
    return {
      ...message,
      reactions:
        index >= 0
          ? reactions.filter((_, at) => at !== index)
          : [...reactions, { reaction, self }],
    };
  });

const withSystem = (state: SessionState, content: string): SessionState => {
  const localSeq = state.localSeq + 1;
  return {
    ...state,
    localSeq,
    messages: [
      ...state.messages,
      { id: `system-${state.sessionId ?? 0}-${localSeq}`, kind: 'system', content },
    ],
  };
};

const withoutOverCards = (messages: ChatMessage[]) =>
  messages.filter((message) => message.kind !== 'over-card');

const applySnapshot = (state: SessionState, snapshot: SessionSnapshot): SessionState => {
  if (snapshot.sessionId === null) return state;
  if (state.sessionId !== null && snapshot.sessionId !== state.sessionId) return state;
  if (state.sessionId === snapshot.sessionId && snapshot.seq < state.lastSeq) return state;
  const base = state.sessionId === null
    ? { ...initialSessionState, sessionId: snapshot.sessionId }
    : state;
  const next = {
    ...base,
    lastSeq: snapshot.seq,
    connectionState: snapshot.state,
    status: snapshot.status,
    peerName: snapshot.peerName,
    epoch: snapshot.epoch,
    fingerprint: snapshot.fingerprint,
    nextRotationUnixMs: snapshot.nextRotationUnixMs,
  };
  return snapshot.state === 'established' && state.connectionState !== 'established'
    ? withSystem(next, '握手完成，通信已加密。对方名称不会在线路上明文出现。')
    : next;
};

const applyEvent = (state: SessionState, event: SessionEvent): SessionState => {
  if (event.sessionId !== state.sessionId || event.seq <= state.lastSeq) return state;
  const current = { ...state, lastSeq: event.seq };

  switch (event.kind) {
    case 'state': {
      const payload = event.payload;
      const next: SessionState = {
        ...current,
        connectionState: payload.state,
        status: payload.status,
        epoch: payload.epoch,
        fingerprint: payload.fingerprint,
        nextRotationUnixMs: payload.nextRotationUnixMs,
      };
      return payload.state === 'established' && state.connectionState !== 'established'
        ? withSystem(next, '握手完成，通信已加密。对方名称不会在线路上明文出现。')
        : next;
    }
    case 'message':
      return {
        ...current,
        messages: [
          ...current.messages,
          {
            id: `remote-${event.sessionId}-${event.seq}`,
            kind: 'theirs',
            sender: event.payload.sender,
            content: event.payload.content,
            sentAtUnixMs: event.payload.sentAtUnixMs,
            uid: event.payload.uid,
          },
        ],
      };
    case 'reaction':
      return {
        ...current,
        messages: toggleReaction(current.messages, event.payload.uid, event.payload.reaction, false),
      };
    case 'peer-name':
      return { ...current, peerName: event.payload.name };
    case 'key-rotated': {
      const next = {
        ...current,
        epoch: event.payload.epoch,
        fingerprint: event.payload.fingerprint,
        nextRotationUnixMs: event.payload.nextRotationUnixMs,
      };
      return event.payload.epoch > state.epoch && event.payload.epoch > 0
        ? withSystem(
            next,
            `会话密钥已轮换 → 纪元 ${event.payload.epoch}，新指纹 ${event.payload.fingerprint}`,
          )
        : next;
    }
    case 'over':
      switch (event.payload.kind) {
        case 'request':
          if (state.overPhase !== 'idle') return current;
          return {
            ...current,
            overPhase: 'peer-asking',
            messages: [
              ...current.messages,
              {
                id: `over-${event.sessionId}-${event.seq}`,
                kind: 'over-card',
                content: '对方按下了 OVER，要结束这次通话吗？',
              },
            ],
          };
        case 'reject':
          return withSystem(
            { ...current, overPhase: 'idle', messages: withoutOverCards(current.messages) },
            '对方选择了继续聊天。',
          );
        case 'cancel':
          return withSystem(
            { ...current, overPhase: 'idle', messages: withoutOverCards(current.messages) },
            '对方撤回了 OVER，继续聊天。',
          );
        case 'accept':
          return current;
      }
    case 'disconnected':
      return withSystem(
        {
          ...current,
          connectionState: 'closed',
          status: `已断开：${event.payload.reason}`,
          nextRotationUnixMs: null,
          generatedCode: '',
          overPhase: 'idle',
          overEnding: false,
          rekeying: false,
          messages: withoutOverCards(current.messages),
        },
        '连接已断开。',
      );
  }
};

export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case 'session-started': {
      const next: SessionState = {
        ...initialSessionState,
        sessionId: action.sessionId,
        connectionState: 'connecting',
        status: action.status,
        generatedCode: action.roomCode ?? '',
      };
      return withSystem(next, action.systemMessage);
    }
    case 'event':
      return applyEvent(state, action.event);
    case 'snapshot':
      return applySnapshot(state, action.snapshot);
    case 'local-message': {
      const localSeq = state.localSeq + 1;
      return {
        ...state,
        localSeq,
        messages: [
          ...state.messages,
          {
            id: `local-${state.sessionId ?? 0}-${localSeq}`,
            kind: 'mine',
            sender: action.sender,
            content: action.content,
            sentAtUnixMs: Date.now(),
            uid: action.uid,
          },
        ],
      };
    }
    case 'toggle-reaction':
      return {
        ...state,
        messages: toggleReaction(state.messages, action.uid, action.reaction, true),
      };
    case 'command-error':
      return withSystem({ ...state, ...(action.status ? { status: action.status } : {}) }, action.message);
    case 'over-requested':
      return withSystem({ ...state, overPhase: 'waiting' }, '你按下了 OVER，等待对方回应…');
    case 'over-cancelled':
      return withSystem({ ...state, overPhase: 'idle' }, '你撤回了 OVER，继续聊天。');
    case 'over-failed':
      return { ...state, overPhase: 'idle' };
    case 'over-rejected':
      return withSystem(
        { ...state, overPhase: 'idle', messages: withoutOverCards(state.messages) },
        '你选择了继续聊天。',
      );
    case 'over-finish':
      return withSystem(
        {
          ...state,
          overPhase: 'idle',
          overEnding: true,
          status: '通话已结束（OVER）',
          messages: withoutOverCards(state.messages),
        },
        `OVER & OUT · ${action.reason}，通话结束，回见！`,
      );
    case 'over-complete':
      return { ...state, messages: [], generatedCode: '' };
    case 'rekeying':
      return { ...state, rekeying: action.value };
    case 'copied':
      return {
        ...state,
        status: action.ok
          ? '房间码已复制，通过你信任的渠道发给对方即可'
          : '复制失败，请手动选中房间码复制',
      };
    case 'local-disconnected':
      return {
        ...state,
        connectionState: 'closed',
        status: '已断开：本地主动断开',
        generatedCode: '',
        nextRotationUnixMs: null,
        overPhase: 'idle',
        overEnding: false,
      };
  }
}

export const isConnected = (state: SessionState) => state.connectionState === 'established';
export const isBusy = (state: SessionState) =>
  state.connectionState === 'listening' ||
  state.connectionState === 'connecting' ||
  state.connectionState === 'handshaking';

export function formatCountdown(nextRotationUnixMs: number | null, now: number): string {
  if (nextRotationUnixMs === null) return '--:--:--';
  const total = Math.max(0, Math.ceil((nextRotationUnixMs - now) / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return [hours, minutes, seconds].map((part) => String(part).padStart(2, '0')).join(':');
}
