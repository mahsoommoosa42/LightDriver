use crate::{
    Message, Tab, Workbench, charts,
    style::{self, badge, heading, label, selector},
};
use iced::{
    Alignment, Element,
    Length::{Fill, FillPortion},
    widget::{
        Column, Space, button, canvas, checkbox, column, container, pick_list, row, rule,
        scrollable, slider, text, text_input,
    },
};
use lightdriver_core::{
    ControlAction, DeviceStatus, LightingChannel, LightingEffect, Mode, Preset, Support,
};

impl Workbench {
    pub fn layout(&self) -> Element<'_, Message> {
        let brand = row![
            lightdriver_branding::mark(46),
            heading("LightDriver", 18),
            label("WORKBENCH"),
            Space::new().width(Fill),
            selector(
                text("Live").size(12),
                Message::Mode(Mode::Live),
                self.mode == Mode::Live
            )
            .on_press_maybe(
                (!self.poll_in_flight && !self.command_in_flight && !self.cpu_enable_in_flight)
                    .then_some(Message::Mode(Mode::Live))
            ),
            selector(
                text("Demo").size(12),
                Message::Mode(Mode::Demo),
                self.mode == Mode::Demo
            )
            .on_press_maybe(
                (!self.poll_in_flight && !self.command_in_flight && !self.cpu_enable_in_flight)
                    .then_some(Message::Mode(Mode::Demo))
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let title = row![
            column![
                label(if matches!(self.tab, Tab::Diagnostics | Tab::Logs) {
                    "SYSTEM WORKSPACE"
                } else {
                    "DEVICE WORKSPACE"
                }),
                heading("Understand. Control. Verify.", 26)
            ]
            .spacing(6),
            Space::new().width(Fill),
            badge(
                if self.mode == Mode::Demo {
                    "DEMO DATA"
                } else {
                    if self.selected().control.enabled {
                        "CONTROL ENABLED"
                    } else {
                        "MONITORING"
                    }
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
                "v{} · {} · {}",
                env!("CARGO_PKG_VERSION"),
                self.renderer,
                if cfg!(debug_assertions) {
                    "Debug"
                } else {
                    "Release"
                }
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
                .id("device-workspace")
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
            let selected = !matches!(self.tab, Tab::Diagnostics | Tab::Logs)
                && self.selected_device == device.id;
            let content = row![
                text(if is_board { "▧" } else { "◉" })
                    .size(18)
                    .color(if selected {
                        style::ACCENT
                    } else {
                        style::MUTED
                    }),
                column![
                    heading(&device.name, 14),
                    label(if is_board {
                        device.status.label()
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
                        label("Fan curves → BIOS"),
                        label(if device.control.lighting_outputs.is_empty() {
                            "ARGB → Vendor adapter"
                        } else {
                            "ARGB → USB HID"
                        }),
                        label("Hub headers → User setup")
                    ]
                    .spacing(9)
                } else if device.control.available {
                    column![label("Pump"), label("Ring + logo")].spacing(9)
                } else {
                    column![label("Automatic discovery")].spacing(9)
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
                rule::horizontal(1),
                selector(
                    text("System diagnostics").size(13),
                    Message::Tab(Tab::Diagnostics),
                    self.tab == Tab::Diagnostics
                )
                .width(Fill),
                selector(
                    text("Live logs").size(13),
                    Message::Tab(Tab::Logs),
                    self.tab == Tab::Logs
                )
                .width(Fill),
                Space::new().height(Fill),
                label("Support is verified per capability."),
                text(service_status).size(12).color(if self.connected {
                    style::ACCENT
                } else {
                    style::AMBER
                }),
                button(text("Reconnect service").size(12))
                    .on_press_maybe(
                        (!self.poll_in_flight
                            && !self.command_in_flight
                            && !self.cpu_enable_in_flight)
                            .then_some(Message::Reconnect)
                    )
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
        if self.tab == Tab::Logs {
            return self.log_view();
        }
        if self.tab == Tab::Diagnostics {
            return self.diagnostics();
        }
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
        if device.id == "inventory" {
            return content
                .push(label(&device.detail))
                .push(self.setup())
                .into();
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
        if self.tab != Tab::Setup {
            content = content.push(self.control_panel());
        }
        content = content.push(match self.tab {
            Tab::Profile => self.profile(),
            Tab::Lighting => self.lighting(),
            Tab::Capabilities => self.capabilities(),
            Tab::Verification => self.verification(),
            Tab::Diagnostics => self.diagnostics(),
            Tab::Setup => self.setup(),
            Tab::Logs => self.log_view(),
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
                        slider(20u8..=59u8, point.coolant_c, move |v| Message::Temperature(
                            i, v
                        )),
                        slider(20u8..=100u8, point.duty, move |v| Message::Duty(i, v)),
                        button(text("Remove point").size(11))
                            .on_press_maybe(
                                (self.draft.curve.points.len() > 2)
                                    .then_some(Message::RemovePoint(i))
                            )
                            .style(style::secondary),
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
            button(text("Add curve point").size(12))
                .on_press_maybe(
                    (self.draft.curve.points.len() < 16
                        && self
                            .draft
                            .curve
                            .points
                            .windows(2)
                            .any(|p| p[1].coolant_c.saturating_sub(p[0].coolant_c) > 1))
                    .then_some(Message::AddPoint)
                )
                .style(style::secondary),
            text(detail).size(12).color(if validation.is_err() {
                style::AMBER
            } else {
                style::MUTED
            }),
            rule::horizontal(1),
            row![
                label("Device runs the curve while the UI is closed."),
                Space::new().width(Fill),
                button(text("Apply pump curve").size(12))
                    .on_press_maybe(
                        (validation.is_ok()
                            && self.can_control()
                            && self.selected().control.enabled)
                            .then(|| Message::Apply(ControlAction::PumpCurve(
                                self.draft.curve.clone()
                            )))
                    )
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
            heading("Kraken lighting", 19),
            label("Color preview · Device effects run on the Kraken"),
            pick_list(
                LightingChannel::ALL,
                Some(self.draft.lighting_channel),
                Message::LightingChannel
            ),
            pick_list(
                LightingEffect::ALL,
                Some(self.draft.lighting_effect),
                Message::LightingEffect
            ),
            colors,
            self.rgb_sliders(),
            row![
                label("Brightness"),
                Space::new().width(Fill),
                label(format!("{}%", self.draft.lighting_brightness))
            ],
            if self.draft.lighting_effect == LightingEffect::Spectrum {
                Element::from(label("Spectrum brightness is controlled by the device."))
            } else {
                slider(
                    0u8..=100u8,
                    self.draft.lighting_brightness,
                    Message::Brightness,
                )
                .into()
            },
            row![
                label("Effect speed"),
                slider(0u8..=4u8, self.draft.lighting_speed, Message::LightingSpeed)
            ]
            .spacing(12),
            button(text("Apply lighting").size(12))
                .on_press_maybe(
                    self.can_submit(&ControlAction::Lighting(self.draft.lighting()))
                        .then(|| Message::Apply(ControlAction::Lighting(self.draft.lighting())))
                )
                .padding([10, 15])
                .style(style::primary),
            label("Case lighting uses its own discovered motherboard adapter."),
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
            label("Editing is a local preview. Apply sends only the selected channels."),
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
        let mut rows = vec![("Local service connection", self.connected)];
        if self.selected().control.pump {
            rows.push(("Fresh coolant telemetry", self.telemetry().is_some()));
            rows.push(("Pump command changes measured RPM", false));
            rows.push(("Ring / logo writes confirmed on hardware", false));
        } else {
            rows.push((
                "Controller identity confirmed",
                self.selected().control.firmware.is_some(),
            ));
            rows.push(("Requested header lighting observed", false));
        }
        rows.push(("Device settings persist after power loss", false));
        let mut list = column![
            heading("Evidence, before the badge.", 19),
            label(
                "Connection, command submission and hardware verification are separate evidence."
            )
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
        list = list.push(self.test_report_panel());
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
        let mut tabs = row![].spacing(7);
        for tab in [
            Tab::Lighting,
            Tab::Capabilities,
            Tab::Verification,
            Tab::Setup,
        ] {
            tabs = tabs.push(selector(
                text(tab.label()).size(12),
                Message::Tab(tab),
                self.tab == tab || self.tab == Tab::Profile && tab == Tab::Lighting,
            ));
        }
        let mut content =
            column![label(&self.selected().detail), tabs, rule::horizontal(1)].spacing(18);
        if self.tab != Tab::Setup {
            content = content.push(self.control_panel());
        }
        content
            .push(match self.tab {
                Tab::Capabilities => self.capabilities(),
                Tab::Verification => self.verification(),
                Tab::Setup => self.setup(),
                _ => self.case_lighting(),
            })
            .into()
    }

    fn control_panel(&self) -> Element<'_, Message> {
        let enabled = self.selected().control.enabled;
        let action = if enabled {
            ControlAction::Release
        } else {
            ControlAction::Enable
        };
        let title = if self.command_in_flight {
            "Sending…"
        } else if enabled {
            "Release control"
        } else if self.mode == Mode::Demo {
            "Enable demo control"
        } else if self.selected().control.pump {
            "Enable Kraken control"
        } else {
            "Enable RGB control"
        };
        let full: Element<'_, Message> = if self.selected().control.pump {
            button(text("Pump 100%").size(12))
                .on_press_maybe(
                    self.can_submit(&ControlAction::PumpFull)
                        .then_some(Message::Apply(ControlAction::PumpFull)),
                )
                .padding([10, 14])
                .style(style::primary)
                .into()
        } else {
            Space::new().width(0).into()
        };
        container(column![
            row![
                column![heading(if enabled {"Device control enabled"}else{"Monitor first. Apply when ready."},15),
                    label("Close competing controller software before enabling. Releasing keeps the last device settings.")].spacing(5).width(Fill),
                button(text(title).size(12)).on_press_maybe(self.can_submit(&action).then_some(Message::Apply(action))).padding([10,14]).style(style::secondary),
                full,
            ].spacing(12).align_y(Alignment::Center),
            label(self.command_summary()),
        ].spacing(10)).padding(16).width(Fill).style(style::box_style(style::PANEL,true,8.0)).into()
    }

    fn case_lighting(&self) -> Element<'_, Message> {
        let outputs = self
            .selected()
            .control
            .lighting_outputs
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>();
        let assigned = self.case_output().map(|c| c.label.as_str());
        let selected = if self.header.is_empty() {
            assigned.map(str::to_owned)
        } else {
            Some(self.header.clone())
        };
        let action = self.case_action();
        let apply = action
            .as_ref()
            .filter(|a| self.can_submit(a))
            .cloned()
            .map(Message::Apply);
        let mut body=column![
            row![heading("Case lighting",22),Space::new().width(Fill),
                badge(assigned.unwrap_or("No header assigned"),if assigned.is_some(){style::ACCENT}else{style::AMBER})].align_y(Alignment::Center),
            label("A passive hub follows its motherboard header. Connected lights receive the same group effect."),
        ].spacing(16);
        if outputs.is_empty() {
            return body.push(label("A compatible lighting adapter is not available for this configuration. Assign your wiring in Setup; unsupported channels stay disabled."))
                .push(self.capabilities()).into();
        }
        body = body.push(
            row![
                pick_list(outputs, selected, Message::Header)
                    .placeholder("Choose connected ARGB header"),
                button("Save connection")
                    .on_press_maybe((!self.header.is_empty()).then_some(Message::SaveWiring))
                    .padding([9, 14])
                    .style(style::secondary),
            ]
            .spacing(12),
        );
        body.push(row![
            pick_list(LightingEffect::ALL,self.draft.lighting_effect.into(),Message::LightingEffect),
            Space::new().width(Fill),
            heading(format!("#{:02X}{:02X}{:02X}",self.draft.lighting_rgb[0],self.draft.lighting_rgb[1],self.draft.lighting_rgb[2]),22),
        ].spacing(12).align_y(Alignment::Center))
        .push(self.rgb_sliders())
        .push(if self.draft.lighting_effect==LightingEffect::Spectrum {
            Element::from(label("Spectrum uses controller-native color cycling at full brightness."))
        } else {
            row![label("Brightness").width(80),slider(0u8..=100u8,self.draft.lighting_brightness,Message::Brightness),
                label(format!("{}%",self.draft.lighting_brightness)).width(38)].spacing(12).into()
        })
        .push(row![label("Effect speed").width(80),slider(0u8..=4u8,self.draft.lighting_speed,Message::LightingSpeed),
            label(format!("{}/4",self.draft.lighting_speed)).width(38)].spacing(12))
        .push(row![button("Apply case lighting").on_press_maybe(apply).padding([11,16]).style(style::primary),
            button("Save draft").on_press_maybe(self.draft.validate().is_ok().then_some(Message::SaveDraft)).padding([11,16]).style(style::secondary)].spacing(12))
        .push(label("Controller-native effects only. USB submission is not visible-response verification; sleep and power-cycle persistence still need testing."))
        .into()
    }

    fn rgb_sliders(&self) -> Element<'_, Message> {
        let mut controls = column![].spacing(7);
        for (index, name) in ["Red", "Green", "Blue"].iter().enumerate() {
            controls = controls.push(
                row![
                    label(*name).width(45),
                    slider(0u8..=255u8, self.draft.lighting_rgb[index], move |v| {
                        Message::ColorComponent(index, v)
                    }),
                    label(self.draft.lighting_rgb[index].to_string()).width(30)
                ]
                .spacing(10),
            );
        }
        controls.into()
    }

    fn setup(&self) -> Element<'_, Message> {
        let profile_names = self
            .profiles
            .profiles
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>();
        let mut routes = column![].spacing(7);
        for route in &self.rig.lighting_routes {
            routes = routes.push(label(format!(
                "{} · {} · {}",
                route.name, route.provider, route.channel
            )));
        }
        if self.rig.lighting_routes.is_empty() {
            routes = routes.push(label("No hub wiring assigned."));
        }
        column![
            heading("Saved profiles",19),
            label("Profiles load as drafts. Startup and reconnection never apply hardware settings."),
            row![pick_list(profile_names,self.selected_profile.clone(),Message::SelectProfile).placeholder("Load profile"),button("Delete selected").on_press_maybe(self.selected_profile.is_some().then_some(Message::DeleteProfile)).style(style::secondary)].spacing(12),
            row![text_input("Profile name",&self.profile_name).on_input(Message::ProfileName),button("Save profile").on_press_maybe((!self.profile_name.trim().is_empty() && self.draft.validate().is_ok()).then_some(Message::SaveProfile)).style(style::primary),button("Export profile").on_press_maybe(self.draft.validate().is_ok().then_some(Message::ExportProfile)).style(style::secondary)].spacing(12),
            row![text_input("Profile JSON file path",&self.import_path).on_input(Message::ImportPath),button("Import").on_press_maybe((!self.import_path.trim().is_empty()).then_some(Message::ImportProfile)).style(style::secondary)].spacing(12),
            rule::horizontal(1),
            heading("Your connections",19),
            label("Assign passive hubs to your actual motherboard headers. Channel availability comes from the detected adapter; a saved label does not establish hardware support."),
            routes,
            if self.selected().id=="motherboard" {
                row![text_input("Case ARGB header, e.g. D_LED2",&self.header).on_input(Message::Header),button("Save case route").on_press_maybe((!self.header.trim().is_empty()).then_some(Message::SaveWiring)).style(style::primary)].spacing(12).into()
            } else {Element::from(label("Select your motherboard to assign the case-lighting route."))},
            label("Motherboard fan curves stay in BIOS in this version."),
        ].spacing(15).into()
    }

    fn test_report_panel(&self) -> Element<'_, Message> {
        column![
            rule::horizontal(1),
            heading("Hardware test report",17),
            label("Record your observations. These do not automatically grant a verified-support badge."),
            checkbox(self.evidence[0]).label("I observed a pump response").on_toggle(|v|Message::Evidence(0,v)),
            checkbox(self.evidence[1]).label("I observed the requested lighting").on_toggle(|v|Message::Evidence(1,v)),
            checkbox(self.evidence[2]).label("I checked persistence after a power cycle").on_toggle(|v|Message::Evidence(2,v)),
            button("Export local test report").on_press(Message::ExportTestReport).style(style::secondary).padding([10,15]),
            label("Saved locally. No automatic upload or contact with other testers."),
        ].spacing(12).into()
    }
}

fn format_number(value: u16) -> String {
    if value >= 1000 {
        format!("{},{:03}", value / 1000, value % 1000)
    } else {
        value.to_string()
    }
}
