import { useCallback, useEffect, useReducer, useRef } from 'react';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import * as api from '../api';
import {
  initialSessionState,
  isConnected,
  sessionReducer,
  type SessionAction,
  type SessionState,
} from '../store';
import type { CreateRoomParams, JoinRoomParams, SessionEvent } from '../types';

const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error));
const wait = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));

const newMessageUid = () =>
  typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `uid-${Date.now()}-${Math.random().toString(36).slice(2)}`;

export function useSession() {
  const [state, reactDispatch] = useReducer(sessionReducer, initialSessionState);
  const stateRef = useRef<SessionState>(state);

  const dispatch = useCallback((action: SessionAction) => {
    stateRef.current = sessionReducer(stateRef.current, action);
    reactDispatch(action);
  }, []);

  const finishOver = useCallback(
    async (reason: string) => {
      const current = stateRef.current;
      if (current.sessionId === null || current.overEnding) return;
      const sessionId = current.sessionId;
      dispatch({ type: 'over-finish', reason });
      await wait(1200);
      dispatch({ type: 'over-complete' });
      try {
        await api.disconnect(sessionId, 'OVER 结束');
      } catch (error) {
        dispatch({ type: 'command-error', message: `断开失败：${errorMessage(error)}` });
      }
    },
    [dispatch],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const handleEvent = (event: SessionEvent) => {
      const current = stateRef.current;
      if (event.sessionId !== current.sessionId || event.seq <= current.lastSeq) return;
      dispatch({ type: 'event', event });

      if (event.kind === 'over' && event.payload.kind === 'accept') {
        void finishOver('对方接受了 OVER');
      } else if (
        event.kind === 'over' &&
        event.payload.kind === 'request' &&
        current.overPhase === 'waiting'
      ) {
        void api
          .sendOver(event.sessionId, 'accept')
          .catch(() => undefined)
          .then(() => finishOver('默契 OVER！你们同时按下了按钮'));
      }
    };

    void api
      .onSessionEvent(handleEvent)
      .then((disposeListener) => {
        if (disposed) disposeListener();
        else unlisten = disposeListener;
      })
      .catch(() => undefined);
    void api
      .getSnapshot()
      .then((snapshot) => {
        if (!disposed) dispatch({ type: 'snapshot', snapshot });
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [dispatch, finishOver]);

  const createRoom = useCallback(
    async (params: CreateRoomParams) => {
      try {
        const result = await api.createRoom(params);
        dispatch({
          type: 'session-started',
          sessionId: result.sessionId,
          roomCode: result.roomCode,
          status: '房间已创建，正在等待对方加入…',
          systemMessage: '房间码已生成（含随机口令）。请通过你信任的渠道发给对方。',
        });
        dispatch({ type: 'snapshot', snapshot: await api.getSnapshot() });
      } catch (error) {
        dispatch({
          type: 'command-error',
          status: `创建房间失败：${errorMessage(error)}`,
          message: '创建房间失败，请检查监听地址和端口后重试。',
        });
      }
    },
    [dispatch],
  );

  const joinRoom = useCallback(
    async (params: JoinRoomParams) => {
      try {
        const result = await api.joinRoom(params);
        dispatch({
          type: 'session-started',
          sessionId: result.sessionId,
          status: '正在连接对方…',
          systemMessage: '正在连接对方并协商加密密钥…',
        });
        dispatch({ type: 'snapshot', snapshot: await api.getSnapshot() });
      } catch (error) {
        dispatch({
          type: 'command-error',
          status: `加入房间失败：${errorMessage(error)}`,
          message: '加入房间失败，请检查房间码是否完整、是否仍然有效。',
        });
      }
    },
    [dispatch],
  );

  const disconnect = useCallback(async () => {
    const sessionId = stateRef.current.sessionId;
    if (sessionId === null) return;
    dispatch({ type: 'local-disconnected' });
    try {
      await api.disconnect(sessionId);
    } catch (error) {
      dispatch({ type: 'command-error', message: `断开失败：${errorMessage(error)}` });
    }
  }, [dispatch]);

  const sendMessage = useCallback(
    async (sender: string, content: string) => {
      const current = stateRef.current;
      const text = content.trim();
      if (current.sessionId === null || !isConnected(current) || !text) return false;
      const uid = newMessageUid();
      dispatch({ type: 'local-message', sender, content: text, uid });
      try {
        await api.sendMessage(current.sessionId, uid, text);
        return true;
      } catch (error) {
        dispatch({ type: 'command-error', message: `发送失败：${errorMessage(error)}` });
        return false;
      }
    },
    [dispatch],
  );

  const sendReaction = useCallback(
    async (uid: string, reaction: string) => {
      const current = stateRef.current;
      if (current.sessionId === null || !isConnected(current)) return;
      dispatch({ type: 'toggle-reaction', uid, reaction });
      try {
        await api.sendReaction(current.sessionId, uid, reaction);
      } catch (error) {
        dispatch({ type: 'toggle-reaction', uid, reaction });
        dispatch({ type: 'command-error', message: `回应发送失败：${errorMessage(error)}` });
      }
    },
    [dispatch],
  );

  const rekeyNow = useCallback(async () => {
    const current = stateRef.current;
    if (current.sessionId === null || !isConnected(current) || current.rekeying) return;
    dispatch({ type: 'rekeying', value: true });
    try {
      await api.rekeyNow(current.sessionId);
    } catch (error) {
      dispatch({ type: 'command-error', message: `手动换钥失败：${errorMessage(error)}` });
    } finally {
      dispatch({ type: 'rekeying', value: false });
    }
  }, [dispatch]);

  const requestOver = useCallback(async () => {
    const current = stateRef.current;
    if (current.sessionId === null || !isConnected(current) || current.overPhase !== 'idle') return;
    dispatch({ type: 'over-requested' });
    try {
      await api.sendOver(current.sessionId, 'request');
    } catch (error) {
      dispatch({ type: 'over-failed' });
      dispatch({ type: 'command-error', message: `OVER 发送失败：${errorMessage(error)}` });
    }
  }, [dispatch]);

  const cancelOver = useCallback(async () => {
    const current = stateRef.current;
    if (current.sessionId === null || current.overPhase !== 'waiting') return;
    dispatch({ type: 'over-cancelled' });
    await api.sendOver(current.sessionId, 'cancel').catch(() => undefined);
  }, [dispatch]);

  const respondOver = useCallback(
    async (accept: boolean) => {
      const current = stateRef.current;
      if (current.sessionId === null || current.overPhase !== 'peer-asking') return;
      if (accept) {
        await api.sendOver(current.sessionId, 'accept').catch(() => undefined);
        await finishOver('对方接受了 OVER');
      } else {
        dispatch({ type: 'over-rejected' });
        await api.sendOver(current.sessionId, 'reject').catch(() => undefined);
      }
    },
    [dispatch, finishOver],
  );

  const copyRoomCode = useCallback(async () => {
    const code = stateRef.current.generatedCode;
    if (!code) return;
    try {
      await writeText(code);
      dispatch({ type: 'copied', ok: true });
    } catch {
      dispatch({ type: 'copied', ok: false });
    }
  }, [dispatch]);

  return {
    state,
    createRoom,
    joinRoom,
    disconnect,
    sendMessage,
    sendReaction,
    rekeyNow,
    requestOver,
    cancelOver,
    respondOver,
    copyRoomCode,
  };
}
