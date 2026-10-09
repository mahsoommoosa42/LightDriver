//! Service-owned adapters. This milestone never writes to device settings.
use lightdriver_core::{
    Device, DeviceStatus, HISTORY_LIMIT, Mode, Snapshot, Telemetry, kraken_target, now_ms,
};

pub trait DeviceAdapter: Send {
    fn poll(&mut self) -> Vec<Device>;
}

#[derive(Default)]
pub struct DemoAdapter {
    ticks: u64,
    history: Vec<Telemetry>,
}

impl DeviceAdapter for DemoAdapter {
    fn poll(&mut self) -> Vec<Device> {
        self.ticks += 1;
        let wave = (self.ticks as f32 / 12.0).sin();
        let telemetry = Telemetry {
            coolant_c: 32.6 + wave * 0.6,
            pump_rpm: (1973.0 + wave * 60.0) as u16,
            pump_duty: 66,
            sampled_at_ms: now_ms(),
        };
        self.history.push(telemetry.clone());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        let mut device = kraken_target();
        device.id = "demo-kraken-x3".into();
        device.name = "Kraken X53 · Demo".into();
        device.status = DeviceStatus::Demo;
        device.detail = "Simulated telemetry. This service never opens a hardware device.".into();
        device.telemetry = Some(telemetry);
        device.history = self.history.clone();
        vec![device]
    }
}

pub fn parse_status(report: &[u8], timestamp: u64) -> Result<Telemetry, &'static str> {
    if !(20..=64).contains(&report.len()) {
        return Err("Expected a 20–64 byte Kraken status report.");
    }
    if report[0] != 0x75 {
        return Err("Not a Kraken X3 periodic status report.");
    }
    if report[15..17] == [0xff, 0xff] {
        return Err("Device reported a coolant sensor fault.");
    }
    if report[16] > 9 || report[15] > 100 || report[19] > 100 {
        return Err("Malformed telemetry fields.");
    }
    Ok(Telemetry {
        coolant_c: report[15] as f32 + report[16] as f32 / 10.0,
        pump_rpm: u16::from_le_bytes([report[17], report[18]]),
        pump_duty: report[19],
        sampled_at_ms: timestamp,
    })
}

#[cfg(windows)]
#[derive(Default)]
pub struct KrakenAdapter {
    history: std::collections::HashMap<String, Vec<Telemetry>>,
}

#[cfg(windows)]
impl DeviceAdapter for KrakenAdapter {
    fn poll(&mut self) -> Vec<Device> {
        use std::hash::{Hash, Hasher};
        let api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(_) => {
                let mut d = kraken_target();
                d.status = DeviceStatus::Error;
                d.detail =
                    "Windows HID enumeration failed. No hardware settings were changed.".into();
                return vec![d];
            }
        };
        let mut devices = vec![];
        let mut seen = std::collections::HashSet::new();
        for info in api
            .device_list()
            .filter(|d| d.vendor_id() == 0x1e71 && [0x2007, 0x2014].contains(&d.product_id()))
        {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            info.path().hash(&mut h);
            let id = format!("kraken-x3-{:016x}", h.finish());
            if !seen.insert(id.clone()) {
                continue;
            }
            let mut device = kraken_target();
            device.id = id.clone();
            device.status = DeviceStatus::Detected;
            match info.open_device(&api) {
                Ok(handle) => {
                    // Read only: no initialization, pump, lighting, or feature-report writes.
                    // Bound draining prevents old queued samples from appearing fresh.
                    let deadline =
                        std::time::Instant::now() + std::time::Duration::from_millis(1200);
                    let mut last = None;
                    let mut error = None;
                    let mut buffer = [0u8; 64];
                    for _ in 0..64 {
                        if std::time::Instant::now() >= deadline {
                            break;
                        }
                        let timeout = if last.is_some() { 0 } else { 150 };
                        match handle.read_timeout(&mut buffer, timeout) {
                            Ok(0) if last.is_some() => break,
                            Ok(0) => continue,
                            Ok(n) => match parse_status(&buffer[..n], now_ms()) {
                                Ok(t) => {
                                    last = Some(t);
                                    error = None;
                                }
                                Err(e) if n > 0 && buffer[0] == 0x75 => {
                                    last = None;
                                    error = Some(e);
                                }
                                Err(_) => {}
                            },
                            Err(_) => {
                                last = None;
                                error = Some(
                                    "USB status read failed. Another controller may own the device.",
                                );
                                break;
                            }
                        }
                    }
                    if let Some(t) = last {
                        let history = self.history.entry(id.clone()).or_default();
                        history.push(t.clone());
                        if history.len() > HISTORY_LIMIT {
                            history.remove(0);
                        }
                        device.telemetry = Some(t);
                        device.history = history.clone();
                        device.status = DeviceStatus::Reading;
                        device.detail =
                            "Read-only USB telemetry. Pump and lighting settings are unchanged."
                                .into();
                    } else {
                        device.detail = error.unwrap_or("Detected, but no status stream is available. Device initialization or a competing controller may be involved. No automatic initialization was performed.").into();
                        if error.is_some() {
                            device.status = DeviceStatus::Error;
                        }
                        self.history.remove(&id);
                    }
                }
                Err(_) => {
                    device.status = DeviceStatus::Error;
                    device.detail = "Could not open Kraken HID. Check USB access and competing control software. No device settings were changed.".into();
                    self.history.remove(&id);
                }
            }
            devices.push(device);
        }
        self.history.retain(|id, _| seen.contains(id));
        if devices.is_empty() {
            devices.push(kraken_target());
        }
        devices
    }
}

