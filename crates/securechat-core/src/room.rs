use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const ROOM_CODE_PREFIX: &str = "SC1-";
pub const PSK_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    pub host: String,
    pub port: u16,
    pub rotation_minutes: u16,
    pub psk_text: String,
}

pub fn generate_psk() -> [u8; PSK_SIZE] {
    let mut psk = [0_u8; PSK_SIZE];
    OsRng.fill_bytes(&mut psk);
    psk
}

pub fn psk_to_text(psk: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(psk)
}

pub fn create_room_code(
    host: &str,
    port: u16,
    rotation_minutes: u16,
    psk: &[u8],
) -> Result<String> {
    if psk.len() != PSK_SIZE {
        return Err(Error::InvalidInput("PSK must be exactly 32 bytes".into()));
    }
    if port == 0 {
        return Err(Error::InvalidInput("port must be in 1..=65535".into()));
    }
    if !(1..=720).contains(&rotation_minutes) {
        return Err(Error::InvalidInput(
            "rotation minutes must be in 1..=720".into(),
        ));
    }
    let ip = host
        .parse::<IpAddr>()
        .map_err(|_| Error::InvalidInput("host must be an IPv4 or IPv6 address".into()))?;
    let ip_bytes = match ip {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    };
    let mut payload = Vec::with_capacity(2 + ip_bytes.len() + 2 + 2 + PSK_SIZE);
    payload.push(1);
    payload.push(ip_bytes.len() as u8);
    payload.extend_from_slice(&ip_bytes);
    payload.extend_from_slice(&port.to_be_bytes());
    payload.extend_from_slice(&rotation_minutes.to_be_bytes());
    payload.extend_from_slice(psk);
    Ok(format!(
        "{ROOM_CODE_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(payload)
    ))
}

pub fn parse_room_code(code: &str) -> Result<RoomInfo> {
    let code = code.trim();
    if !code
        .get(..ROOM_CODE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(ROOM_CODE_PREFIX))
    {
        return Err(Error::InvalidRoomCode("missing SC1- prefix".into()));
    }
    let encoded = &code[ROOM_CODE_PREFIX.len()..];
    let unpadded = encoded.trim_end_matches('=');
    let payload = URL_SAFE_NO_PAD
        .decode(unpadded)
        .map_err(|_| Error::InvalidRoomCode("invalid Base64Url payload".into()))?;
    if payload.len() < 2 + 4 + 2 + 2 + PSK_SIZE || payload[0] != 1 {
        return Err(Error::InvalidRoomCode(
            "unsupported version or truncated payload".into(),
        ));
    }
    let ip_len = payload[1] as usize;
    if ip_len != 4 && ip_len != 16 {
        return Err(Error::InvalidRoomCode("invalid IP length".into()));
    }
    if payload.len() != 2 + ip_len + 2 + 2 + PSK_SIZE {
        return Err(Error::InvalidRoomCode("incorrect payload length".into()));
    }
    let port_offset = 2 + ip_len;
    let rotation_offset = port_offset + 2;
    let port = u16::from_be_bytes(
        payload[port_offset..port_offset + 2]
            .try_into()
            .expect("two bytes"),
    );
    let rotation_minutes = u16::from_be_bytes(
        payload[rotation_offset..rotation_offset + 2]
            .try_into()
            .expect("two bytes"),
    );
    if port == 0 || !(1..=720).contains(&rotation_minutes) {
        return Err(Error::InvalidRoomCode(
            "port or rotation period is out of range".into(),
        ));
    }
    let ip = match ip_len {
        4 => IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(&payload[2..6]).expect("four bytes"),
        )),
        16 => IpAddr::from(<[u8; 16]>::try_from(&payload[2..18]).expect("sixteen bytes")),
        _ => unreachable!(),
    };
    Ok(RoomInfo {
        host: ip.to_string(),
        port,
        rotation_minutes,
        psk_text: psk_to_text(&payload[rotation_offset + 2..]),
    })
}

pub fn try_parse_room_code(code: &str) -> Option<RoomInfo> {
    parse_room_code(code).ok()
}

pub fn candidate_host_ips() -> Vec<String> {
    let mut addresses = Vec::new();
    let mut seen = HashSet::new();
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            let IpAddr::V4(ip) = interface.ip() else {
                continue;
            };
            if interface.is_loopback()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
            {
                continue;
            }
            let text = ip.to_string();
            if seen.insert(text.clone()) {
                addresses.push(text);
            }
        }
    }
    addresses.sort_by_key(|address| {
        let private = address
            .parse::<Ipv4Addr>()
            .map(|ip| ip.is_private())
            .unwrap_or(false);
        (!private, address.clone())
    });
    if addresses.is_empty() {
        addresses.push(Ipv4Addr::LOCALHOST.to_string());
    }
    addresses
}
