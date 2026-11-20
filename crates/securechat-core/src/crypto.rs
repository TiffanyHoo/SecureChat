use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::ecdh::diffie_hellman;
use p256::pkcs8::{DecodePublicKey, EncodePublicKey};
use p256::{PublicKey, SecretKey};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::protocol::FrameType;
use crate::{Error, Result};

pub const PROTOCOL_VERSION: u8 = 1;
pub const NONCE_SIZE: usize = 16;
pub const TIEBREAK_SIZE: usize = 8;
pub const CONFIRM_MAC_SIZE: usize = 32;
pub const NONCE_BASE_SIZE: usize = 8;
pub const COUNTER_SIZE: usize = 4;
pub const TAG_SIZE: usize = 16;

type HmacSha256 = Hmac<Sha256>;

pub struct EphemeralKey {
    secret: SecretKey,
    public_spki: Vec<u8>,
    nonce: [u8; NONCE_SIZE],
}

impl EphemeralKey {
    pub fn generate() -> Result<Self> {
        let mut nonce = [0_u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce);
        Self::from_secret(SecretKey::random(&mut OsRng), nonce)
    }

    pub fn from_secret_bytes(secret: &[u8; 32], nonce: [u8; NONCE_SIZE]) -> Result<Self> {
        let secret = SecretKey::from_slice(secret)
            .map_err(|_| Error::InvalidInput("invalid P-256 secret scalar".into()))?;
        Self::from_secret(secret, nonce)
    }

    fn from_secret(secret: SecretKey, nonce: [u8; NONCE_SIZE]) -> Result<Self> {
        let public_spki = secret
            .public_key()
            .to_public_key_der()
            .map_err(|_| Error::Protocol("failed to encode P-256 SPKI".into()))?
            .as_bytes()
            .to_vec();
        Ok(Self {
            secret,
            public_spki,
            nonce,
        })
    }

    pub fn public_spki(&self) -> &[u8] {
        &self.public_spki
    }

    pub fn nonce(&self) -> &[u8; NONCE_SIZE] {
        &self.nonce
    }

    pub fn hello_payload(&self) -> Result<Vec<u8>> {
        build_hello_payload(&self.public_spki, self.nonce)
    }

