import type { FormEvent } from 'react';
import { isBusy, isConnected, type SessionState } from '../store';

export interface ConnectionConfig {
  mode: 'create' | 'join';
  displayName: string;
  hostIp: string;
  port: string;
  rotationMinutes: string;
  roomCode: string;
}

interface ConnectionPanelProps {
  config: ConnectionConfig;
  hostIps: string[];
  session: SessionState;
  onChange: (field: keyof ConnectionConfig, value: string) => void;
  onSubmit: () => void;
  onCopy: () => void;
  onDisconnect: () => void;
  onOver: () => void;
  onCancelOver: () => void;
}

export function ConnectionPanel({
  config,
  hostIps,
  session,
  onChange,
  onSubmit,
  onCopy,
  onDisconnect,
  onOver,
  onCancelOver,
}: ConnectionPanelProps) {
  const busy = isBusy(session);
  const connected = isConnected(session);
  const canConfigure = !busy && !connected;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSubmit();
  };

  return (
    <form className="connection-panel" onSubmit={submit} aria-labelledby="app-title">
      <fieldset className="configuration" disabled={!canConfigure}>
        <legend className="visually-hidden">连接配置</legend>
        <div className="connection-row identity-row">
          <fieldset className="mode-picker">
            <legend>通话方式</legend>
            <label className={config.mode === 'create' ? 'selected' : ''}>
              <input
                type="radio"
                name="mode"
                value="create"
                checked={config.mode === 'create'}
                onChange={(event) => onChange('mode', event.target.value)}
              />
              创建房间
            </label>
            <label className={config.mode === 'join' ? 'selected' : ''}>
              <input
                type="radio"
                name="mode"
                value="join"
                checked={config.mode === 'join'}
                onChange={(event) => onChange('mode', event.target.value)}
              />
              加入房间
            </label>
          </fieldset>
          <div className="field display-name-field">
            <label htmlFor="display-name">我的显示名</label>
            <input
              id="display-name"
              name="displayName"
              value={config.displayName}
              onChange={(event) => onChange('displayName', event.target.value)}
              autoComplete="nickname"
              maxLength={32}
              required
            />
          </div>
        </div>

        {config.mode === 'create' ? (
          <div className="connection-row listen-settings">
            <div className="field host-field">
              <label htmlFor="host-ip">本机 IP</label>
              <select
                id="host-ip"
                name="hostIp"
                value={config.hostIp}
                onChange={(event) => onChange('hostIp', event.target.value)}
                required
              >
                {hostIps.map((ip) => (
                  <option key={ip} value={ip}>
                    {ip}
                  </option>
                ))}
              </select>
            </div>
            <div className="field compact-field">
              <label htmlFor="listen-port">端口</label>
              <input
                id="listen-port"
                name="port"
                type="number"
                inputMode="numeric"
                min={1}
                max={65535}
                value={config.port}
                onChange={(event) => onChange('port', event.target.value)}
                required
              />
            </div>
            <div className="field compact-field">
              <label htmlFor="rotation-minutes">换钥（分钟）</label>
              <input
                id="rotation-minutes"
                name="rotationMinutes"
                type="number"
                inputMode="numeric"
                min={1}
                max={720}
                value={config.rotationMinutes}
                onChange={(event) => onChange('rotationMinutes', event.target.value)}
                required
              />
            </div>
          </div>
        ) : (
          <div className="field">
            <label htmlFor="room-code-input">对方房间码</label>
            <input
              id="room-code-input"
              name="roomCode"
              className="mono"
              placeholder="粘贴 SC1- 开头的完整房间码"
              value={config.roomCode}
              onChange={(event) => onChange('roomCode', event.target.value)}
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              required
            />
          </div>
        )}
      </fieldset>

      {config.mode === 'create' && (
        <div className="room-code-row">
          <div className="field">
            <label htmlFor="generated-room-code">生成的房间码</label>
            <input
              id="generated-room-code"
              name="generatedRoomCode"
              className="mono"
              value={session.generatedCode}
              readOnly
              placeholder="点击“创建房间”后生成"
              aria-describedby="room-code-hint"
            />
          </div>
          <button type="button" className="secondary-button" disabled={!session.generatedCode} onClick={onCopy}>
            复制房间码
          </button>
        </div>
      )}
      <span id="room-code-hint" className="visually-hidden">
        房间码包含连接地址和随机会话口令，请仅通过可信渠道分享。
      </span>

      <div className="session-actions">
        {canConfigure && (
          <button type="submit" className="primary-button">
            {config.mode === 'create' ? '创建房间' : '加入房间'}
          </button>
        )}
        {connected && session.overPhase === 'idle' && !session.overEnding && (
          <button type="button" className="over-button" onClick={onOver}>
            OVER
          </button>
        )}
        {session.overPhase === 'waiting' && !session.overEnding && (
          <div className="over-waiting" role="status">
            <span>OVER 已发出…</span>
            <button type="button" className="text-button" onClick={onCancelOver}>
              撤回
            </button>
          </div>
        )}
        {busy && (
          <button type="button" className="secondary-button" onClick={onDisconnect}>
            取消等待
          </button>
        )}
      </div>
      <p className="status-text" role="status" aria-live="polite">
        {session.status}
      </p>
    </form>
  );
}
