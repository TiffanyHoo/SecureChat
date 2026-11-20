use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("invalid room code: {0}")]
    InvalidRoomCode(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("operation timed out: {0}")]
    Timeout(&'static str),
    #[error("connection is not established")]
    NotEstablished,
    #[error("connection is closed")]
    Closed,
}
