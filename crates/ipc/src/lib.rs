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

#[cfg(windows)]
mod windows_security;

pub const MAX_FRAME: usize = 128 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestAction {
    #[default]
    Snapshot,
    StopUnprivilegedLiveService,
}
/// Handoff is denied for demo and elevated processes; there are no hardware setters.
pub fn permits_cpu_sensor_handoff(mode: Mode, administrator: bool) -> bool {
    mode == Mode::Live && !administrator
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub action: RequestAction,
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
    let options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        options.security_descriptor(windows_security::descriptor()?)
    };
    options.create_tokio()
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
            action: RequestAction::Snapshot,
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

/// Ask only the ordinary live process to relinquish its endpoint for an explicit UAC action.
pub async fn stop_unprivileged_live_service(mode: Mode) -> io::Result<bool> {
    let stream = connect(mode).await?;
    let (mut reader, mut writer) = stream.split();
    write_frame(
        &mut writer,
        &Request {
            protocol_version: PROTOCOL_VERSION,
            action: RequestAction::StopUnprivilegedLiveService,
        },
    )
    .await?;
    read_frame(&mut reader).await
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
    #[test]
    fn maximum_diagnostic_inventory_fits_frame_budget() {
        use lightdriver_core::*;
        let mut snapshot = Snapshot::empty(Mode::Demo);
        snapshot.diagnostics.cpu_topology = CpuTopology {
            cores: (0..CPU_CORE_LIMIT)
                .map(|i| PhysicalCore {
                    id: i as u16,
                    package: Some(i as u16),
                    cluster: Some(i as u16),
                    efficiency_class: 1,
                    threads: (0..2)
                        .map(|t| CpuThread {
                            group: (i / 32) as u16,
                            index: ((i * 2 + t) % 64) as u8,
                            load: Some(99.99999),
                        })
                        .collect(),
                    temperature: Some(99.99999),
                    clock_mhz: Some(9999.999),
                })
                .collect(),
            clusters: (0..CPU_CORE_LIMIT)
                .map(|i| CpuCluster {
                    id: i as u16,
                    l3_bytes: u32::MAX,
                })
                .collect(),
            total_cores: CPU_CORE_LIMIT,
            total_threads: CPU_THREAD_LIMIT,
            sampled_at_ms: u64::MAX,
            detail: "x".repeat(512),
        };
        snapshot.diagnostics.sensors = (0..SENSOR_LIMIT)
            .map(|i| Sensor {
                id: format!("sensor.{i}"),
                group: "x".repeat(256),
                label: "x".repeat(64),
                unit: Unit::Mhz,
                source: "x".repeat(64),
                basic: true,
                status: SensorStatus::Available,
                detail: "x".repeat(512),
                sample: Some(SensorSample {
                    at_ms: u64::MAX,
                    value: 19999.99999999999,
                }),
                history: (0..SENSOR_HISTORY_LIMIT)
                    .map(|_| SensorSample {
                        at_ms: u64::MAX,
                        value: 19999.99999999999,
                    })
                    .collect(),
            })
            .collect();
        let encoded = serde_json::to_vec(&snapshot).unwrap();
        assert!(
            encoded.len() < MAX_FRAME,
            "{} exceeds {}",
            encoded.len(),
            MAX_FRAME
        );
    }
    #[test]
    fn cpu_handoff_never_stops_demo_or_elevated_services() {
        assert!(permits_cpu_sensor_handoff(Mode::Live, false));
        assert!(!permits_cpu_sensor_handoff(Mode::Live, true));
        assert!(!permits_cpu_sensor_handoff(Mode::Demo, false));
        assert!(!permits_cpu_sensor_handoff(Mode::Demo, true));
    }
}
