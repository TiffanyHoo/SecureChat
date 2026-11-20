use std::collections::HashSet;
use std::net::IpAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use securechat_core::protocol::{
    decode_frame, read_frame, write_frame, ChatMessage, Frame, FrameType, OverMessage,
    ProfileMessage, HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
use securechat_core::{
    candidate_host_ips, create_room_code, generate_psk, parse_room_code, psk_to_text,
    try_parse_room_code, Error,
};
use tokio::io::{duplex, AsyncWriteExt};

#[test]
fn frame_header_is_exact_v1_big_endian() {
    let frame = Frame::new(FrameType::Data, 0x0102_0304, vec![0xaa, 0xbb]).unwrap();
    let expected = vec![0, 0, 0, 11, 3, 1, 2, 3, 4, 0xaa, 0xbb];
    assert_eq!(frame.encode().unwrap(), expected);
    let (decoded, used) = decode_frame(&expected).unwrap().unwrap();
    assert_eq!(used, expected.len());
    assert_eq!(decoded, frame);
}

#[test]
fn incremental_decoder_handles_every_half_packet_split() {
    let first = Frame::new(FrameType::Ping, 7, vec![1, 2, 3]).unwrap();
    let second = Frame::new(FrameType::Over, 8, b"sticky".to_vec()).unwrap();
    let first_bytes = first.encode().unwrap();
    let second_bytes = second.encode().unwrap();
    for split in 0..first_bytes.len() {
        assert!(decode_frame(&first_bytes[..split]).unwrap().is_none());
    }
    let mut coalesced = first_bytes.clone();
    coalesced.extend_from_slice(&second_bytes);
    let (decoded_first, used) = decode_frame(&coalesced).unwrap().unwrap();
    assert_eq!(decoded_first, first);
    let (decoded_second, used_second) = decode_frame(&coalesced[used..]).unwrap().unwrap();
    assert_eq!(decoded_second, second);
    assert_eq!(used + used_second, coalesced.len());
}

#[tokio::test]
async fn async_reader_reassembles_fragmented_coalesced_frames() {
    let first = Frame::new(FrameType::Data, 0, b"first".to_vec()).unwrap();
    let second = Frame::new(FrameType::Ping, 0, Vec::new()).unwrap();
    let mut bytes = first.encode().unwrap();
    bytes.extend_from_slice(&second.encode().unwrap());
    let (mut writer, mut reader) = duplex(128);
    let writing = tokio::spawn(async move {
        for chunk in bytes.chunks(3) {
            writer.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
    });
    assert_eq!(read_frame(&mut reader).await.unwrap(), Some(first));
    assert_eq!(read_frame(&mut reader).await.unwrap(), Some(second));
    writing.await.unwrap();
}

#[test]
fn invalid_frames_are_rejected() {
    for total in [0_u32, 1, 8, (HEADER_SIZE + MAX_PAYLOAD_SIZE + 1) as u32] {
        let mut bytes = vec![0_u8; HEADER_SIZE];
        bytes[..4].copy_from_slice(&total.to_be_bytes());
        bytes[4] = FrameType::Data as u8;
        assert!(matches!(decode_frame(&bytes), Err(Error::InvalidFrame(_))));
    }
    for frame_type in [0_u8, 10, 255] {
        let mut bytes = vec![0_u8; HEADER_SIZE];
        bytes[..4].copy_from_slice(&(HEADER_SIZE as u32).to_be_bytes());
        bytes[4] = frame_type;
        assert!(matches!(decode_frame(&bytes), Err(Error::InvalidFrame(_))));
    }
    assert!(Frame::new(FrameType::Data, 0, vec![0; MAX_PAYLOAD_SIZE + 1]).is_err());
    assert!(Frame::new(FrameType::Data, 0, vec![0; MAX_PAYLOAD_SIZE]).is_ok());
}

#[tokio::test]
async fn truncated_network_frame_is_eof() {
    let (mut writer, mut reader) = duplex(64);
    writer.write_all(&[0, 0, 0, 20, 3, 0]).await.unwrap();
    writer.shutdown().await.unwrap();
    assert_eq!(read_frame(&mut reader).await.unwrap(), None);
}

#[test]
fn json_payloads_match_dotnet_camel_case() {
    let chat = serde_json::to_vec(&ChatMessage {
        content: "你好🦀".into(),
    })
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&chat).unwrap()["content"],
        "你好🦀"
    );
    assert_eq!(
        serde_json::to_string(&ProfileMessage {
            name: "Alice".into()
        })
        .unwrap(),
        r#"{"name":"Alice"}"#
    );
    assert_eq!(
        serde_json::to_string(&OverMessage {
            kind: "request".into()
        })
        .unwrap(),
        r#"{"kind":"request"}"#
    );
    assert!(serde_json::from_slice::<ChatMessage>(b"not-json").is_err());
}

