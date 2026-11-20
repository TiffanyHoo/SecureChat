use std::time::Duration;

use securechat_core::{ConnState, ConnectionEvent, Error, SecureChatOptions, SecureConnection};
use tokio::sync::mpsc;
use tokio::time::timeout;

const PSK: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OTHER_PSK: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

struct Pair {
    server: SecureConnection,
    client: SecureConnection,
    server_events: mpsc::UnboundedReceiver<ConnectionEvent>,
    client_events: mpsc::UnboundedReceiver<ConnectionEvent>,
}

async fn create_pair() -> Pair {
    create_pair_with_psk(PSK, PSK).await
}

async fn create_pair_with_psk(server_psk: &str, client_psk: &str) -> Pair {
    let (server_sender, server_events) = mpsc::unbounded_channel();
    let (client_sender, client_events) = mpsc::unbounded_channel();
    let server = SecureConnection::listen(
        0,
        server_psk,
        "监听方",
        SecureChatOptions::default(),
        server_sender,
    )
    .await
    .unwrap();
    let port = server.local_port().unwrap();
    assert_ne!(port, 0);
    let client = SecureConnection::connect(
        "127.0.0.1",
        port,
        client_psk,
        "连接方",
        SecureChatOptions::default(),
        client_sender,
    )
    .await
    .unwrap();
    Pair {
        server,
        client,
        server_events,
        client_events,
    }
}

