import { useEffect, useState, type FormEvent } from 'react';
import { listHostIps } from './api';
import { ConnectionPanel, type ConnectionConfig } from './components/ConnectionPanel';
import { MessageList } from './components/MessageList';
import { NameSetup, loadDisplayName, saveDisplayName } from './components/NameSetup';
import { useSession } from './hooks/useSession';
import { buttonPrimary } from './ui/ui';
import { formatCountdown, isConnected } from './store';

const initialConfig: ConnectionConfig = {
  mode: 'create',
  displayName: '',
  hostIp: '127.0.0.1',
  port: '9001',
  rotationMinutes: '10',
  roomCode: '',
};

export default function App() {
  const [config, setConfig] = useState<ConnectionConfig>(() => ({
    ...initialConfig,
    displayName: loadDisplayName(),
  }));
  // 首次启动（未设置过名称）时显示设置引导（issue #7）
  const [needsNameSetup, setNeedsNameSetup] = useState(() => !loadDisplayName());
  const [hostIps, setHostIps] = useState(['127.0.0.1']);
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
    let active = true;
    void listHostIps()
      .then((ips) => {
        if (!active || ips.length === 0) return;
        setHostIps(ips);
        setConfig((current) => ({ ...current, hostIp: ips.includes(current.hostIp) ? current.hostIp : ips[0] }));
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const connected = isConnected(session);
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
        hostIp: config.hostIp,
        port: Number(config.port),
        rotationMinutes: Number(config.rotationMinutes),
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

  return (
    <main className="grid h-dvh w-full grid-rows-[auto_minmax(0,1fr)_auto_auto] bg-bg font-sans text-ink">
      {needsNameSetup && <NameSetup onSave={saveNameFromSetup} />}
      <header className="z-10 border-b border-line bg-surface/90 backdrop-blur">
        <div className="flex items-baseline gap-3 px-4 pt-3">
          <h1 id="app-title" className="m-0 flex items-center gap-2 text-lg font-semibold tracking-tight">
            <span
              aria-hidden
              className="inline-block size-3 rounded-md bg-gradient-to-br from-accent to-over shadow-sm"
            />
            SecureChat 安全通话
          </h1>
          {session.peerName && (
            <span className="truncate rounded-full bg-surface-muted px-2.5 py-1 text-xs text-ink-muted ring-1 ring-line">
              对方：{session.peerName}
            </span>
          )}
        </div>
        <ConnectionPanel
          config={config}
          hostIps={hostIps}
          session={session}
          onChange={changeConfig}
          onSubmit={submitConnection}
          onCopy={() => void copyRoomCode()}
          onDisconnect={() => void disconnect()}
          onOver={() => void requestOver()}
          onCancelOver={() => void cancelOver()}
        />
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
            : '尚未建立安全会话'}
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
