import { useEffect, useState, type FormEvent } from 'react';
import { ConnectionPanel, type ConnectionConfig } from './components/ConnectionPanel';
import { MessageList } from './components/MessageList';
import { NameSetup, loadDisplayName, saveDisplayName } from './components/NameSetup';
import { useSession } from './hooks/useSession';
import { buttonOver, buttonPrimary, buttonSecondary, buttonText } from './ui/ui';
import { formatCountdown, isConnected, isBusy } from './store';

export default function App() {
  const [config, setConfig] = useState<ConnectionConfig>(() => ({
    mode: 'create',
    displayName: loadDisplayName(),
    roomCode: '',
  }));
  // 首次启动（未设置过名称）时显示设置引导（issue #7）
  const [needsNameSetup, setNeedsNameSetup] = useState(() => !loadDisplayName());
  const [input, setInput] = useState('');
  const [now, setNow] = useState(Date.now());
  const {
    state: session,
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
  } = useSession();

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const connected = isConnected(session);
  const busy = isBusy(session);
  const inChat = connected || busy;
  const changeConfig = (field: keyof ConnectionConfig, value: string) => {
    setConfig((current) => ({ ...current, [field]: value }));
    // 面板中修改显示名时同步持久化，保证下次启动使用最新名称
    if (field === 'displayName' && value.trim()) saveDisplayName(value.trim());
  };

  const saveNameFromSetup = (name: string) => {
    setConfig((current) => ({ ...current, displayName: name }));
    setNeedsNameSetup(false);
  };

  const submitConnection = () => {
    if (config.mode === 'create') {
      void createRoom({
        displayName: config.displayName.trim(),
      });
    } else {
      void joinRoom({ roomCode: config.roomCode.trim(), displayName: config.displayName.trim() });
    }
  };

  const submitMessage = (event: FormEvent) => {
    event.preventDefault();
    if (!input.trim() || !connected) return;
    const content = input;
    setInput('');
    void sendMessage(config.displayName.trim(), content);
  };

  if (needsNameSetup) {
    return <NameSetup onSave={saveNameFromSetup} />;
  }

  if (!inChat) {
    return (
      <ConnectionPanel
        config={config}
        session={session}
        onChange={changeConfig}
        onSubmit={submitConnection}
        onCopy={() => void copyRoomCode()}
        onDisconnect={() => void disconnect()}
      />
    );
  }

  const headerTitle = connected
    ? session.peerName || '对方'
    : config.mode === 'create'
      ? '等待对方加入…'
      : '正在连接…';

  return (
    <main className="grid h-dvh w-full grid-rows-[auto_minmax(0,1fr)_auto_auto] bg-bg font-sans text-ink">
      <h1 className="sr-only">SecureChat</h1>
      <header className="z-10 flex items-center justify-between gap-3 border-b border-line bg-surface/90 px-4 py-2 backdrop-blur">
        <div className="flex min-w-0 items-center gap-2.5">
          <span
            aria-hidden
            className={`size-2.5 shrink-0 rounded-full ${
              connected
                ? 'bg-emerald-500 shadow-[0_0_0_3px_rgba(16,185,129,0.18)]'
                : 'animate-over-pulse bg-amber-400'
            }`}
          />
          <div className="min-w-0">
            <p className="m-0 truncate text-sm font-semibold leading-tight" aria-label="当前通话对象">
              {headerTitle}
            </p>
            <p
              className="m-0 truncate text-xs leading-tight text-ink-muted [overflow-wrap:anywhere]"
              role="status"
              aria-live="polite"
            >
              {session.status}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {session.generatedCode && (
            <button type="button" className={buttonSecondary} onClick={() => void copyRoomCode()}>
              复制房间码
            </button>
          )}
          {connected && session.overPhase === 'idle' && !session.overEnding && (
            <button type="button" className={buttonOver} onClick={() => void requestOver()}>
              OVER
            </button>
          )}
          {session.overPhase === 'waiting' && !session.overEnding && (
            <div className="flex items-center gap-1.5 text-sm" role="status">
              <span className="animate-over-pulse">OVER 已发出…</span>
              <button type="button" className={buttonText} onClick={() => void cancelOver()}>
                撤回
              </button>
            </div>
          )}
          {busy && (
            <button type="button" className={buttonSecondary} onClick={() => void disconnect()}>
              取消等待
            </button>
          )}
        </div>
      </header>

      <MessageList
        messages={session.messages}
        onRespondOver={(accept) => void respondOver(accept)}
        onToggleReaction={(uid, reaction) => void sendReaction(uid, reaction)}
      />

      <form className="flex gap-2 border-t border-line bg-surface px-3 py-2.5" onSubmit={submitMessage} aria-label="发送消息">
        <label htmlFor="message-input" className="sr-only">
          消息内容
        </label>
        <input
          id="message-input"
          name="message"
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder="输入消息，回车发送（传输前自动加密）"
          enterKeyHint="send"
          autoComplete="off"
          disabled={!connected}
          className="min-w-0 flex-1 cursor-text rounded-full border border-transparent bg-surface-muted px-4 py-2 text-sm text-ink transition placeholder:text-ink-muted/70 focus-visible:border-accent focus-visible:bg-surface focus-visible:outline-none disabled:cursor-not-allowed"
        />
        <button type="submit" className={buttonPrimary} disabled={!connected || !input.trim()}>
          发送
        </button>
      </form>

      <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-line bg-surface-muted px-4 py-2 text-xs text-ink-muted">
        <span className="min-w-0 [overflow-wrap:anywhere]">
          {connected
            ? `加密纪元 ${session.epoch} · 密钥指纹 ${session.fingerprint} · ${formatCountdown(session.nextRotationUnixMs, now)} 后自动换钥`
            : '正在建立安全会话…'}
        </span>
        <button
          type="button"
          className="inline-flex cursor-pointer items-center justify-center whitespace-nowrap rounded-lg border border-line bg-surface px-3 py-1.5 text-xs font-medium text-ink shadow-sm transition hover:bg-surface-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:pointer-events-none disabled:opacity-50"
          disabled={!connected || session.rekeying}
          onClick={() => void rekeyNow()}
        >
          {session.rekeying ? '换钥中…' : '立即换钥'}
        </button>
      </footer>
    </main>
  );
}