async fn wait_established(
    events: &mut mpsc::UnboundedReceiver<ConnectionEvent>,
) -> (u32, String, i64) {
    timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await.expect("event channel closed") {
                ConnectionEvent::Established {
                    epoch,
                    fingerprint,
                    next_rotation_unix_ms,
                } => return (epoch, fingerprint, next_rotation_unix_ms),
                ConnectionEvent::Disconnected { reason } => {
                    panic!("disconnected before establishment: {reason}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("establishment timeout")
}

async fn establish_pair() -> Pair {
    let mut pair = create_pair().await;
    let server = wait_established(&mut pair.server_events).await;
    let client = wait_established(&mut pair.client_events).await;
    assert_eq!(server.1, client.1);
    assert_eq!(server.0, 0);
    assert_eq!(client.0, 0);
    assert!(server.2 > 0 && client.2 > 0);
    assert_eq!(pair.server.state(), ConnState::Established);
    assert_eq!(pair.client.state(), ConnState::Established);
    pair
}

async fn next_message(
    events: &mut mpsc::UnboundedReceiver<ConnectionEvent>,
) -> (String, String, u32) {
    timeout(Duration::from_secs(5), async {
        loop {
            if let ConnectionEvent::Message {
                from,
                content,
                epoch,
                ..
            } = events.recv().await.expect("event channel closed")
            {
                return (from, content, epoch);
            }
        }
    })
    .await
    .expect("message timeout")
}

async fn next_reaction(events: &mut mpsc::UnboundedReceiver<ConnectionEvent>) -> (String, String) {
    timeout(Duration::from_secs(5), async {
        loop {
            if let ConnectionEvent::Reaction { uid, reaction } =
                events.recv().await.expect("event channel closed")
            {
                return (uid, reaction);
            }
        }
    })
    .await
    .expect("reaction timeout")
}

async fn next_over(events: &mut mpsc::UnboundedReceiver<ConnectionEvent>) -> String {
    timeout(Duration::from_secs(5), async {
        loop {
            if let ConnectionEvent::Over { kind } =
                events.recv().await.expect("event channel closed")
            {
                return kind;
            }
        }
    })
    .await
    .expect("OVER timeout")
}

async fn next_disconnect(events: &mut mpsc::UnboundedReceiver<ConnectionEvent>) -> String {
    timeout(Duration::from_secs(5), async {
        loop {
            if let ConnectionEvent::Disconnected { reason } =
                events.recv().await.expect("event channel closed")
            {
                return reason;
            }
        }
    })
    .await
    .expect("disconnect timeout")
}

#[tokio::test]
async fn loopback_messages_and_peer_names_flow_in_both_directions() {
    let mut pair = establish_pair().await;
    pair.client
        .send_message("uid-1", "你好,这是密文通道里的明文")
        .await
        .unwrap();
    let (from, content, epoch) = next_message(&mut pair.server_events).await;
    assert_eq!(from, "连接方");
    assert_eq!(content, "你好,这是密文通道里的明文");
    assert_eq!(epoch, 0);
    pair.server.send_message("uid-2", "reversed").await.unwrap();
    assert_eq!(next_message(&mut pair.client_events).await.1, "reversed");
    timeout(Duration::from_secs(5), async {
        while pair.server.peer_name().as_deref() != Some("连接方")
            || pair.client.peer_name().as_deref() != Some("监听方")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn mismatched_psk_disconnects_without_establishing() {
    let mut pair = create_pair_with_psk(PSK, OTHER_PSK).await;
    let server_reason = next_disconnect(&mut pair.server_events).await;
    let client_reason = next_disconnect(&mut pair.client_events).await;
    assert!(!server_reason.is_empty());
    assert!(!client_reason.is_empty());
    assert_eq!(pair.server.state(), ConnState::Closed);
    assert_eq!(pair.client.state(), ConnState::Closed);
    assert!(matches!(
        pair.client.send_message("uid-no", "no").await,
        Err(Error::Closed)
    ));
}

#[tokio::test]
async fn manual_rekey_rotates_epoch_and_fingerprint_and_preserves_messages() {
    let mut pair = establish_pair().await;
    let old_fingerprint = pair.client.fingerprint();
    pair.client.rekey_now().await.unwrap();
    assert_eq!(pair.client.epoch(), 1);
    assert_eq!(pair.server.epoch(), 1);
    assert_eq!(pair.client.fingerprint(), pair.server.fingerprint());
    assert_ne!(pair.client.fingerprint(), old_fingerprint);
    pair.client
        .send_message("uid-3", "换钥后仍然可达")
        .await
        .unwrap();
    let (_, content, epoch) = next_message(&mut pair.server_events).await;
    assert_eq!(content, "换钥后仍然可达");
    assert_eq!(epoch, 1);
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn simultaneous_rekey_converges_with_tiebreak_driver() {
    let mut pair = establish_pair().await;
    let (client_result, server_result) =
        tokio::join!(pair.client.rekey_now(), pair.server.rekey_now());
    client_result.unwrap();
    server_result.unwrap();
    assert_eq!(pair.client.epoch(), pair.server.epoch());
    assert_eq!(pair.client.epoch(), 1);
    assert_eq!(pair.client.fingerprint(), pair.server.fingerprint());
    pair.client
        .send_message("uid-4", "碰撞后消息正常")
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut pair.server_events).await.1,
        "碰撞后消息正常"
    );
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn messages_can_be_sent_while_rekey_is_in_progress() {
    let mut pair = establish_pair().await;
    let (rekey_result, send_result) = tokio::join!(
        pair.client.rekey_now(),
        pair.client.send_message("uid-5", "during rekey")
    );
    rekey_result.unwrap();
    send_result.unwrap();
    assert_eq!(
        next_message(&mut pair.server_events).await.1,
        "during rekey"
    );
    pair.server
        .send_message("uid-6", "reverse after rekey")
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut pair.client_events).await.1,
        "reverse after rekey"
    );
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn over_request_accept_reject_and_cancel_are_json_payloads() {
    let mut pair = establish_pair().await;
    for kind in ["request", "reject", "cancel"] {
        pair.client.send_over(kind).await.unwrap();
        assert_eq!(next_over(&mut pair.server_events).await, kind);
    }
    pair.server.send_over("accept").await.unwrap();
    assert_eq!(next_over(&mut pair.client_events).await, "accept");
    pair.client
        .send_message("uid-7", "协商后仍在线")
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut pair.server_events).await.1,
        "协商后仍在线"
    );
    assert_eq!(pair.client.state(), ConnState::Established);
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn reaction_targets_peer_message_uid() {
    let mut pair = establish_pair().await;
    pair.client
        .send_message("msg-42", "需要回应的消息")
        .await
        .unwrap();
    let (_, content, _) = next_message(&mut pair.server_events).await;
    assert_eq!(content, "需要回应的消息");
    pair.server.send_reaction("msg-42", "like").await.unwrap();
    assert_eq!(
        next_reaction(&mut pair.client_events).await,
        ("msg-42".into(), "like".into())
    );
    pair.client
        .send_reaction("msg-42", "copythat")
        .await
        .unwrap();
    assert_eq!(
        next_reaction(&mut pair.server_events).await,
        ("msg-42".into(), "copythat".into())
    );
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[tokio::test]
async fn graceful_disconnect_notifies_each_side_exactly_once() {
    let mut pair = establish_pair().await;
    pair.client.disconnect().await.unwrap();
    pair.client.disconnect().await.unwrap();
    assert_eq!(
        next_disconnect(&mut pair.server_events).await,
        "peer ended the session"
    );
    assert_eq!(pair.server.state(), ConnState::Closed);
    assert_eq!(pair.client.state(), ConnState::Closed);
    let mut client_disconnects = 0;
    while let Ok(Some(event)) = timeout(Duration::from_millis(100), pair.client_events.recv()).await
    {
        if matches!(event, ConnectionEvent::Disconnected { .. }) {
            client_disconnects += 1;
        }
    }
    assert_eq!(client_disconnects, 1);
    let extra = timeout(Duration::from_millis(100), pair.server_events.recv()).await;
    assert!(!matches!(
        extra,
        Ok(Some(ConnectionEvent::Disconnected { .. }))
    ));
}

#[tokio::test]
async fn listen_returns_before_accept_and_can_be_cancelled_cleanly() {
    let (sender, mut events) = mpsc::unbounded_channel();
    let server = SecureConnection::listen(0, PSK, "waiting", SecureChatOptions::default(), sender)
        .await
        .unwrap();
    assert_eq!(server.state(), ConnState::Listening);
    assert!(matches!(
        events.recv().await,
        Some(ConnectionEvent::Listening { port }) if port == server.local_port().unwrap()
    ));
    server
        .disconnect_with_reason("cancelled by test")
        .await
        .unwrap();
    assert_eq!(next_disconnect(&mut events).await, "cancelled by test");
    assert_eq!(server.state(), ConnState::Closed);
}

#[tokio::test]
async fn repeated_rekeys_form_a_chain_and_keep_both_directions_usable() {
    let mut pair = establish_pair().await;
    let mut previous_fingerprint = pair.client.fingerprint();
    for target_epoch in 1..=3 {
        if target_epoch % 2 == 1 {
            pair.client.rekey_now().await.unwrap();
            pair.client
                .send_message("uid-c", format!("epoch {target_epoch}"))
                .await
                .unwrap();
            assert_eq!(
                next_message(&mut pair.server_events).await.1,
                format!("epoch {target_epoch}")
            );
        } else {
            pair.server.rekey_now().await.unwrap();
            pair.server
                .send_message("uid-s", format!("epoch {target_epoch}"))
                .await
                .unwrap();
            assert_eq!(
                next_message(&mut pair.client_events).await.1,
                format!("epoch {target_epoch}")
            );
        }
        assert_eq!(pair.client.epoch(), target_epoch);
        assert_eq!(pair.server.epoch(), target_epoch);
        assert_eq!(pair.client.fingerprint(), pair.server.fingerprint());
        assert_ne!(pair.client.fingerprint(), previous_fingerprint);
        previous_fingerprint = pair.client.fingerprint();
    }
    pair.client.disconnect().await.unwrap();
    pair.server.disconnect().await.unwrap();
}

#[test]
fn events_and_snapshots_are_serde_serializable() {
    let event = ConnectionEvent::KeyRotated {
        epoch: 2,
        fingerprint: "0011223344556677".into(),
        next_rotation_unix_ms: 123456,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(
        json,
        r#"{"type":"keyRotated","epoch":2,"fingerprint":"0011223344556677","nextRotationUnixMs":123456}"#
    );
    assert_eq!(
        serde_json::from_str::<ConnectionEvent>(&json).unwrap(),
        event
    );
    assert_eq!(
        serde_json::to_string(&ConnState::Established).unwrap(),
        r#""established""#
    );
}

#[test]
fn default_keepalive_and_rotation_values_match_v1() {
    let options = SecureChatOptions::default();
    assert_eq!(options.keep_alive_seconds, 25);
    assert_eq!(options.dead_seconds, 90);
    assert_eq!(options.rotation_minutes, 10);
}
