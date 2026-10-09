use crate::{
    Message, Tab, Workbench, charts,
    style::{self, badge, heading, label, selector},
};
use iced::{
    Alignment, Element,
    Length::{Fill, FillPortion},
    widget::{
        Column, Space, button, canvas, column, container, row, rule, scrollable, slider, text,
    },
};
use lightdriver_core::{DeviceStatus, Mode, Preset, Support};

impl Workbench {
    pub fn layout(&self) -> Element<'_, Message> {
        let brand = row![
            text("◌").size(27).color(style::ACCENT),
            heading("LightDriver", 18),
            label("WORKBENCH"),
            Space::new().width(Fill),
            selector(
                text("Live").size(12),
                Message::Mode(Mode::Live),
                self.mode == Mode::Live
            )
            .on_press_maybe((!self.poll_in_flight).then_some(Message::Mode(Mode::Live))),
            selector(
                text("Demo").size(12),
                Message::Mode(Mode::Demo),
                self.mode == Mode::Demo
            )
            .on_press_maybe((!self.poll_in_flight).then_some(Message::Mode(Mode::Demo))),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let title = row![
            column![
                label("DEVICE WORKSPACE"),
                heading("Understand. Control. Verify.", 26)
            ]
            .spacing(6),
            Space::new().width(Fill),
            badge(
                if self.mode == Mode::Demo {
                    "DEMO DATA"
                } else {
                    "READ ONLY"
                },
                if self.mode == Mode::Demo {
                    style::AMBER
                } else {
                    style::ACCENT
                }
            ),
        ]
        .align_y(Alignment::Center);
        let footer = row![
            label(format!(
                "v{} · Profiles / Devices / Diagnostics",
                env!("CARGO_PKG_VERSION")
            )),
            label(&self.status)
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Right)
        ]
        .spacing(20)
        .align_y(Alignment::Center);
        column![
            container(brand)
                .padding([10, 25])
                .width(Fill)
                .style(style::box_style(style::PANEL, false, 0.0)),
            rule::horizontal(1),
            container(title).padding([16, 30]).width(Fill),
            rule::horizontal(1),
            row![
                self.sidebar(),
                scrollable(
                    container(self.device_workspace())
                        .padding([20, 28])
                        .width(Fill)
                )
                .width(Fill)
            ]
            .height(Fill),
            rule::horizontal(1),
            container(footer)
                .padding([13, 25])
                .width(Fill)
                .style(style::box_style(style::PANEL, false, 0.0)),
        ]
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut devices = Column::new().spacing(10).push(label("CONTROL PATHS"));
        for device in &self.snapshot.devices {
            let is_board = device.id == "motherboard";
            let selected = self.selected_device == device.id;
            let content = row![
                text(if is_board { "▧" } else { "◉" })
                    .size(18)
                    .color(if selected {
                        style::ACCENT
                    } else {
                        style::MUTED
                    }),
                column![
                    heading(
                        if is_board {
                            "X570 AORUS"
                        } else if self.mode == Mode::Demo {
                            "Kraken X53 · Demo"
                        } else {
                            "Kraken X3"
                        },
                        14
                    ),
                    label(if is_board {
                        "Motherboard · Adapter needed"
                    } else {
                        self.device_status(device)
                    }),
                ]
                .spacing(5),
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            devices = devices.push(
                selector(content, Message::SelectDevice(device.id.clone()), selected).width(Fill),
            );
            devices = devices.push(
                container(if is_board {
                    column![
                        label("CPU_FAN → Radiator"),
                        label("SYS_FAN1 → Case hub"),
                        label("5 V ARGB → Case lighting")
                    ]
                    .spacing(9)
                } else {
                    column![label("Pump"), label("Ring + logo")].spacing(9)
                })
                .padding([3, 28]),
            );
        }
        let service_status = if self.connected {
            "●  Service connected"
        } else if self.poll_in_flight {
            "○  Connecting to service"
        } else {
            "○  Service offline"
        };
        container(
            column![
                devices,
                Space::new().height(Fill),
                label("Support is verified per capability."),
                text(service_status).size(12).color(if self.connected {
                    style::ACCENT
                } else {
                    style::AMBER
                }),
                button(text("Reconnect service").size(12))
                    .on_press_maybe((!self.poll_in_flight).then_some(Message::Reconnect))
                    .padding([9, 12])
                    .style(style::secondary),
            ]
            .spacing(18),
        )
        .padding([24, 18])
        .width(250)
        .height(Fill)
        .style(style::box_style(style::SIDE, false, 0.0))
        .into()
    }

