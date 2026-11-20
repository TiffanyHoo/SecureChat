use std::collections::HashMap;
use std::future::pending;
use std::net::Ipv4Addr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use zeroize::Zeroizing;

use crate::crypto::{
    build_confirm_mac, build_next_transcript, build_transcript, derive_material, open,
    parse_hello_payload, seal, verify_confirm_mac, EphemeralKey, SessionKeys, CONFIRM_MAC_SIZE,
    TIEBREAK_SIZE,
};
use crate::protocol::{
    read_frame, write_frame, ChatMessage, Frame, FrameType, OverMessage, ProfileMessage,
    ReactionMessage,
};
use crate::{Error, Result};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const REKEY_TIMEOUT: Duration = Duration::from_secs(10);
const DISCONNECT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnState {
    Idle,
    Listening,
    Connecting,
    Handshaking,
    Established,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureChatOptions {
    pub rotation_minutes: u16,
    pub keep_alive_seconds: u64,
    pub dead_seconds: u64,
}

impl Default for SecureChatOptions {
    fn default() -> Self {
        Self {
            rotation_minutes: 10,
            keep_alive_seconds: 25,
            dead_seconds: 90,
        }
    }
}

impl SecureChatOptions {
    fn validate(&self) -> Result<()> {
        if !(1..=720).contains(&self.rotation_minutes) {
            return Err(Error::InvalidInput(
                "rotation minutes must be in 1..=720".into(),
            ));
        }
        if self.keep_alive_seconds == 0 || self.dead_seconds == 0 {
            return Err(Error::InvalidInput(
                "keepalive and dead timeouts must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectionEvent {
    Listening {
        port: u16,
    },
    Connecting {
        host: String,
        port: u16,
    },
    Handshaking,
    Established {
        epoch: u32,
        fingerprint: String,
        next_rotation_unix_ms: i64,
    },
    Message {
        uid: String,
        from: String,
        content: String,
        timestamp_unix_ms: i64,
        epoch: u32,
    },
    PeerName {
        name: String,
    },
    KeyRotated {
        epoch: u32,
        fingerprint: String,
        next_rotation_unix_ms: i64,
    },
    Over {
        kind: String,
    },
    Reaction {
        uid: String,
        reaction: String,
    },
    Disconnected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSnapshot {
    pub state: ConnState,
    pub epoch: u32,
    pub fingerprint: String,
    pub peer_name: Option<String>,
    pub next_rotation_unix_ms: Option<i64>,
    pub local_port: Option<u16>,
}

impl ConnectionSnapshot {
    fn new(state: ConnState, local_port: Option<u16>) -> Self {
        Self {
            state,
            epoch: 0,
            fingerprint: String::new(),
            peer_name: None,
            next_rotation_unix_ms: None,
            local_port,
        }
    }
}

type Response = oneshot::Sender<Result<()>>;

enum Command {
    SendMessage {
        uid: String,
        content: String,
        response: Response,
    },
    SendOver {
        kind: String,
        response: Response,
    },
    SendReaction {
        uid: String,
        reaction: String,
        response: Response,
    },
    SetDisplayName {
        name: String,
        response: Response,
    },
    Rekey {
        response: Response,
    },
    Disconnect {
        reason: String,
        response: Option<Response>,
    },
}

pub struct SecureConnection {
    commands: mpsc::Sender<Command>,
    snapshot: Arc<RwLock<ConnectionSnapshot>>,
    local_port: Option<u16>,
}

impl SecureConnection {
    pub async fn listen(
        port: u16,
        psk_text: impl Into<String>,
        display_name: impl Into<String>,
        options: SecureChatOptions,
        event_sender: mpsc::UnboundedSender<ConnectionEvent>,
    ) -> Result<Self> {
        options.validate()?;
        let psk_text = Zeroizing::new(psk_text.into());
        validate_psk(&psk_text)?;
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
        let local_port = listener.local_addr()?.port();
        let snapshot = Arc::new(RwLock::new(ConnectionSnapshot::new(
            ConnState::Listening,
            Some(local_port),
        )));
        let _ = event_sender.send(ConnectionEvent::Listening { port: local_port });
        let (commands, receiver) = mpsc::channel(64);
        let actor = Actor::new(
            psk_text,
            display_name.into(),
            options,
            event_sender,
            Arc::clone(&snapshot),
            receiver,
        );
        tokio::spawn(run_actor(actor, StartMode::Listen(listener)));
        Ok(Self {
            commands,
            snapshot,
            local_port: Some(local_port),
        })
    }

    pub async fn connect(
        host: impl Into<String>,
        port: u16,
        psk_text: impl Into<String>,
        display_name: impl Into<String>,
        options: SecureChatOptions,
        event_sender: mpsc::UnboundedSender<ConnectionEvent>,
    ) -> Result<Self> {
        options.validate()?;
        let psk_text = Zeroizing::new(psk_text.into());
        validate_psk(&psk_text)?;
        let host = host.into();
        if host.trim().is_empty() || port == 0 {
            return Err(Error::InvalidInput(
                "host must be nonempty and port must be in 1..=65535".into(),
            ));
        }
        let snapshot = Arc::new(RwLock::new(ConnectionSnapshot::new(
            ConnState::Connecting,
            None,
        )));
        let _ = event_sender.send(ConnectionEvent::Connecting {
            host: host.clone(),
            port,
        });
        let (commands, receiver) = mpsc::channel(64);
        let actor = Actor::new(
            psk_text,
            display_name.into(),
            options,
            event_sender,
            Arc::clone(&snapshot),
            receiver,
        );
        tokio::spawn(run_actor(actor, StartMode::Connect { host, port }));
        Ok(Self {
            commands,
            snapshot,
            local_port: None,
        })
    }

    pub fn snapshot(&self) -> ConnectionSnapshot {
        read_snapshot(&self.snapshot).clone()
    }

    pub fn state(&self) -> ConnState {
        read_snapshot(&self.snapshot).state
    }

    pub fn epoch(&self) -> u32 {
        read_snapshot(&self.snapshot).epoch
    }

    pub fn fingerprint(&self) -> String {
        read_snapshot(&self.snapshot).fingerprint.clone()
    }

    pub fn peer_name(&self) -> Option<String> {
        read_snapshot(&self.snapshot).peer_name.clone()
    }

    pub fn next_rotation_unix_ms(&self) -> Option<i64> {
        read_snapshot(&self.snapshot).next_rotation_unix_ms
    }

    pub fn local_port(&self) -> Option<u16> {
        self.local_port
    }

    pub async fn send_message(&self, uid: impl Into<String>, content: impl Into<String>) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(Command::SendMessage {
                uid: uid.into(),
                content: content.into(),
                response,
            })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Closed)?
    }

    pub async fn send_over(&self, kind: impl Into<String>) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(Command::SendOver {
                kind: kind.into(),
                response,
            })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Closed)?
    }

    pub async fn send_reaction(&self, uid: impl Into<String>, reaction: impl Into<String>) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(Command::SendReaction {
                uid: uid.into(),
                reaction: reaction.into(),
                response,
            })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Closed)?
    }

    pub async fn set_display_name(&self, name: impl Into<String>) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(Command::SetDisplayName {
                name: name.into(),
                response,
            })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Closed)?
    }

    pub async fn rekey_now(&self) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(Command::Rekey { response })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Closed)?
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.disconnect_with_reason("local disconnect").await
    }

    pub async fn disconnect_with_reason(&self, reason: impl Into<String>) -> Result<()> {
        if self.state() == ConnState::Closed {
            return Ok(());
        }
        let (response, result) = oneshot::channel();
        if self
            .commands
            .send(Command::Disconnect {
                reason: reason.into(),
                response: Some(response),
            })
            .await
            .is_err()
        {
            return Ok(());
        }
        let _ = result.await;
        Ok(())
    }
}

