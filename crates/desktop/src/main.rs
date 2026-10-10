#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod charts;
mod controls;
mod cpu_sensors;
mod diagnostics;
mod logs;
mod performance;
mod persistence;
mod style;
mod ui;

use iced::{Element, Font, Subscription, Task, Theme, window};
use lightdriver_core::{
    CommandReceipt, ControlAction, DeviceStatus, Draft, LightingChannel, LightingEffect, LogBatch,
    LogLevel, Mode, Preset, ProfileLibrary, PumpCurve, RigConfiguration, Snapshot, now_ms,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Profile,
    Lighting,
    Capabilities,
    Verification,
    Diagnostics,
    Setup,
    Logs,
}
impl Tab {
    const ALL: [Self; 6] = [
        Self::Profile,
        Self::Lighting,
        Self::Capabilities,
        Self::Verification,
        Self::Diagnostics,
        Self::Setup,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Profile => "Pump profile",
            Self::Lighting => "Lighting",
            Self::Capabilities => "Capabilities",
            Self::Verification => "Verification",
            Self::Diagnostics => "Diagnostics",
            Self::Setup => "Setup",
            Self::Logs => "Logs",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    LogLevel(LogLevel),
    Logs(u64, Result<LogBatch, String>),
    LogPause,
    LogFollow(bool),
    LogSummary(bool),
    LogFilter(String),
    LogClear,
    LogExport,
    CoreDetails(bool),
    EnableCpuSensors,
    CpuSensorsEnabled(Result<Snapshot, String>),
    Renderer(iced::system::Information),
    Frame(Instant),
    Snapshot(Result<Snapshot, String>),
    Reconnect,
    Mode(Mode),
    SelectDevice(String),
    Tab(Tab),
    Preset(Preset),
    Duty(usize, u8),
    Brightness(u8),
    Color([u8; 3]),
    SaveDraft,
    Apply(ControlAction),
    Applied(Result<CommandReceipt, String>),
    LightingChannel(LightingChannel),
    LightingEffect(LightingEffect),
    LightingSpeed(u8),
    ColorComponent(usize, u8),
    Temperature(usize, u8),
    AddPoint,
    RemovePoint(usize),
    ProfileName(String),
    SelectProfile(String),
    SaveProfile,
    DeleteProfile,
    ImportPath(String),
    ImportProfile,
    ExportProfile,
    Header(String),
    SaveWiring,
    Evidence(usize, bool),
    ExportTestReport,
    CopyReport,
    ExportReport,
    Capture,
    Captured(window::Screenshot),
}

struct Workbench {
    mode: Mode,
    logs: logs::LiveLogs,
    snapshot: Snapshot,
    connected: bool,
    selected_device: String,
    tab: Tab,
    show_cpu_cores: bool,
    cpu_enable_in_flight: bool,
    enable_cpu_on_connect: bool,
    preset: Option<Preset>,
    draft: Draft,
    profiles: ProfileLibrary,
    profile_name: String,
    selected_profile: Option<String>,
    import_path: String,
    rig: RigConfiguration,
    header: String,
    command_in_flight: bool,
    command_count: u64,
    last_receipt: Option<CommandReceipt>,
    evidence: [bool; 3],
    data_directory: PathBuf,
    status: String,
    service_error: Option<String>,
    poll_in_flight: bool,
    booted: Instant,
    capture_path: Option<PathBuf>,
    capture_started: bool,
    renderer: String,
    adapter: String,
    benchmark: Option<performance::Benchmark>,
}

impl Workbench {
    fn new() -> (Self, Task<Message>) {
        let args: Vec<_> = std::env::args().skip(1).collect();
        let value = |key: &str| args.windows(2).find(|p| p[0] == key).map(|p| p[1].clone());
        let mode = if args.iter().any(|a| a == "--demo") {
            Mode::Demo
        } else {
            Mode::Live
        };
        let data_directory = value("--data-dir")
            .map(PathBuf::from)
            .unwrap_or_else(persistence::default_directory);
        let (draft, mut status) = match persistence::load(&data_directory) {
            Ok(d) => (
                d,
                "Profiles load as drafts · Apply settings explicitly".into(),
            ),
            Err(e) => (Draft::default(), e),
        };
        let profiles = match persistence::load_profiles(&data_directory) {
            Ok(p) => p,
            Err(e) => {
                status = format!("Profiles were not loaded: {e}. Existing file is preserved.");
                ProfileLibrary::default()
            }
        };
        let rig = match persistence::load_rig(&data_directory) {
            Ok(p) => p,
            Err(e) => {
                status = format!("Wiring was not loaded: {e}. Existing file is preserved.");
                RigConfiguration::default()
            }
        };
        let tab = match value("--tab").as_deref() {
            Some("lighting") => Tab::Lighting,
            Some("capabilities") => Tab::Capabilities,
            Some("verification") => Tab::Verification,
            Some("diagnostics") => Tab::Diagnostics,
            Some("setup") => Tab::Setup,
            Some("logs") => Tab::Logs,
            _ => Tab::Profile,
        };
        let mut state = Self {
            mode,
            logs: logs::LiveLogs::new(
                value("--log-level")
                    .as_deref()
                    .and_then(LogLevel::parse)
                    .unwrap_or_default(),
            ),
            snapshot: Snapshot::empty(mode),
            connected: false,
            selected_device: value("--device").unwrap_or_else(|| "inventory".into()),
            tab,
            show_cpu_cores: args.iter().any(|a| a == "--cores"),
            cpu_enable_in_flight: false,
            enable_cpu_on_connect: args.iter().any(|a| a == "--enable-cpu-sensors"),
            preset: None,
            draft,
            profiles,
            rig,
            profile_name: String::new(),
            selected_profile: None,
            import_path: String::new(),
            header: String::new(),
            command_in_flight: false,
            command_count: 0,
            last_receipt: None,
            evidence: [false; 3],
            data_directory,
            status,
            service_error: None,
            poll_in_flight: false,
            booted: Instant::now(),
            capture_path: value("--capture").map(PathBuf::from),
            capture_started: false,
            renderer: "Starting renderer".into(),
            adapter: String::new(),
            benchmark: value("--benchmark").map(|p| performance::Benchmark::new(PathBuf::from(p))),
        };
        let _ = state.log_update(&Message::LogSummary(
            args.iter().any(|a| a == "--log-summary"),
        ));
        if let Some(filter) = value("--log-filter") {
            let _ = state.log_update(&Message::LogFilter(filter));
        }
        let task = state.fetch(!args.iter().any(|a| a == "--no-autostart"));
        (
            state,
            Task::batch([task, iced::system::information().map(Message::Renderer)]),
        )
    }

