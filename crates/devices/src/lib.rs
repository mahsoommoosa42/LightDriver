//! Service-owned adapters. Hardware writes require an explicit per-device control session.
mod commands;
use lightdriver_core::{
    CommandReceipt, CommandState, ControlAction, ControlRequest, ControlState, LogStore, Preset,
    PumpCurve,
};
use lightdriver_core::{
    Device, DeviceStatus, HISTORY_LIMIT, Mode, Snapshot, Telemetry, inventory_target,
    kraken_target, now_ms,
};
use lightdriver_gigabyte::{GigabyteBackend, rgb as gigabyte};
pub use lightdriver_hardware::inventory::discover_board;

pub use lightdriver_core::DeviceAdapter;

#[derive(Default)]
pub struct DemoAdapter {
    logger: LogStore,
    ticks: u64,
    history: Vec<Telemetry>,
    enabled: bool,
    board_enabled: bool,
    curve: Option<PumpCurve>,
}

impl DeviceAdapter for DemoAdapter {
    fn set_logger(&mut self, logger: LogStore) {
        self.logger = logger;
    }
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        if request.mode == Mode::Demo && request.device_id == "motherboard" {
            if let Err(e) = request.validate_at(now_ms()) {
                return CommandReceipt::rejected(request, e);
            }
            match &request.action {
                ControlAction::Enable => self.board_enabled = true,
                ControlAction::Release => self.board_enabled = false,
                ControlAction::HeaderLighting {
                    channel_id,
                    settings,
                } if self.board_enabled => {
                    if let Err(e) = gigabyte::effect_reports(channel_id, settings) {
                        return CommandReceipt::rejected(request, e);
                    }
                }
                _ => {
                    return CommandReceipt::rejected(
                        request,
                        "Enable demo RGB control and select an exposed header first.",
                    );
                }
            }
            return CommandReceipt::success(
                request,
                CommandState::Simulated,
                "Simulated motherboard lighting only. No hardware was opened.",
            );
        }
        if request.mode != Mode::Demo || request.device_id != "demo-kraken-x3" {
            return CommandReceipt::rejected(
                request,
                "Demo commands require a demo device and mode.",
            );
        }
        if let Err(e) = request.validate_at(now_ms()) {
            return CommandReceipt::rejected(request, e);
        }
        match &request.action {
            ControlAction::Enable => self.enabled = true,
            ControlAction::Release => self.enabled = false,
            _ if !self.enabled => {
                return CommandReceipt::rejected(request, "Enable demo control first.");
            }
            ControlAction::PumpCurve(curve) => self.curve = Some(curve.clone()),
            ControlAction::PumpFull => {
                self.curve = Some(PumpCurve::example(Preset::Performance).full_speed())
            }
            ControlAction::Lighting(_) => {}
            ControlAction::HeaderLighting { .. } => {
                return CommandReceipt::rejected(
                    request,
                    "Kraken does not expose motherboard headers.",
                );
            }
        }
        CommandReceipt::success(
            request,
            CommandState::Simulated,
            "Simulated command only. No hardware was opened.",
        )
    }
    fn poll(&mut self) -> Vec<Device> {
        self.ticks += 1;
        let wave = (self.ticks as f32 / 12.0).sin();
        let telemetry = Telemetry {
            coolant_c: 32.6 + wave * 0.6,
            pump_rpm: self
                .curve
                .as_ref()
                .map_or((1973.0 + wave * 60.0) as u16, |c| {
                    1000 + u16::from(commands::curve_duty(c, 32.6 + wave * 0.6)) * 20
                }),
            pump_duty: self
                .curve
                .as_ref()
                .map_or(66, |c| commands::curve_duty(c, 32.6 + wave * 0.6)),
            sampled_at_ms: now_ms(),
        };
        if self.logger.enabled(lightdriver_core::LogLevel::Verbose) {
            let mut report = [0u8; 64];
            report[0] = 0x75;
            report[1] = 1;
            report[15] = telemetry.coolant_c as u8;
            report[16] = ((telemetry.coolant_c.fract() * 10.0).round() as u8).min(9);
            report[17..19].copy_from_slice(&telemetry.pump_rpm.to_le_bytes());
            report[19] = telemetry.pump_duty;
            self.logger.incoming_hid("demo.hid · SIMULATED", &report);
        }
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
        device.control.available = true;
        device.control.pump = true;
        device.control.enabled = self.enabled;
        vec![device, gigabyte::demo_board(self.board_enabled)]
    }
}