impl Drop for SecureConnection {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Disconnect {
            reason: "connection handle dropped".into(),
            response: None,
        });
    }
}

fn validate_psk(psk: &str) -> Result<()> {
    if psk.is_empty() {
        Err(Error::InvalidInput("PSK must not be empty".into()))
    } else {
        Ok(())
    }
}

fn read_snapshot(
    snapshot: &RwLock<ConnectionSnapshot>,
) -> std::sync::RwLockReadGuard<'_, ConnectionSnapshot> {
    snapshot.read().unwrap_or_else(|error| error.into_inner())
}

fn update_snapshot(
    snapshot: &RwLock<ConnectionSnapshot>,
    update: impl FnOnce(&mut ConnectionSnapshot),
) {
    let mut guard = snapshot.write().unwrap_or_else(|error| error.into_inner());
    update(&mut guard);
}

enum StartMode {
    Listen(TcpListener),
    Connect { host: String, port: u16 },
}

enum StartupStep {
    Stream(TcpStream, bool),
    End(SessionEnd),
}

enum HandshakeStep {
    Keys(SessionKeys),
    End(SessionEnd),
}

enum ReaderEvent {
    Frame(Frame),
    Closed,
    Error(Error),
}

struct SessionEnd {
    reason: String,
    notify_peer: bool,
    response: Option<Response>,
}

impl SessionEnd {
    fn local(reason: String, response: Option<Response>) -> Self {
        Self {
            reason,
            notify_peer: true,
            response,
        }
    }

    fn peer(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            notify_peer: false,
            response: None,
        }
    }
}

struct RekeyState {
    start_epoch: u32,
    driver: bool,
    ephemeral: Option<EphemeralKey>,
    my_hello: Vec<u8>,
    deadline: Instant,
    switched: bool,
    waiters: Vec<Response>,
}

