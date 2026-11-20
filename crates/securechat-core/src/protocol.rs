use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Result};

pub const HEADER_SIZE: usize = 9;
pub const MAX_PAYLOAD_SIZE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(try_from = "u8", into = "u8")]
pub enum FrameType {
    Hello = 1,
    Confirm = 2,
    Data = 3,
    Profile = 4,
    Rekey = 5,
    Ping = 6,
    Bye = 7,
    RekeyRequest = 8,
    Over = 9,
    Reaction = 10,
}

impl TryFrom<u8> for FrameType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::Confirm),
            3 => Ok(Self::Data),
            4 => Ok(Self::Profile),
            5 => Ok(Self::Rekey),
            6 => Ok(Self::Ping),
            7 => Ok(Self::Bye),
            8 => Ok(Self::RekeyRequest),
            9 => Ok(Self::Over),
            10 => Ok(Self::Reaction),
            _ => Err(Error::InvalidFrame(format!("unknown frame type {value}"))),
        }
    }
}

impl From<FrameType> for u8 {
    fn from(value: FrameType) -> Self {
        value as u8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub frame_type: FrameType,
    pub epoch: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(frame_type: FrameType, epoch: u32, payload: Vec<u8>) -> Result<Self> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::InvalidFrame("payload exceeds 1 MiB".into()));
        }
        Ok(Self {
            frame_type,
            epoch,
            payload,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::InvalidFrame("payload exceeds 1 MiB".into()));
        }
        let total = HEADER_SIZE + self.payload.len();
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&(total as u32).to_be_bytes());
        bytes.push(self.frame_type as u8);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }
}

pub fn decode_frame(bytes: &[u8]) -> Result<Option<(Frame, usize)>> {
    if bytes.len() < HEADER_SIZE {
        return Ok(None);
    }
    let total = u32::from_be_bytes(bytes[0..4].try_into().expect("four bytes")) as usize;
    if !(HEADER_SIZE..=HEADER_SIZE + MAX_PAYLOAD_SIZE).contains(&total) {
        return Err(Error::InvalidFrame(format!("invalid total length {total}")));
    }
    let frame_type = FrameType::try_from(bytes[4])?;
    if bytes.len() < total {
        return Ok(None);
    }
    let epoch = u32::from_be_bytes(bytes[5..9].try_into().expect("four bytes"));
    Ok(Some((
        Frame {
            frame_type,
            epoch,
            payload: bytes[HEADER_SIZE..total].to_vec(),
        },
        total,
    )))
}

pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Frame>>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; HEADER_SIZE];
    if !read_exact_or_eof(reader, &mut header).await? {
        return Ok(None);
    }
    let total = u32::from_be_bytes(header[0..4].try_into().expect("four bytes")) as usize;
    if !(HEADER_SIZE..=HEADER_SIZE + MAX_PAYLOAD_SIZE).contains(&total) {
        return Err(Error::InvalidFrame(format!("invalid total length {total}")));
    }
    let frame_type = FrameType::try_from(header[4])?;
    let epoch = u32::from_be_bytes(header[5..9].try_into().expect("four bytes"));
    let mut payload = vec![0_u8; total - HEADER_SIZE];
    if !read_exact_or_eof(reader, &mut payload).await? {
        return Ok(None);
    }
    Ok(Some(Frame {
        frame_type,
        epoch,
        payload,
    }))
}

pub async fn write_frame<W>(writer: &mut W, frame: &Frame) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&frame.encode()?).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_exact_or_eof<R>(reader: &mut R, mut buffer: &mut [u8]) -> Result<bool>
where
    R: AsyncRead + Unpin,
{
    while !buffer.is_empty() {
        match reader.read(buffer).await {
            Ok(0) => return Ok(false),
            Ok(read) => buffer = &mut buffer[read..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    #[serde(default)]
    pub uid: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionMessage {
    pub uid: String,
    pub reaction: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMessage {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverMessage {
    pub kind: String,
}
