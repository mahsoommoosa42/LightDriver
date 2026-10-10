//! Bounded read-only UEFI transport. Vendor names/layouts belong in adapters.
use serde::Serialize;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Serialize)]
pub struct AccessError {
    pub operation: &'static str,
    pub win32_code: u32,
    pub message: String,
}
impl AccessError {
    #[cfg(windows)]
    pub fn windows(operation: &'static str, code: u32) -> Self {
        Self {
            operation,
            win32_code: code,
            message: std::io::Error::from_raw_os_error(code as i32).to_string(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessStep {
    pub status: &'static str,
    pub error: Option<AccessError>,
}
impl AccessStep {
    pub fn new(status: &'static str) -> Self {
        Self {
            status,
            error: None,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct FirmwareVariable {
    pub name: &'static str,
    pub guid: &'static str,
}
/// Raw bytes are intentionally excluded from Debug and serialization.
/// Only an explicit adapter inspection can retain/export its private snapshot.
pub struct RawVariable {
    data: Vec<u8>,
    pub attributes: u32,
}
impl RawVariable {
    pub fn new(data: Vec<u8>, attributes: u32) -> Self {
        Self { data, attributes }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}
impl std::fmt::Debug for RawVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawVariable")
            .field("length", &self.data.len())
            .field("attributes", &self.attributes)
            .finish()
    }
}
impl Drop for RawVariable {
    fn drop(&mut self) {
        self.data.fill(0);
    }
}
#[derive(Debug)]
pub struct ReadOutcome {
    pub firmware: AccessStep,
    pub privilege: AccessStep,
    pub status: &'static str,
    pub error: Option<AccessError>,
    pub restoration: AccessStep,
    pub value: Option<RawVariable>,
}
impl Default for ReadOutcome {
    fn default() -> Self {
        Self {
            firmware: AccessStep::new("unknown"),
            privilege: AccessStep::new("not_requested"),
            status: "not_read",
            error: None,
            restoration: AccessStep::new("not_requested"),
            value: None,
        }
    }
}
pub trait FirmwareReader {
    fn read(&self, variable: FirmwareVariable) -> ReadOutcome;
}
#[derive(Default)]
pub struct SystemFirmwareReader;
impl FirmwareReader for SystemFirmwareReader {
    fn read(&self, variable: FirmwareVariable) -> ReadOutcome {
        #[cfg(windows)]
        {
            windows::read(variable)
        }
        #[cfg(not(windows))]
        {
            let _ = variable;
            ReadOutcome {
                firmware: AccessStep::new("unsupported_platform"),
                status: "unsupported_platform",
                ..ReadOutcome::default()
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_transport_data_cannot_escape_through_debug_output() {
        let raw = RawVariable::new(b"DO_NOT_LOG_FIRMWARE_DATA".to_vec(), 7);
        let debug = format!("{raw:?}");
        assert!(debug.contains("length"));
        assert!(!debug.contains("DO_NOT_LOG") && !debug.contains("68, 79"));
    }
}
