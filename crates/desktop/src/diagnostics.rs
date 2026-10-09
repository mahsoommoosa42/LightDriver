use crate::{
    Message, Workbench, charts,
    style::{self, badge, heading, label, selector},
};
use iced::{
    Alignment, Element,
    Length::{Fill, FillPortion},
    widget::{Column, Space, button, canvas, column, container, row, rule, text},
};
use lightdriver_core::{Sensor, SensorStatus, Unit, now_ms};

impl Workbench {
    pub(crate) fn diagnostic_value(&self, sensor: &Sensor) -> Option<f64> {
        self.connected
            .then(|| sensor.fresh_value_at(now_ms()))
            .flatten()
    }
    pub(crate) fn diagnostics(&self) -> Element<'_, Message> {
        let mut content = column![
            row![column![heading("System diagnostics", 24), label("Temperatures, power and cooling · Read-only monitoring")].spacing(6),
                Space::new().width(Fill),
                selector(text("Basic").size(12), Message::Advanced(false), !self.diagnostics_advanced),
                selector(text("Advanced").size(12), Message::Advanced(true), self.diagnostics_advanced)]
                .spacing(8).align_y(Alignment::Center),
            label(if self.connected {
                if self.mode == lightdriver_core::Mode::Demo { "Demo mode · All readings are simulated." }
                else { "Live sensors · Missing readings show Unavailable. Measurements are never substituted." }
            } else { "Service offline · Previous readings are hidden until the service reconnects." }),
        ].spacing(18);
        if let Some(error) = &self.service_error {
            content = content.push(text(error).size(12).color(style::AMBER));
        }
        if self.diagnostics_advanced {
            content = content.push(self.advanced_diagnostics());
        } else {
            content = content.push(self.basic_diagnostics());
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
            .push(label(
                "Power is labeled by scope. Whole-PC wall power requires a meter or supported PSU.",
            ))
            .into()
    }
    fn basic_diagnostics(&self) -> Element<'_, Message> {
        let sensors = &self.snapshot.diagnostics.sensors;
        let mut groups: Vec<&str> = vec![];
        for s in sensors
            .iter()
            .filter(|s| s.basic && s.id != "system.wall-power")
        {
            if !groups.contains(&s.group.as_str()) {
                groups.push(&s.group);
            }
        }
        let mut cards: Vec<Element<'_, Message>> = vec![];

