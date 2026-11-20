import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  CreateRoomParams,
  CreateRoomResult,
  JoinRoomParams,
  JoinRoomResult,
  OverKind,
  SessionEvent,
  SessionSnapshot,
} from './types';

export const listHostIps = () => invoke<string[]>('list_host_ips');
export const createRoom = (params: CreateRoomParams) =>
  invoke<CreateRoomResult>('create_room', { params });
export const joinRoom = (params: JoinRoomParams) =>
  invoke<JoinRoomResult>('join_room', { params });
export const getSnapshot = () => invoke<SessionSnapshot>('get_snapshot');

export const disconnect = (sessionId: number, reason = '本地主动断开') =>
  invoke<void>('disconnect', { sessionId, reason });
export const sendMessage = (sessionId: number, content: string) =>
  invoke<void>('send_message', { sessionId, content });
export const rekeyNow = (sessionId: number) =>
  invoke<void>('rekey_now', { sessionId });
export const sendOver = (sessionId: number, kind: OverKind) =>
  invoke<void>('send_over', { sessionId, kind });

export const onSessionEvent = (handler: (event: SessionEvent) => void) =>
  listen<SessionEvent>('session-event', (event) => handler(event.payload));
