//! Independent Rust implementation of the ITE 8297 HID protocol.
//! Protocol/layout facts were checked against pinned OpenRGB source; no SDK or C++ runtime.
use crate::{DeviceAdapter, motherboard};
use lightdriver_core::{
    CommandReceipt, CommandState, ControlAction, ControlRequest, ControlState, Device,
    DeviceStatus, LightingEffect, LightingOutput, LightingSettings, Mode, Support, now_ms,
};

fn known_layout(name: &str) -> bool {
    let normalized = name.trim().to_ascii_uppercase();
    matches!(
        normalized.trim_end_matches("-CF"),
        "X570 AORUS ELITE"
            | "X570 AORUS ELITE WIFI"
            | "X570 AORUS PRO"
            | "X570 AORUS PRO WIFI"
            | "X570 AORUS ULTRA"
    )
}
pub(crate) fn outputs() -> Vec<LightingOutput> {
    vec![
        LightingOutput {
            id: "argb-1".into(),
            label: "D_LED1".into(),
        },
        LightingOutput {
            id: "argb-2".into(),
            label: "D_LED2".into(),
        },
    ]
}
fn channel_index(id: &str) -> Result<u8, String> {
    match id {
        "argb-1" => Ok(5),
        "argb-2" => Ok(6),
        _ => Err("Channel is not exposed by this board adapter.".into()),
    }
}
fn firmware_report(report: &[u8]) -> Result<String, String> {
    if report.len() != 64 || ![0, 0xcc].contains(&report[0]) || report[1] != 1 || report[2] != 0 {
        return Err("Unexpected ITE identity report or unsupported controller instance.".into());
    }
    let name = &report[12..40];
    let end = name.iter().position(|&v| v == 0).unwrap_or(name.len());
    let name = std::str::from_utf8(&name[..end]).map_err(|_| "Invalid ITE product string.")?;
    if !name.starts_with("IT8297")
        || name.bytes().any(|c| !c.is_ascii_graphic() && c != b' ')
        || report[4..8] == [0; 4]
        || report[4..8] == [255; 4]
    {
        return Err("Controller identity did not confirm the expected IT8297 family.".into());
    }
    Ok(format!(
        "{}.{}.{}.{}",
        report[4], report[5], report[6], report[7]
    ))
}
pub(crate) fn effect_reports(
    channel: &str,
    settings: &LightingSettings,
) -> Result<[[u8; 64]; 2], String> {
    settings.validate()?;
    let index = channel_index(channel)?;
    let mask = 1u32 << index;
    let mut effect = [0u8; 64];
    effect[0] = 0xcc;
    effect[1] = 0x20 + index;
    effect[2..6].copy_from_slice(&mask.to_le_bytes());
    effect[11] = match settings.effect {
        LightingEffect::Breathing => 2,
        LightingEffect::Spectrum => 4,
        _ => 1,
    };
    effect[12] = if settings.effect == LightingEffect::Off {
        0
    } else {
        settings.brightness
    };
    if matches!(
        settings.effect,
        LightingEffect::Static | LightingEffect::Breathing
    ) {
        effect[14..17].copy_from_slice(&[settings.rgb[2], settings.rgb[1], settings.rgb[0]]);
    }
    // Five UI speeds, using controller-native millisecond timing fields.
    let period: u16 = match settings.effect {
        LightingEffect::Breathing => [1600, 1200, 1000, 800, 400][settings.speed as usize],
        LightingEffect::Spectrum => [2400, 1000, 800, 500, 300][settings.speed as usize],
        _ => 0,
    };
    effect[22..24].copy_from_slice(&period.to_le_bytes());
    let falling = if settings.effect == LightingEffect::Spectrum {
        period.saturating_sub(200)
    } else {
        period
    };
    effect[24..26].copy_from_slice(&falling.to_le_bytes());
    if settings.effect == LightingEffect::Breathing {
        effect[26..28].copy_from_slice(&200u16.to_le_bytes());
    }
    if settings.effect == LightingEffect::Spectrum {
        effect[30] = 7;
    }
    let mut apply = [0u8; 64];
    apply[..2].copy_from_slice(&[0xcc, 0x28]);
    apply[2..6].copy_from_slice(&mask.to_le_bytes());
    Ok([effect, apply])
}

