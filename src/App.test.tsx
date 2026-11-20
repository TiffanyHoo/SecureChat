import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import type { SessionEvent, SessionSnapshot } from './types';

const mocks = vi.hoisted(() => ({
  createRoom: vi.fn(),
  joinRoom: vi.fn(),
  disconnect: vi.fn(),
  sendMessage: vi.fn(),
  sendReaction: vi.fn(),
  rekeyNow: vi.fn(),
  sendOver: vi.fn(),
  eventHandler: undefined as ((event: SessionEvent) => void) | undefined,
  onSessionEvent: vi.fn(),
  getSnapshot: vi.fn(),
  listHostIps: vi.fn(),
  writeText: vi.fn(),
}));

vi.mock('./api', () => ({
  createRoom: mocks.createRoom,
  joinRoom: mocks.joinRoom,
  disconnect: mocks.disconnect,
  sendMessage: mocks.sendMessage,
  sendReaction: mocks.sendReaction,
  rekeyNow: mocks.rekeyNow,
  sendOver: mocks.sendOver,
  onSessionEvent: mocks.onSessionEvent,
  getSnapshot: mocks.getSnapshot,
  listHostIps: mocks.listHostIps,
}));

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: mocks.writeText }));

const emptySnapshot: SessionSnapshot = {
  sessionId: null,
  seq: 0,
  state: 'idle',
  status: '未连接',
  peerName: '',
  epoch: 0,
  fingerprint: '',
  nextRotationUnixMs: null,
};

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.eventHandler = undefined;
    mocks.onSessionEvent.mockImplementation((handler) => {
      mocks.eventHandler = handler;
      return Promise.resolve(vi.fn());
    });
    mocks.getSnapshot.mockResolvedValue(emptySnapshot);
    mocks.listHostIps.mockResolvedValue(['127.0.0.1', '192.168.1.8']);
    mocks.createRoom.mockResolvedValue({ sessionId: 1, roomCode: 'SC1-test-room' });
    mocks.sendMessage.mockResolvedValue(undefined);
    mocks.sendOver.mockResolvedValue(undefined);
    mocks.disconnect.mockResolvedValue(undefined);
  });

  it('创建后立即展示可复制的房间码', async () => {
    render(<App />);
    await waitFor(() => expect(mocks.onSessionEvent).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: '创建房间' }));

    await waitFor(() =>
      expect(mocks.createRoom).toHaveBeenCalledWith({
        hostIp: '127.0.0.1',
        port: 9001,
        rotationMinutes: 10,
        displayName: '我',
      }),
    );
    expect(screen.getByLabelText('生成的房间码')).toHaveValue('SC1-test-room');
    expect(screen.getByRole('button', { name: '复制房间码' })).toBeEnabled();
  });

  it('建立连接后通过原生表单发送消息', async () => {
    render(<App />);
    await waitFor(() => expect(mocks.eventHandler).toBeDefined());
    fireEvent.click(screen.getByRole('button', { name: '创建房间' }));
    await waitFor(() => expect(screen.getByLabelText('生成的房间码')).toHaveValue('SC1-test-room'));

    act(() => {
      mocks.eventHandler?.({
        sessionId: 1,
        seq: 1,
        kind: 'state',
        payload: {
          state: 'established',
          status: '安全会话已建立',
          epoch: 0,
          fingerprint: 'abc',
          nextRotationUnixMs: Date.now() + 600_000,
        },
      });
    });

    const input = screen.getByLabelText('消息内容');
    fireEvent.change(input, { target: { value: '加密消息' } });
    fireEvent.click(screen.getByRole('button', { name: '发送' }));
    await waitFor(() =>
      expect(mocks.sendMessage).toHaveBeenCalledWith(1, expect.any(String), '加密消息'),
    );
    expect(input).toHaveValue('');
    expect(screen.getByText('加密消息')).toBeInTheDocument();
  });
});
