//! Reviewed profiles are compiled adapter data, not executable user imports.
//! RGB, tachometer mapping and firmware version gates are independent.
use lightdriver_core::{MotherboardIdentity, MotherboardSupport};

pub const VENDOR_ID: &str = "gigabyte";
pub const FIRMWARE_PROFILE_ID: &str = "gigabyte.x570-aorus-elite-wifi.f41c";
pub const RPM_PROFILE_ID: &str = "gigabyte.x570-aorus-elite-wifi.it8688";
pub const RPM_HEADERS: [&str; 5] = ["CPU_FAN", "SYS_FAN1", "SYS_FAN2", "PCH_FAN", "CPU_OPT"];
pub const VARIABLE_NAME: &str = "GcSensorVarName";
pub const VARIABLE_GUID: &str = "{5489A3E4-C745-11EA-87D0-0242AC130003}";
pub const REFERENCE_LENGTH: usize = 569;
pub const REFERENCE_BOARD_ID: &[u8; 8] = b"8AMTS010";

pub fn rgb_layout(product: &str) -> bool {
    let normalized = product.trim().to_ascii_uppercase();
    matches!(
        normalized.trim_end_matches("-CF"),
        "X570 AORUS ELITE"
            | "X570 AORUS ELITE WIFI"
            | "X570 AORUS PRO"
            | "X570 AORUS PRO WIFI"
            | "X570 AORUS ULTRA"
    )
}
pub fn rpm_supported(manufacturer: &str, product: &str) -> bool {
    manufacturer
        .trim()
        .eq_ignore_ascii_case("Gigabyte Technology Co., Ltd.")
        && product.trim().eq_ignore_ascii_case("X570 AORUS ELITE WIFI")
}
pub trait GigabyteIdentity {
    fn reference_match(&self) -> bool;
}
impl GigabyteIdentity for MotherboardIdentity {
    fn reference_match(&self) -> bool {
        rpm_supported(&self.manufacturer, &self.product)
            && self.bios_version.trim() == "F41c"
            && self.bios_date.trim() == "08/18/2026"
    }
}
pub fn support(identity: &MotherboardIdentity) -> MotherboardSupport {
    let vendor = identity
        .manufacturer
        .trim()
        .eq_ignore_ascii_case("Gigabyte Technology Co., Ltd.");
    MotherboardSupport {
        vendor_id: VENDOR_ID,
        rgb_layout: (vendor && rgb_layout(&identity.product)).then_some("gigabyte.ite8297.x570"),
        rpm_profile: rpm_supported(&identity.manufacturer, &identity.product)
            .then_some(RPM_PROFILE_ID),
        firmware_inspection_profile: identity.reference_match().then_some(FIRMWARE_PROFILE_ID),
        // Partial decoding/read success never implies writable firmware support.
        firmware_save: false,
        immediate_apply: false,
        reboot_retention_verified: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> MotherboardIdentity {
        MotherboardIdentity {
            manufacturer: "Gigabyte Technology Co., Ltd.".into(),
            product: "X570 AORUS ELITE WIFI".into(),
            bios_version: "F41c".into(),
            bios_date: "08/18/2026".into(),
        }
    }
    #[test]
    fn bios_updates_do_not_inherit_firmware_support_from_rgb_or_chip_mapping() {
        let mut identity = reference();
        assert!(support(&identity).firmware_inspection_profile.is_some());
        for version in ["F40", "F41", "F41d", "", "F41c-extra"] {
            identity.bios_version = version.into();
            let s = support(&identity);
            assert!(s.rgb_layout.is_some() && s.rpm_profile.is_some());
            assert!(s.firmware_inspection_profile.is_none());
            assert!(!s.firmware_save && !s.immediate_apply && !s.reboot_retention_verified);
        }
        identity = reference();
        identity.bios_date = "08/19/2026".into();
        assert!(!identity.reference_match());
    }
    #[test]
    fn related_boards_and_other_vendors_do_not_inherit_firmware_or_header_mapping() {
        for board in [
            "X570 AORUS PRO",
            "X570 AORUS ELITE",
            "X570 AORUS ELITE WIFI II",
        ] {
            let mut identity = reference();
            identity.product = board.into();
            let s = support(&identity);
            assert!(s.rpm_profile.is_none() && s.firmware_inspection_profile.is_none());
        }
        let mut identity = reference();
        identity.manufacturer = "ASUS".into();
        let s = support(&identity);
        assert!(
            s.rgb_layout.is_none()
                && s.rpm_profile.is_none()
                && s.firmware_inspection_profile.is_none()
        );
    }
}
