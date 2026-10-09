//! Versioned, bounded local IPC. No TCP listener or device-control commands.
use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, ToNsName,
    tokio::{Listener, Stream, prelude::*},
};
use lightdriver_core::{Mode, PROTOCOL_VERSION, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    io,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_FRAME: usize = 128 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub protocol_version: u16,
}

pub fn endpoint(mode: Mode) -> String {
    let mut hasher = DefaultHasher::new();
    std::env::var("USERDOMAIN")
        .unwrap_or_default()
        .hash(&mut hasher);
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .hash(&mut hasher);
    format!(
        "lightdriver-v1-{:016x}-{}",
        hasher.finish(),
        if mode == Mode::Demo { "demo" } else { "live" }
    )
}

pub fn bind(mode: Mode) -> io::Result<Listener> {
    let name = endpoint(mode)
        .to_ns_name::<GenericNamespaced>()?
        .into_owned();
    ListenerOptions::new().name(name).create_tokio()
}

pub async fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept().await
}

pub fn split(
    stream: Stream,
) -> (
    impl tokio::io::AsyncRead + Unpin + Send,
    impl tokio::io::AsyncWrite + Unpin + Send,
) {
    stream.split()
}

pub async fn connect(mode: Mode) -> io::Result<Stream> {
    let name = endpoint(mode)
        .to_ns_name::<GenericNamespaced>()?
        .into_owned();
    tokio::time::timeout(IO_TIMEOUT, Stream::connect(name))
        .await
        .map_err(|_| timed_out())?
}

fn timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "Local service did not respond within three seconds.",
    )
}

pub async fn write_frame<T: Serialize>(
    writer: &mut (impl AsyncWriteExt + Unpin),
    value: &T,
) -> io::Result<()> {
    let data = serde_json::to_vec(value).map_err(io::Error::other)?;
    if data.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC message too large.",
        ));
    }
    tokio::time::timeout(IO_TIMEOUT, async {
        writer.write_u32(data.len() as u32).await?;
        writer.write_all(&data).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| timed_out())?
}

pub async fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut (impl AsyncReadExt + Unpin),
) -> io::Result<T> {
    tokio::time::timeout(IO_TIMEOUT, async {
        let len = reader.read_u32().await? as usize;
        if len == 0 || len > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid IPC frame length.",
            ));
        }
        let mut data = vec![0; len];
        reader.read_exact(&mut data).await?;
        serde_json::from_slice(&data).map_err(io::Error::other)
    })
    .await
    .map_err(|_| timed_out())?
}

pub async fn snapshot(mode: Mode) -> io::Result<Snapshot> {
    let stream = connect(mode).await?;
    let (mut reader, mut writer) = stream.split();
    write_frame(
        &mut writer,
        &Request {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;
    let response: Snapshot = read_frame(&mut reader).await?;
    if response.protocol_version != PROTOCOL_VERSION || response.mode != mode {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Service version or live/demo mode mismatch.",
        ));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_oversize_before_allocating_payload() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer.write_u32(MAX_FRAME as u32 + 1).await.unwrap();
        let error = read_frame::<Request>(&mut reader).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    #[tokio::test]
    async fn bounded_frames_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(MAX_FRAME);
        let source = Snapshot::empty(Mode::Demo);
        write_frame(&mut a, &source).await.unwrap();
        let decoded: Snapshot = read_frame(&mut b).await.unwrap();
        assert_eq!(decoded.mode, Mode::Demo);
    }
}
