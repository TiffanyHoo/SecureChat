export type ConnectionState =
  | 'idle'
  | 'listening'
  | 'connecting'
  | 'handshaking'
  | 'established'
  | 'closed';

export type OverKind = 'request' | 'accept' | 'reject' | 'cancel';

export interface CreateRoomParams {
  hostIp: string;
  port: number;
  rotationMinutes: number;
  displayName: string;
}

export interface JoinRoomParams {
  roomCode: string;
  displayName: string;
}

export interface CreateRoomResult {
  sessionId: number;
  roomCode: string;
}

export interface JoinRoomResult {
  sessionId: number;
}

export interface StatePayload {
  state: ConnectionState;
  status: string;
  epoch: number;
  fingerprint: string;
  nextRotationUnixMs: number | null;
}

export interface MessagePayload {
  uid: string;
  sender: string;
  content: string;
  sentAtUnixMs: number;
}

export interface KeyPayload {
  epoch: number;
  fingerprint: string;
  nextRotationUnixMs: number | null;
}

export interface SessionSnapshot {
  sessionId: number | null;
  seq: number;
  state: ConnectionState;
  status: string;
  peerName: string;
  epoch: number;
  fingerprint: string;
  nextRotationUnixMs: number | null;
}

interface EventEnvelope {
  sessionId: number;
  seq: number;
}

export type SessionEvent = EventEnvelope &
  (
    | { kind: 'state'; payload: StatePayload }
    | { kind: 'message'; payload: MessagePayload }
    | { kind: 'peer-name'; payload: { name: string } }
    | { kind: 'key-rotated'; payload: KeyPayload }
    | { kind: 'over'; payload: { kind: OverKind } }
    | { kind: 'reaction'; payload: { uid: string; reaction: string } }
    | { kind: 'disconnected'; payload: { reason: string } }
  );