impl RekeyState {
    fn new(start_epoch: u32, driver: bool, waiter: Option<Response>) -> Result<Self> {
        let ephemeral = EphemeralKey::generate()?;
        let my_hello = ephemeral.hello_payload()?;
        Ok(Self {
            start_epoch,
            driver,
            ephemeral: Some(ephemeral),
            my_hello,
            deadline: Instant::now() + REKEY_TIMEOUT,
            switched: false,
            waiters: waiter.into_iter().collect(),
        })
    }
}

struct Actor {
    psk: Zeroizing<String>,
    my_name: String,
    peer_name: Option<String>,
    options: SecureChatOptions,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    snapshot: Arc<RwLock<ConnectionSnapshot>>,
    commands: mpsc::Receiver<Command>,
    reader: Option<mpsc::Receiver<ReaderEvent>>,
    reader_task: Option<JoinHandle<()>>,
    writer: Option<OwnedWriteHalf>,
    keys: Option<SessionKeys>,
    previous_recv_keys: Option<SessionKeys>,
    send_counter: u32,
    recv_counters: HashMap<u32, u32>,
    rekey: Option<RekeyState>,
    last_receive: Instant,
    rotation_deadline: Instant,
    next_rotation_unix_ms: i64,
}

impl Actor {
    fn new(
        psk: Zeroizing<String>,
        my_name: String,
        options: SecureChatOptions,
        events: mpsc::UnboundedSender<ConnectionEvent>,
        snapshot: Arc<RwLock<ConnectionSnapshot>>,
        commands: mpsc::Receiver<Command>,
    ) -> Self {
        Self {
            psk,
            my_name,
            peer_name: None,
            options,
            events,
            snapshot,
            commands,
            reader: None,
            reader_task: None,
            writer: None,
            keys: None,
            previous_recv_keys: None,
            send_counter: 0,
            recv_counters: HashMap::new(),
            rekey: None,
            last_receive: Instant::now(),
            rotation_deadline: Instant::now(),
            next_rotation_unix_ms: 0,
        }
    }

    async fn start(&mut self, mode: StartMode) -> Result<SessionEnd> {
        let step = match mode {
            StartMode::Listen(listener) => self.accept(listener).await?,
            StartMode::Connect { host, port } => self.connect(host, port).await?,
        };
        let (mut stream, i_am_a) = match step {
            StartupStep::Stream(stream, i_am_a) => (stream, i_am_a),
            StartupStep::End(end) => return Ok(end),
        };
        stream.set_nodelay(true)?;
        update_snapshot(&self.snapshot, |snapshot| {
            snapshot.state = ConnState::Handshaking;
        });
        let _ = self.events.send(ConnectionEvent::Handshaking);
        let keys = match self.handshake(&mut stream, i_am_a).await? {
            HandshakeStep::Keys(keys) => keys,
            HandshakeStep::End(end) => return Ok(end),
        };
        self.keys = Some(keys);
        self.send_counter = 0;
        self.last_receive = Instant::now();
        self.reset_rotation();
        update_snapshot(&self.snapshot, |snapshot| {
            snapshot.state = ConnState::Established;
            snapshot.epoch = 0;
            snapshot.fingerprint = self.keys.as_ref().expect("keys set").fingerprint();
            snapshot.next_rotation_unix_ms = Some(self.next_rotation_unix_ms);
        });

        let (reader, writer) = stream.into_split();
        let (reader_sender, reader_receiver) = mpsc::channel(64);
        self.reader_task = Some(tokio::spawn(reader_loop(reader, reader_sender)));
        self.reader = Some(reader_receiver);
        self.writer = Some(writer);

        if !self.my_name.is_empty() {
            let payload = serde_json::to_vec(&ProfileMessage {
                name: self.my_name.clone(),
            })
            .map_err(|error| Error::Protocol(error.to_string()))?;
            self.send_encrypted(FrameType::Profile, &payload).await?;
        }
        let _ = self.events.send(ConnectionEvent::Established {
            epoch: 0,
            fingerprint: self.keys.as_ref().expect("keys set").fingerprint(),
            next_rotation_unix_ms: self.next_rotation_unix_ms,
        });
        self.session_loop().await
    }

