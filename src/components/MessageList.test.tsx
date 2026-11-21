import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

const messageAt = (
  id: string,
  sentAtUnixMs: number | undefined,
  overrides: Partial<ChatMessage> = {},
): ChatMessage => ({
  ...baseMessage,
  id,
  uid: `uid-${id}`,
  sentAtUnixMs,
  ...overrides,
});

const peerGroupHeaders = () => screen.queryAllByText(/^对方 ·/);

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

describe('MessageList 消息分组', () => {
  it('同一发送者的相邻消息在五分钟内合并，恰好五分钟仍属于同组', () => {
    renderList([
      messageAt('m1', 0),
      messageAt('m2', 60_000),
      messageAt('m3', 360_000),
    ]);

    expect(peerGroupHeaders()).toHaveLength(1);
    expect(screen.getAllByRole('listitem')).toHaveLength(3);
    expect(document.querySelectorAll('time')).toHaveLength(3);
  });

  it('超过五分钟时开始新分组', () => {
    renderList([messageAt('m1', 0), messageAt('m2', 300_001)]);

    expect(peerGroupHeaders()).toHaveLength(2);
  });

  it('发送方向或发送者变化时开始新分组', () => {
    renderList([
      messageAt('m1', 0),
      messageAt('m2', 1_000, { kind: 'mine', sender: '我' }),
      messageAt('m3', 2_000, { sender: '另一位' }),
    ]);

    expect(peerGroupHeaders()).toHaveLength(1);
    expect(screen.getByText(/^我 ·/)).toBeInTheDocument();
    expect(screen.getByText(/^另一位 ·/)).toBeInTheDocument();
  });

  it.each(['system', 'over-card'] as const)('%s 消息会打断分组', (kind) => {
    renderList([
      messageAt('m1', 0),
      messageAt('divider', 60_000, { kind, sender: undefined, uid: undefined }),
      messageAt('m2', 120_000),
    ]);

    expect(peerGroupHeaders()).toHaveLength(2);
  });

  it('缺少时间或时间倒序时不合并消息', () => {
    const { rerender } = renderList([messageAt('m1', 0), messageAt('m2', undefined)]);
    expect(peerGroupHeaders()).toHaveLength(2);

    rerender(
      <MessageList
        messages={[messageAt('m1', 60_000), messageAt('m2', 59_999)]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );
    expect(peerGroupHeaders()).toHaveLength(2);
  });

  it('回应变化不打断分组，已有回应才渲染回应区域', () => {
    const first = messageAt('m1', 0);
    const second = messageAt('m2', 60_000);
    const { rerender } = renderList([first, second]);

    expect(screen.queryByLabelText('消息回应')).not.toBeInTheDocument();
    expect(screen.queryByRole('group', { name: '添加回应' })).not.toBeInTheDocument();

    rerender(
      <MessageList
        messages={[
          first,
          { ...second, reactions: [{ reaction: 'like', self: false }] },
        ]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );

    expect(peerGroupHeaders()).toHaveLength(1);
    expect(screen.getByLabelText('消息回应')).not.toHaveClass('absolute');
  });
});

describe('MessageList 右键回应面板', () => {
  it('默认不显示，右键对方消息后打开并显示完整回应选项', () => {
    renderList([baseMessage]);
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });

    expect(screen.queryByRole('group', { name: '添加回应' })).not.toBeInTheDocument();
    fireEvent.contextMenu(bubble);

    expect(screen.getByRole('group', { name: '添加回应' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '回应：Copy that' })).toHaveTextContent(
      '📻 COPY THAT',
    );
    expect(bubble).toHaveAttribute('aria-expanded', 'true');
  });

  it('面板使用消息气泡的坐标定位在其正上方', async () => {
    renderList([baseMessage]);
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });
    vi.spyOn(bubble, 'getBoundingClientRect').mockReturnValue({
      x: 120,
      y: 200,
      width: 160,
      height: 40,
      top: 200,
      right: 280,
      bottom: 240,
      left: 120,
      toJSON: () => ({}),
    });

    fireEvent.contextMenu(bubble);
    const panel = screen.getByRole('group', { name: '添加回应' });

    await waitFor(() => {
      expect(panel).toHaveStyle({ left: '120px', top: '192px' });
    });
  });

  it('自己的消息不打开回应面板', () => {
    renderList([{ ...baseMessage, kind: 'mine' }]);

    fireEvent.contextMenu(screen.getByText(baseMessage.content));

    expect(screen.queryByRole('group', { name: '添加回应' })).not.toBeInTheDocument();
  });

  it('Shift+F10 也可以打开回应面板', () => {
    renderList([baseMessage]);
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });

    fireEvent.keyDown(bubble, { key: 'F10', shiftKey: true });

    expect(screen.getByRole('group', { name: '添加回应' })).toBeInTheDocument();
  });

  it('完成回应后执行回调并进入关闭动画', async () => {
    const onToggleReaction = vi.fn();
    render(
      <MessageList
        messages={[baseMessage]}
        onRespondOver={() => {}}
        onToggleReaction={onToggleReaction}
      />,
    );
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });
    fireEvent.contextMenu(bubble);

    fireEvent.click(screen.getByRole('button', { name: '回应：赞' }));

    expect(onToggleReaction).toHaveBeenCalledWith('uid-1', 'like');
    await waitFor(() => expect(bubble).toHaveAttribute('aria-expanded', 'false'));
    expect(screen.getByRole('group', { name: '添加回应' })).toHaveStyle({ opacity: 0 });
  });

  it('点击外部后关闭面板', async () => {
    renderList([baseMessage]);
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });

    fireEvent.contextMenu(bubble);
    fireEvent.pointerDown(document.body);

    await waitFor(() => expect(bubble).toHaveAttribute('aria-expanded', 'false'));
  });

  it('按 Esc 后关闭面板并将焦点还给消息', async () => {
    renderList([baseMessage]);
    const bubble = screen.getByRole('group', { name: `消息：${baseMessage.content}` });

    fireEvent.contextMenu(bubble);
    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() => expect(bubble).toHaveAttribute('aria-expanded', 'false'));
    expect(bubble).toHaveFocus();
  });

  it('右键另一条消息时直接切换锚点，只保留一个面板', () => {
    const first = messageAt('m1', 0, { content: '第一条' });
    const second = messageAt('m2', 60_000, { content: '第二条' });
    renderList([first, second]);
    const firstBubble = screen.getByRole('group', { name: '消息：第一条' });
    const secondBubble = screen.getByRole('group', { name: '消息：第二条' });

    fireEvent.contextMenu(firstBubble);
    expect(firstBubble).toHaveAttribute('aria-expanded', 'true');

    fireEvent.pointerDown(secondBubble);
    fireEvent.contextMenu(secondBubble);
    expect(screen.getAllByRole('group', { name: '添加回应' })).toHaveLength(1);
    expect(firstBubble).toHaveAttribute('aria-expanded', 'false');
    expect(secondBubble).toHaveAttribute('aria-expanded', 'true');
  });
});

describe('MessageList 回应后的滚动', () => {
  const reactedMessage = {
    ...baseMessage,
    reactions: [{ reaction: 'like', self: false }],
  };

  it('贴底时回应变化后继续跟随底部', () => {
    const { rerender } = renderList([baseMessage]);
    const list = screen.getByRole('list', { name: '聊天记录' });
    Object.defineProperty(list, 'scrollHeight', { configurable: true, value: 600 });

    rerender(
      <MessageList
        messages={[reactedMessage]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );

    expect(list.scrollTop).toBe(600);
  });

  it('向上翻阅时回应变化不会强制拉回底部', () => {
    const { rerender } = renderList([baseMessage]);
    const list = screen.getByRole('list', { name: '聊天记录' });
    Object.defineProperties(list, {
      scrollHeight: { configurable: true, value: 600 },
      clientHeight: { configurable: true, value: 200 },
    });
    list.scrollTop = 0;
    fireEvent.scroll(list);

    rerender(
      <MessageList
        messages={[reactedMessage]}
        onRespondOver={() => {}}
        onToggleReaction={() => {}}
      />,
    );

    expect(list.scrollTop).toBe(0);
  });
});
