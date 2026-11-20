import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MessageList } from './MessageList';
import type { ChatMessage } from '../store';

const baseMessage: ChatMessage = {
  id: 'm1',
  kind: 'theirs',
  sender: '对方',
  content: '收到请回答',
  sentAtUnixMs: 0,
  uid: 'uid-1',
};

const renderList = (messages: ChatMessage[]) =>
  render(
    <MessageList messages={messages} onRespondOver={() => {}} onToggleReaction={() => {}} />,
  );

describe('MessageList 印章动画', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('远端新增 copythat 回应时，消息归属方也显示印章（双方可见）', () => {
    const { rerender } = renderList([baseMessage]);
    expect(screen.queryByRole('img', { name: 'Copy that' })).not.toBeInTheDocument();

    rerender(
      <MessageList
        messages={[{ ...baseMessage, reactions: [{ reaction: 'copythat', self: false }] }]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );
    expect(screen.getByRole('img', { name: 'Copy that' })).toBeInTheDocument();
  });

  it('自己的消息被回应 copythat 时同样显示印章', () => {
    const mine: ChatMessage = { ...baseMessage, kind: 'mine' };
    const { rerender } = renderList([mine]);
    rerender(
      <MessageList
        messages={[{ ...mine, reactions: [{ reaction: 'copythat', self: false }] }]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );
    expect(screen.getByRole('img', { name: 'Copy that' })).toBeInTheDocument();
  });

  it('印章在一段时间后自动消失；取消回应不会再次触发', () => {
    const reacted = { ...baseMessage, reactions: [{ reaction: 'copythat', self: false }] };
    const { rerender } = renderList([reacted]);
    expect(screen.getByRole('img', { name: 'Copy that' })).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1500);
    });
    rerender(
      <MessageList messages={[baseMessage]} onRespondOver={() => {}} onToggleReaction={() => {}} />,
    );
    expect(screen.queryByRole('img', { name: 'Copy that' })).not.toBeInTheDocument();
  });

  it('like/heart 回应不触发印章', () => {
    const { rerender } = renderList([baseMessage]);
    rerender(
      <MessageList
        messages={[{ ...baseMessage, reactions: [{ reaction: 'like', self: false }] }]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );
    expect(screen.queryByRole('img', { name: 'Copy that' })).not.toBeInTheDocument();
  });
});
