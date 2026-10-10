use crate::{
    Message, Workbench, charts,
    style::{self, heading, label, selector},
};
use iced::{
    Alignment, Element,
    Length::{Fill, FillPortion},
    widget::{
        Column, Row, Space, button, canvas, column, container, progress_bar, responsive, row, rule,
        text,
    },
};
use lightdriver_core::{PhysicalCore, Sensor, SensorStatus, Unit, now_ms};
impl Workbench {
    pub(crate) fn diagnostic_value(&self, sensor: &Sensor) -> Option<f64> {
        self.connected
            .then(|| sensor.fresh_value_at(now_ms()))
            .flatten()
    }
    pub(crate) fn diagnostics(&self) -> Element<'_, Message> {
        let mut content = column![
            row![
                column![
                    heading("System diagnostics", 24),
                    label("CPU & GPU · Live load, temperature and clocks")
                ]
                .spacing(6),
                Space::new().width(Fill)
            ]
            .align_y(Alignment::Center),
            label(if !self.connected {
                "Service offline · Previous readings are hidden."
            } else if self.mode == lightdriver_core::Mode::Demo {
                "Demo mode · All readings are simulated."
            } else {
                "Read-only monitoring · Updated once per second"
            }),
        ]
        .spacing(18);
        if let Some(error) = &self.service_error {
            content = content.push(text(error).size(12).color(style::AMBER));
        }
        let sensors = &self.snapshot.diagnostics.sensors;
        let cpu = sensors
            .iter()
            .find(|s| s.id == "cpu.load")
            .map(|s| s.group.as_str())
            .unwrap_or("CPU");
        let mut groups = vec![cpu];
        for sensor in sensors
            .iter()
            .filter(|s| s.unit == Unit::Percent && s.label.contains("GPU load"))
        {
            if !groups.contains(&sensor.group.as_str()) {
                groups.push(&sensor.group);
            }
        }
        if groups.len() == 1 {
            groups.push("GPU");
        }
        let mut cards = Column::new().spacing(14);
        for pair in groups.chunks(2) {
            let mut r = Row::new().spacing(14);
            for group in pair {
                r = r.push(self.processor_card(group, *group == cpu));
            }
            if pair.len() == 1 {
                r = r.push(Space::new().width(FillPortion(1)));
            }
            cards = cards.push(r);
        }
        content = content.push(cards).push(
            row![
                column![
                    heading("CPU cores", 18),
                    label("Physical cores · Separate load bars for each SMT thread")
                ]
                .spacing(5),
                Space::new().width(Fill),
                selector(
                    text(if self.show_cpu_cores {
                        "Hide cores"
                    } else {
                        "Show cores"
                    })
                    .size(12),
                    Message::CoreDetails(!self.show_cpu_cores),
                    self.show_cpu_cores
                )
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        if self.show_cpu_cores {
            content = content.push(self.core_diagram());
        }
        content
            .push(rule::horizontal(1))
            .push(
                row![
                    button("Copy diagnostics")
                        .on_press(Message::CopyReport)
                        .style(style::secondary)
                        .padding([9, 14]),
                    button("Export report")
                        .on_press(Message::ExportReport)
                        .style(style::secondary)
                        .padding([9, 14])
                ]
                .spacing(10),
            )
            .into()
    }
    fn processor_card<'a>(&'a self, group: &str, cpu: bool) -> Element<'a, Message> {
        let sensors = &self.snapshot.diagnostics.sensors;
        let load = sensors.iter().find(|s| {
            s.group == group
                && s.unit == Unit::Percent
                && s.label.contains("load")
                && (cpu && s.id == "cpu.load" || !cpu)
        });
        let temperature = sensors
            .iter()
            .find(|s| s.group == group && s.unit == Unit::Celsius && s.basic);
        let clock = sensors.iter().find(|s| {
            s.group == group
                && s.unit == Unit::Mhz
                && !s.label.to_ascii_lowercase().contains("memory")
        });
        let value = |s: Option<&Sensor>, unit: Unit| {
            s.and_then(|s| self.diagnostic_value(s))
                .map(|v| formatted(v, unit))
                .unwrap_or_else(|| s.map_or("Unavailable", |s| availability(self, s)).into())
        };
        let mut card = column![
            label(if cpu { "PROCESSOR" } else { "GRAPHICS" }),
            heading(
                group
                    .trim_start_matches("CPU · ")
                    .trim_start_matches("GPU · "),
                16
            ),
            row![
                column![
                    canvas(charts::RadialLoad {
                        value: load
                            .and_then(|s| self.diagnostic_value(s))
                            .map(|v| v as f32)
                    })
                    .width(108)
                    .height(108),
                    label("Load")
                ]
                .align_x(Alignment::Center)
                .spacing(2),
                column![
                    column![
                        label("Temperature"),
                        heading(value(temperature, Unit::Celsius), 20)
                    ]
                    .spacing(4),
                    column![
                        label(clock.map_or("Clock speed", |s| s.label.as_str())),
                        heading(value(clock, Unit::Mhz), 20)
                    ]
                    .spacing(4)
                ]
                .spacing(17)
                .width(Fill)
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        ]
        .spacing(12);
        if cpu && temperature.is_some_and(|s| self.diagnostic_value(s).is_none()) {
            card = card.push(label(temperature.unwrap().detail.clone()));
        }
        if cpu
            && self.mode == lightdriver_core::Mode::Live
            && self.snapshot.diagnostics.cpu_elevation_needed
        {
            card = card.push(
                button(if self.cpu_enable_in_flight {
                    "Waiting for Windows…"
                } else {
                    "Enable CPU sensors"
                })
                .on_press_maybe(
                    (self.connected && !self.cpu_enable_in_flight)
                        .then_some(Message::EnableCpuSensors),
                )
                .style(style::secondary)
                .padding([8, 12]),
            );
        }
        if !cpu && load.is_none() {
            card = card.push(label("No supported GPU sensor provider is available."));
        }
        container(card)
            .padding(20)
            .width(FillPortion(1))
            .style(style::box_style(style::PANEL, false, 10.0))
            .into()
    }
    fn core_diagram(&self) -> Element<'_, Message> {
        let topology = &self.snapshot.diagnostics.cpu_topology;
        if topology.cores.is_empty() {
            return label(if topology.detail.is_empty() {
                "Waiting for CPU topology."
            } else {
                &topology.detail
            })
            .into();
        }
        let current = self.connected && topology.is_fresh_at(now_ms());
        let hybrid = topology
            .cores
            .iter()
            .any(|c| c.efficiency_class != topology.cores[0].efficiency_class);
        let mut groups = vec![];
        for core in &topology.cores {
            let key = (core.package, core.cluster);
            if !groups.contains(&key) {
                groups.push(key);
            }
        }
        let mut content = column![label(format!(
            "{} physical cores · {} threads · {} shared L3 groups",
            topology.total_cores,
            topology.total_threads,
            topology.clusters.len()
        ))]
        .spacing(12);
        for (package, cluster) in groups {
            let selected: Vec<_> = topology
                .cores
                .iter()
                .filter(|c| c.package == package && c.cluster == cluster)
                .collect();
            let socket = package
                .map(|v| format!("Socket {}", v + 1))
                .unwrap_or_else(|| "Socket unknown".into());
            let name = cluster
                .map(|v| format!("{socket} · Shared L3 {}", v + 1))
                .unwrap_or_else(|| format!("{socket} · Cache grouping unavailable"));
            let cache = topology
                .clusters
                .iter()
                .find(|c| Some(c.id) == cluster)
                .map(|c| {
                    format!(
                        "{} MiB L3 · {} cores",
                        c.l3_bytes / 1_048_576,
                        selected.len()
                    )
                })
                .unwrap_or_else(|| format!("{} cores", selected.len()));
            let tiles = responsive(move |size| {
                let count = ((size.width + 10.0) / 134.0).floor().max(1.0) as usize;
                let mut grid = Column::new().spacing(10).width(Fill);
                for cores in selected.chunks(count) {
                    let mut r = Row::new().spacing(10);
                    for core in cores {
                        r = r.push(core_tile(core, current, hybrid));
                    }
                    for _ in cores.len()..count {
                        r = r.push(Space::new().width(FillPortion(1)));
                    }
                    grid = grid.push(r);
                }
                grid.into()
            });
            content = content.push(
                container(
                    column![
                        row![heading(name, 14), Space::new().width(Fill), label(cache)]
                            .spacing(12)
                            .align_y(Alignment::Center),
                        tiles
                    ]
                    .spacing(14),
                )
                .padding(16)
                .width(Fill)
                .style(style::box_style(style::SIDE, true, 10.0)),
            );
        }
        content=content.push(label("Shared L3 is used for CCX-like grouping; cache groups are not assumed to be vendor CCX IDs."));
        if !topology.detail.is_empty() {
            content = content.push(label(&topology.detail));
        }
        content.into()
    }
}
fn core_tile(core: &PhysicalCore, current: bool, hybrid: bool) -> Element<'_, Message> {
    let load = current.then(|| core.load()).flatten();
    let mut tile = column![
        row![
            heading(format!("Core {:02}", core.id + 1), 12),
            Space::new().width(Fill),
            text(
                load.map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "—".into())
            )
            .size(12)
            .color(style::ACCENT)
        ]
        .spacing(5),
        text(
            if current {
                core.temperature
                    .filter(|v| Unit::Celsius.accepts(f64::from(*v)))
                    .map(|v| format!("{v:.1} °C"))
            } else {
                None
            }
            .unwrap_or_else(|| "— °C".into())
        )
        .size(17),
        label(
            if current {
                core.clock_mhz
                    .filter(|v| Unit::Mhz.accepts(f64::from(*v)))
                    .map(|v| format!("{v:.0} MHz"))
            } else {
                None
            }
            .unwrap_or_else(|| "— MHz".into())
        )
    ]
    .spacing(7);
    for (i, thread) in core.threads.iter().enumerate() {
        let load = current
            .then_some(thread.load)
            .flatten()
            .filter(|v| Unit::Percent.accepts(f64::from(*v)));
        tile = tile.push(
            column![
                row![
                    text(format!("T{}", i + 1)).size(9).color(style::MUTED),
                    Space::new().width(Fill),
                    text(
                        load.map(|v| format!("{v:.0}%"))
                            .unwrap_or_else(|| "—".into())
                    )
                    .size(9)
                    .color(style::MUTED)
                ],
                progress_bar(0.0..=100.0, load.unwrap_or(0.0))
                    .girth(4)
                    .style(move |_| iced::widget::progress_bar::Style {
                        background: style::LINE.into(),
                        bar: if load.is_some() {
                            style::ACCENT
                        } else {
                            style::LINE
                        }
                        .into(),
                        border: iced::Border::default()
                    })
            ]
            .spacing(3),
        );
    }
    if hybrid {
        tile = tile.push(
            text(format!("Efficiency class {}", core.efficiency_class))
                .size(9)
                .color(style::MUTED),
        );
    }
    container(tile)
        .padding(12)
        .width(FillPortion(1))
        .style(style::box_style(style::PANEL, false, 7.0))
        .into()
}
fn availability<'a>(state: &Workbench, sensor: &'a Sensor) -> &'a str {
    if !state.connected {
        return "Offline";
    }
    match sensor.status {
        SensorStatus::Available => "Stale",
        SensorStatus::Unavailable => "Unavailable",
        SensorStatus::Error => "Read error",
    }
}
fn formatted(value: f64, unit: Unit) -> String {
    if unit == Unit::Mhz {
        format!("{value:.0} MHz")
    } else {
        format!("{value:.1} {}", unit.label())
    }
}
