//! Models shared by the service, device adapters, and native application.
mod diagnostics;
mod topology;
pub use diagnostics::*;
pub use topology::*;
pub const PROTOCOL_VERSION: u16 = 3;
pub const STALE_AFTER_MS: u64 = 5_000;
pub const HISTORY_LIMIT: usize = 60;

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Live,
    Demo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    Reading,
    Demo,
    Disconnected,
    Detected,
    Error,
    AdapterNeeded,
}

impl DeviceStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Reading => "Reading",
            Self::Demo => "Demo data",
            Self::Disconnected => "Not detected",
            Self::Detected => "Detected",
            Self::Error => "Read error",
            Self::AdapterNeeded => "Adapter needed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Support {
    Implemented,
    Planned,
    External,
    Unavailable,
}

impl Support {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Implemented => "Implemented",
            Self::Planned => "Planned",
            Self::External => "Motherboard path",
            Self::Unavailable => "Not exposed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub support: Support,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub coolant_c: f32,
    pub pump_rpm: u16,
    pub pump_duty: u8,
    pub sampled_at_ms: u64,
}

impl Telemetry {
    pub fn is_fresh_at(&self, now: u64) -> bool {
        self.sampled_at_ms <= now && now - self.sampled_at_ms <= STALE_AFTER_MS
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub status: DeviceStatus,
    pub detail: String,
    pub capabilities: Vec<Capability>,
    pub telemetry: Option<Telemetry>,
    pub history: Vec<Telemetry>,
}

pub fn kraken_target() -> Device {
    Device {
        id: "kraken-x3".into(),
        name: "NZXT Kraken X3".into(),
        connection: "USB HID · X53 target".into(),
        status: DeviceStatus::Disconnected,
        detail: "Waiting for a Kraken X3 USB device. X53, X63 and X73 share this USB identity."
            .into(),
        capabilities: vec![
            Capability {
                name: "Coolant / pump RPM / duty".into(),
                support: Support::Implemented,
                detail: "Read-only USB status reports; each sample is timestamped.".into(),
            },
            Capability {
                name: "Coolant-based pump curve".into(),
                support: Support::Planned,
                detail: "Local drafts only. Hardware application awaits validation.".into(),
            },
            Capability {
                name: "Ring + logo lighting".into(),
                support: Support::Planned,
                detail: "Local preview only. USB lighting writes are not implemented.".into(),
            },
            Capability {
                name: "Radiator fan control".into(),
                support: Support::External,
                detail: "The X53 does not expose fan control. Your radiator group uses CPU_FAN."
                    .into(),
            },
            Capability {
                name: "Stored curve readback".into(),
                support: Support::Unavailable,
                detail: "A local draft must not be presented as the device's current curve.".into(),
            },
        ],
        telemetry: None,
        history: vec![],
    }
}

pub fn motherboard_target() -> Device {
    Device {
        id: "motherboard".into(), name: "X570 AORUS ELITE WIFI".into(), connection: "Recorded build · not auto-detected".into(),
        status: DeviceStatus::AdapterNeeded,
        detail: "Recorded wiring: CPU_FAN → radiator fans; SYS_FAN1 → LANCOOL 216 hub. ARGB connection needs confirmation. BIOS keeps its current fan settings.".into(),
        capabilities: vec![
            Capability { name: "CPU_FAN radiator group".into(), support: Support::Planned, detail: "Motherboard PWM adapter and measurements needed.".into() },
            Capability { name: "SYS_FAN1 case fan group".into(), support: Support::Planned, detail: "The stock hub shares PWM; independent fan control/RPM is not assumed.".into() },
            Capability { name: "5 V ARGB case lighting".into(), support: Support::Planned, detail: "Motherboard RGB adapter and actual wiring validation needed.".into() },
        ], telemetry: None, history: vec![],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub protocol_version: u16,
    pub service_version: String,
    pub mode: Mode,
    pub generated_at_ms: u64,
    pub devices: Vec<Device>,
    pub diagnostics: Diagnostics,
}

impl Snapshot {
    pub fn empty(mode: Mode) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            service_version: env!("CARGO_PKG_VERSION").into(),
            mode,
            generated_at_ms: now_ms(),
            devices: vec![kraken_target(), motherboard_target()],
            diagnostics: Diagnostics::default(),
        }
    }

