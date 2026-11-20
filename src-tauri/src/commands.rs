use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use securechat_core::{
    candidate_host_ips, create_room_code, generate_psk, parse_room_code, psk_to_text, ConnState,
    SecureChatOptions, SecureConnection,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::bridge;

#[derive(Default)]
pub struct AppState {
    current: Mutex<Option<Arc<Session>>>,
    next_session_id: AtomicU64,
}

pub struct Session {
    pub id: u64,
    pub connection: Arc<SecureConnection>,
    pub events: Arc<EventState>,
}

#[derive(Default)]
pub struct EventState {
    pub seq: AtomicU64,
    pub status: Mutex<String>,
}

impl AppState {
    fn ensure_startable(&self) -> Result<(), String> {
        let current = lock(&self.current);
        if current
            .as_ref()
            .is_some_and(|session| session.connection.state() != ConnState::Closed)
        {
            return Err("当前会话尚未结束，请先断开".into());
        }
        Ok(())
    }

    fn install(&self, session: Arc<Session>) {
        *lock(&self.current) = Some(session);
    }

    fn get(&self, session_id: u64) -> Result<Arc<Session>, String> {
        lock(&self.current)
            .as_ref()
            .filter(|session| session.id == session_id)
            .cloned()
            .ok_or_else(|| "会话不存在或已结束".into())
    }

    fn current(&self) -> Option<Arc<Session>> {
        lock(&self.current).clone()
    }

    fn take(&self) -> Option<Arc<Session>> {
        lock(&self.current).take()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomParams {
    host_ip: String,
    port: u16,
    rotation_minutes: u16,
    display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoomParams {
    room_code: String,
    display_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomResult {
    session_id: u64,
    room_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoomResult {
    session_id: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    session_id: Option<u64>,
    seq: u64,
    state: ConnState,
    status: String,
    peer_name: String,
    epoch: u32,
    fingerprint: String,
    next_rotation_unix_ms: Option<i64>,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            session_id: None,
            seq: 0,
            state: ConnState::Idle,
            status: "未连接".into(),
            peer_name: String::new(),
            epoch: 0,
            fingerprint: String::new(),
            next_rotation_unix_ms: None,
        }
    }
}

#[tauri::command]
pub fn list_host_ips() -> Vec<String> {
    candidate_host_ips()
}

#[tauri::command]
pub async fn create_room(
    app: AppHandle,
    state: State<'_, AppState>,
    params: CreateRoomParams,
) -> Result<CreateRoomResult, String> {
    state.ensure_startable()?;
    let psk = Zeroizing::new(generate_psk());
    let room_code = create_room_code(
        &params.host_ip,
        params.port,
        params.rotation_minutes,
        psk.as_slice(),
    )
    .map_err(to_message)?;
    let psk_text = Zeroizing::new(psk_to_text(psk.as_slice()));
    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
    let events = Arc::new(EventState::default());
    *lock(&events.status) = format!("房间已创建，正在监听端口 {}，等待对方加入…", params.port);
    let (sender, receiver) = mpsc::unbounded_channel();
    let connection = SecureConnection::listen(
        params.port,
        psk_text.as_str(),
        params.display_name,
        SecureChatOptions {
            rotation_minutes: params.rotation_minutes,
            ..SecureChatOptions::default()
        },
        sender,
    )
    .await
    .map_err(to_message)?;
    let connection = Arc::new(connection);
    bridge::spawn(app, session_id, Arc::clone(&events), receiver);
    state.install(Arc::new(Session {
        id: session_id,
        connection,
        events,
    }));
    Ok(CreateRoomResult {
        session_id,
        room_code,
    })
}

#[tauri::command]
pub async fn join_room(
    app: AppHandle,
    state: State<'_, AppState>,
    params: JoinRoomParams,
) -> Result<JoinRoomResult, String> {
    state.ensure_startable()?;
    let room = parse_room_code(&params.room_code).map_err(to_message)?;
    let psk_text = Zeroizing::new(room.psk_text);
    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
    let events = Arc::new(EventState::default());
    *lock(&events.status) = format!("正在连接 {}:{}…", room.host, room.port);
    let (sender, receiver) = mpsc::unbounded_channel();
    let connection = SecureConnection::connect(
        room.host,
        room.port,
        psk_text.as_str(),
        params.display_name,
        SecureChatOptions {
            rotation_minutes: room.rotation_minutes,
            ..SecureChatOptions::default()
        },
        sender,
    )
    .await
    .map_err(to_message)?;
    let connection = Arc::new(connection);
    bridge::spawn(app, session_id, Arc::clone(&events), receiver);
    state.install(Arc::new(Session {
        id: session_id,
        connection,
        events,
    }));
    Ok(JoinRoomResult { session_id })
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> SessionSnapshot {
    let Some(session) = state.current() else {
        return SessionSnapshot::default();
    };
    let snapshot = session.connection.snapshot();
    let status = lock(&session.events.status).clone();
    SessionSnapshot {
        session_id: Some(session.id),
        seq: session.events.seq.load(Ordering::Relaxed),
        state: snapshot.state,
        status,
        peer_name: snapshot.peer_name.unwrap_or_default(),
        epoch: snapshot.epoch,
        fingerprint: snapshot.fingerprint,
        next_rotation_unix_ms: snapshot.next_rotation_unix_ms,
    }
}

#[tauri::command]
pub async fn disconnect(
    state: State<'_, AppState>,
    session_id: u64,
    reason: Option<String>,
) -> Result<(), String> {
    let session = state.get(session_id)?;
    session
        .connection
        .disconnect_with_reason(reason.unwrap_or_else(|| "本地主动断开".into()))
        .await
        .map_err(to_message)
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    session_id: u64,
    uid: String,
    content: String,
) -> Result<(), String> {
    let session = state.get(session_id)?;
    session
        .connection
        .send_message(uid, content)
        .await
        .map_err(to_message)
}

#[tauri::command]
pub async fn send_reaction(
    state: State<'_, AppState>,
    session_id: u64,
    uid: String,
    reaction: String,
) -> Result<(), String> {
    if !matches!(reaction.as_str(), "like" | "heart" | "copythat") {
        return Err("无效的表情回应".into());
    }
    if uid.trim().is_empty() {
        return Err("无效的消息标识".into());
    }
    let session = state.get(session_id)?;
    session
        .connection
        .send_reaction(uid, reaction)
        .await
        .map_err(to_message)
}

#[tauri::command]
pub async fn rekey_now(state: State<'_, AppState>, session_id: u64) -> Result<(), String> {
    let session = state.get(session_id)?;
    session.connection.rekey_now().await.map_err(to_message)
}

#[tauri::command]
pub async fn send_over(
    state: State<'_, AppState>,
    session_id: u64,
    kind: String,
) -> Result<(), String> {
    if !matches!(kind.as_str(), "request" | "accept" | "reject" | "cancel") {
        return Err("无效的 OVER 操作".into());
    }
    let session = state.get(session_id)?;
    session.connection.send_over(kind).await.map_err(to_message)
}

pub fn shutdown(app: &AppHandle) {
    let session = app.state::<AppState>().take();
    if let Some(session) = session {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = session.connection.disconnect_with_reason("应用退出").await;
            app.exit(0);
        });
    } else {
        app.exit(0);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn to_message(error: impl std::fmt::Display) -> String {
    error.to_string()
}
