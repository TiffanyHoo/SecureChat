import type { FormEvent } from 'react';
import { motion } from 'motion/react';
import { isBusy, type SessionState } from '../store';
import { buttonPrimary, buttonSecondary, inputBase } from '../ui/ui';
import { springEnter } from '../ui/motion';

export interface ConnectionConfig {
  mode: 'create' | 'join';
  displayName: string;
  roomCode: string;
}

interface ConnectionPanelProps {
  config: ConnectionConfig;
  session: SessionState;
  onChange: (field: keyof ConnectionConfig, value: string) => void;
  onSubmit: () => void;
  onCopy: () => void;
  onDisconnect: () => void;
}

const modeOptionClass = (selected: boolean) =>
  `flex shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm whitespace-nowrap transition ${
    selected
      ? 'bg-surface font-medium text-ink shadow-sm'
      : 'text-ink-muted hover:text-ink'
  }`;

// 未连接时的全屏设置页：一次性完成模式选择、命名与配对；连接建立后整页退场，交给聊天视图
export function ConnectionPanel({
  config,
  session,
  onChange,
  onSubmit,
  onCopy,
  onDisconnect,
}: ConnectionPanelProps) {
  const busy = isBusy(session);
  const canConfigure = !busy;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSubmit();
  };

  return (
    <div className="grid h-dvh w-full place-items-center bg-bg px-4 font-sans text-ink">
      <motion.form
        initial={{ opacity: 0, y: 18, scale: 0.97 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={springEnter}
        className="grid w-full max-w-sm gap-4 rounded-2xl border border-line bg-surface p-5 shadow-lg"
        onSubmit={submit}
        aria-label="开始安全通话"
      >
        <fieldset
          disabled={!canConfigure}
          className="m-0 grid gap-4 border-0 p-0 disabled:opacity-60"
        >
          <legend className="sr-only">连接配置</legend>

          <div className="flex items-center gap-2">
            <span
              aria-hidden
              className="inline-block size-3 rounded-md bg-gradient-to-br from-accent to-over shadow-sm"
            />
            <h1 className="m-0 text-base font-semibold tracking-tight">开始一次安全通话</h1>
          </div>

          <fieldset className="m-0 flex items-center border-0 p-0">
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

          <div className="grid gap-1">
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

          {config.mode === 'join' && (
            <div className="grid gap-1">
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

        <div className="flex min-h-8 items-center gap-2.5">
          {canConfigure ? (
            <button type="submit" className={`${buttonPrimary} min-w-30 flex-1`}>
              {config.mode === 'create' ? '创建房间' : '加入房间'}
            </button>
          ) : (
            <button type="button" className={`${buttonSecondary} flex-1`} onClick={onDisconnect}>
              取消等待
            </button>
          )}
        </div>
        <p
          className="m-0 min-h-[1.1rem] text-xs leading-normal text-ink-muted [overflow-wrap:anywhere]"
          role="status"
          aria-live="polite"
        >
          {session.status}
        </p>
      </motion.form>
    </div>
  );
}