        for group in groups {
            let load = sensors.iter().find(|s| {
                s.basic && s.group == group && s.unit == Unit::Percent && s.label.contains("load")
            });
            let memory = sensors
                .iter()
                .find(|s| s.group == group && s.id == "memory.used");
            let total = sensors
                .iter()
                .find(|s| s.group == group && s.id == "memory.total");
            let percentage = load.and_then(|s| self.diagnostic_value(s)).or_else(|| {
                let used = self.diagnostic_value(memory?)?;
                let total = self.diagnostic_value(total?)?;
                (total > 0.0 && used <= total).then_some(used / total * 100.0)
            });
            let gauge = load.is_some() || memory.is_some();
            let mut readings = Column::new().spacing(14).width(Fill);
            for s in sensors.iter().filter(|s| {
                s.group == group
                    && (s.basic || s.id == "memory.total")
                    && load.is_none_or(|l| l.id != s.id)
            }) {
                let value = self.diagnostic_value(s);
                readings = readings.push(
                    column![
                        label(&s.label),
                        text(
                            value
                                .map(|v| formatted(v, s.unit))
                                .unwrap_or_else(|| availability(self, s).into())
                        )
                        .size(18)
                        .color(if value.is_some() {
                            style::FG
                        } else {
                            style::MUTED
                        }),
                    ]
                    .spacing(5),
                );
            }
            let body: Element<'_, Message> = if gauge {
                row![
                    column![
                        canvas(charts::RadialLoad {
                            value: percentage.map(|v| v as f32)
                        })
                        .width(104)
                        .height(104),
                        label(load.map_or("Memory load", |s| s.label.as_str())),
                        label(if percentage.is_some() {
                            "of 100%"
                        } else {
                            "Unavailable"
                        }),
                    ]
                    .spacing(3)
                    .align_x(Alignment::Center)
                    .width(118),
                    readings,
                ]
                .spacing(16)
                .align_y(Alignment::Center)
                .into()
            } else {
                readings.into()
            };
            cards.push(
                container(
                    column![heading(group.trim(), 16), rule::horizontal(1), body].spacing(15),
                )
                .padding(20)
                .width(FillPortion(1))
                .style(style::box_style(style::PANEL, false, 9.0))
                .into(),
            );
        }
        let mut cooling = column![heading("Cooling", 16), rule::horizontal(1)].spacing(13);
        for d in self
            .snapshot
            .devices
            .iter()
            .filter(|d| d.id != "motherboard")
        {
            let t = d.telemetry.as_ref().filter(|t| {
                self.connected
                    && t.is_fresh_at(now_ms())
                    && matches!(
                        d.status,
                        lightdriver_core::DeviceStatus::Reading
                            | lightdriver_core::DeviceStatus::Demo
                    )
            });
            cooling = cooling.push(label(&d.name));
            for (name, value) in [
                ("Coolant", t.map(|t| format!("{:.1} °C", t.coolant_c))),
                ("Pump", t.map(|t| format!("{} RPM", t.pump_rpm))),
            ] {
                cooling = cooling.push(
                    row![
                        label(name).width(Fill),
                        text(value.unwrap_or_else(|| "Unavailable".into())).size(18)
                    ]
                    .spacing(12),
                );
            }
        }
        cooling = cooling.push(label("Radiator / case fan RPM · Adapter needed"));
        cards.push(
            container(cooling)
                .padding(20)
                .width(FillPortion(1))
                .style(style::box_style(style::PANEL, false, 9.0))
                .into(),
        );
        let mut grid = Column::new().spacing(14);
        let mut cards = cards.into_iter();
        while let Some(first) = cards.next() {
            let mut pair = row![first].spacing(14);
            if let Some(second) = cards.next() {
                pair = pair.push(second);
            } else {
                pair = pair.push(Space::new().width(FillPortion(1)));
            }
            grid = grid.push(pair);
        }
        if sensors.is_empty() {
            grid = grid.push(label("Waiting for the service's system sensor inventory."));
        }
        grid.into()
    }
    fn advanced_diagnostics(&self) -> Element<'_, Message> {
        let sensors = &self.snapshot.diagnostics.sensors;
        let selected = sensors
            .iter()
            .find(|s| s.id == self.selected_sensor)
            .or_else(|| sensors.iter().find(|s| s.sample.is_some()));
        let mut content = Column::new().spacing(16);
        if let Some(s) = selected {
            let current = self.diagnostic_value(s).is_some();
            let range = if current {
                s.range().map(|(a, b)| {
                    format!(
                        "Min {} · Max {}",
                        formatted(a, s.unit),
                        formatted(b, s.unit)
                    )
                })
            } else {
                None
            };
            content = content.push(
                container(column![
                    row![column![heading(&s.label,18), label(&s.group)].spacing(4), Space::new().width(Fill),
                        label(range.unwrap_or_else(|| "No current history".into()))].spacing(12).align_y(Alignment::Center),
                    canvas(charts::SensorHistory { samples: if current { s.history.clone() } else { vec![] }, unit: s.unit })
                        .width(Fill).height(130),
                    label("Rolling history · Last 30 samples; min/max cover this window, not the full session."),
                ].spacing(12)).padding(18).width(Fill).style(style::box_style(style::PANEL,false,9.0))
            );
        }
        content = content
            .push(
                row![
                    label("SENSOR / SOURCE").width(Fill),
                    label("VALUE").width(140),
                    label("FRESHNESS").width(90)
                ]
                .spacing(14),
            )
            .push(rule::horizontal(1));
        for s in sensors {
            let value = self.diagnostic_value(s);
            let age = if self.connected {
                s.sample
                    .as_ref()
                    .map(|v| {
                        if v.at_ms > now_ms() {
                            "Future".into()
                        } else {
                            format!("{:.1}s", now_ms().saturating_sub(v.at_ms) as f64 / 1000.0)
                        }
                    })
                    .unwrap_or_else(|| "—".into())
            } else {
                "Offline".into()
            };
            content = content.push(
                column![
                    selector(
                        row![
                            column![
                                text(&s.label).size(13),
                                label(format!("{} · {}", s.group, s.source))
                            ]
                            .spacing(4)
                            .width(Fill),
                            text(
                                value
                                    .map(|v| formatted(v, s.unit))
                                    .unwrap_or_else(|| availability(self, s).into())
                            )
                            .size(13)
                            .width(140),
                            label(age).width(90)
                        ]
                        .spacing(14)
                        .align_y(Alignment::Center),
                        Message::Sensor(s.id.clone()),
                        self.selected_sensor == s.id
                    )
                    .width(Fill),
                    label(&s.detail),
                    rule::horizontal(1)
                ]
                .spacing(6),
            );
        }
        content = content.push(heading("Collectors", 18));
        for p in &self.snapshot.diagnostics.providers {
            content = content.push(
                column![
                    row![
                        heading(&p.name, 14),
                        Space::new().width(Fill),
                        badge("READ ONLY", style::MUTED)
                    ]
                    .spacing(10),
                    label(if self.connected {
                        &p.detail
                    } else {
                        "Service offline; this inventory is historical"
                    })
                ]
                .spacing(5),
            );
        }
        content.push(label("Adapter implementation does not establish coverage of every model or driver. NVIDIA and Intel hardware validation is pending.")).into()
    }
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
    match unit {
        Unit::Rpm | Unit::Mhz => format!("{value:.0} {}", unit.label()),
        _ => format!("{value:.1} {}", unit.label()),
    }
}