fn validate_cooling_action(
    action: &ControlAction,
    sample: Option<&Telemetry>,
    now: u64,
) -> Result<(), String> {
    if matches!(action, ControlAction::PumpCurve(_)) {
        let sample = sample.filter(|t| t.is_fresh_at(now)).ok_or("Fresh coolant telemetry is required for a pump curve. The explicit 100% pump command remains available.")?;
        if sample.coolant_c >= 59.0 {
            return Err("Coolant is critical. Use the explicit 100% pump command.".into());
        }
    }
    Ok(())
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
    logger: LogStore,
    history: std::collections::HashMap<String, Vec<Telemetry>>,
    owned: std::collections::HashMap<String, OwnedKraken>,
}
#[cfg(windows)]
struct OwnedKraken {
    handle: hidapi::HidDevice,
    firmware: String,
}
#[cfg(windows)]
fn kraken_id(info: &hidapi::DeviceInfo) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    info.path().hash(&mut h);
    format!("kraken-x3-{:016x}", h.finish())
}

#[cfg(windows)]
impl DeviceAdapter for KrakenAdapter {
    fn set_logger(&mut self, logger: LogStore) {
        self.logger = logger;
    }
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        let outcome = (|| -> Result<(CommandState, String), String> {
            request.validate_at(now_ms())?;
            if request.mode != Mode::Live {
                return Err("Live adapter refuses demo commands.".into());
            }
            if !request.device_id.starts_with("kraken-x3-") {
                return Err("This adapter controls detected Kraken X3 targets only.".into());
            }
            if request.action == ControlAction::Release {
                self.owned.remove(&request.device_id);
                return Ok((CommandState::Released, "Control released. Last device curve/lighting remain; previous controller settings cannot be read back.".into()));
            }
            if let Some(conflict) = lightdriver_hardware::conflicts::competing_controller() {
                self.owned.remove(&request.device_id);
                return Err(conflict);
            }
            let api = hidapi::HidApi::new().map_err(|_| "HID enumeration failed.")?;
            let info = api
                .device_list()
                .find(|d| {
                    d.vendor_id() == 0x1e71
                        && [0x2007, 0x2014].contains(&d.product_id())
                        && kraken_id(d) == request.device_id
                })
                .ok_or("Selected Kraken is no longer connected.")?;
            if request.action == ControlAction::Enable {
                if self.owned.contains_key(&request.device_id) {
                    return Ok((CommandState::Enabled, "Control is already enabled.".into()));
                }
                let handle = info
                    .open_device(&api)
                    .map_err(|_| "Could not open the selected Kraken.")?;
                for bytes in [
                    &[0x10, 0x01][..],
                    &[0x20, 0x03],
                    &[0x70, 0x02, 0x01, 0xb8, 0x01],
                    &[0x70, 0x01],
                ] {
                    let mut packet = [0; 64];
                    packet[..bytes.len()].copy_from_slice(bytes);
                    if handle
                        .write(&packet)
                        .map_err(|_| "Kraken initialization failed.")?
                        != 64
                    {
                        return Err("Short Kraken initialization write.".into());
                    }
                }
                let mut firmware = None;
                let mut lighting = false;
                let mut buffer = [0; 64];
                for _ in 0..16 {
                    let n = handle
                        .read_timeout(&mut buffer, 100)
                        .map_err(|_| "Kraken initialization read failed.")?;
                    self.logger.incoming_hid("kraken.init", &buffer[..n]);
                    if n >= 20 && buffer[..2] == [0x11, 0x01] {
                        firmware = Some(format!("{}.{}.{}", buffer[17], buffer[18], buffer[19]));
                    }
                    if n >= 32 && buffer[..2] == [0x21, 0x03] && buffer[14] == 3 {
                        lighting = true;
                    }
                    if firmware.is_some() && lighting {
                        break;
                    }
                }
                let firmware = firmware.ok_or("No firmware response. Control remains disabled.")?;
                if !lighting {
                    return Err("Expected X3 lighting channels were not reported. Control remains disabled.".into());
                }
                self.owned.insert(
                    request.device_id.clone(),
                    OwnedKraken {
                        handle,
                        firmware: firmware.clone(),
                    },
                );
                return Ok((
                    CommandState::Enabled,
                    format!(
                        "Control enabled · Firmware {firmware}. No pump or color setting was applied."
                    ),
                ));
            }
            let owned = self
                .owned
                .get(&request.device_id)
                .ok_or("Enable control for this device first.")?;
            validate_cooling_action(
                &request.action,
                self.history.get(&request.device_id).and_then(|h| h.last()),
                now_ms(),
            )?;
            let packets = match &request.action {
                ControlAction::PumpCurve(curve) => vec![commands::pump_report(curve)?],
                ControlAction::PumpFull => vec![commands::pump_report(
                    &PumpCurve::example(Preset::Performance).full_speed(),
                )?],
                ControlAction::Lighting(settings) => commands::lighting_reports(settings)?,
                _ => return Err("Unsupported control command.".into()),
            };
            for packet in packets {
                if owned.handle.write(&packet).map_err(|_|"USB command failed; device settings may be partially changed. No automatic retry.")? != 64 { return Err("Short USB write. Outcome is uncertain; no automatic retry.".into()); }
            }
            Ok((CommandState::Submitted,"USB command submitted. Check live response/visible lighting; this is not stored-setting readback or hardware verification.".into()))
        })();
        match outcome {
            Ok((state, detail)) => CommandReceipt::success(request, state, detail),
            Err(detail) => {
                if detail.contains("USB") || detail.contains("longer connected") {
                    self.owned.remove(&request.device_id);
                }
                CommandReceipt::rejected(request, detail)
            }
        }
    }
    fn poll(&mut self) -> Vec<Device> {
        let api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(_) => {
                self.owned.clear();
                self.history.clear();
                let mut d = inventory_target();
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
            let id = kraken_id(info);
            if !seen.insert(id.clone()) {
                continue;
            }
            let mut device = kraken_target();
            device.id = id.clone();
            device.status = DeviceStatus::Detected;
            device.control.available = true;
            device.control.pump = true;
            let opened = if self.owned.contains_key(&id) {
                Ok(None)
            } else {
                info.open_device(&api).map(Some)
            };
            match opened {
                Ok(temporary) => {
                    let handle = temporary
                        .as_ref()
                        .unwrap_or_else(|| &self.owned.get(&id).unwrap().handle);
                    device.control = self.owned.get(&id).map_or_else(
                        || ControlState {
                            available: true,
                            pump: true,
                            ..ControlState::default()
                        },
                        |owned| ControlState {
                            available: true,
                            pump: true,
                            lighting_outputs: vec![],
                            enabled: true,
                            firmware: Some(owned.firmware.clone()),
                            conflict: None,
                        },
                    );
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
                            Ok(n) => {
                                self.logger.incoming_hid(&id, &buffer[..n]);
                                match parse_status(&buffer[..n], now_ms()) {
                                    Ok(t) => {
                                        last = Some(t);
                                        error = None;
                                    }
                                    Err(e) if n > 0 && buffer[0] == 0x75 => {
                                        last = None;
                                        error = Some(e);
                                    }
                                    Err(_) => {}
                                }
                            }
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
                            "Live USB telemetry. Device settings change only through explicit commands."
                                .into();
                    } else {
                        device.detail = error.unwrap_or("Detected, but no status stream is available. Device initialization or a competing controller may be involved. No automatic initialization was performed.").into();
                        if error.is_some() {
                            device.status = DeviceStatus::Error;
                            if error.is_some_and(|e| e.starts_with("USB")) {
                                self.owned.remove(&id);
                                device.control.enabled = false;
                            }
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
        self.owned.retain(|id, _| seen.contains(id));
        if devices.is_empty() {
            devices.push(inventory_target());
        }
        devices
    }
}

#[cfg(windows)]
#[derive(Default)]
struct LiveAdapters {
    kraken: KrakenAdapter,
    gigabyte: GigabyteBackend,
}
#[cfg(windows)]
impl DeviceAdapter for LiveAdapters {
    fn set_logger(&mut self, logger: LogStore) {
        self.kraken.set_logger(logger.clone());
        self.gigabyte.set_logger(logger);
    }
    fn poll(&mut self) -> Vec<Device> {
        let mut devices = self.kraken.poll();
        devices.extend(self.gigabyte.poll());
        devices
    }
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        if request.device_id == "motherboard" {
            self.gigabyte.control(request)
        } else {
            self.kraken.control(request)
        }
    }
}

pub fn adapter(mode: Mode) -> Box<dyn DeviceAdapter> {
    if mode == Mode::Demo {
        return Box::new(DemoAdapter::default());
    }
    #[cfg(windows)]
    {
        Box::new(LiveAdapters::default())
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
        let mut d = inventory_target();
        d.status = DeviceStatus::AdapterNeeded;
        d.detail = "Live HID adapter currently targets Windows. Demo mode is available.".into();
        vec![d]
    }
}

pub fn poll_snapshot(adapter: &mut dyn DeviceAdapter, mode: Mode) -> Snapshot {
    let mut snapshot = Snapshot::empty(mode);
    let mut devices = adapter.poll();
    if !devices.iter().any(|d| d.id == "motherboard") {
        devices.push(discover_board());
    }
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
    fn cooling_guard_preserves_full_pump_and_independent_lighting_without_telemetry() {
        let curve = ControlAction::PumpCurve(PumpCurve::example(Preset::Balanced));
        assert!(validate_cooling_action(&curve, None, 100).is_err());
        assert!(validate_cooling_action(&ControlAction::PumpFull, None, 100).is_ok());
        let lighting = ControlAction::Lighting(lightdriver_core::Draft::default().lighting());
        assert!(validate_cooling_action(&lighting, None, 100).is_ok());
        let mut sample = parse_status(&report(), 100).unwrap();
        sample.coolant_c = 59.0;
        assert!(validate_cooling_action(&curve, Some(&sample), 100).is_err());
        assert!(validate_cooling_action(&ControlAction::PumpFull, Some(&sample), 100).is_ok());
        sample.coolant_c = 32.0;
        assert!(validate_cooling_action(&curve, Some(&sample), 100).is_ok());
        assert!(validate_cooling_action(&curve, Some(&sample), 5_101).is_err());
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