    /// User-shareable diagnostics omit USB paths and serial numbers by design.
    pub fn diagnostic_report(&self) -> String {
        let mut report = format!(
            "LightDriver {}\nMode: {:?}\nSnapshot timestamp: {}\nHardware writes: disabled in this milestone\n\n",
            self.service_version, self.mode, self.generated_at_ms
        );
        for device in &self.devices {
            report.push_str(&format!(
                "{} | {}\n{}\n",
                device.name,
                device.status.label(),
                device.detail
            ));
            if let Some(t) = &device.telemetry {
                report.push_str(&format!(
                    "Coolant: {:.1} °C | Pump: {} RPM | Duty: {}% | Sample: {}\n",
                    t.coolant_c, t.pump_rpm, t.pump_duty, t.sampled_at_ms
                ));
            }
            for c in &device.capabilities {
                report.push_str(&format!("  {}: {}\n", c.name, c.support.label()));
            }
            report.push('\n');
        }
        report.push_str("System sensors\n");
        for s in &self.diagnostics.sensors {
            let reading = s
                .sample
                .as_ref()
                .map(|v| format!("{:.2} {} at {}", v.value, s.unit.label(), v.at_ms))
                .unwrap_or_else(|| "Unavailable".into());
            report.push_str(&format!(
                "{} | {} | {:?} | {} | {} | {}\n",
                s.group, s.label, s.status, reading, s.source, s.detail
            ));
        }
        for p in &self.diagnostics.providers {
            report.push_str(&format!("Collector {}: {}\n", p.name, p.detail));
        }
        report
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Quiet,
    Balanced,
    Performance,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::Quiet, Self::Balanced, Self::Performance];
    pub fn label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub coolant_c: u8,
    pub duty: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PumpCurve {
    pub points: Vec<CurvePoint>,
}

impl PumpCurve {
    pub fn example(preset: Preset) -> Self {
        let duties = match preset {
            Preset::Quiet => [60, 65, 80, 100],
            Preset::Balanced => [60, 75, 90, 100],
            Preset::Performance => [75, 85, 95, 100],
        };
        Self {
            points: duties
                .into_iter()
                .enumerate()
                .map(|(i, duty)| CurvePoint {
                    coolant_c: 30 + i as u8 * 5,
                    duty,
                })
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(2..=16).contains(&self.points.len()) {
            return Err("Use 2–16 curve points.".into());
        }
        for p in &self.points {
            if !(20..=100).contains(&p.duty) {
                return Err("Pump duty must be 20–100%.".into());
            }
            if !(20..=59).contains(&p.coolant_c) {
                return Err("Curve temperatures must be 20–59 °C.".into());
            }
        }
        for pair in self.points.windows(2) {
            if pair[0].coolant_c >= pair[1].coolant_c {
                return Err("Temperature points must increase.".into());
            }
            if pair[0].duty > pair[1].duty {
                return Err("Pump duty must not decrease as coolant gets warmer.".into());
            }
        }
        if self.points.last().is_none_or(|p| p.duty != 100) {
            return Err("The final point must reach 100%.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub schema_version: u16,
    pub curve: PumpCurve,
    pub lighting_rgb: [u8; 3],
    pub lighting_brightness: u8,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            schema_version: 1,
            curve: PumpCurve::example(Preset::Balanced),
            lighting_rgb: [83, 221, 202],
            lighting_brightness: 70,
        }
    }
}

impl Draft {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported draft version.".into());
        }
        if self.lighting_brightness > 100 {
            return Err("Brightness must be 0–100%.".into());
        }
        self.curve.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_or_ambiguous_curves() {
        let mut curve = PumpCurve::example(Preset::Balanced);
        assert!(curve.validate().is_ok());
        curve.points[1].duty = 19;
        assert!(curve.validate().is_err());
        curve.points[1].duty = 55;
        assert!(curve.validate().is_err());
        curve.points[1].duty = 75;
        curve.points[1].coolant_c = 30;
        assert!(curve.validate().is_err());
        curve.points[1].coolant_c = 35;
        curve.points[3].duty = 99;
        assert!(curve.validate().is_err());
    }
    #[test]
    fn stale_and_future_samples_are_not_live() {
        let t = Telemetry {
            coolant_c: 32.6,
            pump_rpm: 1973,
            pump_duty: 66,
            sampled_at_ms: 10_000,
        };
        assert!(t.is_fresh_at(15_000));
        assert!(!t.is_fresh_at(15_001));
        assert!(!t.is_fresh_at(9_999));
    }
    #[test]
    fn wire_roundtrip_preserves_mode_and_missing_readings() {
        let s = Snapshot::empty(Mode::Live);
        let decoded: Snapshot = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(decoded.mode, Mode::Live);
        assert!(decoded.devices[0].telemetry.is_none());
    }
}