pub fn adapter(mode: Mode) -> Box<dyn DeviceAdapter> {
    if mode == Mode::Demo {
        return Box::new(DemoAdapter::default());
    }
    #[cfg(windows)]
    {
        Box::new(KrakenAdapter::default())
    }
    #[cfg(not(windows))]
    {
        Box::new(UnsupportedAdapter)
    }
}

#[cfg(not(windows))]
struct UnsupportedAdapter;
#[cfg(not(windows))]
impl DeviceAdapter for UnsupportedAdapter {
    fn poll(&mut self) -> Vec<Device> {
        let mut d = kraken_target();
        d.status = DeviceStatus::AdapterNeeded;
        d.detail = "Live HID adapter currently targets Windows. Demo mode is available.".into();
        vec![d]
    }
}

pub fn poll_snapshot(adapter: &mut dyn DeviceAdapter, mode: Mode) -> Snapshot {
    let mut snapshot = Snapshot::empty(mode);
    let mut devices = adapter.poll();
    devices.push(lightdriver_core::motherboard_target());
    snapshot.devices = devices;
    snapshot.generated_at_ms = now_ms();
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> [u8; 64] {
        let mut r = [0; 64];
        r[0] = 0x75;
        r[1] = 1;
        r[15] = 32;
        r[16] = 6;
        r[17..19].copy_from_slice(&1973u16.to_le_bytes());
        r[19] = 66;
        r
    }
    #[test]
    fn decodes_wire_fields_and_rejects_faults() {
        assert_eq!(
            parse_status(&report(), 123).unwrap(),
            Telemetry {
                coolant_c: 32.6,
                pump_rpm: 1973,
                pump_duty: 66,
                sampled_at_ms: 123
            }
        );
        assert!(parse_status(&report()[..19], 123).is_err());
        let mut r = report();
        r[0] = 0x11;
        assert!(parse_status(&r, 123).is_err());
        r = report();
        r[15] = 0xff;
        r[16] = 0xff;
        assert!(parse_status(&r, 123).is_err());
        r = report();
        r[19] = 101;
        assert!(parse_status(&r, 123).is_err());
    }
    #[test]
    fn demo_is_explicit_and_history_is_bounded() {
        let mut adapter = DemoAdapter::default();
        for _ in 0..100 {
            let s = poll_snapshot(&mut adapter, Mode::Demo);
            assert_eq!(s.devices[0].status, DeviceStatus::Demo);
            assert!(s.devices[0].history.len() <= HISTORY_LIMIT);
        }
    }
}
