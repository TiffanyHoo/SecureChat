import type { FormEvent } from 'react';
import { isBusy, isConnected, type SessionState } from '../store';
import { buttonOver, buttonPrimary, buttonSecondary, buttonText, inputBase } from '../ui/ui';

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

const modeOptionClass = (selected: boolean) =>
  `flex shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm whitespace-nowrap transition ${
    selected
      ? 'bg-surface font-medium text-ink shadow-sm'
      : 'text-ink-muted hover:text-ink'
  }`;

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
    <form className="grid gap-2.5 px-4 pb-3 pt-2" onSubmit={submit} aria-labelledby="app-title">
      <fieldset
        disabled={!canConfigure}
        className="m-0 grid min-w-0 gap-2.5 border-0 p-0 disabled:opacity-60"
      >
        <legend className="sr-only">连接配置</legend>
        <div className="flex min-w-0 flex-wrap items-end gap-x-4 gap-y-2.5">
          <fieldset className="m-0 flex min-w-0 items-center border-0 p-0">
            <legend className="sr-only">通话方式</legend>
            <div className="flex rounded-xl bg-surface-muted p-1 ring-1 ring-line">
              <label className={modeOptionClass(config.mode === 'create')}>
                <input
                  type="radio"
                  name="mode"
                  value="create"
                  checked={config.mode === 'create'}
                  onChange={(event) => onChange('mode', event.target.value)}
                  className="sr-only"
                />
                创建房间
              </label>
              <label className={modeOptionClass(config.mode === 'join')}>
                <input
                  type="radio"
                  name="mode"
                  value="join"
                  checked={config.mode === 'join'}
                  onChange={(event) => onChange('mode', event.target.value)}
                  className="sr-only"
                />
                加入房间
              </label>
            </div>
          </fieldset>
          <div className="grid min-w-0 flex-1 gap-1">
            <label htmlFor="display-name" className="text-xs leading-snug text-ink-muted">
              我的显示名
            </label>
            <input
              id="display-name"
              name="displayName"
              value={config.displayName}
              onChange={(event) => onChange('displayName', event.target.value)}
              autoComplete="nickname"
              maxLength={32}
              required
              className={inputBase}
            />
          </div>
        </div>

        {config.mode === 'create' ? (
          <div className="flex min-w-0 flex-wrap items-end gap-2.5">
            <div className="grid min-w-0 flex-[1.5] gap-1">
              <label htmlFor="host-ip" className="text-xs leading-snug text-ink-muted">
                本机 IP
              </label>
              <select
                id="host-ip"
                name="hostIp"
                value={config.hostIp}
                onChange={(event) => onChange('hostIp', event.target.value)}
                required
                className={`${inputBase} cursor-pointer`}
              >
                {hostIps.map((ip) => (
                  <option key={ip} value={ip}>
                    {ip}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid min-w-0 flex-[0.65] gap-1">
              <label htmlFor="listen-port" className="text-xs leading-snug text-ink-muted">
                端口
              </label>
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
                className={inputBase}
              />
            </div>
            <div className="grid min-w-0 flex-[0.65] gap-1">
              <label htmlFor="rotation-minutes" className="text-xs leading-snug text-ink-muted">
                换钥（分钟）
              </label>
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
                className={inputBase}
              />
            </div>
          </div>
        ) : (
          <div className="grid min-w-0 flex-1 gap-1">
            <label htmlFor="room-code-input" className="text-xs leading-snug text-ink-muted">
              对方房间码
            </label>
            <input
              id="room-code-input"
              name="roomCode"
              placeholder="粘贴 SC1- 开头的完整房间码"
              value={config.roomCode}
              onChange={(event) => onChange('roomCode', event.target.value)}
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              required
              className={`${inputBase} font-mono text-xs`}
            />
          </div>
        )}
      </fieldset>

      {config.mode === 'create' && (
        <div className="flex items-end gap-2.5">
          <div className="grid min-w-0 flex-1 gap-1">
            <label htmlFor="generated-room-code" className="text-xs leading-snug text-ink-muted">
              生成的房间码
            </label>
            <input
              id="generated-room-code"
              name="generatedRoomCode"
              value={session.generatedCode}
              readOnly
              placeholder="点击“创建房间”后生成"
              aria-describedby="room-code-hint"
              className={`${inputBase} font-mono text-xs`}
            />
          </div>
          <button
            type="button"
            className={buttonSecondary}
            disabled={!session.generatedCode}
            onClick={onCopy}
          >
            复制房间码
          </button>
        </div>
      )}
      <span id="room-code-hint" className="sr-only">
        房间码包含连接地址和随机会话口令，请仅通过可信渠道分享。
      </span>

      <div className="flex min-h-8 flex-wrap items-center gap-2.5">
        {canConfigure && (
          <button type="submit" className={`${buttonPrimary} min-w-30`}>
            {config.mode === 'create' ? '创建房间' : '加入房间'}
          </button>
        )}
        {connected && session.overPhase === 'idle' && !session.overEnding && (
          <button type="button" className={buttonOver} onClick={onOver}>
            OVER
          </button>
        )}
        {session.overPhase === 'waiting' && !session.overEnding && (
          <div className="flex items-center gap-1.5 text-sm" role="status">
            <span className="animate-over-pulse">OVER 已发出…</span>
            <button type="button" className={buttonText} onClick={onCancelOver}>
              撤回
            </button>
          </div>
        )}
        {busy && (
          <button type="button" className={buttonSecondary} onClick={onDisconnect}>
            取消等待
          </button>
        )}
      </div>
      <p className="m-0 min-h-[1.1rem] text-xs leading-normal text-ink-muted [overflow-wrap:anywhere]" role="status" aria-live="polite">
        {session.status}
      </p>
    </form>
  );
}