    async fn accept(&mut self, listener: TcpListener) -> Result<StartupStep> {
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    return Ok(StartupStep::Stream(accepted?.0, false));
                }
                command = self.commands.recv() => {
                    if let Some(end) = Self::preestablish_command(command) {
                        return Ok(StartupStep::End(end));
                    }
                }
            }
        }
    }

    async fn connect(&mut self, host: String, port: u16) -> Result<StartupStep> {
        let connecting = TcpStream::connect((host.as_str(), port));
        tokio::pin!(connecting);
        loop {
            tokio::select! {
                connected = &mut connecting => {
                    return Ok(StartupStep::Stream(connected?, true));
                }
                command = self.commands.recv() => {
                    if let Some(end) = Self::preestablish_command(command) {
                        return Ok(StartupStep::End(end));
                    }
                }
            }
        }
    }

    fn preestablish_command(command: Option<Command>) -> Option<SessionEnd> {
        match command {
            Some(Command::Disconnect { reason, response }) => {
                Some(SessionEnd::local(reason, response))
            }
            Some(Command::SendMessage { response, .. })
            | Some(Command::SendOver { response, .. })
            | Some(Command::SendReaction { response, .. })
            | Some(Command::SetDisplayName { response, .. })
            | Some(Command::Rekey { response }) => {
                let _ = response.send(Err(Error::NotEstablished));
                None
            }
            None => Some(SessionEnd::local("connection handle dropped".into(), None)),
        }
    }

    async fn handshake(&mut self, stream: &mut TcpStream, i_am_a: bool) -> Result<HandshakeStep> {
        let handshake = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            perform_handshake(stream, &self.psk, i_am_a),
        );
        tokio::pin!(handshake);
        loop {
            tokio::select! {
                result = &mut handshake => {
                    let keys = result.map_err(|_| Error::Timeout("handshake"))??;
                    return Ok(HandshakeStep::Keys(keys));
                }
                command = self.commands.recv() => {
                    if let Some(end) = Self::preestablish_command(command) {
                        return Ok(HandshakeStep::End(end));
                    }
                }
            }
        }
    }

    async fn session_loop(&mut self) -> Result<SessionEnd> {
        let mut maintenance = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        maintenance.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let rekey_deadline = self.rekey.as_ref().map(|state| state.deadline);
            let rotation_deadline = self.rotation_deadline;
            tokio::select! {
                reader_event = async {
                    match self.reader.as_mut() {
                        Some(reader) => reader.recv().await,
                        None => pending().await,
                    }
                } => {
                    match reader_event {
                        Some(ReaderEvent::Frame(frame)) => {
                            self.last_receive = Instant::now();
                            if let Some(end) = self.handle_frame(frame).await? {
                                return Ok(end);
                            }
                        }
                        Some(ReaderEvent::Closed) | None => {
                            return Err(Error::Protocol("peer closed the connection".into()));
                        }
                        Some(ReaderEvent::Error(error)) => return Err(error),
                    }
                }
                command = self.commands.recv() => {
                    if let Some(end) = self.handle_command(command).await? {
                        return Ok(end);
                    }
                }
                _ = maintenance.tick() => {
                    self.maintenance().await?;
                }
                _ = tokio::time::sleep_until(rotation_deadline) => {
                    self.rotation_deadline = Instant::now() + self.rotation_period();
                    if self.rekey.is_none() {
                        self.begin_rekey(None).await?;
                    }
                }
                _ = async {
                    match rekey_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => pending().await,
                    }
                } => {
                    self.expire_rekey();
                }
            }
        }
    }

    async fn handle_command(&mut self, command: Option<Command>) -> Result<Option<SessionEnd>> {
        match command {
            Some(Command::SendMessage { uid, content, response }) => {
                let payload = serde_json::to_vec(&ChatMessage { uid, content })
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                let result = self.send_encrypted(FrameType::Data, &payload).await;
                if let Err(error) = result {
                    let reason = error.to_string();
                    let _ = response.send(Err(error));
                    return Err(Error::Protocol(reason));
                }
                let _ = response.send(Ok(()));
            }
            Some(Command::SendOver { kind, response }) => {
                let payload = serde_json::to_vec(&OverMessage { kind })
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                let result = self.send_encrypted(FrameType::Over, &payload).await;
                if let Err(error) = result {
                    let reason = error.to_string();
                    let _ = response.send(Err(error));
                    return Err(Error::Protocol(reason));
                }
                let _ = response.send(Ok(()));
            }
            Some(Command::SendReaction { uid, reaction, response }) => {
                let payload = serde_json::to_vec(&ReactionMessage { uid, reaction })
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                let result = self.send_encrypted(FrameType::Reaction, &payload).await;
                if let Err(error) = result {
                    let reason = error.to_string();
                    let _ = response.send(Err(error));
                    return Err(Error::Protocol(reason));
                }
                let _ = response.send(Ok(()));
            }
            Some(Command::SetDisplayName { name, response }) => {
                self.my_name = name;
                if !self.my_name.is_empty() {
                    let payload = serde_json::to_vec(&ProfileMessage {
                        name: self.my_name.clone(),
                    })
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                    if let Err(error) = self.send_encrypted(FrameType::Profile, &payload).await {
                        let reason = error.to_string();
                        let _ = response.send(Err(error));
                        return Err(Error::Protocol(reason));
                    }
                }
                let _ = response.send(Ok(()));
            }
            Some(Command::Rekey { response }) => {
                if let Some(rekey) = self.rekey.as_mut() {
                    rekey.waiters.push(response);
                } else {
                    self.begin_rekey(Some(response)).await?;
                }
            }
            Some(Command::Disconnect { reason, response }) => {
                return Ok(Some(SessionEnd::local(reason, response)));
            }
            None => {
                return Ok(Some(SessionEnd::local(
                    "connection handle dropped".into(),
                    None,
                )));
            }
        }
        Ok(None)
    }

    async fn handle_frame(&mut self, frame: Frame) -> Result<Option<SessionEnd>> {
        match frame.frame_type {
            FrameType::Data => {
                let epoch = frame.epoch;
                let plaintext = self.decrypt_incoming(&frame)?;
                let message: ChatMessage = serde_json::from_slice(&plaintext)
                    .map_err(|_| Error::Protocol("malformed chat message JSON".into()))?;
                let _ = self.events.send(ConnectionEvent::Message {
                    uid: message.uid,
                    from: self.peer_name.clone().unwrap_or_else(|| "对方".into()),
                    content: message.content,
                    timestamp_unix_ms: unix_time_ms(),
                    epoch,
                });
            }
            FrameType::Profile => {
                let plaintext = self.decrypt_incoming(&frame)?;
                let profile: ProfileMessage = serde_json::from_slice(&plaintext)
                    .map_err(|_| Error::Protocol("malformed profile JSON".into()))?;
                self.peer_name = Some(profile.name.clone());
                update_snapshot(&self.snapshot, |snapshot| {
                    snapshot.peer_name = Some(profile.name.clone());
                });
                let _ = self
                    .events
                    .send(ConnectionEvent::PeerName { name: profile.name });
            }
            FrameType::Rekey => self.handle_rekey(frame).await?,
            FrameType::RekeyRequest => {
                self.decrypt_incoming(&frame)?;
                if self
                    .keys
                    .as_ref()
                    .expect("session keys")
                    .i_am_low_tiebreak()
                    && self.rekey.is_none()
                {
                    self.begin_rekey(None).await?;
                }
            }
            FrameType::Ping => {
                self.decrypt_incoming(&frame)?;
                let should_finish = self.rekey.as_ref().is_some_and(|state| {
                    state.switched && !state.driver && frame.epoch == state.start_epoch + 1
                });
                if should_finish {
                    self.finish_rekey();
                }
            }
            FrameType::Over => {
                let plaintext = self.decrypt_incoming(&frame)?;
                let over: OverMessage = serde_json::from_slice(&plaintext)
                    .map_err(|_| Error::Protocol("malformed OVER JSON".into()))?;
                let _ = self.events.send(ConnectionEvent::Over { kind: over.kind });
            }
            FrameType::Reaction => {
                let plaintext = self.decrypt_incoming(&frame)?;
                let reaction: ReactionMessage = serde_json::from_slice(&plaintext)
                    .map_err(|_| Error::Protocol("malformed reaction JSON".into()))?;
                let _ = self.events.send(ConnectionEvent::Reaction {
                    uid: reaction.uid,
                    reaction: reaction.reaction,
                });
            }
            FrameType::Bye => return Ok(Some(SessionEnd::peer("peer ended the session"))),
            FrameType::Hello | FrameType::Confirm => {
                return Err(Error::Protocol(format!(
                    "unexpected {:?} frame in established session",
                    frame.frame_type
                )));
            }
        }
        Ok(None)
    }

    fn decrypt_incoming(&mut self, frame: &Frame) -> Result<Vec<u8>> {
        let current = self.keys.as_ref().ok_or(Error::NotEstablished)?;
        let keys = if frame.epoch == current.epoch {
            current
        } else if self
            .previous_recv_keys
            .as_ref()
            .is_some_and(|previous| previous.epoch == frame.epoch)
        {
            self.previous_recv_keys.as_ref().expect("previous keys")
        } else {
            return Err(Error::Protocol(format!(
                "unknown key epoch {}",
                frame.epoch
            )));
        };
        let plaintext = open(
            keys.peer_key(),
            keys.peer_nonce_base(),
            frame.frame_type,
            frame.epoch,
            &frame.payload,
        )?;
        let counter = u32::from_be_bytes(frame.payload[..4].try_into().expect("open checked size"));
        let last = self.recv_counters.get(&frame.epoch).copied().unwrap_or(0);
        if counter <= last {
            return Err(Error::Protocol(
                "replayed or out-of-order encrypted frame".into(),
            ));
        }
        self.recv_counters.insert(frame.epoch, counter);
        Ok(plaintext)
    }

    async fn send_encrypted(&mut self, frame_type: FrameType, plaintext: &[u8]) -> Result<()> {
        let keys = self.keys.as_ref().ok_or(Error::NotEstablished)?;
        let counter = self
            .send_counter
            .checked_add(1)
            .ok_or_else(|| Error::Protocol("send counter exhausted; rekey required".into()))?;
        let payload = seal(
            keys.my_key(),
            keys.my_nonce_base(),
            counter,
            frame_type,
            keys.epoch,
            plaintext,
        )?;
        let frame = Frame::new(frame_type, keys.epoch, payload)?;
        let writer = self.writer.as_mut().ok_or(Error::Closed)?;
        self.send_counter = counter;
        write_frame(writer, &frame).await
    }

    async fn begin_rekey(&mut self, waiter: Option<Response>) -> Result<()> {
        let keys = self.keys.as_ref().ok_or(Error::NotEstablished)?;
        let epoch = keys.epoch;
        let driver = keys.i_am_low_tiebreak();
        let state = RekeyState::new(epoch, driver, waiter)?;
        let hello = state.my_hello.clone();
        self.rekey = Some(state);
        if driver {
            self.send_encrypted(FrameType::Rekey, &hello).await?;
        } else {
            self.send_encrypted(FrameType::RekeyRequest, &[]).await?;
        }
        Ok(())
    }

    async fn handle_rekey(&mut self, frame: Frame) -> Result<()> {
        let current_epoch = self.keys.as_ref().ok_or(Error::NotEstablished)?.epoch;
        if frame.epoch < current_epoch {
            let duplicate_previous = self
                .previous_recv_keys
                .as_ref()
                .is_some_and(|keys| keys.epoch == frame.epoch)
                && frame.epoch.checked_add(1) == Some(current_epoch);
            if duplicate_previous {
                return Ok(());
            }
        }
        if frame.epoch != current_epoch {
            return Err(Error::Protocol(format!(
                "invalid Rekey epoch {}",
                frame.epoch
            )));
        }
        let plaintext = self.decrypt_incoming(&frame)?;
        let peer = parse_hello_payload(&plaintext)?;
        if self.rekey.is_none() {
            let driver = self
                .keys
                .as_ref()
                .expect("session keys")
                .i_am_low_tiebreak();
            self.rekey = Some(RekeyState::new(current_epoch, driver, None)?);
        }

        let (next, my_hello, driver) = {
            let current = self.keys.as_ref().expect("session keys");
            let rekey = self.rekey.as_mut().expect("rekey state");
            if rekey.start_epoch != current_epoch || rekey.switched {
                return Err(Error::Protocol("conflicting Rekey state".into()));
            }
            let ephemeral = rekey
                .ephemeral
                .take()
                .ok_or_else(|| Error::Protocol("duplicate current-epoch Rekey".into()))?;
            let own = parse_hello_payload(&rekey.my_hello)?;
            let shared = ephemeral.shared_secret(&peer.public_spki)?;
            let transcript = build_next_transcript(
                current,
                &own.public_spki,
                &own.nonce,
                &peer.public_spki,
                &peer.nonce,
            );
            let material = derive_material(&shared, &self.psk, &transcript, current_epoch + 1)?;
            let next = SessionKeys::new(
                material,
                current_epoch + 1,
                transcript,
                current.i_am_a,
                current.my_tiebreak,
                current.peer_tiebreak,
            );
            (next, rekey.my_hello.clone(), rekey.driver)
        };

        if !driver {
            self.send_encrypted(FrameType::Rekey, &my_hello).await?;
        }
        let previous = self.keys.take().expect("session keys");
        let previous_epoch = previous.epoch;
        self.previous_recv_keys = Some(previous);
        self.recv_counters
            .retain(|epoch, _| *epoch == previous_epoch || *epoch == current_epoch + 1);
        self.keys = Some(next);
        self.send_counter = 0;
        self.reset_rotation();
        if let Some(rekey) = self.rekey.as_mut() {
            rekey.switched = true;
        }
        let keys = self.keys.as_ref().expect("next keys");
        let epoch = keys.epoch;
        let fingerprint = keys.fingerprint();
        let next_rotation_unix_ms = self.next_rotation_unix_ms;
        update_snapshot(&self.snapshot, |snapshot| {
            snapshot.epoch = epoch;
            snapshot.fingerprint = fingerprint.clone();
            snapshot.next_rotation_unix_ms = Some(next_rotation_unix_ms);
        });
        let _ = self.events.send(ConnectionEvent::KeyRotated {
            epoch,
            fingerprint,
            next_rotation_unix_ms,
        });

        if driver {
            self.send_encrypted(FrameType::Ping, &[]).await?;
            self.finish_rekey();
        }
        Ok(())
    }

    fn finish_rekey(&mut self) {
        if let Some(mut rekey) = self.rekey.take() {
            for waiter in rekey.waiters.drain(..) {
                let _ = waiter.send(Ok(()));
            }
        }
    }

    fn expire_rekey(&mut self) {
        if let Some(mut rekey) = self.rekey.take() {
            for waiter in rekey.waiters.drain(..) {
                let _ = waiter.send(Err(Error::Timeout("rekey")));
            }
        }
    }

    async fn maintenance(&mut self) -> Result<()> {
        if self.last_receive.elapsed() > Duration::from_secs(self.options.dead_seconds) {
            return Err(Error::Timeout("connection dead timeout"));
        }
        if self.last_receive.elapsed() >= Duration::from_secs(self.options.keep_alive_seconds) {
            self.send_encrypted(FrameType::Ping, &[]).await?;
        }
        Ok(())
    }

    fn rotation_period(&self) -> Duration {
        Duration::from_secs(u64::from(self.options.rotation_minutes) * 60)
    }

    fn reset_rotation(&mut self) {
        self.rotation_deadline = Instant::now() + self.rotation_period();
        self.next_rotation_unix_ms = unix_time_ms()
            .saturating_add((self.rotation_period().as_millis()).min(i64::MAX as u128) as i64);
    }

    async fn finalize(&mut self, end: SessionEnd) {
        if end.notify_peer && self.writer.is_some() && self.keys.is_some() {
            let _ = tokio::time::timeout(
                DISCONNECT_WRITE_TIMEOUT,
                self.send_encrypted(FrameType::Bye, &[]),
            )
            .await;
        }
        if let Some(task) = self.reader_task.take() {
            task.abort();
            let _ = task.await;
        }
        self.reader = None;
        if let Some(writer) = self.writer.as_mut() {
            let _ = tokio::time::timeout(DISCONNECT_WRITE_TIMEOUT, writer.shutdown()).await;
        }
        self.writer = None;
        self.rekey = None;
        self.keys = None;
        self.previous_recv_keys = None;
        self.recv_counters.clear();
        update_snapshot(&self.snapshot, |snapshot| {
            snapshot.state = ConnState::Closed;
            snapshot.next_rotation_unix_ms = None;
        });
        let _ = self
            .events
            .send(ConnectionEvent::Disconnected { reason: end.reason });
        if let Some(response) = end.response {
            let _ = response.send(Ok(()));
        }
    }
}

