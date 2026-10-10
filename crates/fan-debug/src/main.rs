#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Standalone firmware preview and independent read-only tachometer monitor.
mod rpm;
mod rpm_check;
use iced::{
    Border, Color, Element, Font, Length, Subscription, Task, Theme,
    widget::{button, column, container, pick_list, row, scrollable, slider, text},
    window,
};
use lightdriver_firmware_probe::{
    Inspection, ProbeReport,
    reference_layout::{Header, HeaderSettings, LevelPreview, preview, synthetic_example},
};
use std::{collections::VecDeque, path::PathBuf, time::Duration};

const ACCENT: Color = Color::from_rgb8(101, 223, 204);
const BG: Color = Color::from_rgb8(16, 19, 25);
const PANEL: Color = Color::from_rgb8(25, 29, 37);
const LINE: Color = Color::from_rgb8(42, 49, 60);
const MUTED: Color = Color::from_rgb8(153, 167, 185);
const AMBER: Color = Color::from_rgb8(215, 182, 116);

#[derive(Debug, Clone)]
enum Message {
    Read,
    ReadFinished(Box<Result<Inspection, String>>),
    Header(Header),
    Percent(u8),
    Preview,
    RpmToggle,
    RpmTick,
    RpmFinished {
        generation: u64,
        result: Box<Result<rpm::Snapshot, String>>,
    },
    Renderer(iced::system::Information),
    Capture,
    Captured(window::Screenshot),
}

struct FanDebug {
    demo: bool,
    busy: bool,
    fields: Option<[HeaderSettings; 4]>,
    selected: Option<Header>,
    percent: u8,
    draft: Option<LevelPreview>,
    status: String,
    logs: VecDeque<String>,
    renderer: String,
    capture_path: Option<PathBuf>,
    capture_started: bool,
    rpm_monitoring: bool,
    rpm_busy: bool,
    rpm_generation: u64,
    rpm_snapshot: Option<rpm::Snapshot>,
    rpm_status: String,
}