    pub fn shared_secret(&self, peer_spki: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let peer = PublicKey::from_public_key_der(peer_spki)
            .map_err(|_| Error::Protocol("invalid peer P-256 SPKI public key".into()))?;
        let shared = diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        Ok(Zeroizing::new(shared.raw_secret_bytes().to_vec()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloPayload {
    pub public_spki: Vec<u8>,
    pub nonce: [u8; NONCE_SIZE],
}

pub fn build_hello_payload(public_spki: &[u8], nonce: [u8; NONCE_SIZE]) -> Result<Vec<u8>> {
    let public_len = u16::try_from(public_spki.len())
        .map_err(|_| Error::InvalidInput("SPKI public key is too long".into()))?;
    let mut payload = Vec::with_capacity(3 + public_spki.len() + NONCE_SIZE);
    payload.push(PROTOCOL_VERSION);
    payload.extend_from_slice(&public_len.to_be_bytes());
    payload.extend_from_slice(public_spki);
    payload.extend_from_slice(&nonce);
    Ok(payload)
}

pub fn parse_hello_payload(payload: &[u8]) -> Result<HelloPayload> {
    if payload.len() < 3 + NONCE_SIZE || payload[0] != PROTOCOL_VERSION {
        return Err(Error::Protocol(
            "unsupported protocol version or malformed Hello".into(),
        ));
    }
    let public_len = u16::from_be_bytes([payload[1], payload[2]]) as usize;
    if payload.len() != 3 + public_len + NONCE_SIZE {
        return Err(Error::Protocol("incorrect Hello payload length".into()));
    }
    Ok(HelloPayload {
        public_spki: payload[3..3 + public_len].to_vec(),
        nonce: payload[3 + public_len..]
            .try_into()
            .expect("checked nonce length"),
    })
}

pub fn build_transcript(
    my_public: &[u8],
    my_nonce: &[u8; NONCE_SIZE],
    peer_public: &[u8],
    peer_nonce: &[u8; NONCE_SIZE],
    i_am_a: bool,
) -> [u8; 32] {
    let (public_a, nonce_a, public_b, nonce_b) = if i_am_a {
        (my_public, my_nonce, peer_public, peer_nonce)
    } else {
        (peer_public, peer_nonce, my_public, my_nonce)
    };
    let mut hasher = Sha256::new();
    hasher.update(public_a);
    hasher.update(nonce_a);
    hasher.update(public_b);
    hasher.update(nonce_b);
    hasher.finalize().into()
}

pub fn info_for(epoch: u32) -> Vec<u8> {
    format!("SecureChat/v1/epoch{epoch}").into_bytes()
}

pub struct DerivedMaterial {
    pub key_a2b: [u8; 32],
    pub nonce_base_a2b: [u8; NONCE_BASE_SIZE],
    pub key_b2a: [u8; 32],
    pub nonce_base_b2a: [u8; NONCE_BASE_SIZE],
    pub confirm_key: [u8; 32],
}

impl Drop for DerivedMaterial {
    fn drop(&mut self) {
        self.key_a2b.zeroize();
        self.nonce_base_a2b.zeroize();
        self.key_b2a.zeroize();
        self.nonce_base_b2a.zeroize();
        self.confirm_key.zeroize();
    }
}

pub fn derive_material(
    shared_secret: &[u8],
    psk_text: &str,
    transcript: &[u8; 32],
    epoch: u32,
) -> Result<DerivedMaterial> {
    let mut ikm = Zeroizing::new(Vec::with_capacity(shared_secret.len() + psk_text.len()));
    ikm.extend_from_slice(shared_secret);
    ikm.extend_from_slice(psk_text.as_bytes());
    let hkdf = Hkdf::<Sha256>::new(Some(transcript), &ikm);
    let mut okm = Zeroizing::new([0_u8; 96]);
    hkdf.expand(&info_for(epoch), &mut *okm)
        .map_err(|_| Error::Protocol("HKDF expansion failed".into()))?;

    let mut key_a2b = [0_u8; 32];
    let mut key_b2a = [0_u8; 32];
    let mut confirm_key = [0_u8; 32];
    key_a2b.copy_from_slice(&okm[0..32]);
    key_b2a.copy_from_slice(&okm[32..64]);
    confirm_key.copy_from_slice(&okm[64..96]);
    let mut nonce_seed_a = hmac_sha256(&confirm_key, b"nonce-base-a");
    let mut nonce_seed_b = hmac_sha256(&confirm_key, b"nonce-base-b");
    let mut nonce_base_a2b = [0_u8; NONCE_BASE_SIZE];
    let mut nonce_base_b2a = [0_u8; NONCE_BASE_SIZE];
    nonce_base_a2b.copy_from_slice(&nonce_seed_a[..NONCE_BASE_SIZE]);
    nonce_base_b2a.copy_from_slice(&nonce_seed_b[..NONCE_BASE_SIZE]);
    nonce_seed_a.zeroize();
    nonce_seed_b.zeroize();
    Ok(DerivedMaterial {
        key_a2b,
        nonce_base_a2b,
        key_b2a,
        nonce_base_b2a,
        confirm_key,
    })
}

pub struct SessionKeys {
    pub epoch: u32,
    pub key_a2b: [u8; 32],
    pub nonce_base_a2b: [u8; NONCE_BASE_SIZE],
    pub key_b2a: [u8; 32],
    pub nonce_base_b2a: [u8; NONCE_BASE_SIZE],
    pub i_am_a: bool,
    pub my_tiebreak: [u8; TIEBREAK_SIZE],
    pub peer_tiebreak: [u8; TIEBREAK_SIZE],
    pub transcript: [u8; 32],
    pub confirm_key: [u8; 32],
}

impl SessionKeys {
    pub fn new(
        material: DerivedMaterial,
        epoch: u32,
        transcript: [u8; 32],
        i_am_a: bool,
        my_tiebreak: [u8; TIEBREAK_SIZE],
        peer_tiebreak: [u8; TIEBREAK_SIZE],
    ) -> Self {
        Self {
            epoch,
            key_a2b: material.key_a2b,
            nonce_base_a2b: material.nonce_base_a2b,
            key_b2a: material.key_b2a,
            nonce_base_b2a: material.nonce_base_b2a,
            i_am_a,
            my_tiebreak,
            peer_tiebreak,
            transcript,
            confirm_key: material.confirm_key,
        }
    }

    pub fn my_key(&self) -> &[u8; 32] {
        if self.i_am_a {
            &self.key_a2b
        } else {
            &self.key_b2a
        }
    }

    pub fn my_nonce_base(&self) -> &[u8; NONCE_BASE_SIZE] {
        if self.i_am_a {
            &self.nonce_base_a2b
        } else {
            &self.nonce_base_b2a
        }
    }

    pub fn peer_key(&self) -> &[u8; 32] {
        if self.i_am_a {
            &self.key_b2a
        } else {
            &self.key_a2b
        }
    }

    pub fn peer_nonce_base(&self) -> &[u8; NONCE_BASE_SIZE] {
        if self.i_am_a {
            &self.nonce_base_b2a
        } else {
            &self.nonce_base_a2b
        }
    }

    pub fn i_am_low_tiebreak(&self) -> bool {
        self.my_tiebreak < self.peer_tiebreak
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.key_a2b);
        hasher.update(self.key_b2a);
        let digest = hasher.finalize();
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

impl Drop for SessionKeys {
    fn drop(&mut self) {
        self.key_a2b.zeroize();
        self.nonce_base_a2b.zeroize();
        self.key_b2a.zeroize();
        self.nonce_base_b2a.zeroize();
        self.my_tiebreak.zeroize();
        self.peer_tiebreak.zeroize();
        self.transcript.zeroize();
        self.confirm_key.zeroize();
    }
}

pub fn build_next_transcript(
    current: &SessionKeys,
    my_public: &[u8],
    my_nonce: &[u8; NONCE_SIZE],
    peer_public: &[u8],
    peer_nonce: &[u8; NONCE_SIZE],
) -> [u8; 32] {
    let (public_low, nonce_low, public_high, nonce_high) = if current.i_am_low_tiebreak() {
        (my_public, my_nonce, peer_public, peer_nonce)
    } else {
        (peer_public, peer_nonce, my_public, my_nonce)
    };
    let mut hasher = Sha256::new();
    hasher.update(current.transcript);
    hasher.update(public_low);
    hasher.update(nonce_low);
    hasher.update(public_high);
    hasher.update(nonce_high);
    hasher.finalize().into()
}

pub fn build_confirm_mac(
    confirm_key: &[u8; 32],
    transcript: &[u8; 32],
    i_am_a: bool,
) -> [u8; CONFIRM_MAC_SIZE] {
    let mut input = [0_u8; 33];
    input[..32].copy_from_slice(transcript);
    input[32] = if i_am_a { 1 } else { 2 };
    hmac_sha256(confirm_key, &input)
}

pub fn verify_confirm_mac(
    confirm_key: &[u8; 32],
    transcript: &[u8; 32],
    i_am_a: bool,
    mac: &[u8],
) -> Result<()> {
    let mut verifier =
        <HmacSha256 as Mac>::new_from_slice(confirm_key).expect("HMAC accepts any key size");
    verifier.update(transcript);
    verifier.update(&[if i_am_a { 1 } else { 2 }]);
    verifier.verify_slice(mac).map_err(|_| {
        Error::Authentication("PSK does not match or the handshake was intercepted".into())
    })
}

pub fn build_aad(frame_type: FrameType, epoch: u32, counter: u32, plain_len: usize) -> [u8; 13] {
    let mut aad = [0_u8; 13];
    aad[0] = frame_type as u8;
    aad[1..5].copy_from_slice(&epoch.to_be_bytes());
    aad[5..9].copy_from_slice(&counter.to_be_bytes());
    aad[9..13].copy_from_slice(&(plain_len as i32).to_be_bytes());
    aad
}

pub fn seal(
    key: &[u8; 32],
    nonce_base: &[u8; NONCE_BASE_SIZE],
    counter: u32,
    frame_type: FrameType,
    epoch: u32,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let plain_len = i32::try_from(plaintext.len())
        .map_err(|_| Error::InvalidInput("plaintext is too large".into()))?;
    let mut nonce = [0_u8; 12];
    nonce[..NONCE_BASE_SIZE].copy_from_slice(nonce_base);
    nonce[NONCE_BASE_SIZE..].copy_from_slice(&counter.to_be_bytes());
    let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256 keys are 32 bytes");
    let aad = build_aad(frame_type, epoch, counter, plain_len as usize);
    let ciphertext = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| Error::Protocol("AES-GCM encryption failed".into()))?;
    let mut sealed = Vec::with_capacity(COUNTER_SIZE + ciphertext.len());
    sealed.extend_from_slice(&counter.to_be_bytes());
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

pub fn open(
    key: &[u8; 32],
    nonce_base: &[u8; NONCE_BASE_SIZE],
    frame_type: FrameType,
    epoch: u32,
    payload: &[u8],
) -> Result<Vec<u8>> {
    if payload.len() < COUNTER_SIZE + TAG_SIZE {
        return Err(Error::Authentication(
            "encrypted payload is too short".into(),
        ));
    }
    let counter = u32::from_be_bytes(payload[..COUNTER_SIZE].try_into().expect("four bytes"));
    let plain_len = payload.len() - COUNTER_SIZE - TAG_SIZE;
    let mut nonce = [0_u8; 12];
    nonce[..NONCE_BASE_SIZE].copy_from_slice(nonce_base);
    nonce[NONCE_BASE_SIZE..].copy_from_slice(&counter.to_be_bytes());
    let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256 keys are 32 bytes");
    let aad = build_aad(frame_type, epoch, counter, plain_len);
    cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &payload[COUNTER_SIZE..],
                aad: &aad,
            },
        )
        .map_err(|_| Error::Authentication("AES-GCM authentication failed".into()))
}

fn hmac_sha256(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().into()
}