async fn run_actor(mut actor: Actor, mode: StartMode) {
    let end = match actor.start(mode).await {
        Ok(end) => end,
        Err(error) => SessionEnd::peer(error.to_string()),
    };
    actor.finalize(end).await;
}

async fn reader_loop(mut reader: OwnedReadHalf, sender: mpsc::Sender<ReaderEvent>) {
    loop {
        match read_frame(&mut reader).await {
            Ok(Some(frame)) => {
                if sender.send(ReaderEvent::Frame(frame)).await.is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = sender.send(ReaderEvent::Closed).await;
                return;
            }
            Err(error) => {
                let _ = sender.send(ReaderEvent::Error(error)).await;
                return;
            }
        }
    }
}

async fn perform_handshake(stream: &mut TcpStream, psk: &str, i_am_a: bool) -> Result<SessionKeys> {
    let ephemeral = EphemeralKey::generate()?;
    let hello = ephemeral.hello_payload()?;
    let peer_frame = if i_am_a {
        write_frame(stream, &Frame::new(FrameType::Hello, 0, hello.clone())?).await?;
        expect_frame(stream, FrameType::Hello).await?
    } else {
        let peer = expect_frame(stream, FrameType::Hello).await?;
        write_frame(stream, &Frame::new(FrameType::Hello, 0, hello.clone())?).await?;
        peer
    };
    let peer = parse_hello_payload(&peer_frame.payload)?;
    let transcript = build_transcript(
        ephemeral.public_spki(),
        ephemeral.nonce(),
        &peer.public_spki,
        &peer.nonce,
        i_am_a,
    );
    let shared = ephemeral.shared_secret(&peer.public_spki)?;
    let material = derive_material(&shared, psk, &transcript, 0)?;

    let mut my_tiebreak = [0_u8; TIEBREAK_SIZE];
    OsRng.fill_bytes(&mut my_tiebreak);
    let my_mac = build_confirm_mac(&material.confirm_key, &transcript, i_am_a);
    let mut confirm = [0_u8; CONFIRM_MAC_SIZE + TIEBREAK_SIZE];
    confirm[..CONFIRM_MAC_SIZE].copy_from_slice(&my_mac);
    confirm[CONFIRM_MAC_SIZE..].copy_from_slice(&my_tiebreak);

    let peer_tiebreak = if i_am_a {
        write_frame(
            stream,
            &Frame::new(FrameType::Confirm, 0, confirm.to_vec())?,
        )
        .await?;
        let peer = expect_frame(stream, FrameType::Confirm).await?;
        verify_peer_confirm(&peer, &material.confirm_key, &transcript, i_am_a)?
    } else {
        let peer = expect_frame(stream, FrameType::Confirm).await?;
        let peer_tiebreak = verify_peer_confirm(&peer, &material.confirm_key, &transcript, i_am_a)?;
        write_frame(
            stream,
            &Frame::new(FrameType::Confirm, 0, confirm.to_vec())?,
        )
        .await?;
        peer_tiebreak
    };
    Ok(SessionKeys::new(
        material,
        0,
        transcript,
        i_am_a,
        my_tiebreak,
        peer_tiebreak,
    ))
}