pub(crate) fn demo_board(enabled: bool) -> Device {
    let mut board = motherboard::demo_board();
    board.status = DeviceStatus::Demo;
    board.control = ControlState {
        available: true,
        pump: false,
        lighting_outputs: outputs(),
        enabled,
        ..ControlState::default()
    };
    board.capabilities[1].support = Support::Implemented;
    board.capabilities[1].detail =
        "Simulated independent ARGB header targets. No hardware is opened.".into();
    board
}

#[cfg(windows)]
#[derive(Default)]
pub(crate) struct GigabyteAdapter {
    logger: lightdriver_core::LogStore,
    owned: Option<(std::ffi::CString, hidapi::HidDevice, String)>,
}
#[cfg(windows)]
fn matches(info: &hidapi::DeviceInfo) -> bool {
    info.vendor_id() == 0x048d
        && info.product_id() == 0x8297
        && info.usage_page() == 0xff89
        && info.usage() == 0xcc
}
#[cfg(windows)]
impl DeviceAdapter for GigabyteAdapter {
    fn set_logger(&mut self, logger: lightdriver_core::LogStore) {
        self.logger = logger;
    }
    fn poll(&mut self) -> Vec<Device> {
        let mut board = motherboard::discover_board();
        if !known_layout(&board.name) {
            self.owned = None;
            return vec![board];
        }
        let Ok(api) = hidapi::HidApi::new() else {
            self.owned = None;
            board.detail =
                "Board identified, but USB enumeration failed. RGB control is disabled.".into();
            return vec![board];
        };
        let matches: Vec<_> = api.device_list().filter(|d| matches(d)).collect();
        if matches.len() != 1 {
            self.owned = None;
            board.detail="Known board layout, but a single matching IT8297 lighting interface was not found. RGB control is disabled.".into();
            return vec![board];
        }
        let info = matches[0];
        if self
            .owned
            .as_ref()
            .is_some_and(|(path, _, _)| path.as_c_str() != info.path())
        {
            self.owned = None;
        }
        board.connection = "ITE 8297 · USB HID · motherboard ARGB".into();
        board.status = DeviceStatus::Detected;
        board.control = ControlState {
            available: true,
            pump: false,
            lighting_outputs: outputs(),
            enabled: self.owned.is_some(),
            firmware: self.owned.as_ref().map(|(_, _, v)| v.clone()),
            conflict: None,
        };
        board.detail="Recognized motherboard lighting controller. Assign your hub to a header, then explicitly enable RGB control. Fans remain controlled by BIOS.".into();
        board.capabilities[1].support = Support::Implemented;
        board.capabilities[1].detail="Independent Rust HID adapter: D_LED1/D_LED2 static, breathing, color cycle and off. Physical write validation is pending; stored-setting readback is unavailable.".into();
        vec![board]
    }
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        let result = (|| -> Result<(CommandState, String), String> {
            request.validate_at(now_ms())?;
            if request.mode != Mode::Live || request.device_id != "motherboard" {
                return Err(
                    "Motherboard RGB requires the detected live motherboard target.".into(),
                );
            }
            if request.action == ControlAction::Release {
                self.owned = None;
                return Ok((
                    CommandState::Released,
                    "RGB control released. Last device lighting remains.".into(),
                ));
            }
            if let Some(conflict) = crate::competing_controller() {
                self.owned = None;
                return Err(conflict);
            }
            let board = motherboard::discover_board();
            if !known_layout(&board.name) {
                return Err("This board layout is not supported by the ITE adapter.".into());
            }
            let api = hidapi::HidApi::new().map_err(|_| "HID enumeration failed.")?;
            let matches: Vec<_> = api.device_list().filter(|d| matches(d)).collect();
            if matches.len() != 1 {
                self.owned = None;
                return Err("Matching motherboard lighting controller is no longer connected or is ambiguous.".into());
            }
            let info = matches[0];
            if self
                .owned
                .as_ref()
                .is_some_and(|(p, _, _)| p.as_c_str() != info.path())
            {
                self.owned = None;
            }
            if request.action == ControlAction::Enable {
                if self.owned.is_some() {
                    return Ok((
                        CommandState::Enabled,
                        "RGB control is already enabled.".into(),
                    ));
                }
                let handle = info
                    .open_device(&api)
                    .map_err(|_| "Could not open motherboard lighting interface.")?;
                let mut query = [0u8; 64];
                query[..2].copy_from_slice(&[0xcc, 0x60]);
                send(&handle, &query)?;
                let mut report = [0u8; 64];
                report[0] = 0xcc;
                let n = handle
                    .get_feature_report(&mut report)
                    .map_err(|_| "ITE identity read failed.")?;
                self.logger.incoming_hid("aorus.identity", &report[..n]);
                let firmware = firmware_report(&report[..n])?;
                self.owned = Some((info.path().to_owned(), handle, firmware.clone()));
                return Ok((
                    CommandState::Enabled,
                    format!(
                        "RGB control enabled · Firmware {firmware}. Identity queried; no reset or lighting setting applied."
                    ),
                ));
            }
            let ControlAction::HeaderLighting {
                channel_id,
                settings,
            } = &request.action
            else {
                return Err(
                    "This controller exposes ARGB headers; pump/ring commands are not supported."
                        .into(),
                );
            };
            let packets = effect_reports(channel_id, settings)?;
            let (_, handle, _) = self.owned.as_ref().ok_or("Enable RGB control first.")?;
            for packet in packets {
                send(handle, &packet)?;
            }
            Ok((CommandState::Submitted,"Header effect submitted. Confirm visible response; stored settings and hardware verification are unavailable. No automatic retry.".into()))
        })();
        match result {
            Ok((state, detail)) => CommandReceipt::success(request, state, detail),
            Err(detail) => {
                if detail.contains("USB") || detail.contains("connected") {
                    self.owned = None;
                }
                CommandReceipt::rejected(request, detail)
            }
        }
    }
}
#[cfg(windows)]
fn send(handle: &hidapi::HidDevice, packet: &[u8; 64]) -> Result<(), String> {
    match handle.send_feature_report(packet) {
        Ok(()) => Ok(()),
        Err(_) => Err(
            "USB feature write failed. Settings may be partially changed; no automatic retry."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_is_model_specific_and_header_masks_cannot_target_other_zones() {
        assert!(known_layout("X570 AORUS ELITE WIFI"));
        assert!(known_layout("x570 aorus pro-cf"));
        assert!(!known_layout("X570 AORUS MASTER"));
        assert!(!known_layout("Other motherboard"));
        let s = lightdriver_core::Draft::default().lighting();
        let p = effect_reports("argb-2", &s).unwrap();
        assert_eq!(&p[0][..6], &[0xcc, 0x26, 0x40, 0, 0, 0]);
        assert_eq!(&p[0][14..17], &[202, 221, 83]);
        assert_eq!(&p[1][..6], &[0xcc, 0x28, 0x40, 0, 0, 0]);
        assert!(p[1][6..].iter().all(|&v| v == 0));
        assert!(effect_reports("all", &s).is_err());
    }
    #[test]
    fn identity_parser_accepts_known_report_id_quirk_but_rejects_unknown_instances() {
        let mut report = [0u8; 64];
        report[1] = 1;
        report[4..8].copy_from_slice(&[1, 0, 10, 0]);
        report[12..18].copy_from_slice(b"IT8297");
        assert_eq!(firmware_report(&report).unwrap(), "1.0.10.0");
        report[0] = 0xcc;
        assert!(firmware_report(&report).is_ok());
        report[2] = 1;
        assert!(firmware_report(&report).is_err());
        report[2] = 0;
        report[12..18].copy_from_slice(b"IT5702");
        assert!(firmware_report(&report).is_err());
        assert!(firmware_report(&report[..63]).is_err());
    }
    #[test]
    fn off_and_native_effects_have_bounded_color_brightness_and_timing() {
        let mut s = lightdriver_core::Draft::default().lighting();
        s.effect = LightingEffect::Off;
        let p = effect_reports("argb-1", &s).unwrap();
        assert_eq!(p[0][12], 0);
        assert_eq!(&p[0][14..17], &[0, 0, 0]);
        s.effect = LightingEffect::Breathing;
        s.speed = 4;
        let p = effect_reports("argb-1", &s).unwrap();
        assert_eq!(&p[0][22..24], &400u16.to_le_bytes());
        s.effect = LightingEffect::Spectrum;
        s.brightness = 100;
        let p = effect_reports("argb-1", &s).unwrap();
        assert_eq!(p[0][11], 4);
        assert_eq!(p[0][30], 7);
        s.speed = 5;
        assert!(effect_reports("argb-1", &s).is_err());
    }
}
