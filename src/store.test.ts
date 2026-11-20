import { describe, expect, it } from 'vitest';
import {
  formatCountdown,
  initialSessionState,
  sessionReducer,
  type SessionState,
} from './store';
import type { SessionEvent } from './types';

const started = () =>
  sessionReducer(initialSessionState, {
    type: 'session-started',
    sessionId: 7,
    status: '正在连接',
    systemMessage: '开始连接',
  });

const event = (seq: number, kind: SessionEvent['kind'], payload: never): SessionEvent =>
  ({ sessionId: 7, seq, kind, payload }) as SessionEvent;

describe('sessionReducer', () => {
  it('忽略旧会话和重复序号事件', () => {
    const state = started();
    const oldSession = {
      sessionId: 6,
      seq: 1,
      kind: 'message',
      payload: { sender: '旧连接', content: '不应出现', sentAtUnixMs: 1 },
    } as const;
    expect(sessionReducer(state, { type: 'event', event: oldSession })).toBe(state);

    const message = event(2, 'message', {
      sender: '对方',
      content: '你好',
      sentAtUnixMs: 1,
    } as never);
    const received = sessionReducer(state, { type: 'event', event: message });
    expect(received.messages.at(-1)?.content).toBe('你好');
    expect(sessionReducer(received, { type: 'event', event: message })).toBe(received);
  });

  it('建立连接时应用密钥快照且只添加一次系统消息', () => {
    const state = started();
    const payload = {
      state: 'established',
      status: '安全会话已建立',
      epoch: 0,
      fingerprint: 'abc123',
      nextRotationUnixMs: 1000,
    } as const;
    const connected = sessionReducer(state, {
      type: 'event',
      event: event(1, 'state', payload as never),
    });
    expect(connected.connectionState).toBe('established');
    expect(connected.fingerprint).toBe('abc123');
    expect(connected.messages).toHaveLength(2);

    const snapshot = sessionReducer(connected, {
      type: 'snapshot',
      snapshot: { sessionId: 7, seq: 1, peerName: '', ...payload },
    });
    expect(snapshot.messages).toHaveLength(2);
  });

  it('收到 OVER 请求时插入卡片，撤回时移除', () => {
    const state: SessionState = {
      ...started(),
      connectionState: 'established',
    };
    const asking = sessionReducer(state, {
      type: 'event',
      event: event(1, 'over', { kind: 'request' } as never),
    });
    expect(asking.overPhase).toBe('peer-asking');
    expect(asking.messages.some((message) => message.kind === 'over-card')).toBe(true);

    const cancelled = sessionReducer(asking, {
      type: 'event',
      event: event(2, 'over', { kind: 'cancel' } as never),
    });
    expect(cancelled.overPhase).toBe('idle');
    expect(cancelled.messages.some((message) => message.kind === 'over-card')).toBe(false);
  });

  it('新会话清空消息、房间码和 OVER 状态', () => {
    const state: SessionState = {
      ...started(),
      generatedCode: 'SC1-secret',
      overPhase: 'waiting',
      overEnding: true,
    };
    const next = sessionReducer(state, {
      type: 'session-started',
      sessionId: 8,
      status: '新会话',
      systemMessage: '重新开始',
    });
    expect(next.sessionId).toBe(8);
    expect(next.generatedCode).toBe('');
    expect(next.overPhase).toBe('idle');
    expect(next.overEnding).toBe(false);
    expect(next.messages).toHaveLength(1);
  });
});

describe('formatCountdown', () => {
  it('向上取整并 clamp 到零', () => {
    expect(formatCountdown(11_001, 10_000)).toBe('00:00:02');
    expect(formatCountdown(9_000, 10_000)).toBe('00:00:00');
    expect(formatCountdown(null, 10_000)).toBe('--:--:--');
  });
});
