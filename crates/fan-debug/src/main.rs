#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Standalone firmware preview and independent read-only tachometer monitor.
mod firmware_check;
mod graph;
use lightdriver_core::MotherboardBackend;
use lightdriver_gigabyte::{firmware::recovery, rpm};
mod rpm_check;
use iced::{
    Border, Color, Element, Font, Length, Subscription, Task, Theme,
    widget::{
        button, checkbox, column, container, pick_list, row, scrollable, slider, text, text_input,
    },
    window,
};
use lightdriver_gigabyte::firmware::{
    ProbeReport, SnapshotInspection,
    reference_layout::{Header, HeaderSettings, LevelPreview},
    snapshot::{
        Comparison, GigabyteIdentity, OfflineCandidate, Origin, RestoreCandidate, RestoreTarget,
        Snapshot,
    },
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
    InitialFinished(Box<Result<recovery::Recovery, String>>),
    RetryInitial,
    RestorePreview(RestoreTarget),
    Page(LabPage),
    ReadFinished(Box<Result<SnapshotInspection, String>>),
    SnapshotPath(String),
    SaveSnapshot,
    PinBaseline,
    LoadBaseline,
    Verbose(bool),
    FileFinished(Box<Result<FileResult, String>>),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabPage {
    Curve,
    Snapshots,
    Events,
}
impl LabPage {
    const ALL: [Self; 3] = [Self::Curve, Self::Snapshots, Self::Events];
}
impl std::fmt::Display for LabPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Curve => "Curve inspector",
            Self::Snapshots => "Snapshots & comparison",
            Self::Events => "Session events",
        })
    }
}
#[derive(Debug, Clone)]
enum FileResult {
    Saved(String),
    Loaded(Box<Snapshot>),
}