#[test]
fn room_code_matches_fixed_v1_base64url_vector() {
    let psk: Vec<u8> = (0_u8..32).collect();
    let expected = "SC1-AQTAqAFkIykADwABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";
    assert_eq!(
        create_room_code("192.168.1.100", 9001, 15, &psk).unwrap(),
        expected
    );
    let parsed = parse_room_code(expected).unwrap();
    assert_eq!(parsed.host, "192.168.1.100");
    assert_eq!(parsed.port, 9001);
    assert_eq!(parsed.rotation_minutes, 15);
    assert_eq!(parsed.psk_text, psk_to_text(&psk));
}

#[test]
fn room_code_roundtrip_supports_ipv4_ipv6_and_boundaries() {
    let psk: Vec<u8> = (0_u8..32).collect();
    for (host, port, rotation) in [
        ("192.168.1.100", 1, 1),
        ("2001:db8::1", 65535, 720),
        ("::1", 9001, 15),
    ] {
        let code = create_room_code(host, port, rotation, &psk).unwrap();
        let mixed_case_prefix = format!("  sc1-{}\r\n", &code[4..]);
        let room = parse_room_code(&mixed_case_prefix).unwrap();
        assert_eq!(room.host, host.parse::<IpAddr>().unwrap().to_string());
        assert_eq!(room.port, port);
        assert_eq!(room.rotation_minutes, rotation);
        assert_eq!(room.psk_text, psk_to_text(&psk));
        assert_eq!(room.psk_text.len(), 43);
    }
}

#[test]
fn room_code_rejects_malformed_and_out_of_range_values() {
    let psk = [7_u8; 32];
    assert!(create_room_code("not an IP", 1, 1, &psk).is_err());
    assert!(create_room_code("127.0.0.1", 0, 1, &psk).is_err());
    assert!(create_room_code("127.0.0.1", 1, 0, &psk).is_err());
    assert!(create_room_code("127.0.0.1", 1, 721, &psk).is_err());
    assert!(create_room_code("127.0.0.1", 1, 1, &psk[..31]).is_err());
    for invalid in ["", "plain text", "SC1-!!!!", "SC1-YQ", "SC1-YQ=="] {
        assert!(try_parse_room_code(invalid).is_none(), "accepted {invalid}");
    }
    let valid = create_room_code("10.0.0.5", 7000, 30, &psk).unwrap();
    assert!(try_parse_room_code(&valid[..valid.len() - 4]).is_none());
    assert!(try_parse_room_code(&(valid.clone() + "abcd")).is_none());

    let invalid_cases: [(u8, u8, u16, u16); 5] = [
        (2, 4, 1, 1),
        (1, 5, 1, 1),
        (1, 4, 0, 1),
        (1, 4, 1, 0),
        (1, 4, 1, 721),
    ];
    for (version, ip_len, port, rotation) in invalid_cases {
        let mut payload = vec![version, ip_len];
        payload.extend_from_slice(&[127, 0, 0, 1]);
        payload.extend_from_slice(&port.to_be_bytes());
        payload.extend_from_slice(&rotation.to_be_bytes());
        payload.extend_from_slice(&psk);
        let code = format!("SC1-{}", URL_SAFE_NO_PAD.encode(payload));
        assert!(try_parse_room_code(&code).is_none());
    }
}

#[test]
fn generated_psks_are_random_256_bit_values() {
    let first = generate_psk();
    let second = generate_psk();
    assert_ne!(first, second);
    assert_eq!(psk_to_text(&first).len(), 43);
}

#[test]
fn candidate_ips_are_unique_ipv4_addresses() {
    let candidates = candidate_host_ips();
    assert!(!candidates.is_empty());
    let unique: HashSet<_> = candidates.iter().collect();
    assert_eq!(unique.len(), candidates.len());
    for candidate in candidates {
        assert!(matches!(candidate.parse::<IpAddr>(), Ok(IpAddr::V4(_))));
    }
}

#[tokio::test]
async fn write_frame_emits_the_same_bytes_as_encode() {
    let frame = Frame::new(FrameType::Bye, 99, vec![4, 5, 6]).unwrap();
    let (mut writer, mut reader) = duplex(64);
    write_frame(&mut writer, &frame).await.unwrap();
    assert_eq!(read_frame(&mut reader).await.unwrap(), Some(frame));
}