    fn fetch(&mut self, try_start: bool) -> Task<Message> {
        if self.poll_in_flight || self.cpu_enable_in_flight || self.command_in_flight {
            return Task::none();
        }
        self.poll_in_flight = true;
        Task::perform(fetch_snapshot(self.mode, try_start), Message::Snapshot)
    }

    fn selected(&self) -> &lightdriver_core::Device {
        self.snapshot
            .devices
            .iter()
            .find(|d| d.id == self.selected_device)
            .unwrap_or(&self.snapshot.devices[0])
    }

    fn telemetry(&self) -> Option<&lightdriver_core::Telemetry> {
        if !self.connected
            || !matches!(
                self.selected().status,
                DeviceStatus::Reading | DeviceStatus::Demo
            )
        {
            return None;
        }
        self.selected()
            .telemetry
            .as_ref()
            .filter(|t| t.is_fresh_at(now_ms()))
    }

    fn device_status<'a>(&self, device: &'a lightdriver_core::Device) -> &'a str {
        if device.id == "motherboard" {
            return device.status.label();
        }
        if !self.connected {
            return "Service offline";
        }
        if matches!(device.status, DeviceStatus::Reading | DeviceStatus::Demo)
            && device
                .telemetry
                .as_ref()
                .is_none_or(|t| !t.is_fresh_at(now_ms()))
        {
            return "Stale telemetry";
        }
        device.status.label()
    }

    fn report(&self) -> String {
        let connection = if self.connected {
            "Connected"
        } else {
            "Offline; snapshot is historical"
        };
        format!(
            "Renderer: {}\nAdapter: {}\nLocal service: {connection}\n{}",
            self.renderer,
            self.adapter,
            self.snapshot.diagnostic_report()
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if let Some(task) = self.log_update(&message) {
            return task;
        }
        if let Some(task) = self.control_update(&message) {
            return task;
        }
        match message {
            Message::CoreDetails(value) => self.show_cpu_cores = value,
            Message::EnableCpuSensors => {
                if self.mode != Mode::Live
                    || self.cpu_enable_in_flight
                    || self.command_in_flight
                    || !self.connected
                    || !self.snapshot.diagnostics.cpu_elevation_needed
                {
                    return Task::none();
                }
                self.cpu_enable_in_flight = true;
                self.connected = false;
                self.status = "Waiting for Windows permission to enable CPU sensors".into();
                return Task::perform(cpu_sensors::enable(), Message::CpuSensorsEnabled);
            }
            Message::CpuSensorsEnabled(result) => {
                self.cpu_enable_in_flight = false;
                self.booted = Instant::now();
                match result {
                    Ok(snapshot) => {
                        self.status = "CPU sensor service started · Read-only monitoring".into();
                        return self.update(Message::Snapshot(Ok(snapshot)));
                    }
                    Err(error) => {
                        self.status = error;
                        return self.fetch(true);
                    }
                }
            }
            Message::Renderer(info) => {
                self.renderer = info.graphics_backend;
                self.adapter = info.graphics_adapter;
            }
            Message::Frame(at) => {
                if let Some(benchmark) = &mut self.benchmark {
                    if benchmark.frame(at) {
                        if let Err(e) = benchmark.save(&self.renderer, &self.adapter) {
                            eprintln!("Benchmark failed: {e}");
                        }
                        return iced::exit();
                    }
                    let count = benchmark.count();
                    self.draft.curve.points[0].duty = 60 + (count % 15) as u8;
                    self.draft.lighting_brightness = 20 + (count % 81) as u8;
                    let scroll = iced::widget::operation::snap_to(
                        "device-workspace",
                        iced::widget::scrollable::RelativeOffset {
                            x: 0.0,
                            y: (count % 80) as f32 / 80.0,
                        },
                    );
                    if count > 0 && count.is_multiple_of(20) {
                        let size = if count.is_multiple_of(40) {
                            iced::Size::new(1180.0, 880.0)
                        } else {
                            iced::Size::new(980.0, 720.0)
                        };
                        return Task::batch([
                            scroll,
                            window::latest().and_then(move |id| window::resize(id, size)),
                        ]);
                    }
                    return scroll;
                }
            }
            Message::Tick => {
                if let Some(benchmark) = &self.benchmark
                    && benchmark.timed_out()
                {
                    if let Err(e) = benchmark.save(&self.renderer, &self.adapter) {
                        eprintln!("Benchmark failed: {e}");
                    }
                    return iced::exit();
                }
                if !self.cpu_enable_in_flight
                    && !self.command_in_flight
                    && self.capture_path.is_some()
                    && !self.capture_started
                    && self.booted.elapsed() >= Duration::from_secs(3)
                {
                    self.capture_started = true;
                    return Task::done(Message::Capture);
                }
                let logs = if self.tab == Tab::Logs {
                    self.fetch_logs(None)
                } else {
                    Task::none()
                };
                return Task::batch([self.fetch(false), logs]);
            }
            Message::Snapshot(result) => {
                self.poll_in_flight = false;
                match result {
                    Ok(snapshot) if !snapshot.devices.is_empty() => {
                        if !snapshot
                            .devices
                            .iter()
                            .any(|d| d.id == self.selected_device)
                        {
                            self.selected_device = snapshot.devices[0].id.clone();
                            self.evidence = [false; 3];
                        }
                        let was_connected = self.connected;
                        self.snapshot = snapshot;
                        self.connected = true;
                        self.service_error = None;
                        if self.enable_cpu_on_connect
                            && self.mode == Mode::Live
                            && self.snapshot.diagnostics.cpu_elevation_needed
                        {
                            self.enable_cpu_on_connect = false;
                            return self.update(Message::EnableCpuSensors);
                        }
                        if !was_connected {
                            return self.fetch_logs(Some(self.logs.level));
                        }
                    }
                    Ok(_) => {
                        self.connected = false;
                        self.service_error =
                            Some("Service returned an empty device inventory.".into());
                    }
                    Err(error) => {
                        self.connected = false;
                        self.service_error = Some(error);
                    }
                }
            }
            Message::Reconnect => return self.fetch(true),
            Message::Mode(mode) => {
                if self.poll_in_flight
                    || self.cpu_enable_in_flight
                    || self.command_in_flight
                    || self.mode == mode
                {
                    return Task::none();
                }
                let generation = self.logs.generation.wrapping_add(1);
                self.logs = logs::LiveLogs::new(LogLevel::Info);
                self.logs.generation = generation;
                self.last_receipt = None;
                self.evidence = [false; 3];
                self.mode = mode;
                self.connected = false;
                self.snapshot = Snapshot::empty(mode);
                self.selected_device = "inventory".into();
                self.service_error = None;
                self.status = if mode == Mode::Demo {
                    "Demo mode · Simulated data; hardware is unchanged".into()
                } else {
                    "Live mode · Waiting for device telemetry".into()
                };
                return self.fetch(true);
            }
            Message::SelectDevice(id) => {
                if self.command_in_flight {
                    return Task::none();
                }
                if self.selected_device != id {
                    self.evidence = [false; 3];
                }
                self.selected_device = id;
                if matches!(self.tab, Tab::Diagnostics | Tab::Logs) {
                    self.tab = Tab::Profile;
                }
            }
            Message::Tab(tab) => {
                self.tab = tab;
                if tab == Tab::Logs {
                    return self.fetch_logs(None);
                }
            }
            Message::Preset(preset) => {
                self.preset = Some(preset);
                self.draft.curve = PumpCurve::example(preset);
                self.status = "Example curve selected · Local draft only".into();
            }
            Message::Duty(index, duty) => {
                if let Some(p) = self.draft.curve.points.get_mut(index) {
                    p.duty = duty;
                    self.preset = None;
                }
            }
            Message::Brightness(value) => self.draft.lighting_brightness = value,
            Message::Color(value) => self.draft.lighting_rgb = value,
            Message::SaveDraft => {
                self.status = match persistence::save(&self.data_directory, &self.draft) {
                    Ok(_) => "Draft saved locally · Hardware settings are unchanged".into(),
                    Err(e) => format!("Draft not saved: {e}"),
                }
            }
            Message::CopyReport => {
                self.status = "Diagnostic report copied · USB paths and serials omitted".into();
                return iced::clipboard::write(self.report());
            }
            Message::ExportReport => {
                let result = std::fs::create_dir_all(&self.data_directory).and_then(|_| {
                    std::fs::write(self.data_directory.join("diagnostics.txt"), self.report())
                });
                self.status = match result {
                    Ok(()) => format!(
                        "Report saved: {}",
                        self.data_directory.join("diagnostics.txt").display()
                    ),
                    Err(e) => format!("Report not saved: {e}"),
                };
            }
            Message::Capture => {
                return window::latest()
                    .and_then(window::screenshot)
                    .map(Message::Captured);
            }
            Message::LogLevel(_)
            | Message::Logs(_, _)
            | Message::LogPause
            | Message::LogFollow(_)
            | Message::LogSummary(_)
            | Message::LogFilter(_)
            | Message::LogClear
            | Message::LogExport => {}
            Message::Apply(_)
            | Message::Applied(_)
            | Message::LightingChannel(_)
            | Message::LightingEffect(_)
            | Message::LightingSpeed(_)
            | Message::ColorComponent(_, _)
            | Message::Temperature(_, _)
            | Message::AddPoint
            | Message::RemovePoint(_)
            | Message::ProfileName(_)
            | Message::SelectProfile(_)
            | Message::SaveProfile
            | Message::DeleteProfile
            | Message::ImportPath(_)
            | Message::ImportProfile
            | Message::ExportProfile
            | Message::Header(_)
            | Message::SaveWiring
            | Message::Evidence(_, _)
            | Message::ExportTestReport => {}
            Message::Captured(screenshot) => {
                if let Some(path) = &self.capture_path {
                    let _ = std::fs::write(
                        path.with_extension("renderer.txt"),
                        format!("Renderer: {}\nAdapter: {}\n", self.renderer, self.adapter),
                    );
                    if let Err(error) = save_capture(path, screenshot) {
                        eprintln!("Capture failed: {error}");
                    }
                    return iced::exit();
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let polling = iced::time::every(Duration::from_millis(1000)).map(|_| Message::Tick);
        if self.benchmark.is_some() {
            Subscription::batch([polling, window::frames().map(Message::Frame)])
        } else {
            polling
        }
    }
    fn theme(&self) -> Theme {
        style::theme()
    }
    fn view(&self) -> Element<'_, Message> {
        self.layout()
    }
}

async fn fetch_snapshot(mode: Mode, try_start: bool) -> Result<Snapshot, String> {
    match lightdriver_ipc::snapshot(mode).await {
        Ok(snapshot) => return Ok(snapshot),
        Err(e)
            if try_start
                && matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
        Err(e) => return Err(format!("Local service unavailable: {e}")),
    }
    let binary = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(if cfg!(windows) {
            "lightdriver-service.exe"
        } else {
            "lightdriver-service"
        });
    if !binary.is_file() {
        return Err(
            "Build the workspace first so lightdriver-service is beside the desktop executable."
                .into(),
        );
    }
    let mut command = std::process::Command::new(binary);
    if mode == Mode::Demo {
        command.arg("--demo");
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|e| format!("Could not start the local service: {e}"))?;
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Ok(snapshot) = lightdriver_ipc::snapshot(mode).await {
            return Ok(snapshot);
        }
    }
    Err("Service started but did not respond. Reconnect to retry.".into())
}

fn save_capture(path: &std::path::Path, screenshot: window::Screenshot) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&screenshot.rgba)
        .map_err(|e| e.to_string())
}

