import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { DISPLAY_NAME_STORAGE_KEY } from './components/NameSetup';
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
    window.localStorage.clear();
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

  // 首次启动会先弹出显示名设置引导（issue #7），此辅助完成设置流程
  const completeNameSetup = (name: string) => {
    fireEvent.change(screen.getByPlaceholderText('输入显示名'), { target: { value: name } });
    fireEvent.click(screen.getByRole('button', { name: '保存并继续' }));
  };

  it('首次启动要求设置显示名并保存到 localStorage', () => {
    render(<App />);
    expect(screen.getByRole('button', { name: '保存并继续' })).toBeInTheDocument();

    completeNameSetup('阿汤');

    expect(window.localStorage.getItem(DISPLAY_NAME_STORAGE_KEY)).toBe('阿汤');
    expect(screen.getByLabelText('我的显示名')).toHaveValue('阿汤');
    expect(screen.queryByPlaceholderText('输入显示名')).not.toBeInTheDocument();
  });

  it('已保存过名称时启动直接使用，不再弹设置引导', () => {
    window.localStorage.setItem(DISPLAY_NAME_STORAGE_KEY, '老用户');
    render(<App />);

    expect(screen.queryByRole('button', { name: '保存并继续' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('我的显示名')).toHaveValue('老用户');
  });

  it('创建后立即展示可复制的房间码', async () => {
    render(<App />);
    await waitFor(() => expect(mocks.onSessionEvent).toHaveBeenCalled());
    completeNameSetup('我');
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
    completeNameSetup('我');
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