    fn device_workspace(&self) -> Element<'_, Message> {
        let device = self.selected();
        let mut content = column![
            row![
                column![heading(&device.name, 22), label(&device.connection)].spacing(6),
                Space::new().width(Fill),
                badge(
                    self.device_status(device),
                    if self.connected
                        && matches!(device.status, DeviceStatus::Reading | DeviceStatus::Demo)
                        && self.telemetry().is_some()
                    {
                        style::ACCENT
                    } else {
                        style::AMBER
                    }
                )
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(14);
        if let Some(error) = &self.service_error {
            content = content.push(
                container(text(error).size(12).color(style::AMBER))
                    .padding(14)
                    .width(Fill)
                    .style(style::box_style(style::BG, true, 8.0)),
            );
        }
        if device.id == "motherboard" {
            return content.push(self.motherboard()).into();
        }
        let telemetry = self.telemetry();
        let metric = |name: &str, value: String, unit: &str| -> Element<'_, Message> {
            column![
                label(name),
                row![heading(value, 31), label(unit)]
                    .spacing(6)
                    .align_y(Alignment::End)
            ]
            .spacing(5)
            .width(FillPortion(1))
            .into()
        };
        content = content.push(
            container(
                row![
                    metric(
                        "Coolant",
                        telemetry
                            .map(|t| format!("{:.1}", t.coolant_c))
                            .unwrap_or_else(|| "—".into()),
                        "°C"
                    ),
                    metric(
                        "Pump",
                        telemetry
                            .map(|t| format_number(t.pump_rpm))
                            .unwrap_or_else(|| "—".into()),
                        "RPM"
                    ),
                    metric(
                        "Duty",
                        telemetry
                            .map(|t| t.pump_duty.to_string())
                            .unwrap_or_else(|| "—".into()),
                        "%"
                    ),
                ]
                .spacing(25),
            )
            .padding([12, 22])
            .width(Fill)
            .style(style::box_style(style::PANEL, false, 9.0)),
        );
        content = content.push(label(if telemetry.is_some() {
            &device.detail
        } else if !self.connected {
            "Waiting for the local service. Unavailable readings are never replaced by demo data."
        } else {
            &device.detail
        }));
        let mut tabs = row![].spacing(7);
        for tab in Tab::ALL {
            tabs = tabs.push(selector(
                text(tab.label()).size(12),
                Message::Tab(tab),
                self.tab == tab,
            ));
        }
        content = content.push(tabs).push(rule::horizontal(1));
        content = content.push(match self.tab {
            Tab::Profile => self.profile(),
            Tab::Lighting => self.lighting(),
            Tab::Capabilities => self.capabilities(),
            Tab::Verification => self.verification(),
        });
        content.into()
    }