fn main() -> iced::Result {
    let args: Vec<_> = std::env::args().collect();
    let size = args
        .windows(2)
        .find(|p| p[0] == "--size")
        .and_then(|p| p[1].split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
        .map(|(w, h)| iced::Size::new(f32::from(w.max(980)), f32::from(h.max(720))))
        .unwrap_or_else(|| iced::Size::new(1180.0, 880.0));
    iced::application(Workbench::new, Workbench::update, Workbench::view)
        .title("LightDriver · Workbench")
        .subscription(Workbench::subscription)
        .theme(Workbench::theme)
        .default_font(Font::with_name("Segoe UI"))
        .window(window::Settings {
            icon: lightdriver_branding::window_icon(),
            size,
            min_size: Some(iced::Size::new(980.0, 720.0)),
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::Telemetry;

    #[test]
    fn controls_use_capabilities_and_explicit_enable_with_independent_full_pump_guard() {
        let (mut state, _) = Workbench::new();
        state.poll_in_flight = false;
        state.mode = Mode::Demo;
        state.snapshot = Snapshot::empty(Mode::Demo);
        state.connected = true;
        assert!(!state.can_submit(&ControlAction::Enable));
        let mut device = lightdriver_core::kraken_target();
        device.control.available = true;
        device.control.pump = true;
        device.status = DeviceStatus::Detected;
        state.snapshot.devices[0] = device;
        state.selected_device = state.snapshot.devices[0].id.clone();
        assert!(state.can_submit(&ControlAction::Enable));
        assert!(!state.can_submit(&ControlAction::PumpFull));
        state.snapshot.devices[0].control.enabled = true;
        assert!(state.can_submit(&ControlAction::PumpFull));
        assert!(state.can_submit(&ControlAction::Lighting(state.draft.lighting())));
        assert!(!state.can_control());
        state.evidence = [true; 3];
        let _ = state.update(Message::Apply(ControlAction::PumpFull));
        assert!(state.command_in_flight);
        let _ = state.update(Message::Mode(Mode::Live));
        assert_eq!(state.mode, Mode::Demo);
        let _ = state.update(Message::SelectDevice("motherboard".into()));
        assert_ne!(state.selected_device, "motherboard");
        state.command_in_flight = false;
        let _ = state.update(Message::SelectDevice("motherboard".into()));
        assert_eq!(state.evidence, [false; 3]);
        assert!(!state.can_submit(&ControlAction::Enable));
        state.connected = false;
        assert!(!state.can_submit(&ControlAction::Release));
    }

    #[test]
    fn case_routes_require_matching_device_identity_and_saved_header() {
        let (mut state, _) = Workbench::new();
        state.connected = true;
        state.poll_in_flight = false;
        let mut board = lightdriver_core::motherboard_target();
        board.name = "Example motherboard".into();
        board.control.available = true;
        board.control.enabled = true;
        board.control.lighting_outputs = vec![lightdriver_core::LightingOutput {
            id: "argb-2".into(),
            label: "D_LED2".into(),
        }];
        state.snapshot.devices = vec![board];
        state.selected_device = "motherboard".into();
        assert!(!state.can_submit(&ControlAction::PumpFull));
        assert!(state.case_action().is_none());
        state.rig.lighting_routes = vec![lightdriver_core::LightingRoute {
            name: "Case lighting".into(),
            device_id: "motherboard".into(),
            provider: "usb-hid".into(),
            channel: "D_LED2".into(),
            board_identity: Some("Example motherboard".into()),
        }];
        assert!(state.can_submit(&state.case_action().unwrap()));
        state.header = "D_LED1".into();
        assert!(state.case_action().is_none());
        state.header.clear();
        state.snapshot.devices[0].name = "Replacement motherboard".into();
        assert!(state.case_action().is_none());
    }

    fn reading_snapshot(timestamp: u64) -> Snapshot {
        let mut snapshot = Snapshot::empty(Mode::Live);
        snapshot.devices[0].status = DeviceStatus::Reading;
        snapshot.devices[0].telemetry = Some(Telemetry {
            coolant_c: 32.6,
            pump_rpm: 1973,
            pump_duty: 66,
            sampled_at_ms: timestamp,
        });
        snapshot
    }

    #[test]
    fn disconnect_and_mode_change_remove_previous_live_readings() {
        let (mut workbench, _) = Workbench::new();
        let _ = workbench.update(Message::Snapshot(Ok(reading_snapshot(now_ms()))));
        assert!(workbench.telemetry().is_some());
        let _ = workbench.update(Message::Snapshot(Err("service disconnected".into())));
        assert!(workbench.telemetry().is_none());
        assert_eq!(
            workbench.device_status(workbench.selected()),
            "Service offline"
        );
        let _ = workbench.update(Message::Mode(Mode::Demo));
        assert_eq!(workbench.snapshot.mode, Mode::Demo);
        assert!(workbench.telemetry().is_none());
        assert!(workbench.snapshot.devices[0].telemetry.is_none());
    }

    #[test]
    fn connected_service_with_stale_data_is_not_a_live_reading() {
        let (mut workbench, _) = Workbench::new();
        let _ = workbench.update(Message::Snapshot(Ok(reading_snapshot(now_ms() - 5_100))));
        assert!(workbench.connected);
        assert!(workbench.telemetry().is_none());
        assert_eq!(
            workbench.device_status(workbench.selected()),
            "Stale telemetry"
        );
    }

    #[test]
    fn diagnostics_hide_cached_values_after_service_disconnect() {
        let (mut workbench, _) = Workbench::new();
        let mut snapshot = Snapshot::empty(Mode::Live);
        snapshot.diagnostics.sensors.push(lightdriver_core::Sensor {
            id: "gpu.power".into(),
            group: "GPU".into(),
            label: "Power".into(),
            unit: lightdriver_core::Unit::Watts,
            source: "test".into(),
            basic: true,
            status: lightdriver_core::SensorStatus::Available,
            detail: String::new(),
            sample: Some(lightdriver_core::SensorSample {
                at_ms: now_ms(),
                value: 100.0,
            }),
            history: vec![],
        });
        let _ = workbench.update(Message::Snapshot(Ok(snapshot)));
        assert_eq!(
            workbench.diagnostic_value(&workbench.snapshot.diagnostics.sensors[0]),
            Some(100.0)
        );
        let _ = workbench.update(Message::Snapshot(Err("offline".into())));
        assert_eq!(
            workbench.diagnostic_value(&workbench.snapshot.diagnostics.sensors[0]),
            None
        );
    }
    #[test]
    fn cpu_elevation_is_explicit_live_only_and_blocks_repeat_requests() {
        let (mut state, _) = Workbench::new();
        state.mode = Mode::Demo;
        state.connected = true;
        state.snapshot.diagnostics.cpu_elevation_needed = true;
        let _ = state.update(Message::EnableCpuSensors);
        assert!(!state.cpu_enable_in_flight);
        state.mode = Mode::Live;
        let _ = state.update(Message::EnableCpuSensors);
        assert!(state.cpu_enable_in_flight);
        assert!(!state.connected);
        let _ = state.update(Message::Mode(Mode::Demo));
        assert_eq!(state.mode, Mode::Live);
        let _ = state.update(Message::CpuSensorsEnabled(Err("Canceled".into())));
        assert!(!state.cpu_enable_in_flight);
        assert_eq!(state.status, "Canceled");
    }
}
