//! Typed commands; no raw USB packets, paths, register addresses or arbitrary SDK calls.
use crate::{Mode, PumpCurve, Telemetry, now_ms};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingChannel {
    Ring,
    Logo,
    #[default]
    Both,
}
impl LightingChannel {
    pub const ALL: [Self; 3] = [Self::Both, Self::Ring, Self::Logo];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ring => "Ring",
            Self::Logo => "Logo",
            Self::Both => "Ring + logo",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingEffect {
    #[default]
    Static,
    Breathing,
    Spectrum,
    Off,
}
impl LightingEffect {
    pub const ALL: [Self; 4] = [Self::Static, Self::Breathing, Self::Spectrum, Self::Off];
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Breathing => "Breathing",
            Self::Spectrum => "Spectrum",
            Self::Off => "Off",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightingSettings {
    pub channel: LightingChannel,
    pub effect: LightingEffect,
    pub rgb: [u8; 3],
    pub brightness: u8,
    pub speed: u8,
}
impl LightingSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.brightness > 100 || self.speed > 4 {
            return Err("Brightness must be 0–100%; effect speed must be 0–4.".into());
        }
        if self.effect == LightingEffect::Spectrum && self.brightness != 100 {
            return Err(
                "Spectrum uses device-controlled brightness; set brightness to 100%.".into(),
            );
        }
        Ok(())
    }
}
impl std::fmt::Display for LightingChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
impl std::fmt::Display for LightingEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlAction {
    Enable,
    Release,
    PumpCurve(PumpCurve),
    Lighting(LightingSettings),
    HeaderLighting {
        channel_id: String,
        settings: LightingSettings,
    },
    PumpFull,
}
impl ControlAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Enable => "Enable control",
            Self::Release => "Release control",
            Self::PumpCurve(_) => "Pump curve",
            Self::Lighting(_) | Self::HeaderLighting { .. } => "Lighting",
            Self::PumpFull => "Pump 100%",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::PumpCurve(curve) => curve.validate(),
            Self::Lighting(settings) => settings.validate(),
            Self::HeaderLighting {
                channel_id,
                settings,
            } => {
                if channel_id.is_empty()
                    || channel_id.len() > 32
                    || !channel_id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err("Invalid lighting channel identifier.".into());
                }
                settings.validate()
            }
            _ => Ok(()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequest {
    pub request_id: String,
    pub device_id: String,
    pub mode: Mode,
    pub expires_at_ms: u64,
    pub action: ControlAction,
}
impl ControlRequest {
    pub fn validate_at(&self, now: u64) -> Result<(), String> {
        if self.request_id.is_empty()
            || self.request_id.len() > 64
            || !self
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("Invalid command identifier.".into());
        }
        if self.device_id.is_empty() || self.device_id.len() > 80 {
            return Err("Invalid device identifier.".into());
        }
        if self.expires_at_ms < now || self.expires_at_ms.saturating_sub(now) > 10_000 {
            return Err("Command expired or deadline is invalid. Commands are never replayed automatically.".into());
        }
        self.action.validate()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingOutput {
    pub id: String,
    pub label: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlState {
    /// Adapter-backed target, never inferred from a user-assigned header label.
    pub available: bool,
    pub pump: bool,
    pub lighting_outputs: Vec<LightingOutput>,
    pub enabled: bool,
    pub firmware: Option<String>,
    pub conflict: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandState {
    Enabled,
    Released,
    Submitted,
    Simulated,
    Rejected,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub request_id: String,
    pub device_id: String,
    pub action: String,
    pub state: CommandState,
    pub detail: String,
    pub at_ms: u64,
    pub observed: Option<Telemetry>,
}
impl CommandReceipt {
    pub fn rejected(request: &ControlRequest, detail: impl Into<String>) -> Self {
        Self {
            request_id: request.request_id.clone(),
            device_id: request.device_id.clone(),
            action: request.action.label().into(),
            state: CommandState::Rejected,
            detail: detail.into(),
            at_ms: now_ms(),
            observed: None,
        }
    }
    pub fn success(
        request: &ControlRequest,
        state: CommandState,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            state,
            ..Self::rejected(request, detail)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedProfile {
    pub name: String,
    pub draft: crate::Draft,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLibrary {
    pub schema_version: u16,
    pub profiles: Vec<NamedProfile>,
}
impl Default for ProfileLibrary {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profiles: vec![],
        }
    }
}
impl ProfileLibrary {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.profiles.len() > 32 {
            return Err("Unsupported or oversized profile library.".into());
        }
        let mut names = std::collections::HashSet::new();
        for p in &self.profiles {
            if p.name.trim().is_empty()
                || p.name.len() > 64
                || p.name.chars().any(char::is_control)
                || !names.insert(p.name.trim().to_lowercase())
            {
                return Err(
                    "Profile names must be unique, 1–64 bytes and contain no control characters."
                        .into(),
                );
            }
            p.draft.validate()?;
        }
        Ok(())
    }
}
/// User wiring is separate from automatic discovery and is never a global board default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingRoute {
    pub name: String,
    pub device_id: String,
    pub provider: String,
    pub channel: String,
    #[serde(default)]
    pub board_identity: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigConfiguration {
    pub schema_version: u16,
    pub lighting_routes: Vec<LightingRoute>,
}
impl Default for RigConfiguration {
    fn default() -> Self {
        Self {
            schema_version: 1,
            lighting_routes: vec![],
        }
    }
}
impl RigConfiguration {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.lighting_routes.len() > 32 {
            return Err("Unsupported or oversized wiring configuration.".into());
        }
        for route in &self.lighting_routes {
            if route
                .board_identity
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
            {
                return Err("Invalid wiring device identity.".into());
            }
            for s in [
                &route.name,
                &route.device_id,
                &route.provider,
                &route.channel,
            ] {
                if s.trim().is_empty() || s.len() > 80 || s.chars().any(char::is_control) {
                    return Err("Invalid lighting route.".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_invalid_and_oversized_commands_are_rejected() {
        let mut r = ControlRequest {
            request_id: "test-1".into(),
            device_id: "demo-kraken-x3".into(),
            mode: Mode::Demo,
            expires_at_ms: 200,
            action: ControlAction::PumpCurve(PumpCurve::example(crate::Preset::Balanced)),
        };
        assert!(r.validate_at(100).is_ok());
        assert!(r.validate_at(201).is_err());
        r.request_id = "../other".into();
        assert!(r.validate_at(100).is_err());
        r.request_id = "good".into();
        r.action = ControlAction::Lighting(LightingSettings {
            channel: LightingChannel::Both,
            effect: LightingEffect::Static,
            rgb: [255; 3],
            brightness: 101,
            speed: 2,
        });
        assert!(r.validate_at(100).is_err());
    }
    #[test]
    fn fresh_install_has_no_assumed_hub_or_header_and_profiles_are_bounded() {
        assert!(RigConfiguration::default().lighting_routes.is_empty());
        let mut library = ProfileLibrary::default();
        library.profiles.push(NamedProfile {
            name: "Quiet".into(),
            draft: crate::Draft::default(),
        });
        library.profiles.push(NamedProfile {
            name: "quiet".into(),
            draft: crate::Draft::default(),
        });
        assert!(library.validate().is_err());
    }
}