    fn profile(&self) -> Element<'_, Message> {
        let mut presets = row![].spacing(7);
        for preset in Preset::ALL {
            presets = presets.push(selector(
                text(preset.label()).size(12),
                Message::Preset(preset),
                self.preset == Some(preset),
            ));
        }
        let mut points = column![].spacing(18);
        for (group, chunk) in self.draft.curve.points.chunks(4).enumerate() {
            let mut controls = row![].spacing(24);
            for (offset, point) in chunk.iter().enumerate() {
                let i = group * 4 + offset;
                controls = controls.push(
                    column![
                        row![
                            label(format!("{} °C", point.coolant_c)),
                            Space::new().width(Fill),
                            text(format!("{}%", point.duty)).size(12)
                        ],
                        slider(20u8..=100u8, point.duty, move |v| Message::Duty(i, v)),
                    ]
                    .spacing(10)
                    .width(FillPortion(1)),
                );
            }
            points = points.push(controls);
        }
        let validation = self.draft.validate();
        let detail = validation.clone().err().unwrap_or_else(|| {
            "Local draft · This is not a readback of the device’s current curve.".into()
        });
        column![
            row![
                label("Coolant → Pump duty"),
                Space::new().width(Fill),
                badge("LOCAL DRAFT", style::MUTED)
            ],
            presets,
            canvas(charts::Curve {
                curve: self.draft.curve.clone()
            })
            .width(Fill)
            .height(175),
            points,
            text(detail).size(12).color(if validation.is_err() {
                style::AMBER
            } else {
                style::MUTED
            }),
            rule::horizontal(1),
            row![
                label("Hardware application awaits validation."),
                Space::new().width(Fill),
                button(text("Apply to device").size(12))
                    .padding([10, 15])
                    .style(style::primary),
                button(text("Save draft").size(12))
                    .on_press_maybe(validation.is_ok().then_some(Message::SaveDraft))
                    .padding([10, 15])
                    .style(style::primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(12)
        .into()
    }

    fn lighting(&self) -> Element<'_, Message> {
        let palette = [
            ("Sea glass", [83, 221, 202]),
            ("Periwinkle", [134, 165, 255]),
            ("Amber", [235, 196, 148]),
            ("Rose", [223, 143, 175]),
        ];
        let mut colors = column![].spacing(7);
        for pair in palette.chunks(2) {
            let mut swatches = row![].spacing(9);
            for &(name, rgb) in pair {
                let selected = self.draft.lighting_rgb == rgb;
                swatches = swatches.push(selector(
                    text(name)
                        .size(12)
                        .color(iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2])),
                    Message::Color(rgb),
                    selected,
                ));
            }
            colors = colors.push(swatches);
        }
        let editor = column![
            heading("Ring + logo", 19),
            label("Static preview · Kraken X53"),
            colors,
            row![
                label("Brightness"),
                Space::new().width(Fill),
                label(format!("{}%", self.draft.lighting_brightness))
            ],
            slider(
                0u8..=100u8,
                self.draft.lighting_brightness,
                Message::Brightness
            ),
            label("USB lighting writes are planned. Case ARGB needs a motherboard adapter."),
            button(text("Save draft").size(12))
                .padding([10, 15])
                .style(style::primary)
                .on_press_maybe(self.draft.validate().is_ok().then_some(Message::SaveDraft)),
        ]
        .spacing(19)
        .width(FillPortion(2));
        column![
            row![
                canvas(charts::Ring {
                    rgb: self.draft.lighting_rgb,
                    brightness: self.draft.lighting_brightness
                })
                .width(FillPortion(1))
                .height(250),
                editor
            ]
            .spacing(26)
            .align_y(Alignment::Center),
            rule::horizontal(1),
            label("Preview only · The existing hardware lighting is unchanged."),
        ]
        .spacing(22)
        .into()
    }

    fn capabilities(&self) -> Element<'_, Message> {
        let mut list = column![
            row![
                label("CAPABILITY"),
                Space::new().width(Fill),
                label("SUPPORT")
            ],
            rule::horizontal(1)
        ]
        .spacing(14);
        for c in &self.selected().capabilities {
            list = list.push(
                column![
                    row![
                        heading(&c.name, 14),
                        Space::new().width(Fill),
                        badge(
                            c.support.label(),
                            if c.support == Support::Implemented {
                                style::ACCENT
                            } else {
                                style::AMBER
                            }
                        )
                    ]
                    .align_y(Alignment::Center),
                    label(&c.detail),
                    rule::horizontal(1)
                ]
                .spacing(9),
            );
        }
        list.into()
    }

    fn verification(&self) -> Element<'_, Message> {
        let rows = [
            ("Local service connection", self.connected),
            ("Fresh Kraken telemetry", self.telemetry().is_some()),
            ("Pump command changes measured RPM", false),
            ("Profile restored after power loss", false),
            ("Ring / logo writes confirmed on hardware", false),
            ("Motherboard fan / ARGB support", false),
        ];
        let mut list = column![
            heading("Evidence, before the badge.", 19),
            label("Passing telemetry checks does not mark unimplemented controls as verified.")
        ]
        .spacing(15);
        for (name, verified) in rows {
            let status = if verified {
                if self.mode == Mode::Demo {
                    "Demo only"
                } else {
                    "Observed"
                }
            } else {
                "Pending"
            };
            list = list.push(
                row![
                    text(if verified { "●" } else { "○" }).color(if verified {
                        style::ACCENT
                    } else {
                        style::MUTED
                    }),
                    text(name).size(13),
                    Space::new().width(Fill),
                    label(status)
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        list.push(rule::horizontal(1))
            .push(
                row![
                    button("Copy diagnostics")
                        .on_press(Message::CopyReport)
                        .padding([10, 15])
                        .style(style::secondary),
                    button("Export report")
                        .on_press(Message::ExportReport)
                        .padding([10, 15])
                        .style(style::secondary)
                ]
                .spacing(10),
            )
            .push(label("Reports omit USB paths and serial numbers."))
            .into()
    }

    fn motherboard(&self) -> Element<'_, Message> {
        column![container(column![heading("A hub shares a signal.", 20), label(&self.selected().detail)].spacing(12)).padding(23).width(Fill).style(style::box_style(style::BG, true, 9.0)),
            self.capabilities(), label("These are recorded build targets. LightDriver has not detected or verified the board’s fan / RGB controller."),
            row![button("Copy diagnostics").on_press(Message::CopyReport).padding([10, 15]).style(style::secondary), button("Export report").on_press(Message::ExportReport).padding([10, 15]).style(style::secondary)].spacing(10),
        ].spacing(25).into()
    }
}

fn format_number(value: u16) -> String {
    if value >= 1000 {
        format!("{},{:03}", value / 1000, value % 1000)
    } else {
        value.to_string()
    }
}
