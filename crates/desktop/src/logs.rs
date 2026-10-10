use crate::{
    Message, Tab, Workbench,
    style::{self, heading, label},
};
use iced::{
    Alignment, Element, Font,
    Length::Fill,
    Task,
    widget::{
        Column, Space, button, checkbox, column, container, pick_list, row, scrollable, text,
        text_input,
    },
};
use lightdriver_core::{LOG_CAPACITY, LogBatch, LogEntry, LogLevel, Mode, now_ms};
use std::{collections::VecDeque, io::Write};
const VIEW_LIMIT: usize = 128;
#[derive(Default)]
pub(crate) struct LiveLogs {
    pub level: LogLevel,
    synced_level: Option<LogLevel>,
    pub entries: VecDeque<LogEntry>,
    summary: Vec<String>,
    pub in_flight: bool,
    pending_level: Option<LogLevel>,
    pub generation: u64,
    cursor: u64,
    latest: u64,
    session_id: Option<u64>,
    paused: bool,
    follow: bool,
    filter: String,
    show_summary: bool,
    error: Option<String>,
    gap: bool,
}
impl LiveLogs {
    pub fn new(level: LogLevel) -> Self {
        Self {
            level,
            follow: true,
            ..Self::default()
        }
    }
    fn accept(&mut self, batch: LogBatch) -> bool {
        let restarted = self.session_id.is_some_and(|id| id != batch.session_id);
        if restarted {
            self.entries.clear();
            self.cursor = 0;
            self.latest = 0;
            self.gap = false;
        }
        self.session_id = Some(batch.session_id);
        self.summary = batch.system_summary;
        self.synced_level = Some(batch.level);
        self.entries.retain(|entry| entry.visible_at(self.level));
        if restarted {
            return true;
        }
        if self.cursor > 0 && batch.first_available_sequence > self.cursor.saturating_add(1) {
            self.gap = true;
        }
        self.cursor = batch.next_sequence;
        self.latest = batch.latest_sequence;
        for entry in batch
            .entries
            .into_iter()
            .filter(|entry| entry.visible_at(self.level))
        {
            if self
                .entries
                .back()
                .is_none_or(|prior| prior.sequence < entry.sequence)
            {
                self.entries.push_back(entry);
            }
        }
        while self.entries.len() > LOG_CAPACITY {
            self.entries.pop_front();
        }
        self.error = None;
        false
    }
    fn export_entries(&self) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.visible_at(self.level))
            .collect()
    }
}
impl Workbench {
    pub(crate) fn fetch_logs(&mut self, level: Option<LogLevel>) -> Task<Message> {
        if !self.connected
            || self.logs.in_flight
            || self.cpu_enable_in_flight
            || self.logs.paused && level.is_none()
        {
            return Task::none();
        }
        self.logs.in_flight = true;
        let generation = self.logs.generation;
        let mode = self.mode;
        let after = self.logs.cursor;
        let level = level.or_else(|| {
            (self.logs.synced_level != Some(self.logs.level)).then_some(self.logs.level)
        });
        Task::perform(
            async move {
                lightdriver_ipc::logs(mode, after, level)
                    .await
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Logs(generation, result),
        )
    }
    pub(crate) fn log_update(&mut self, message: &Message) -> Option<Task<Message>> {
        match message {
            Message::LogLevel(level) => {
                self.logs.level = *level;
                self.logs.entries.retain(|entry| entry.visible_at(*level));
                if self.logs.in_flight {
                    self.logs.pending_level = Some(*level);
                    return Some(Task::none());
                }
                return Some(self.fetch_logs(Some(*level)));
            }
            Message::Logs(generation, result) => {
                if *generation != self.logs.generation {
                    return Some(Task::none());
                }
                self.logs.in_flight = false;
                if let Some(level) = self.logs.pending_level.take() {
                    return Some(self.fetch_logs(Some(level)));
                }
                match result {
                    Ok(batch) if batch.mode == self.mode => {
                        if self.logs.paused {
                            self.logs.summary = batch.system_summary.clone();
                            self.logs.latest = batch.latest_sequence;
                            self.logs.synced_level = Some(batch.level);
                            return Some(Task::none());
                        }
                        if self.logs.accept(batch.clone()) {
                            return Some(self.fetch_logs(None));
                        }
                        if self.logs.follow && self.tab == Tab::Logs {
                            return Some(iced::widget::operation::snap_to(
                                "live-log-stream",
                                iced::widget::scrollable::RelativeOffset { x: 0.0, y: 1.0 },
                            ));
                        }
                    }
                    Ok(_) => self.logs.error = Some("Log mode mismatch.".into()),
                    Err(error) => self.logs.error = Some(error.clone()),
                }
            }
            Message::LogPause => {
                self.logs.paused = !self.logs.paused;
                if !self.logs.paused {
                    return Some(self.fetch_logs(None));
                }
            }
            Message::LogFollow(value) => self.logs.follow = *value,
            Message::LogSummary(value) => self.logs.show_summary = *value,
            Message::LogFilter(value) => self.logs.filter = value.clone(),
            Message::LogClear => {
                if self.logs.in_flight {
                    return Some(Task::none());
                }
                self.logs.entries.clear();
                self.logs.cursor = self.logs.latest;
                self.logs.gap = false;
            }
            Message::LogExport => {
                #[derive(serde::Serialize)]
                struct Export<'a> {
                    application_version: &'a str,
                    exported_at_ms: u64,
                    mode: Mode,
                    verbosity: LogLevel,
                    capture_verbosity: Option<LogLevel>,
                    service_session: Option<u64>,
                    renderer: &'a str,
                    graphics_adapter: &'a str,
                    system_summary: &'a [String],
                    wiring: &'a lightdriver_core::RigConfiguration,
                    entries: Vec<&'a LogEntry>,
                }
                let export = Export {
                    application_version: env!("CARGO_PKG_VERSION"),
                    exported_at_ms: now_ms(),
                    mode: self.mode,
                    verbosity: self.logs.level,
                    capture_verbosity: self.logs.synced_level,
                    service_session: self.logs.session_id,
                    renderer: &self.renderer,
                    graphics_adapter: &self.adapter,
                    system_summary: &self.logs.summary,
                    wiring: &self.rig,
                    entries: self.logs.export_entries(),
                };
                let path = self
                    .data_directory
                    .join(format!("logs-{}.json", export.exported_at_ms));
                let result = (|| -> Result<(), String> {
                    let bytes = serde_json::to_vec_pretty(&export).map_err(|e| e.to_string())?;
                    std::fs::create_dir_all(&self.data_directory).map_err(|e| e.to_string())?;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|e| e.to_string())?;
                    file.write_all(&bytes).map_err(|e| e.to_string())?;
                    file.sync_all().map_err(|e| e.to_string())
                })();
                self.status = match result {
                    Ok(()) => format!("Log exported: {}", path.display()),
                    Err(e) => format!("Log export failed: {e}"),
                };
            }
            _ => return None,
        }
        Some(Task::none())
    }
    pub(crate) fn log_view(&self) -> Element<'_, Message> {
        let connected = self.connected && !self.cpu_enable_in_flight;
        let verbosity: Element<'_, Message> = if connected {
            pick_list(LogLevel::ALL, Some(self.logs.level), Message::LogLevel)
                .width(135)
                .into()
        } else {
            container(text(self.logs.level.label()).size(14))
                .padding(8)
                .width(135)
                .into()
        };
        let toolbar = row![
            label("Verbosity"),
            verbosity,
            Space::new().width(Fill),
            button(if self.logs.paused {
                "Resume view"
            } else {
                "Pause view"
            })
            .on_press(Message::LogPause)
            .style(style::secondary),
            button("Clear view")
                .on_press_maybe((!self.logs.in_flight).then_some(Message::LogClear))
                .style(style::secondary),
            button("Export log")
                .on_press(Message::LogExport)
                .style(style::secondary),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut content = column![heading("Live logs", 24),
            label("Connection events, commands and sensor status · raw incoming packets only in Verbose"),
            toolbar,
            row![
                text_input("Filter by source or message…", &self.logs.filter).on_input(Message::LogFilter).width(Fill),
                checkbox(self.logs.follow).label("Follow").on_toggle(Message::LogFollow),
            ].spacing(16).align_y(Alignment::Center),
            checkbox(self.logs.show_summary).label("Show system configuration").on_toggle(Message::LogSummary),
        ].spacing(16);
        if self.logs.show_summary {
            let mut summary = Column::new().spacing(7).push(label(format!(
                "Renderer: {} · {}",
                self.renderer, self.adapter
            )));
            for line in &self.logs.summary {
                summary = summary.push(label(line));
            }
            for route in &self.rig.lighting_routes {
                summary = summary.push(label(format!(
                    "Wiring: {} → {} · {}",
                    route.name, route.channel, route.provider
                )));
            }
            content = content.push(
                container(summary)
                    .padding(16)
                    .width(Fill)
                    .style(style::box_style(style::PANEL, true, 8.0)),
            );
        }
        let state = if !connected {
            "Service offline · retained log is historical"
        } else if self.logs.paused {
            "View paused · service buffer continues collecting"
        } else if self.logs.synced_level != Some(self.logs.level) {
            "Verbosity awaiting service confirmation · raw capture state unconfirmed"
        } else if self.logs.level == LogLevel::Verbose {
            "Live · raw packet capture enabled · exported logs include packet bytes"
        } else {
            "Live · raw packet capture disabled"
        };
        content = content.push(
            text(state)
                .size(12)
                .color(if connected && !self.logs.paused {
                    style::ACCENT
                } else {
                    style::AMBER
                }),
        );
        if let Some(error) = &self.logs.error {
            content = content.push(text(error).size(12).color(style::AMBER));
        }
        if self.logs.gap {
            content = content.push(label(
                "Some older entries expired from the bounded service buffer.",
            ));
        }
        let needle = self.logs.filter.to_lowercase();
        let matching: Vec<_> = self
            .logs
            .entries
            .iter()
            .filter(|entry| {
                entry.visible_at(self.logs.level)
                    && (needle.is_empty()
                        || entry.source.to_lowercase().contains(&needle)
                        || entry.message.to_lowercase().contains(&needle))
            })
            .collect();
        let skip = matching.len().saturating_sub(VIEW_LIMIT);
        let mut stream = Column::new().spacing(12).width(Fill);
        for entry in matching.iter().skip(skip) {
            let millis = entry.at_ms % 1000;
            let seconds = entry.at_ms / 1000;
            let stamp = format!(
                "{:02}:{:02}:{:02}.{millis:03} UTC",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            );
            stream = stream.push(
                column![
                    text(format!(
                        "{}  {}  {}{}",
                        stamp,
                        entry.level.label(),
                        entry.source,
                        if entry.raw { "  RX RAW" } else { "" }
                    ))
                    .size(11)
                    .font(Font::MONOSPACE)
                    .width(Fill)
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .color(match entry.level {
                        LogLevel::Error => iced::Color::from_rgb8(235, 123, 134),
                        LogLevel::Warning => style::AMBER,
                        LogLevel::Verbose => style::ACCENT,
                        _ => style::MUTED,
                    }),
                    text(&entry.message)
                        .size(12)
                        .font(Font::MONOSPACE)
                        .shaping(iced::widget::text::Shaping::Advanced)
                        .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                        .width(Fill),
                ]
                .spacing(4)
                .width(Fill),
            );
        }
        if matching.is_empty() {
            stream = stream.push(label(if self.logs.entries.is_empty() {
                "Waiting for events at this verbosity…"
            } else {
                "No entries match this filter."
            }));
        }
        content.push(container(scrollable(stream).id("live-log-stream").height(420).width(Fill))
            .padding(16).width(Fill).style(style::box_style(style::BG, true, 8.0)))
            .push(label(format!("{} shown · {} retained for export · {} entry service buffer · memory only until Export log",
                matching.len().min(VIEW_LIMIT), self.logs.export_entries().len(), LOG_CAPACITY))).into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::LogStore;
    #[test]
    fn exported_report_contains_context_and_only_selected_verbosity() {
        let store = LogStore::default();
        store.read(Mode::Demo, 0, Some(LogLevel::Verbose));
        store.event(LogLevel::Info, "configuration", "Demo processor");
        store.incoming_hid("demo", &[0x75, 1]);
        let mut batch = store.read(Mode::Demo, 0, None);
        batch.system_summary = vec!["OS: Demo".into(), "CPU: simulated".into()];
        let (mut state, _) = Workbench::new();
        state.mode = Mode::Demo;
        state.logs = LiveLogs::new(LogLevel::Verbose);
        state.logs.accept(batch);
        state.logs.level = LogLevel::Info;
        state.data_directory = std::env::temp_dir().join(format!(
            "lightdriver-log-export-{}-{}",
            std::process::id(),
            now_ms()
        ));
        assert!(!state.data_directory.exists());
        state.log_update(&Message::LogExport);
        let files: Vec<_> = std::fs::read_dir(&state.data_directory).unwrap().collect();
        assert_eq!(files.len(), 1);
        let path = files.into_iter().next().unwrap().unwrap().path();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(report["mode"], "Demo");
        assert_eq!(report["verbosity"], "Info");
        assert_eq!(report["system_summary"][0], "OS: Demo");
        assert_eq!(report["entries"].as_array().unwrap().len(), 1);
        assert_eq!(report["entries"][0]["raw"], false);
        assert!(report.get("renderer").is_some());
        assert!(report.get("wiring").is_some());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(&state.data_directory).unwrap();
    }
    #[test]
    fn queued_level_change_and_old_mode_response_cannot_restore_raw() {
        let store = LogStore::default();
        store.read(Mode::Demo, 0, Some(LogLevel::Verbose));
        store.incoming_hid("demo", &[0x75, 1]);
        let old_batch = store.read(Mode::Demo, 0, None);
        let (mut state, _) = Workbench::new();
        state.logs = LiveLogs::new(LogLevel::Verbose);
        state.logs.accept(old_batch.clone());
        state.logs.in_flight = true;
        state.connected = true;
        state.mode = Mode::Demo;
        state.logs.generation = 10;
        state.log_update(&Message::LogLevel(LogLevel::Info));
        assert!(state.logs.export_entries().is_empty());
        state.log_update(&Message::Logs(9, Ok(old_batch.clone())));
        assert!(state.logs.in_flight);
        state.log_update(&Message::Logs(10, Ok(old_batch)));
        assert_eq!(state.logs.level, LogLevel::Info);
        assert!(state.logs.in_flight);
        assert!(state.logs.export_entries().is_empty());
        let batch = store.read(Mode::Demo, 0, Some(LogLevel::Info));
        state.log_update(&Message::Logs(10, Ok(batch)));
        assert!(!state.logs.in_flight);
        assert_eq!(state.logs.synced_level, Some(LogLevel::Info));
        assert!(state.logs.export_entries().is_empty());
    }
    #[test]
    fn lowering_verbosity_clears_raw_from_view_and_export_and_restarts_reset_cursor() {
        let store = LogStore::default();
        store.read(Mode::Demo, 0, Some(LogLevel::Verbose));
        store.incoming_hid("demo", &[0x75, 1]);
        store.event(LogLevel::Info, "config", "processor");
        let mut state = LiveLogs::new(LogLevel::Verbose);
        assert!(!state.accept(store.read(Mode::Demo, 0, None)));
        assert_eq!(state.export_entries().len(), 2);
        state.level = LogLevel::Info;
        state.accept(store.read(Mode::Demo, state.cursor, Some(LogLevel::Info)));
        assert_eq!(state.export_entries().len(), 1);
        assert!(!state.entries.iter().any(|entry| entry.raw));
        let mut restarted = store.read(Mode::Demo, 0, None);
        restarted.session_id += 1;
        assert!(state.accept(restarted));
        assert_eq!(state.cursor, 0);
        assert!(state.entries.is_empty());
    }
}
