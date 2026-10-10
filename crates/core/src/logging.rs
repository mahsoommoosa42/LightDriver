//! Bounded, service-owned logs. Raw device bytes are collected only at Verbose.
use crate::{Mode, PROTOCOL_VERSION, now_ms};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};

pub const LOG_CAPACITY: usize = 512;
pub const LOG_BATCH_LIMIT: usize = 64;
pub const LOG_MESSAGE_BYTES: usize = 512;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warning,
    #[default]
    Info,
    Debug,
    Verbose,
}
impl LogLevel {
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warning,
        Self::Info,
        Self::Debug,
        Self::Verbose,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Verbose => "Verbose",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.label().eq_ignore_ascii_case(value))
    }
}
impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub sequence: u64,
    pub at_ms: u64,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
    pub raw: bool,
}
impl LogEntry {
    pub fn visible_at(&self, level: LogLevel) -> bool {
        self.level <= level
            && (!self.raw || level == LogLevel::Verbose && self.level == LogLevel::Verbose)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogBatch {
    pub protocol_version: u16,
    pub mode: Mode,
    pub session_id: u64,
    pub level: LogLevel,
    pub system_summary: Vec<String>,
    pub entries: Vec<LogEntry>,
    pub next_sequence: u64,
    pub latest_sequence: u64,
    pub first_available_sequence: u64,
}
struct Buffer {
    session_id: u64,
    dropped_through: u64,
    level: LogLevel,
    sequence: u64,
    entries: VecDeque<LogEntry>,
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            session_id: now_ms(),
            dropped_through: 0,
            level: LogLevel::Info,
            sequence: 0,
            entries: VecDeque::new(),
        }
    }
}
#[derive(Clone, Default)]
pub struct LogStore(Arc<Mutex<Buffer>>);
impl LogStore {
    pub fn level(&self) -> LogLevel {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).level
    }
    pub fn enabled(&self, level: LogLevel) -> bool {
        level <= self.level()
    }
    pub fn event(&self, level: LogLevel, source: &str, message: impl AsRef<str>) {
        self.push(level, source, message.as_ref(), false);
    }
    pub fn incoming_hid(&self, source: &str, bytes: &[u8]) {
        if !self.enabled(LogLevel::Verbose) || bytes.is_empty() {
            return;
        }
        use std::fmt::Write;
        let mut message = format!("RX HID · {} bytes · ", bytes.len());
        for (index, byte) in bytes.iter().take(128).enumerate() {
            if index > 0 {
                message.push(' ');
            }
            let _ = write!(message, "{byte:02X}");
        }
        if bytes.len() > 128 {
            message.push_str(" … truncated");
        }
        // Recheck under the same lock that changes verbosity: no raw append races.
        self.push(LogLevel::Verbose, source, &message, true);
    }
    fn push(&self, level: LogLevel, source: &str, message: &str, raw: bool) {
        let mut buffer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if level > buffer.level || raw && buffer.level != LogLevel::Verbose {
            return;
        }
        buffer.sequence = buffer.sequence.saturating_add(1);
        let entry = LogEntry {
            sequence: buffer.sequence,
            at_ms: now_ms(),
            level,
            source: bounded_text(source, 64),
            message: bounded_text(message, LOG_MESSAGE_BYTES),
            raw,
        };
        buffer.entries.push_back(entry);
        if buffer.entries.len() > LOG_CAPACITY
            && let Some(expired) = buffer.entries.pop_front()
        {
            buffer.dropped_through = expired.sequence;
        }
    }
    /// Level changes and reads are atomic. Lowering verbosity discards raw bytes.
    pub fn read(&self, mode: Mode, after: u64, requested_level: Option<LogLevel>) -> LogBatch {
        let mut buffer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(level) = requested_level {
            buffer.level = level;
            if level != LogLevel::Verbose {
                buffer.entries.retain(|entry| !entry.raw);
            }
        }
        let entries: Vec<_> = buffer
            .entries
            .iter()
            .filter(|entry| entry.sequence > after && entry.visible_at(buffer.level))
            .take(LOG_BATCH_LIMIT)
            .cloned()
            .collect();
        let next_sequence = entries
            .last()
            .map_or(buffer.sequence, |entry| entry.sequence);
        LogBatch {
            protocol_version: PROTOCOL_VERSION,
            mode,
            session_id: buffer.session_id,
            level: buffer.level,
            system_summary: vec![],
            entries,
            next_sequence,
            latest_sequence: buffer.sequence,
            first_available_sequence: buffer.dropped_through.saturating_add(1),
        }
    }
}
fn bounded_text(value: &str, bytes: usize) -> String {
    let mut text = String::new();
    for c in value.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if text.len() + c.len_utf8() > bytes {
            break;
        }
        text.push(c);
    }
    text
}
/// Stable context accompanies every batch so packet floods cannot erase tester configuration.
pub fn log_system_summary(snapshot: &crate::Snapshot, os: &str) -> Vec<String> {
    let mut lines = vec![
        format!(
            "LightDriver {} · protocol {} · {:?} mode",
            snapshot.service_version, snapshot.protocol_version, snapshot.mode
        ),
        format!("OS: {os}"),
    ];
    let topology = &snapshot.diagnostics.cpu_topology;
    lines.push(format!("CPU topology: {} physical cores · {} logical threads · {} shared-L3 groups · {} represented cores",
        topology.total_cores, topology.total_threads, topology.clusters.len(), topology.cores.len()));
    let packages: std::collections::BTreeSet<_> = topology
        .cores
        .iter()
        .filter_map(|core| core.package)
        .collect();
    let classes: std::collections::BTreeSet<_> = topology
        .cores
        .iter()
        .map(|core| core.efficiency_class)
        .collect();
    lines.push(format!(
        "CPU sockets: {} · efficiency classes: {:?} · {}",
        packages.len(),
        classes,
        topology.detail
    ));
    let groups: std::collections::BTreeSet<_> = snapshot
        .diagnostics
        .sensors
        .iter()
        .filter(|sensor| sensor.group.starts_with("CPU") || sensor.group.starts_with("GPU"))
        .map(|sensor| sensor.group.as_str())
        .collect();
    for group in groups {
        lines.push(format!("Processor: {group}"));
    }
    if let Some(sample) = snapshot
        .diagnostics
        .sensors
        .iter()
        .find(|sensor| sensor.id == "memory.total")
        .and_then(|sensor| sensor.sample.as_ref())
    {
        lines.push(format!("Memory: {:.1} GiB total", sample.value));
    }
    for device in &snapshot.devices {
        lines.push(format!(
            "Controller: {} · {} · {} · firmware {} · control {}",
            device.name,
            device.connection,
            device.status.label(),
            device.control.firmware.as_deref().unwrap_or("not queried"),
            if device.control.enabled {
                "enabled"
            } else {
                "disabled"
            }
        ));
        if !device.control.lighting_outputs.is_empty() {
            lines.push(format!(
                "Lighting outputs: {}",
                device
                    .control
                    .lighting_outputs
                    .iter()
                    .map(|output| output.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    for provider in &snapshot.diagnostics.providers {
        lines.push(format!("Adapter: {} · {}", provider.name, provider.detail));
    }
    let available = snapshot
        .diagnostics
        .sensors
        .iter()
        .filter(|sensor| sensor.sample.is_some())
        .count();
    lines.push(format!(
        "Sensors: {available}/{} available · CPU elevation required: {}",
        snapshot.diagnostics.sensors.len(),
        snapshot.diagnostics.cpu_elevation_needed
    ));
    lines
        .into_iter()
        .take(32)
        .map(|line| bounded_text(&line, LOG_MESSAGE_BYTES))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_is_opt_in_and_removed_when_verbosity_is_lowered() {
        let log = LogStore::default();
        log.incoming_hid("device", &[0x75, 1]);
        log.event(LogLevel::Debug, "device", "hidden");
        assert!(log.read(Mode::Demo, 0, None).entries.is_empty());
        log.read(Mode::Demo, 0, Some(LogLevel::Verbose));
        log.incoming_hid("device", &[0x75, 1]);
        log.event(LogLevel::Info, "device", "connected");
        let batch = log.read(Mode::Demo, 0, None);
        assert_eq!(batch.entries[0].message, "RX HID · 2 bytes · 75 01");
        assert!(batch.entries[0].raw);
        let batch = log.read(Mode::Demo, 0, Some(LogLevel::Info));
        assert_eq!(batch.entries.len(), 1);
        assert!(!batch.entries[0].raw);
        assert!(
            !log.read(Mode::Demo, 0, Some(LogLevel::Verbose))
                .entries
                .iter()
                .any(|e| e.raw)
        );
    }
    #[test]
    fn summary_and_maximum_escaped_log_batch_fit_the_transport_budget() {
        let store = LogStore::default();
        for _ in 0..LOG_CAPACITY {
            store.event(
                LogLevel::Info,
                &"\"".repeat(64),
                "\"".repeat(LOG_MESSAGE_BYTES),
            );
        }
        let mut snapshot = crate::Snapshot::empty(Mode::Demo);
        for index in 0..crate::SENSOR_LIMIT {
            snapshot.diagnostics.sensors.push(crate::Sensor {
                id: index.to_string(),
                group: format!("GPU {}", "x".repeat(1000)),
                label: String::new(),
                unit: crate::Unit::Celsius,
                source: String::new(),
                basic: true,
                status: crate::SensorStatus::Unavailable,
                detail: String::new(),
                sample: None,
                history: vec![],
            });
        }
        let mut batch = store.read(Mode::Demo, 0, None);
        batch.system_summary = log_system_summary(&snapshot, &"\"".repeat(1000));
        assert!(batch.system_summary.len() <= 32);
        assert!(
            batch
                .system_summary
                .iter()
                .all(|line| line.len() <= LOG_MESSAGE_BYTES)
        );
        assert!(serde_json::to_vec(&batch).unwrap().len() < 128 * 1024);
    }
    #[test]
    fn buffer_cursor_batch_and_text_are_bounded() {
        let log = LogStore::default();
        for _ in 0..600 {
            log.event(LogLevel::Info, &"x".repeat(200), "\n".repeat(900));
        }
        let batch = log.read(Mode::Live, 0, None);
        assert_eq!(batch.entries.len(), LOG_BATCH_LIMIT);
        assert_eq!(batch.first_available_sequence, 89);
        assert_eq!(batch.next_sequence, 152);
        assert_eq!(batch.latest_sequence, 600);
        assert_eq!(batch.entries[0].source.len(), 64);
        assert_eq!(batch.entries[0].message.len(), LOG_MESSAGE_BYTES);
        assert!(batch.entries[0].message.chars().all(|c| !c.is_control()));
        let batch = log.read(Mode::Live, batch.next_sequence, None);
        assert_eq!(batch.entries[0].sequence, 153);
        log.event(LogLevel::Info, "utf8", "温".repeat(300));
        let batch = log.read(Mode::Live, 600, None);
        assert!(batch.entries[0].message.len() <= LOG_MESSAGE_BYTES);
        assert!(
            serde_json::to_vec(&log.read(Mode::Live, 0, None))
                .unwrap()
                .len()
                < 128 * 1024
        );
    }
}
