use crate::{Message, Workbench, persistence};
use iced::Task;
use lightdriver_core::{
    CommandState, ControlAction, ControlRequest, LightingEffect, LightingRoute, NamedProfile,
    now_ms,
};

impl Workbench {
    pub fn can_submit(&self, action: &ControlAction) -> bool {
        if !self.connected || self.command_in_flight || self.cpu_enable_in_flight {
            return false;
        }
        let control = &self.selected().control;
        match action {
            ControlAction::Release => control.enabled,
            ControlAction::Enable => control.available,
            ControlAction::PumpCurve(_) => {
                control.pump
                    && control.available
                    && control.enabled
                    && self.telemetry().is_some_and(|t| t.coolant_c < 59.0)
            }
            ControlAction::PumpFull | ControlAction::Lighting(_) => {
                control.pump && control.available && control.enabled
            }
            ControlAction::HeaderLighting { channel_id, .. } => {
                control.available
                    && control.enabled
                    && control.lighting_outputs.iter().any(|c| &c.id == channel_id)
            }
        }
    }
    pub fn case_output(&self) -> Option<&lightdriver_core::LightingOutput> {
        let selected = self.selected();
        let route = self.rig.lighting_routes.iter().find(|r| {
            r.device_id == selected.id
                && r.name == "Case lighting"
                && r.board_identity.as_deref() == Some(selected.name.as_str())
        })?;
        selected
            .control
            .lighting_outputs
            .iter()
            .find(|c| c.label.eq_ignore_ascii_case(&route.channel))
    }
    pub fn case_action(&self) -> Option<ControlAction> {
        self.case_output()
            .filter(|c| self.header.is_empty() || c.label.eq_ignore_ascii_case(&self.header))
            .map(|c| ControlAction::HeaderLighting {
                channel_id: c.id.clone(),
                settings: self.draft.lighting(),
            })
    }
    pub fn can_control(&self) -> bool {
        self.can_submit(&ControlAction::PumpCurve(self.draft.curve.clone()))
    }
    pub fn control_update(&mut self, message: &Message) -> Option<Task<Message>> {
        match message {
            Message::Apply(action) => {
                if !self.can_submit(action) {
                    self.status = "Connect a supported device and enable control first. Pump curves require fresh, noncritical coolant telemetry.".into();
                    return Some(Task::none());
                }
                if let Err(e) = action.validate() {
                    self.status = e;
                    return Some(Task::none());
                }
                self.command_count += 1;
                let request = ControlRequest {
                    request_id: format!(
                        "{}-{}-{}",
                        std::process::id(),
                        now_ms(),
                        self.command_count
                    ),
                    device_id: self.selected().id.clone(),
                    mode: self.mode,
                    expires_at_ms: now_ms() + 5_000,
                    action: action.clone(),
                };
                let mode = self.mode;
                self.command_in_flight = true;
                self.status = format!("Sending {}…", action.label());
                return Some(Task::perform(
                    async move {
                        lightdriver_ipc::control(mode,request).await.map_err(|e|format!("Command outcome is unknown: {e}. No automatic retry; check device state."))
                    },
                    Message::Applied,
                ));
            }
            Message::Applied(result) => {
                self.command_in_flight = false;
                match result {
                    Ok(receipt) => {
                        self.status = format!("{:?}: {}", receipt.state, receipt.detail);
                        self.last_receipt = Some(receipt.clone());
                    }
                    Err(e) => self.status = e.clone(),
                }
                return Some(self.fetch(false));
            }
            Message::LightingChannel(value) => self.draft.lighting_channel = *value,
            Message::LightingEffect(value) => {
                self.draft.lighting_effect = *value;
                if *value == LightingEffect::Spectrum {
                    self.draft.lighting_brightness = 100;
                }
            }
            Message::LightingSpeed(value) => self.draft.lighting_speed = *value,
            Message::ColorComponent(index, value) => {
                if let Some(c) = self.draft.lighting_rgb.get_mut(*index) {
                    *c = *value;
                }
            }
            Message::Temperature(index, value) => {
                if let Some(p) = self.draft.curve.points.get_mut(*index) {
                    p.coolant_c = *value;
                    self.preset = None;
                }
            }
            Message::AddPoint => {
                if self.draft.curve.points.len() < 16
                    && let Some(gap) = self
                        .draft
                        .curve
                        .points
                        .windows(2)
                        .position(|p| p[1].coolant_c.saturating_sub(p[0].coolant_c) > 1)
                {
                    let a = &self.draft.curve.points[gap];
                    let b = &self.draft.curve.points[gap + 1];
                    self.draft.curve.points.insert(
                        gap + 1,
                        lightdriver_core::CurvePoint {
                            coolant_c: (u16::from(a.coolant_c) + u16::from(b.coolant_c)).div_ceil(2)
                                as u8,
                            duty: (u16::from(a.duty) + u16::from(b.duty)).div_ceil(2) as u8,
                        },
                    );
                }
            }
            Message::RemovePoint(index) => {
                if self.draft.curve.points.len() > 2 && *index < self.draft.curve.points.len() {
                    self.draft.curve.points.remove(*index);
                }
            }
            Message::ProfileName(name) => self.profile_name = name.clone(),
            Message::SelectProfile(name) => {
                if let Some(profile) = self.profiles.profiles.iter().find(|p| &p.name == name) {
                    self.draft = profile.draft.clone();
                    self.profile_name = name.clone();
                    self.selected_profile = Some(name.clone());
                    self.preset = None;
                    self.status =
                        "Profile loaded as a local draft. Apply explicitly to send it.".into();
                }
            }
            Message::SaveProfile => {
                if let Err(e) = persistence::load_profiles(&self.data_directory) {
                    self.status =
                        format!("Existing profile library needs repair before saving: {e}");
                    return Some(Task::none());
                }
                let name = self.profile_name.trim().to_string();
                let mut updated = self.profiles.clone();
                if let Some(profile) = updated
                    .profiles
                    .iter_mut()
                    .find(|p| p.name.eq_ignore_ascii_case(&name))
                {
                    profile.draft = self.draft.clone();
                } else {
                    updated.profiles.push(NamedProfile {
                        name: name.clone(),
                        draft: self.draft.clone(),
                    });
                }
                match persistence::save_profiles(&self.data_directory, &updated) {
                    Ok(_) => {
                        self.profiles = updated;
                        self.selected_profile = Some(name);
                        self.status =
                            "Profile saved locally; no automatic hardware application.".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            Message::DeleteProfile => {
                if let Err(e) = persistence::load_profiles(&self.data_directory) {
                    self.status =
                        format!("Existing profile library needs repair before deleting: {e}");
                    return Some(Task::none());
                }
                if let Some(name) = &self.selected_profile {
                    let mut updated = self.profiles.clone();
                    updated.profiles.retain(|p| &p.name != name);
                    match persistence::save_profiles(&self.data_directory, &updated) {
                        Ok(_) => {
                            self.profiles = updated;
                            self.selected_profile = None;
                            self.profile_name.clear();
                            self.status = "Saved profile removed.".into();
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            Message::ImportPath(path) => self.import_path = path.clone(),
            Message::ImportProfile => {
                match persistence::import_draft(std::path::Path::new(&self.import_path)) {
                    Ok(draft) => {
                        self.draft = draft;
                        self.preset = None;
                        self.status = "Imported validated profile as a draft.".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            Message::ExportProfile => {
                self.status = match self.draft.validate().and_then(|_| {
                    persistence::save_json(&self.data_directory, "profile-export.json", &self.draft)
                }) {
                    Ok(path) => format!("Profile exported: {}", path.display()),
                    Err(e) => e,
                };
            }
            Message::Header(value) => self.header = value.clone(),
            Message::SaveWiring => {
                if self.selected().id != "motherboard" {
                    self.status =
                        "Select the detected motherboard before assigning a header.".into();
                    return Some(Task::none());
                }
                if let Err(e) = persistence::load_rig(&self.data_directory) {
                    self.status = format!("Existing wiring needs repair before saving: {e}");
                    return Some(Task::none());
                }
                if self.header.trim().is_empty() {
                    self.status = "Enter the actual header name first.".into();
                } else {
                    let mut updated = self.rig.clone();
                    updated.lighting_routes.retain(|r| {
                        !(r.device_id == self.selected().id && r.name == "Case lighting")
                    });
                    updated.lighting_routes.push(LightingRoute {
                        name: "Case lighting".into(),
                        device_id: self.selected().id.clone(),
                        provider: if self.selected().control.lighting_outputs.is_empty() {
                            "vendor-sdk"
                        } else {
                            "usb-hid"
                        }
                        .into(),
                        channel: self.header.trim().into(),
                        board_identity: Some(self.selected().name.clone()),
                    });
                    match persistence::save_rig(&self.data_directory, &updated) {
                        Ok(_) => {
                            self.rig = updated;
                            self.status="Wiring saved for this user. The detected adapter must expose this header before lighting can be applied.".into();
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            Message::Evidence(index, value) => {
                if let Some(flag) = self.evidence.get_mut(*index) {
                    *flag = *value;
                }
            }
            Message::ExportTestReport => {
                let report = serde_json::json!({
                    "schema_version":1,"application":env!("CARGO_PKG_VERSION"),"mode":format!("{:?}",self.mode),
                    "hardware_verified":false,"selected_device":self.selected().name,
                    "user_observations":{"pump_response":self.evidence[0],"lighting_response":self.evidence[1],"power_cycle_persistence":self.evidence[2]},
                    "devices":self.snapshot.devices.iter().map(|d|serde_json::json!({"name":d.name,"connection":d.connection,"status":d.status,"capabilities":d.capabilities})).collect::<Vec<_>>(),
                    "commands":self.snapshot.command_receipts.iter().filter(|r| r.device_id==self.selected().id).map(|r|serde_json::json!({"action":r.action,"state":r.state,"detail":r.detail,"at_ms":r.at_ms})).collect::<Vec<_>>(),
                    "firmware":self.selected().control.firmware,
                    "telemetry_current":self.telemetry(),
                    "limitations":"User observations are self-reported. Export is local and excludes USB paths, serials, machine/user names and SDK paths."
                });
                self.status = match persistence::save_json(
                    &self.data_directory,
                    "hardware-test-report.json",
                    &report,
                ) {
                    Ok(path) => format!("Test report saved locally: {}", path.display()),
                    Err(e) => e,
                };
            }
            _ => return None,
        }
        Some(Task::none())
    }
    pub fn command_summary(&self) -> String {
        self.last_receipt
            .as_ref()
            .filter(|r| r.device_id == self.selected().id)
            .map_or("No command submitted in this window.".into(), |r| {
                let state = match r.state {
                    CommandState::Submitted => "Submitted; physical verification pending",
                    CommandState::Rejected => "Rejected",
                    CommandState::Simulated => "Demo simulation",
                    CommandState::Enabled => "Control enabled",
                    CommandState::Released => "Control released",
                };
                format!("{} · {state}\n{}", r.action, r.detail)
            })
    }
}
