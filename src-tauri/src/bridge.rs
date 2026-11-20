use std::sync::{atomic::Ordering, Arc};

use securechat_core::ConnectionEvent;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::{commands::EventState, desktop};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionEvent {
    session_id: u64,
    seq: u64,
    kind: &'static str,
    payload: Value,
}

pub fn spawn(
    app: AppHandle,
    session_id: u64,
    events: Arc<EventState>,
    mut receiver: mpsc::UnboundedReceiver<ConnectionEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let seq = events.seq.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(status) = status_for(&event) {
                *events
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = status;
            }
            if let ConnectionEvent::Message { ref from, .. } = event {
                desktop::notify_incoming(&app, from);
            }
            let (kind, payload) = convert(event);
            let _ = app.emit(
                "session-event",
                SessionEvent {
                    session_id,
                    seq,
                    kind,
                    payload,
                },
            );
        }
    });
}

fn status_for(event: &ConnectionEvent) -> Option<String> {
    match event {
        ConnectionEvent::Listening { port } => {
            Some(format!("房间已创建，正在监听端口 {port}，等待对方加入…"))
        }
        ConnectionEvent::Connecting { host, port } => Some(format!("正在连接 {host}:{port}…")),
        ConnectionEvent::Handshaking => Some("正在协商端到端加密密钥…".into()),
        ConnectionEvent::Established { .. } => Some("安全会话已建立（端到端加密）".into()),
        ConnectionEvent::Disconnected { reason } => Some(format!("已断开：{reason}")),
        _ => None,
    }
}

fn convert(event: ConnectionEvent) -> (&'static str, Value) {
    match event {
        ConnectionEvent::Listening { .. } => (
            "state",
            state_payload("listening", "房间已创建，等待对方加入…", 0, "", None),
        ),
        ConnectionEvent::Connecting { host, port } => (
            "state",
            state_payload(
                "connecting",
                &format!("正在连接 {host}:{port}…"),
                0,
                "",
                None,
            ),
        ),
        ConnectionEvent::Handshaking => (
            "state",
            state_payload("handshaking", "正在协商端到端加密密钥…", 0, "", None),
        ),
        ConnectionEvent::Established {
            epoch,
            fingerprint,
            next_rotation_unix_ms,
        } => (
            "state",
            state_payload(
                "established",
                "安全会话已建立（端到端加密）",
                epoch,
                &fingerprint,
                Some(next_rotation_unix_ms),
            ),
        ),
        ConnectionEvent::Message {
            uid,
            from,
            content,
            timestamp_unix_ms,
            ..
        } => (
            "message",
            json!({
                "uid": uid,
                "sender": from,
                "content": content,
                "sentAtUnixMs": timestamp_unix_ms,
            }),
        ),
        ConnectionEvent::PeerName { name } => ("peer-name", json!({ "name": name })),
        ConnectionEvent::KeyRotated {
            epoch,
            fingerprint,
            next_rotation_unix_ms,
        } => (
            "key-rotated",
            json!({
                "epoch": epoch,
                "fingerprint": fingerprint,
                "nextRotationUnixMs": next_rotation_unix_ms,
            }),
        ),
        ConnectionEvent::Over { kind } => ("over", json!({ "kind": kind })),
        ConnectionEvent::Reaction { uid, reaction } => {
            ("reaction", json!({ "uid": uid, "reaction": reaction }))
        }
        ConnectionEvent::Disconnected { reason } => ("disconnected", json!({ "reason": reason })),
    }
}

fn state_payload(
    state: &'static str,
    status: &str,
    epoch: u32,
    fingerprint: &str,
    next_rotation_unix_ms: Option<i64>,
) -> Value {
    json!({
        "state": state,
        "status": status,
        "epoch": epoch,
        "fingerprint": fingerprint,
        "nextRotationUnixMs": next_rotation_unix_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_event_does_not_change_shape_or_include_epoch() {
        let (kind, payload) = convert(ConnectionEvent::Message {
            uid: "uid-1".into(),
            from: "peer".into(),
            content: "secret".into(),
            timestamp_unix_ms: 42,
            epoch: 3,
        });
        assert_eq!(kind, "message");
        assert_eq!(
            payload,
            json!({"uid": "uid-1", "sender": "peer", "content": "secret", "sentAtUnixMs": 42})
        );
    }

    #[test]
    fn over_event_preserves_v1_kind() {
        let (kind, payload) = convert(ConnectionEvent::Over {
            kind: "request".into(),
        });
        assert_eq!(kind, "over");
        assert_eq!(payload, json!({"kind": "request"}));
    }
}