async fn expect_frame(stream: &mut TcpStream, frame_type: FrameType) -> Result<Frame> {
    let frame = read_frame(stream)
        .await?
        .ok_or_else(|| Error::Protocol("peer closed during handshake".into()))?;
    if frame.frame_type != frame_type {
        return Err(Error::Protocol(format!(
            "expected {frame_type:?}, received {:?}",
            frame.frame_type
        )));
    }
    Ok(frame)
}

fn verify_peer_confirm(
    frame: &Frame,
    confirm_key: &[u8; 32],
    transcript: &[u8; 32],
    i_am_a: bool,
) -> Result<[u8; TIEBREAK_SIZE]> {
    if frame.payload.len() != CONFIRM_MAC_SIZE + TIEBREAK_SIZE {
        return Err(Error::Protocol("invalid Confirm payload length".into()));
    }
    verify_confirm_mac(
        confirm_key,
        transcript,
        !i_am_a,
        &frame.payload[..CONFIRM_MAC_SIZE],
    )?;
    Ok(frame.payload[CONFIRM_MAC_SIZE..]
        .try_into()
        .expect("checked tiebreak length"))
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keys(epoch: u32) -> SessionKeys {
        let material = derive_material(&[7; 32], "test-psk", &[9; 32], epoch).unwrap();
        SessionKeys::new(material, epoch, [9; 32], true, [1; 8], [2; 8])
    }

    fn test_actor() -> Actor {
        let (events, _) = mpsc::unbounded_channel();
        let (_, commands) = mpsc::channel(1);
        let snapshot = Arc::new(RwLock::new(ConnectionSnapshot::new(
            ConnState::Established,
            None,
        )));
        let mut actor = Actor::new(
            Zeroizing::new("test-psk".into()),
            "test".into(),
            SecureChatOptions::default(),
            events,
            snapshot,
            commands,
        );
        actor.keys = Some(test_keys(0));
        actor
    }

    fn incoming_data(actor: &Actor, counter: u32) -> Frame {
        let keys = actor.keys.as_ref().unwrap();
        let payload = seal(
            keys.peer_key(),
            keys.peer_nonce_base(),
            counter,
            FrameType::Data,
            keys.epoch,
            b"payload",
        )
        .unwrap();
        Frame::new(FrameType::Data, keys.epoch, payload).unwrap()
    }

    #[test]
    fn replay_is_rejected_and_failed_authentication_does_not_advance_counter() {
        let mut actor = test_actor();
        let first = incoming_data(&actor, 1);
        assert_eq!(actor.decrypt_incoming(&first).unwrap(), b"payload");
        assert!(matches!(
            actor.decrypt_incoming(&first),
            Err(Error::Protocol(_))
        ));

        let mut tampered = incoming_data(&actor, 2);
        tampered.payload[5] ^= 1;
        assert!(matches!(
            actor.decrypt_incoming(&tampered),
            Err(Error::Authentication(_))
        ));
        let legitimate_second = incoming_data(&actor, 2);
        assert_eq!(
            actor.decrypt_incoming(&legitimate_second).unwrap(),
            b"payload"
        );
    }

    #[test]
    fn only_current_and_previous_receive_epochs_are_accepted() {
        let mut actor = test_actor();
        let old_frame = incoming_data(&actor, 1);
        actor.previous_recv_keys = actor.keys.take();
        actor.keys = Some(test_keys(1));
        assert_eq!(actor.decrypt_incoming(&old_frame).unwrap(), b"payload");
        assert!(matches!(
            actor.decrypt_incoming(&old_frame),
            Err(Error::Protocol(_))
        ));
        let mut unknown = incoming_data(&actor, 1);
        unknown.epoch = 2;
        assert!(actor.decrypt_incoming(&unknown).is_err());
    }

    #[tokio::test]
    async fn duplicate_rekey_from_previous_epoch_is_ignored() {
        let mut actor = test_actor();
        actor.previous_recv_keys = actor.keys.take();
        actor.keys = Some(test_keys(1));
        let duplicate = Frame::new(FrameType::Rekey, 0, Vec::new()).unwrap();
        actor.handle_rekey(duplicate).await.unwrap();
        assert_eq!(actor.keys.as_ref().unwrap().epoch, 1);
    }

    #[test]
    fn rekey_state_uses_a_ten_second_deadline() {
        let state = RekeyState::new(0, true, None).unwrap();
        let remaining = state.deadline.duration_since(Instant::now());
        assert!(remaining <= REKEY_TIMEOUT);
        assert!(remaining > REKEY_TIMEOUT - Duration::from_secs(1));
    }
}
