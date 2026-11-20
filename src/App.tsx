import { useEffect, useState, type FormEvent } from 'react';
import { listHostIps } from './api';
import { ConnectionPanel, type ConnectionConfig } from './components/ConnectionPanel';
import { MessageList } from './components/MessageList';
import { useSession } from './hooks/useSession';
import { formatCountdown, isConnected } from './store';

const initialConfig: ConnectionConfig = {
  mode: 'create',
  displayName: '我',
  hostIp: '127.0.0.1',
  port: '9001',
  rotationMinutes: '10',
  roomCode: '',
};

export default function App() {
  const [config, setConfig] = useState(initialConfig);
  const [hostIps, setHostIps] = useState(['127.0.0.1']);
  const [input, setInput] = useState('');
  const [now, setNow] = useState(Date.now());
  const {
    state: session,
    createRoom,
    joinRoom,
    disconnect,
    sendMessage,
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
  const changeConfig = (field: keyof ConnectionConfig, value: string) =>
    setConfig((current) => ({ ...current, [field]: value }));

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
    <main className="app-shell">
      <header className="app-header">
        <div className="title-row">
          <h1 id="app-title">SecureChat 安全通话</h1>
          {session.peerName && <span className="peer-label">对方：{session.peerName}</span>}
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

      <MessageList messages={session.messages} onRespondOver={(accept) => void respondOver(accept)} />

      <form className="message-composer" onSubmit={submitMessage} aria-label="发送消息">
        <label htmlFor="message-input" className="visually-hidden">
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
        />
        <button type="submit" className="primary-button" disabled={!connected || !input.trim()}>
          发送
        </button>
      </form>

      <footer className="key-status">
        <span>
          {connected
            ? `加密纪元 ${session.epoch} · 密钥指纹 ${session.fingerprint} · ${formatCountdown(session.nextRotationUnixMs, now)} 后自动换钥`
            : '尚未建立安全会话'}
        </span>
        <button
          type="button"
          className="secondary-button"
          disabled={!connected || session.rekeying}
          onClick={() => void rekeyNow()}
        >
          {session.rekeying ? '换钥中…' : '立即换钥'}
        </button>
      </footer>
    </main>
  );
}