struct FanDebug {
    demo: bool,
    page: LabPage,
    busy: bool,
    fields: Option<[HeaderSettings; 4]>,
    snapshot: Option<Snapshot>,
    initial_snapshot: Option<Snapshot>,
    initial_path: Option<PathBuf>,
    initial_busy: bool,
    initial_error: Option<String>,
    restore_draft: Option<RestoreCandidate>,
    baseline: Option<Snapshot>,
    comparison: Option<Comparison>,
    candidate: Option<OfflineCandidate>,
    snapshot_path: String,
    file_busy: bool,
    saved_fingerprint: Option<String>,
    verbose: bool,
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
        state.page = args
            .windows(2)
            .find(|v| v[0] == "--page")
            .map(|v| match v[1].to_str() {
                Some("snapshots") => LabPage::Snapshots,
                Some("events") => LabPage::Events,
                _ => LabPage::Curve,
            })
            .unwrap_or(LabPage::Curve);
        if demo {
            state.rpm_monitoring = true;
            state.rpm_snapshot = Some(rpm::simulated());
            state.rpm_status = "SIMULATED readings · 1 Hz".into();
            state.accept_read(Ok(Self::simulated()));
            state.initial_snapshot = state.snapshot.clone();
            if state.capture_path.is_some() {
                state.selected = Some(Header::SysFan1);
                state.build_preview();
                state.baseline = state.snapshot.clone();
                state.compare_baseline();
                if let Some(choice) = args.windows(2).find(|v| v[0] == "--restore-preview") {
                    let target = if choice[1] == "factory" {
                        RestoreTarget::Factory
                    } else {
                        RestoreTarget::Initial
                    };
                    let _ = state.update(Message::RestorePreview(target));
                }
            }
        }
        if demo {
            state.initial_snapshot = state.snapshot.clone();
        }
        let tasks = Task::batch([
            iced::system::information().map(Message::Renderer),
            if demo {
                Task::none()
            } else {
                Task::done(Message::Read)
            },
        ]);
        (state, tasks)
    }
    fn empty(demo: bool) -> Self {
        Self {
            demo,
            page: LabPage::Curve,
            busy: false,
            fields: None,
            snapshot: None,
            initial_snapshot: None,
            initial_path: None,
            initial_busy: false,
            initial_error: None,
            restore_draft: None,
            baseline: None,
            comparison: None,
            candidate: None,
            snapshot_path: new_snapshot_path(),
            file_busy: false,
            saved_fingerprint: None,
            verbose: false,
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
    fn simulated() -> SnapshotInspection {
        let mut report = ProbeReport::new();
        report.firmware.status = "simulated";
        report.variable.status = "simulated";
        SnapshotInspection {
            report,
            snapshot: Some(Snapshot::synthetic()),
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
    fn accept_read(&mut self, result: Result<SnapshotInspection, String>) {
        self.busy = false;
        // A failed/replaced read invalidates the old proposal.
        self.fields = None;
        self.snapshot = None;
        self.comparison = None;
        self.candidate = None;
        self.selected = None;
        self.draft = None;
        self.restore_draft = None;
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
                } else if let Some(snapshot) = inspection.snapshot {
                    self.log(format!(
                        "Board: {} · BIOS {} · reviewed identity {}",
                        snapshot.identity.product,
                        snapshot.identity.bios_version,
                        snapshot.identity.reference_match()
                    ));
                    self.fields = Some(snapshot.fields());
                    self.snapshot = Some(snapshot);
                    self.compare_baseline();
                    self.status = if self.demo {
                        "SIMULATED settings · no firmware API was called.".into()
                    } else {
                        "Read complete · reference format matched · writable BIOS identity is unverified.".into()
                    };
                    self.log("Observed snapshot only. No write or write-readback verification performed.");
                } else {
                    self.status = if r.privilege.status == "not_assigned" {
                        "Windows requires elevation for this read. Close and launch FanDebug.cmd as administrator, the startup read runs once.".into()
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
        self.restore_draft = None;
        self.candidate = None;
        if let Some(settings) = self.current() {
            let result = self
                .snapshot
                .as_ref()
                .ok_or("No current snapshot".to_string())
                .and_then(|snapshot| snapshot.candidate(settings.header, self.percent));
            match result {
                Ok(candidate) => {
                    let draft = candidate.preview.clone();
                    self.candidate = Some(candidate);
                    self.log(format!("{} · {}% preview · raw level {} · {:.2}% representable · {} candidate byte changes · NOT WRITTEN",
                        settings.header, draft.requested_percent, draft.level_raw,
                        draft.representable_percent, draft.partial_changes.len()));
                    self.draft = Some(draft);
                }
                Err(error) => {
                    self.status = error.clone();
                    self.log(error);
                }
            }
        }
    }
    fn protect_initial(&mut self) -> Task<Message> {
        if self.initial_snapshot.is_some() || self.initial_busy {
            return Task::none();
        }
        if let Some(snapshot) = self.snapshot.clone() {
            self.initial_snapshot = Some(snapshot); // Anchor before file I/O; failure must not replace the initial state.
            if self.demo {
                return Task::none();
            }
            return self.save_initial();
        }
        Task::none()
    }
    fn save_initial(&mut self) -> Task<Message> {
        if self.initial_busy || self.demo {
            return Task::none();
        }
        if let Some(snapshot) = self.initial_snapshot.clone() {
            self.initial_busy = true;
            self.initial_error = None;
            self.status = "Saving and verifying the initial recovery snapshot…".into();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || recovery::save_initial(snapshot))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|v| v)
                },
                |result| Message::InitialFinished(Box::new(result)),
            );
        }
        Task::none()
    }
    fn compare_baseline(&mut self) {
        self.comparison = None;
        if let (Some(baseline), Some(current)) = (&self.baseline, &self.snapshot) {
            match baseline.compare(current) {
                Ok(comparison) => {
                    self.log(format!("Full-variable comparison: {} changed bytes, {} unidentified; no write was performed.",
                        comparison.changed_bytes, comparison.unidentified_changed_bytes));
                    self.comparison = Some(comparison);
                }
                Err(error) => self.log(error),
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
            Message::Page(page) => self.page = page,
            Message::Read if !self.busy && !self.file_busy && !self.initial_busy => {
                self.busy = true;
                self.fields = None;
                self.snapshot = None;
                self.comparison = None;
                self.candidate = None;
                self.selected = None;
                self.draft = None;
                self.restore_draft = None;
                self.status = "Reading the fixed fan-settings variable…".into();
                if self.demo {
                    self.accept_read(Ok(Self::simulated()));
                } else {
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(|| {
                                lightdriver_gigabyte::GigabyteBackend::default().inspect_firmware()
                            })
                            .await
                            .map_err(|e| e.to_string())
                        },
                        |result| Message::ReadFinished(Box::new(result)),
                    );
                }
            }
            Message::ReadFinished(result) => {
                self.accept_read(*result);
                return self.protect_initial();
            }
            Message::RetryInitial if !self.busy && !self.file_busy && !self.initial_busy => {
                return self.save_initial();
            }
            Message::InitialFinished(result) => {
                self.initial_busy = false;
                match *result {
                    Ok(recovery) => {
                        self.saved_fingerprint = Some(recovery.snapshot.fingerprint());
                        self.initial_path = Some(recovery.path);
                        if self.baseline.is_none() {
                            self.baseline = self.initial_snapshot.clone();
                        }
                        self.compare_baseline();
                        self.status = "Initial settings saved and file readback verified. Later reads keep this recovery original.".into();
                        self.log(self.status.clone());
                    }
                    Err(error) => {
                        self.initial_error = Some(error.clone());
                        self.status = format!(
                            "Initial recovery backup failed; original retained in memory: {error}"
                        );
                        self.log(self.status.clone());
                    }
                }
            }
            Message::RestorePreview(target) => {
                self.draft = None;
                self.restore_draft = None;
                self.candidate = None;
                if let (Some(snapshot), Some(header)) = (&self.snapshot, self.selected) {
                    match snapshot.restore_candidate(header, target, self.initial_snapshot.as_ref())
                    {
                        Ok(plan) => {
                            self.status = format!(
                                "{} restore plan prepared. No firmware write performed.",
                                plan.target
                            );
                            self.log(self.status.clone());
                            self.restore_draft = Some(plan);
                        }
                        Err(error) => {
                            self.status = error;
                            self.log(self.status.clone());
                        }
                    }
                }
            }
            Message::Header(header)
                if !self.busy && Header::CASE.contains(&header) && self.fields.is_some() =>
            {
                self.selected = Some(header);
                self.draft = None;
                self.restore_draft = None;
                self.candidate = None;
                self.status = format!(
                    "Selected {header}. Confirm the physical fan wiring before any future write."
                );
            }
            Message::Percent(percent) => {
                self.percent = percent.clamp(20, 100);
                self.draft = None;
                self.restore_draft = None;
                self.candidate = None;
            }
            Message::Preview => self.build_preview(),
            Message::Verbose(value) => self.verbose = value,
            Message::SnapshotPath(path) if !self.file_busy => self.snapshot_path = path,
            Message::PinBaseline if !self.busy && !self.file_busy && !self.initial_busy => {
                self.baseline = self.snapshot.clone();
                self.log("Current full snapshot set as comparison baseline in memory.");
                self.compare_baseline();
            }
            Message::SaveSnapshot if !self.busy && !self.file_busy && !self.initial_busy => {
                if let Some(snapshot) = self.snapshot.clone() {
                    let path = PathBuf::from(&self.snapshot_path);
                    self.file_busy = true;
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                snapshot.save_private(&path)?;
                                Ok(FileResult::Saved(snapshot.fingerprint()))
                            })
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|v| v)
                        },
                        |result| Message::FileFinished(Box::new(result)),
                    );
                }
            }
            Message::LoadBaseline if !self.busy && !self.file_busy && !self.initial_busy => {
                self.baseline = None;
                self.comparison = None;
                let path = PathBuf::from(&self.snapshot_path);
                let expected = if self.demo {
                    Origin::Synthetic
                } else {
                    Origin::Live
                };
                self.file_busy = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let snapshot = Snapshot::load_private(&path)?;
                            if snapshot.origin != expected {
                                return Err("Snapshot origin does not match demo/live mode".into());
                            }
                            Ok(FileResult::Loaded(Box::new(snapshot)))
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|v| v)
                    },
                    |result| Message::FileFinished(Box::new(result)),
                );
            }
            Message::FileFinished(result) => {
                self.file_busy = false;
                match *result {
                    Ok(FileResult::Saved(fingerprint)) => {
                        self.saved_fingerprint = Some(fingerprint);
                        self.status = "Private snapshot saved and synced. Keep this file local; it contains the full variable.".into();
                    }
                    Ok(FileResult::Loaded(snapshot)) => {
                        self.baseline = Some(*snapshot);
                        self.status =
                            "Comparison baseline loaded. Read firmware to compare current storage."
                                .into();
                        self.compare_baseline();
                    }
                    Err(error) => self.status = format!("Snapshot operation failed: {error}"),
                }
                self.log(self.status.clone());
            }
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
                row![
                    lightdriver_branding::mark(44),
                    text("LightDriver").size(14).color(ACCENT)
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
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
            .on_press_maybe(
                (!self.busy && !self.file_busy && !self.initial_busy).then_some(Message::Read)
            )
            .padding([11, 18]),
            pick_list(Header::CASE.as_slice(), self.selected, Message::Header)
                .placeholder("Select case-fan header")
                .width(230),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);
        let idle = !self.busy && !self.file_busy && !self.initial_busy;
        let mut snapshots = column![
            text("Private snapshots & comparison").size(20),
            text("Save a full local snapshot, then load it after a reboot and Read firmware again. Files contain raw firmware bytes; keep them private. An imported file is never trusted as a write authorization.").size(12).color(MUTED),
            text_input("Existing parent folder + new snapshot filename", &self.snapshot_path)
                .on_input(Message::SnapshotPath).size(12),
            row![
                button("Save current snapshot").on_press_maybe((idle && self.snapshot.is_some()).then_some(Message::SaveSnapshot)).padding([9,12]),
                button("Use current as baseline").on_press_maybe((idle && self.snapshot.is_some()).then_some(Message::PinBaseline)).padding([9,12]),
                button("Load baseline from path").on_press_maybe(idle.then_some(Message::LoadBaseline)).padding([9,12]),
            ].spacing(8),
            text("Recovery targets").size(16),
            row![
                button("Preview initial restore").on_press_maybe((idle && self.initial_snapshot.is_some() && self.selected.is_some()).then_some(Message::RestorePreview(RestoreTarget::Initial))).padding([9,12]),
                button("Preview factory restore").on_press_maybe((idle && self.snapshot.as_ref().is_some_and(|s| s.identity.reference_match()) && self.selected.is_some()).then_some(Message::RestorePreview(RestoreTarget::Factory))).padding([9,12]),
                button("Retry initial backup").on_press_maybe((idle && self.initial_error.is_some()).then_some(Message::RetryInitial)).padding([9,12]),
            ].spacing(8),
            text(if self.demo { "Initial snapshot is synthetic and stays in memory." } else if self.initial_busy { "Initial recovery file is being saved and verified." }
                else if self.initial_path.is_some() { "Initial snapshot protected for this app session. Factory target: reviewed F41c Normal preset; sensor/PWM/fan-stop configuration is preserved." }
                else if self.initial_snapshot.is_some() { "Initial snapshot retained in memory; its recovery file is not ready." }
                else { "No initial snapshot yet. The first successful startup read saves it automatically." }).size(12).color(MUTED),
            checkbox(self.verbose).label("Verbose inspection · show raw changed bytes").on_toggle(Message::Verbose),
        ].spacing(12);
        if let Some(path) = &self.initial_path {
            snapshots = snapshots.push(
                text(format!("Initial recovery file: {}", path.display()))
                    .size(11)
                    .color(MUTED),
            );
        }
        if let Some(error) = &self.initial_error {
            snapshots = snapshots.push(text(error).size(12).color(AMBER));
        }
        if let Some(snapshot) = &self.snapshot {
            snapshots = snapshots.push(
                text(format!(
                    "{} · BIOS {} ({}) · snapshot {} bytes · SHA256 {}",
                    snapshot.identity.product,
                    snapshot.identity.bios_version,
                    snapshot.identity.bios_date,
                    lightdriver_gigabyte::firmware::reference_layout::PAYLOAD_LENGTH,
                    snapshot.fingerprint()
                ))
                .size(11)
                .color(MUTED),
            );
        }
        if let Some(comparison) = &self.comparison {
            snapshots = snapshots.push(
                text(if comparison.identical {
                    "Storage matches baseline byte for byte.".into()
                } else {
                    format!(
                        "{} changed bytes · {} in unidentified fields",
                        comparison.changed_bytes, comparison.unidentified_changed_bytes
                    )
                })
                .size(15)
                .color(if comparison.identical { ACCENT } else { AMBER }),
            );
            for diff in comparison.differences.iter().take(30) {
                let detail = if self.verbose {
                    format!(" · {:02X} → {:02X}", diff.before, diff.after)
                } else {
                    String::new()
                };
                snapshots = snapshots.push(
                    text(format!("0x{:03X} · {}{detail}", diff.offset, diff.field))
                        .size(12)
                        .color(MUTED),
                );
            }
            if comparison.changed_bytes > 30 {
                snapshots = snapshots.push(
                    text("First 30 changes shown; use the offline CLI report for all offsets.")
                        .size(11)
                        .color(MUTED),
                );
            }
            snapshots = snapshots.push(text(comparison.interpretation).size(11).color(AMBER));
        } else {
            snapshots = snapshots.push(
                text("No comparison yet. Choose a baseline, then read current firmware.")
                    .size(12)
                    .color(MUTED),
            );
        }
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
            button("Write + read back").padding([10,14]),
            row![button("Restore initial").padding([10,14]), button("Restore factory").padding([10,14])].spacing(8),
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
            if s.mode_raw != 2 {
                rows = column![
                    text(format!("{} · observed preset",s.header)).size(20),
                    pair("Stored mode",format!("{} ({})",s.mode_label(),s.mode_raw)),
                    text("Stored manual-curve fields are inactive in this preset. Zero values do not represent active temperature thresholds.").size(13).color(AMBER),
                    pair("Interval / source",format!("{} / {} (stored)",s.interval_raw,s.source_raw)),
                    pair("PWM/DC / fan stop",format!("{} / {} (stored)",s.pwm_dc_raw,s.fan_stop_raw)),
                    text("Percentage candidates need a controlled Manual-curve capture before preset conversion can be validated. Restore previews are available in Snapshots & comparison.").size(12).color(MUTED),
                ].spacing(12);
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
        let mut curve = column![text("Curve reconstruction").size(20)].spacing(10);
        let observed_graph = match self.restore_draft.as_ref().map(|r| r.target) {
            Some(RestoreTarget::Initial) => self.initial_snapshot.as_ref().and_then(|s| {
                s.fields()
                    .into_iter()
                    .find(|s| Some(s.header) == self.selected)
            }),
            Some(RestoreTarget::Factory) => self
                .selected
                .and_then(lightdriver_gigabyte::firmware::reference_layout::factory_normal_graph),
            None => self.current(),
        };
        let graph_settings = observed_graph.map(|settings| {
            if settings.mode_raw == 0
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.identity.reference_match())
            {
                lightdriver_gigabyte::firmware::reference_layout::factory_normal_graph(
                    settings.header,
                )
                .unwrap_or(settings)
            } else {
                settings
            }
        });
        if let Some(settings) = graph_settings {
            match graph::CurveGraph::new(settings, self.draft.as_ref()) {
                Ok(graph) => {
                    curve = curve.push(graph);
                }
                Err(error) => {
                    curve = curve.push(text(error).size(13).color(AMBER));
                }
            }
        } else {
            curve = curve.push(
                text("Select a case header to inspect its manual-curve shape.")
                    .size(13)
                    .color(MUTED),
            );
        }
        curve = curve.push(text("Mint: stored/initial manual estimate or reviewed Normal-preset reference. Amber: fixed-level candidate. Full threshold returns to 100%; below start and sensor behaviour are excluded. This graph is not a measured fan response.").size(11).color(MUTED));
        let mut readiness = column![text("Firmware write readiness").size(18)].spacing(10);
        let identity = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.identity.reference_match());
        let backup = self
            .snapshot
            .as_ref()
            .is_some_and(|s| Some(s.fingerprint()) == self.saved_fingerprint);
        readiness = readiness
            .push(pair(
                "Reviewed board + BIOS inventory",
                if identity {
                    "Matches reference (not a write allowlist)"
                } else {
                    "Not established"
                }
                .into(),
            ))
            .push(pair(
                "Full private snapshot saved this session",
                if backup {
                    "Matching snapshot saved"
                } else {
                    "Not saved for current bytes"
                }
                .into(),
            ))
            .push(pair(
                "Initial on-open recovery file",
                if self.demo && self.initial_snapshot.is_some() {
                    "Synthetic · memory only"
                } else if self.initial_path.is_some() {
                    "Saved and file readback verified"
                } else {
                    "Not ready"
                }
                .into(),
            ))
            .push(pair(
                "Complete codec / callback validation",
                "Pending".into(),
            ))
            .push(pair(
                "Firmware write policy / recovery",
                "Unverified · writes locked".into(),
            ))
            .push(pair(
                "Fan response / reboot / cold boot / resume",
                "Not tested".into(),
            ));
        let mut proposal = column![text("Proposed change").size(20)].spacing(10);
        if let Some(plan) = &self.restore_draft {
            proposal = proposal.push(text(format!("{} · {} · {} candidate byte changes",plan.header,plan.target,plan.changes.len())).size(17).color(ACCENT))
                .push(text("Restore preview only. Firmware writes and verified restoration are still locked. Factory selects the reviewed Normal preset; initial restore proposes this case header’s original block and mode/stop fields. Other headers are preserved.").size(12).color(AMBER));
            for change in &plan.changes {
                let detail = if self.verbose {
                    format!(
                        " · 0x{:03X}: {:02X} → {:02X}",
                        change.offset, change.before, change.after
                    )
                } else {
                    String::new()
                };
                proposal = proposal.push(
                    text(format!(
                        "{}{detail}",
                        lightdriver_gigabyte::firmware::snapshot::field_at(change.offset)
                    ))
                    .size(12)
                    .color(MUTED),
                );
            }
        } else if let Some(p) = &self.draft {
            proposal = proposal.push(text(format!("{}% → {}/255 → {:.2}%", p.requested_percent, p.level_raw, p.representable_percent)).size(18).color(ACCENT))
                .push(text(format!("Constant-level candidate between existing {}–{} °C start/full thresholds; existing off/full-speed behavior is retained.", p.start_temperature, p.full_temperature)).size(13).color(MUTED))
                .push(text("Offline patch only. Firmware callback semantics remain unverified; no firmware write performed.").size(12).color(AMBER));
            if let Some(candidate) = &self.candidate {
                proposal = proposal.push(text(format!("{}-byte offline patch · {} changed bytes · all other bytes preserved · candidate SHA256 {}",
                    candidate.data_length(), candidate.changed_bytes(), candidate.candidate_sha256)).size(11).color(MUTED));
            }
            for change in &p.partial_changes {
                proposal = proposal.push(
                    text(format!(
                        "{}{}",
                        lightdriver_gigabyte::firmware::snapshot::field_at(change.offset),
                        if self.verbose {
                            format!(
                                " · 0x{:03X}: 0x{:02X} → 0x{:02X}",
                                change.offset, change.before, change.after
                            )
                        } else {
                            String::new()
                        }
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
        let pages = row(LabPage::ALL.map(|page| {
            button(text(page.to_string()))
                .on_press(Message::Page(page))
                .padding([9, 14])
                .style(if self.page == page {
                    button::primary
                } else {
                    button::secondary
                })
                .into()
        }))
        .spacing(8);
        let mut body = column![
            heading,
            pages,
            container(rpm_view)
                .padding(20)
                .width(Length::Fill)
                .style(panel),
            controls,
            container(text(&self.status).size(13))
                .padding(14)
                .width(Length::Fill)
                .style(panel),
        ]
        .spacing(18);
        match self.page {
            LabPage::Curve => {
                body = body
                    .push(
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
                    )
                    .push(
                        container(curve)
                            .padding(22)
                            .width(Length::Fill)
                            .style(panel),
                    )
                    .push(
                        container(proposal)
                            .padding(22)
                            .width(Length::Fill)
                            .style(panel),
                    )
                    .push(
                        container(readiness)
                            .padding(22)
                            .width(Length::Fill)
                            .style(panel),
                    );
            }
            LabPage::Snapshots => {
                body = body
                    .push(
                        container(snapshots)
                            .padding(22)
                            .width(Length::Fill)
                            .style(panel),
                    )
                    .push(
                        container(readiness)
                            .padding(22)
                            .width(Length::Fill)
                            .style(panel),
                    );
            }
            LabPage::Events => {
                body = body.push(
                    container(events)
                        .padding(18)
                        .width(Length::Fill)
                        .style(panel),
                );
            }
        }
        body = body
            .push(
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
            )
            .padding(26);
        scrollable(body).into()
    }
}
fn new_snapshot_path() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    base.join(format!("fan-snapshot-{stamp}.private.json"))
        .to_string_lossy()
        .into_owned()
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
    if check_args
        .iter()
        .any(|v| v == "--firmware-snapshot" || v == "--compare-firmware")
    {
        match firmware_check::run(&check_args) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
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
            icon: lightdriver_branding::window_icon(),
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
    fn full_snapshot_comparison_survives_refresh_but_failed_reads_clear_current_state() {
        let mut state = FanDebug::empty(true);
        state.accept_read(Ok(FanDebug::simulated()));
        let _ = state.update(Message::PinBaseline);
        assert!(state.comparison.as_ref().unwrap().identical);
        state.accept_read(Err("test failure".into()));
        assert!(
            state.snapshot.is_none() && state.comparison.is_none() && state.candidate.is_none()
        );
        assert!(state.baseline.is_some());
        state.accept_read(Ok(FanDebug::simulated()));
        assert!(state.comparison.as_ref().unwrap().identical);
        state.file_busy = true;
        let _ = state.update(Message::Read);
        assert!(state.snapshot.is_some()); // No concurrent read during a save/import.
        state.file_busy = false;
        let _ = state.update(Message::LoadBaseline);
        assert!(state.baseline.is_none() && state.comparison.is_none());
    }
    #[test]
    fn failed_privilege_restoration_never_exposes_snapshot_or_preview() {
        let mut state = FanDebug::empty(true);
        let mut inspection = FanDebug::simulated();
        // This is synthetic: no privilege or firmware API is invoked.
        inspection.report.privilege_restoration.error =
            Some(lightdriver_gigabyte::firmware::ProbeError {
                operation: "synthetic restore",
                win32_code: 5,
                message: "test failure".into(),
            });
        state.accept_read(Ok(inspection));
        assert!(state.snapshot.is_none() && state.fields.is_none());
    }
    #[test]
    fn initial_state_is_anchored_once_and_restore_plans_are_invalidated_by_edits() {
        let mut state = FanDebug::empty(true);
        state.accept_read(Ok(FanDebug::simulated()));
        let _ = state.protect_initial();
        let original_stamp = state.initial_snapshot.as_ref().unwrap().captured_unix_ms;
        let mut later = FanDebug::simulated();
        later.snapshot.as_mut().unwrap().captured_unix_ms = original_stamp + 10_000;
        state.accept_read(Ok(later));
        let _ = state.protect_initial();
        assert_eq!(
            state.initial_snapshot.as_ref().unwrap().captured_unix_ms,
            original_stamp
        );
        let _ = state.update(Message::Header(Header::SysFan1));
        let _ = state.update(Message::RestorePreview(RestoreTarget::Factory));
        assert_eq!(state.restore_draft.as_ref().unwrap().changes.len(), 1);
        let _ = state.update(Message::Percent(50));
        assert!(state.restore_draft.is_none());
        let _ = state.update(Message::RestorePreview(RestoreTarget::Initial));
        assert!(state.restore_draft.as_ref().unwrap().changes.is_empty());
        state.accept_read(Err("failed refresh".into()));
        assert!(state.restore_draft.is_none() && state.snapshot.is_none());
        assert!(state.initial_snapshot.is_some());
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
