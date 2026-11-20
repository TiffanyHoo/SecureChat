use securechat_core::crypto::{
    build_confirm_mac, build_next_transcript, build_transcript, derive_material, open,
    parse_hello_payload, seal, verify_confirm_mac, EphemeralKey, SessionKeys, CONFIRM_MAC_SIZE,
};
use securechat_core::protocol::FrameType;

const PSK: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn decode_hex<const N: usize>(text: &str) -> [u8; N] {
    assert_eq!(text.len(), N * 2);
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
    }
    output
}

fn decode_vec(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

fn secret(value: u8) -> [u8; 32] {
    let mut secret = [0_u8; 32];
    secret[31] = value;
    secret
}

#[test]
fn deterministic_p256_hkdf_transcript_and_confirm_match_independent_vector() {
    let nonce_a = decode_hex("000102030405060708090a0b0c0d0e0f");
    let nonce_b = decode_hex("101112131415161718191a1b1c1d1e1f");
    let a = EphemeralKey::from_secret_bytes(&secret(1), nonce_a).unwrap();
    let b = EphemeralKey::from_secret_bytes(&secret(2), nonce_b).unwrap();
    assert_eq!(a.public_spki(), decode_vec("3059301306072a8648ce3d020106082a8648ce3d030107034200046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"));
    assert_eq!(b.public_spki(), decode_vec("3059301306072a8648ce3d020106082a8648ce3d030107034200047cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc4766997807775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1"));
    let shared_a = a.shared_secret(b.public_spki()).unwrap();
    let shared_b = b.shared_secret(a.public_spki()).unwrap();
    assert_eq!(&*shared_a, &*shared_b);
    assert_eq!(
        shared_a.as_slice(),
        decode_vec("7cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc47669978")
    );

    let transcript_a =
        build_transcript(a.public_spki(), a.nonce(), b.public_spki(), b.nonce(), true);
    let transcript_b = build_transcript(
        b.public_spki(),
        b.nonce(),
        a.public_spki(),
        a.nonce(),
        false,
    );
    assert_eq!(transcript_a, transcript_b);
    assert_eq!(
        transcript_a,
        decode_hex("1f27b690f9870e422136433ba738b88017de962b6dda740ffb96044f2266f3a6")
    );
    let material = derive_material(&shared_a, PSK, &transcript_a, 0).unwrap();
    assert_eq!(
        material.key_a2b,
        decode_hex("c193adc250c9b6793182e27fdd7ca1ba51175ddd4586ce0c492abecc8e8f16f6")
    );
    assert_eq!(
        material.key_b2a,
        decode_hex("076260ffef113365ab8054e2e94ae968126b1fe15ddc2547fb1b00ee872d0f9e")
    );
    assert_eq!(
        material.confirm_key,
        decode_hex("402a1322a32781b4b9913da8b4e74d7dd4f562ce3e801444db589ee1c4c641ff")
    );
    assert_eq!(material.nonce_base_a2b, decode_hex("13c96b74c30518a3"));
    assert_eq!(material.nonce_base_b2a, decode_hex("7b1b3abd63e87b7a"));
    let confirm_a = build_confirm_mac(&material.confirm_key, &transcript_a, true);
    let confirm_b = build_confirm_mac(&material.confirm_key, &transcript_a, false);
    assert_eq!(
        confirm_a,
        decode_hex::<CONFIRM_MAC_SIZE>(
            "1645c4158001245d2899b3a2678dcc46fa8f376c01409b2c6e61ac45bcba366d"
        )
    );
    assert_eq!(
        confirm_b,
        decode_hex::<CONFIRM_MAC_SIZE>(
            "2639570965acbca05fad3ae053a862884466f721047f9b1e27af0d3c308cbed6"
        )
    );
    verify_confirm_mac(&material.confirm_key, &transcript_a, false, &confirm_b).unwrap();
    assert!(verify_confirm_mac(&material.confirm_key, &transcript_a, true, &confirm_b).is_err());

    let keys = SessionKeys::new(material, 0, transcript_a, true, [1; 8], [2; 8]);
    assert_eq!(keys.fingerprint(), "a7fb5129169d0309");
    assert!(keys.i_am_low_tiebreak());
}

#[test]
fn deterministic_aes_gcm_matches_counter_aad_and_tag_vector() {
    let key = decode_hex("c193adc250c9b6793182e27fdd7ca1ba51175ddd4586ce0c492abecc8e8f16f6");
    let nonce = decode_hex("13c96b74c30518a3");
    let expected = decode_vec("00000001d9c4bfcb36783fbb78b9c3f2068354ad0c3ecd3493e70b0035916f");
    let sealed = seal(&key, &nonce, 1, FrameType::Data, 0, b"test vector").unwrap();
    assert_eq!(sealed, expected);
    assert_eq!(
        open(&key, &nonce, FrameType::Data, 0, &sealed).unwrap(),
        b"test vector"
    );
}

#[test]
fn hello_payload_preserves_spki_and_nonce_and_rejects_bad_lengths() {
    let key = EphemeralKey::from_secret_bytes(&secret(1), [9; 16]).unwrap();
    let payload = key.hello_payload().unwrap();
    assert_eq!(payload[0], 1);
    assert_eq!(
        u16::from_be_bytes([payload[1], payload[2]]) as usize,
        key.public_spki().len()
    );
    let parsed = parse_hello_payload(&payload).unwrap();
    assert_eq!(parsed.public_spki, key.public_spki());
    assert_eq!(parsed.nonce, [9; 16]);
    assert!(parse_hello_payload(&payload[..payload.len() - 1]).is_err());
    let mut wrong_version = payload.clone();
    wrong_version[0] = 2;
    assert!(parse_hello_payload(&wrong_version).is_err());
    let mut wrong_public_length = payload;
    wrong_public_length[2] += 1;
    assert!(parse_hello_payload(&wrong_public_length).is_err());
}

#[test]
fn gcm_authenticates_ciphertext_type_epoch_counter_and_length() {
    let key = [11_u8; 32];
    let nonce = [22_u8; 8];
    let sealed = seal(&key, &nonce, 9, FrameType::Data, 3, b"attack at dawn").unwrap();
    let mut tampered = sealed.clone();
    *tampered.last_mut().unwrap() ^= 0x80;
    assert!(open(&key, &nonce, FrameType::Data, 3, &tampered).is_err());
    assert!(open(&key, &nonce, FrameType::Data, 4, &sealed).is_err());
    assert!(open(&key, &nonce, FrameType::Profile, 3, &sealed).is_err());
    let mut wrong_counter = sealed.clone();
    wrong_counter[3] ^= 1;
    assert!(open(&key, &nonce, FrameType::Data, 3, &wrong_counter).is_err());
    let mut truncated = sealed.clone();
    truncated.pop();
    assert!(open(&key, &nonce, FrameType::Data, 3, &truncated).is_err());
    assert!(open(&key, &nonce, FrameType::Data, 3, &[0; 19]).is_err());
}

#[test]
fn epochs_and_rekey_transcripts_are_deterministic_and_role_independent() {
    let transcript = [3_u8; 32];
    let first = derive_material(&[5; 32], PSK, &transcript, 0).unwrap();
    let first_again = derive_material(&[5; 32], PSK, &transcript, 0).unwrap();
    let second = derive_material(&[5; 32], PSK, &transcript, 1).unwrap();
    assert_eq!(first.key_a2b, first_again.key_a2b);
    assert_ne!(first.key_a2b, first.key_b2a);
    assert_ne!(first.key_a2b, second.key_a2b);
    assert_ne!(
        first.key_a2b,
        derive_material(&[5; 32], "wrong", &transcript, 0)
            .unwrap()
            .key_a2b
    );

    let current_a = SessionKeys::new(first, 0, transcript, true, [1; 8], [2; 8]);
    let current_b = SessionKeys::new(first_again, 0, transcript, false, [2; 8], [1; 8]);
    let a = EphemeralKey::from_secret_bytes(&secret(3), [3; 16]).unwrap();
    let b = EphemeralKey::from_secret_bytes(&secret(4), [4; 16]).unwrap();
    let next_a = build_next_transcript(
        &current_a,
        a.public_spki(),
        a.nonce(),
        b.public_spki(),
        b.nonce(),
    );
    let next_b = build_next_transcript(
        &current_b,
        b.public_spki(),
        b.nonce(),
        a.public_spki(),
        a.nonce(),
    );
    assert_eq!(next_a, next_b);
    assert_ne!(next_a, transcript);
}
