//! Vendor-neutral identity, capability and read interfaces.
//! Persistent saving and immediate application are distinct capabilities.
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotherboardIdentity {
    pub manufacturer: String,
    pub product: String,
    pub bios_version: String,
    pub bios_date: String,
}
impl MotherboardIdentity {
    pub fn valid(&self) -> bool {
        [
            &self.manufacturer,
            &self.product,
            &self.bios_version,
            &self.bios_date,
        ]
        .iter()
        .all(|s| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotherboardSupport {
    pub vendor_id: &'static str,
    pub rgb_layout: Option<&'static str>,
    pub rpm_profile: Option<&'static str>,
    pub firmware_inspection_profile: Option<&'static str>,
    pub firmware_save: bool,
    pub immediate_apply: bool,
    pub reboot_retention_verified: bool,
}
#[derive(Debug, Clone)]
pub struct FanRpmReading {
    pub header: &'static str,
    pub channel: u8,
    pub rpm: Option<u32>,
    pub detail: &'static str,
}
#[derive(Debug, Clone)]
pub struct FanRpmSnapshot {
    pub readings: Vec<FanRpmReading>,
    pub source: String,
    pub sampled_at: Instant,
}
/// Each firmware family keeps its own typed inspection/codec. The common
/// service contract cannot accidentally apply another vendor's raw format.
/// Construction and support discovery must not open hardware or elevate.
pub trait MotherboardBackend: Send {
    type FirmwareInspection;
    fn support(&self, identity: &MotherboardIdentity) -> MotherboardSupport;
    fn inspect_firmware(&self) -> Self::FirmwareInspection;
    fn sample_fans(&self) -> Result<FanRpmSnapshot, String>;
}