impl FanDebug {
    fn new() -> (Self, Task<Message>) {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let demo = args.iter().any(|a| a == "--demo");
        let capture_path = args
            .windows(2)
            .find(|a| a[0] == "--capture")
            .filter(|_| demo)
            .map(|a| PathBuf::from(&a[1]));
        let mut state = Self::empty(demo);
        state.capture_path = capture_path;
        if demo {
            state.rpm_monitoring = true;
            state.rpm_snapshot = Some(rpm::simulated());
            state.rpm_status = "SIMULATED readings · 1 Hz".into();
            state.accept_read(Ok(Self::simulated()));
            if state.capture_path.is_some() {
                state.selected = Some(Header::SysFan1);
                state.build_preview();
            }
        }
        (state, iced::system::information().map(Message::Renderer))
    }
    fn empty(demo: bool) -> Self {
        Self {
            demo,
            busy: false,
            fields: None,
            selected: None,
            percent: 75,
            draft: None,
            status: "Read firmware to inspect identified settings.".into(),
            logs: VecDeque::new(),
            renderer: "Starting GPU renderer".into(),
            capture_path: None,
            capture_started: false,
            rpm_monitoring: false,
            rpm_busy: false,
            rpm_generation: 0,
            rpm_snapshot: None,
            rpm_status: "Monitoring stopped. Click Start RPM monitor to read tachometers.".into(),
        }
    }
    fn simulated() -> Inspection {
        let mut report = ProbeReport::new();
        report.firmware.status = "simulated";
        report.variable.status = "simulated";
        Inspection {
            report,
            fields: Some(synthetic_example()),
        }
    }
    fn log(&mut self, value: impl Into<String>) {
        if self.logs.len() == 80 {
            self.logs.pop_front();
        }
        self.logs.push_back(value.into());
    }
    fn current(&self) -> Option<HeaderSettings> {
        self.fields?
            .into_iter()
            .find(|s| Some(s.header) == self.selected)
    }
    fn accept_read(&mut self, result: Result<Inspection, String>) {
        self.busy = false;
        // A failed/replaced read invalidates the old proposal.
        self.fields = None;
        self.selected = None;
        self.draft = None;
        match result {
            Err(error) => {
                self.status = error;
                self.log(self.status.clone());
            }
            Ok(inspection) => {
                let r = inspection.report;
                self.log(format!(
                    "Firmware: {} · privilege: {} · restoration: {}",
                    r.firmware.status, r.privilege.status, r.privilege_restoration.status
                ));
                let privilege_failed = r.privilege_restoration.error.is_some();
                for step in [&r.firmware, &r.privilege, &r.privilege_restoration] {
                    if let Some(e) = &step.error {
                        self.log(format!(
                            "{}: Win32 {} · {}",
                            e.operation, e.win32_code, e.message
                        ));
                    }
                }
                if let Some(e) = &r.variable.error {
                    self.log(format!(
                        "{}: Win32 {} · {}",
                        e.operation, e.win32_code, e.message
                    ));
                }
                if let Some(m) = &r.variable.metadata {
                    self.log(format!(
                        "{} bytes · attributes {} · image {} · declared length {}",
                        m.length_bytes,
                        m.attributes_hex,
                        m.header.board_image_id.as_deref().unwrap_or("unknown"),
                        m.header
                            .declared_length_bytes
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "unknown".into())
                    ));
                }
                if privilege_failed {
                    self.status =
                        "Privilege restoration failed. Close this debug app before continuing."
                            .into();
                } else if let Some(fields) = inspection.fields {
                    self.fields = Some(fields);
                    self.status = if self.demo {
                        "SIMULATED settings · no firmware API was called.".into()
                    } else {
                        "Read complete · reference format matched · writable BIOS identity is unverified.".into()
                    };
                    self.log("Observed snapshot only. No write or write-readback verification performed.");
                } else {
                    self.status = if r.privilege.status == "not_assigned" {
                        "Windows requires elevation for this read. Close and launch FanDebug.cmd as administrator, then click Read firmware.".into()
                    } else {
                        "No matching reference fields available. Check the read result below."
                            .into()
                    };
                }
            }
        }
    }
    fn build_preview(&mut self) {
        self.draft = None;
        if let Some(settings) = self.current() {
            match preview(settings, self.percent) {
                Ok(draft) => {
                    self.log(format!("{} · {}% preview · raw level {} · {:.2}% representable · {} candidate byte changes · NOT WRITTEN",
                        settings.header, draft.requested_percent, draft.level_raw,
                        draft.representable_percent, draft.partial_changes.len()));
                    self.draft = Some(draft);
                }
                Err(error) => {
                    self.status = error.into();
                    self.log(error);
                }
            }
        }
    }
    fn poll_rpm(&mut self) -> Task<Message> {
        if !self.rpm_monitoring || self.rpm_busy {
            return Task::none();
        }
        if self.demo {
            self.rpm_snapshot = Some(rpm::simulated());
            return Task::none();
        }
        self.rpm_busy = true;
        let generation = self.rpm_generation;
        Task::perform(
            async {
                tokio::task::spawn_blocking(rpm::sample)
                    .await
                    .unwrap_or_else(|e| Err(format!("RPM worker failed: {e}")))
            },
            move |result| Message::RpmFinished {
                generation,
                result: Box::new(result),
            },
        )
    }
    fn accept_rpm(&mut self, generation: u64, result: Result<rpm::Snapshot, String>) {
        self.rpm_busy = false;
        if generation != self.rpm_generation || !self.rpm_monitoring {
            return;
        }
        match result {
            Ok(snapshot) => {
                if self.rpm_snapshot.is_none() {
                    self.log(format!("RPM source: {}", snapshot.source));
                }
                self.rpm_snapshot = Some(snapshot);
                self.rpm_status = "Live tachometer readings · 1 Hz · fan settings unchanged".into();
            }
            Err(error) => {
                self.rpm_snapshot = None;
                self.rpm_monitoring = false;
                self.rpm_status = error;
                self.log(format!("RPM monitor stopped: {}", self.rpm_status));
            }
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Read if !self.busy => {
                self.busy = true;
                self.fields = None;
                self.selected = None;
                self.draft = None;
                self.status = "Reading the fixed fan-settings variable…".into();
                if self.demo {
                    self.accept_read(Ok(Self::simulated()));
                } else {
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(lightdriver_firmware_probe::inspect_fields)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        |result| Message::ReadFinished(Box::new(result)),
                    );
                }
            }
            Message::ReadFinished(result) => self.accept_read(*result),
            Message::Header(header) if Header::CASE.contains(&header) && self.fields.is_some() => {
                self.selected = Some(header);
                self.draft = None;
                self.status = format!(
                    "Selected {header}. Confirm the physical fan wiring before any future write."
                );
            }
            Message::Percent(percent) => {
                self.percent = percent.clamp(20, 100);
                self.draft = None;
            }
            Message::Preview => self.build_preview(),
            Message::RpmToggle => {
                self.rpm_monitoring = !self.rpm_monitoring;
                self.rpm_generation = self.rpm_generation.wrapping_add(1);
                self.rpm_snapshot = None;
                self.rpm_status = if self.rpm_monitoring {
                    "Connecting to read-only tachometers…".into()
                } else {
                    "Monitoring stopped.".into()
                };
                self.log(self.rpm_status.clone());
                return self.poll_rpm();
            }
            Message::RpmTick => return self.poll_rpm(),
            Message::RpmFinished { generation, result } => self.accept_rpm(generation, *result),
            Message::Renderer(info) => self.renderer = info.graphics_backend,
            Message::Capture if !self.capture_started => {
                self.capture_started = true;
                return window::latest()
                    .and_then(window::screenshot)
                    .map(Message::Captured);
            }
            Message::Captured(screenshot) => {
                if let Some(path) = &self.capture_path
                    && let Err(error) = save_capture(path, screenshot)
                {
                    self.status = format!("Capture failed: {error}");
                }
                return iced::exit();
            }
            _ => {}
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
        if self.capture_path.is_some() && !self.capture_started {
            subscriptions.push(iced::time::every(Duration::from_secs(2)).map(|_| Message::Capture));
        }
        if self.rpm_monitoring {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::RpmTick));
        }
        Subscription::batch(subscriptions)
    }
    fn theme(&self) -> Theme {
        Theme::custom(
            "LightDriver Fan Debug",
            iced::theme::Palette {
                background: BG,
                text: Color::from_rgb8(237, 242, 248),
                primary: ACCENT,
                success: ACCENT,
                warning: AMBER,
                danger: Color::from_rgb8(235, 123, 134),
            },
        )
    }
    fn view(&self) -> Element<'_, Message> {
        let heading = row![
            column![
                text("LightDriver").size(14).color(ACCENT),
                text("Firmware fan lab").size(30),
                text("Fixed levels · inspect · preview · verify")
                    .size(13)
                    .color(MUTED)
            ]
            .spacing(5),
            iced::widget::Space::new().width(Length::Fill),
            container(
                text(if self.demo {
                    "DEMO / SIMULATED"
                } else {
                    "LIVE / READ ONLY"
                })
                .size(12)
                .color(AMBER)
            )
            .padding(10)
            .style(panel),
        ]
        .align_y(iced::Alignment::Center);
        let rpm_action = button(if self.rpm_monitoring {
            "Stop RPM monitor"
        } else {
            "Start RPM monitor"
        })
        .on_press(Message::RpmToggle)
        .padding([10, 15]);
        let mut rpm_view = column![
            row![
                text("Fan speeds").size(20),
                iced::widget::Space::new().width(Length::Fill),
                rpm_action
            ]
            .align_y(iced::Alignment::Center),
            text(&self.rpm_status)
                .size(12)
                .color(if self.rpm_monitoring { MUTED } else { AMBER }),
        ]
        .spacing(12);
        if let Some(snapshot) = &self.rpm_snapshot {
            let fresh = snapshot.sampled_at.elapsed() < Duration::from_secs(3);
            rpm_view = rpm_view
                .push(
                    row(snapshot.readings.iter().map(|reading| {
                        container(
                            column![
                                text(reading.header).size(12).color(MUTED),
                                text(if fresh {
                                    reading
                                        .rpm
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "—".into())
                                } else {
                                    "—".into()
                                })
                                .size(30)
                                .color(ACCENT),
                                text(format!("RPM · channel {}", reading.channel))
                                    .size(10)
                                    .color(MUTED),
                                text(if fresh {
                                    reading.detail
                                } else {
                                    "Stale sample"
                                })
                                .size(10)
                                .color(MUTED),
                            ]
                            .spacing(5),
                        )
                        .padding(12)
                        .width(Length::Fill)
                        .style(panel)
                        .into()
                    }))
                    .spacing(9),
                )
                .push(
                    text(format!(
                        "{} · sample age {:.1}s{}",
                        snapshot.source,
                        snapshot.sampled_at.elapsed().as_secs_f32(),
                        if fresh { "" } else { " · STALE" }
                    ))
                    .size(11)
                    .color(MUTED),
                );
        }
        rpm_view = rpm_view.push(text("A hub usually returns one fan’s tachometer signal. Header labels need physical confirmation; absent tach pulses can mean stopped or disconnected.").size(11).color(MUTED));
        let controls = row![
            button(if self.busy {
                "Reading…"
            } else if self.demo {
                "Reload demo"
            } else {
                "Read firmware"
            })
            .on_press_maybe((!self.busy).then_some(Message::Read))
            .padding([11, 18]),
            pick_list(Header::CASE.as_slice(), self.selected, Message::Header)
                .placeholder("Select case-fan header")
                .width(230),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);
        let presets = row([30, 50, 75, 100].map(|p| {
            button(text(format!("{p}%")))
                .on_press(Message::Percent(p))
                .padding([9, 15])
                .into()
        }))
        .spacing(8);
        let target = column![
            label("REQUESTED LEVEL"),
            row![text(format!("{}%", self.percent)).size(58).color(ACCENT),
                iced::widget::Space::new().width(Length::Fill), text("PWM level draft").size(13).color(MUTED)].align_y(iced::Alignment::Center),
            slider(20..=100, self.percent, Message::Percent),
            presets,
            button("Preview identified byte changes").on_press_maybe(self.current().map(|_| Message::Preview)).padding([11, 16]),
            row![button("Write + read back").padding([10, 14]),
                button("Restore original").padding([10, 14])].spacing(8),
            text("Writes are locked: complete curve encoding, exact BIOS identity and recovery are not verified.")
                .size(13).color(AMBER),
            text("A stored firmware change may require reboot before fans respond. A matching variable readback would verify storage only.")
                .size(12).color(MUTED),
        ].spacing(16);
        let current: Element<'_, Message> = if let Some(s) = self.current() {
            let mut rows = column![
                text(format!("{} · observed fields", s.header)).size(20),
                pair(
                    "Stored mode",
                    format!("{} ({})", s.mode_label(), s.mode_raw)
                ),
                pair("Start level", format!("{}/255 (raw)", s.start_level_raw)),
                pair(
                    "Off / start / full thresholds",
                    format!(
                        "{} / {} / {} °C",
                        s.off_temperature, s.start_temperature, s.full_temperature
                    )
                ),
                pair("Base coefficient", s.base_coefficient_raw.to_string()),
                pair(
                    "Interval / source",
                    format!("{} / {} (raw enums)", s.interval_raw, s.source_raw)
                ),
                pair(
                    "PWM/DC / fan stop",
                    format!("{} / {} (raw enums)", s.pwm_dc_raw, s.fan_stop_raw)
                ),
            ]
            .spacing(12);
            for (i, v) in s.segments.iter().enumerate() {
                rows = rows.push(pair(
                    &format!("Extra segment {}", i + 1),
                    format!(
                        "{} °C · delta {:+} · aux 0x{:02X}",
                        v.temperature, v.coefficient_delta, v.auxiliary_raw
                    ),
                ));
            }
            rows.push(text("Identified fields only; auxiliary and unknown bytes do not have a verified complete codec.").size(12).color(MUTED)).into()
        } else {
            column![
                text("Observed settings").size(20),
                text("Read firmware, then select SYS_FAN1 or SYS_FAN2.")
                    .size(14)
                    .color(MUTED),
                text("CPU and chipset fan headers are excluded from this first experiment.")
                    .size(12)
                    .color(MUTED)
            ]
            .spacing(15)
            .into()
        };
        let mut proposal = column![text("Proposed change").size(20)].spacing(10);
        if let Some(p) = &self.draft {
            proposal = proposal.push(text(format!("{}% → {}/255 → {:.2}%", p.requested_percent, p.level_raw, p.representable_percent)).size(18).color(ACCENT))
                .push(text(format!("Constant-level candidate between existing {}–{} °C start/full thresholds; existing off/full-speed behavior is retained.", p.start_temperature, p.full_temperature)).size(13).color(MUTED))
                .push(text("Partial proposal only. No complete payload is encoded or written.").size(12).color(AMBER));
            for change in &p.partial_changes {
                proposal = proposal.push(
                    text(format!(
                        "0x{:03X}    0x{:02X} → 0x{:02X}",
                        change.offset, change.before, change.after
                    ))
                    .font(Font::MONOSPACE)
                    .size(13),
                );
            }
        } else {
            proposal = proposal.push(
                text("Choose a header and percentage, then Preview.")
                    .size(13)
                    .color(MUTED),
            );
        }
        let mut events = column![text("Read / preview events").size(18)].spacing(7);
        for log in self.logs.iter().rev().take(12) {
            events = events.push(text(log).size(12).color(MUTED));
        }
        let body = column![
            heading,
            container(rpm_view)
                .padding(20)
                .width(Length::Fill)
                .style(panel),
            controls,
            container(text(&self.status).size(13))
                .padding(14)
                .width(Length::Fill)
                .style(panel),
            row![
                container(target)
                    .padding(22)
                    .width(Length::FillPortion(1))
                    .style(panel),
                container(current)
                    .padding(22)
                    .width(Length::FillPortion(1))
                    .style(panel)
            ]
            .spacing(16),
            container(proposal)
                .padding(22)
                .width(Length::Fill)
                .style(panel),
            container(events)
                .padding(18)
                .width(Length::Fill)
                .style(panel),
            text(format!(
                "{} · {} · no background regulation or startup reapplication",
                self.renderer,
                if cfg!(debug_assertions) {
                    "Debug"
                } else {
                    "Release"
                }
            ))
            .size(11)
            .color(MUTED),
        ]
        .spacing(18)
        .padding(26);
        scrollable(body).into()
    }
}
fn label(value: &str) -> iced::widget::Text<'_> {
    text(value).size(11).color(MUTED)
}
fn pair<'a>(name: &str, value: String) -> Element<'a, Message> {
    row![
        text(name.to_string())
            .size(12)
            .color(MUTED)
            .width(Length::Fill),
        text(value).size(12).width(Length::Fill)
    ]
    .spacing(14)
    .into()
}
fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PANEL.into()),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}
fn save_capture(path: &std::path::Path, screenshot: window::Screenshot) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| e.to_string())?
        .write_image_data(&screenshot.rgba)
        .map_err(|e| e.to_string())
}
fn main() -> iced::Result {
    let check_args: Vec<_> = std::env::args_os().skip(1).collect();
    if check_args.iter().any(|v| v == "--rpm-check") {
        match rpm_check::run(&check_args) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
    let args: Vec<_> = std::env::args().collect();
    let size = args
        .windows(2)
        .find(|a| a[0] == "--size")
        .and_then(|a| a[1].split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
        .map(|(w, h)| iced::Size::new(f32::from(w.max(920)), f32::from(h.max(680))))
        .unwrap_or(iced::Size::new(1080.0, 900.0));
    iced::application(FanDebug::new, FanDebug::update, FanDebug::view)
        .title("LightDriver · Firmware fan lab")
        .theme(FanDebug::theme)
        .subscription(FanDebug::subscription)
        .default_font(Font::with_name("Segoe UI"))
        .window(window::Settings {
            size,
            min_size: Some(iced::Size::new(920.0, 680.0)),
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_refresh_clears_stale_fields_selection_and_draft() {
        let mut state = FanDebug::empty(true);
        state.accept_read(Ok(FanDebug::simulated()));
        state.selected = Some(Header::SysFan1);
        state.build_preview();
        assert!(state.draft.is_some());
        state.accept_read(Err("read failed".into()));
        assert!(state.fields.is_none() && state.selected.is_none() && state.draft.is_none());
    }
    #[test]
    fn cpu_selection_and_percent_changes_cannot_leave_a_stale_preview() {
        let mut state = FanDebug::empty(true);
        state.accept_read(Ok(FanDebug::simulated()));
        let _ = state.update(Message::Header(Header::CpuFan));
        assert!(state.selected.is_none());
        let _ = state.update(Message::Header(Header::SysFan2));
        let _ = state.update(Message::Preview);
        assert_eq!(state.draft.as_ref().unwrap().level_raw, 191);
        let _ = state.update(Message::Percent(50));
        assert!(state.draft.is_none());
        let _ = state.update(Message::Preview);
        assert_eq!(state.draft.as_ref().unwrap().level_raw, 128);
        for i in 0..200 {
            state.log(format!("event {i}"));
        }
        assert_eq!(state.logs.len(), 80);
    }
    #[test]
    fn rpm_failures_and_stopped_sessions_cannot_leave_live_values() {
        let mut state = FanDebug::empty(false);
        state.rpm_monitoring = true;
        state.rpm_generation = 7;
        state.accept_rpm(7, Ok(rpm::simulated()));
        assert!(state.rpm_snapshot.is_some());
        state.accept_rpm(7, Err("driver unavailable".into()));
        assert!(state.rpm_snapshot.is_none());
        assert!(!state.rpm_monitoring);
        state.rpm_monitoring = true;
        state.rpm_generation = 9;
        state.rpm_busy = true;
        state.accept_rpm(8, Ok(rpm::simulated())); // late result from old session
        assert!(state.rpm_snapshot.is_none());
        assert!(!state.rpm_busy);
        state.rpm_monitoring = false;
        state.accept_rpm(9, Ok(rpm::simulated()));
        assert!(state.rpm_snapshot.is_none());
    }
}
