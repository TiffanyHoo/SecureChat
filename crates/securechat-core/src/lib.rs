mod connection;
pub mod crypto;
mod error;
pub mod protocol;
mod room;

pub use connection::{
    ConnState, ConnectionEvent, ConnectionSnapshot, SecureChatOptions, SecureConnection,
};
pub use error::{Error, Result};
pub use room::{
    candidate_host_ips, create_room_code, generate_psk, parse_room_code, psk_to_text,
    try_parse_room_code, RoomInfo, PSK_SIZE, ROOM_CODE_PREFIX,
};
