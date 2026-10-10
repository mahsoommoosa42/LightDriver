use serde::Serialize;

pub const VARIABLE_NAME: &str = "GcSensorVarName";
pub const VARIABLE_GUID: &str = "{5489A3E4-C745-11EA-87D0-0242AC130003}";
#[cfg(any(windows, test))]
pub const REFERENCE_LENGTH: usize = 569;
#[cfg(any(windows, test))]
const REFERENCE_BOARD_ID: &[u8; 8] = b"8AMTS010";

#[derive(Serialize)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub probe_version: &'static str,
    pub access_mode: &'static str,
    pub variable_name: &'static str,
    pub vendor_guid: &'static str,
    pub firmware: StepStatus,
    pub privilege: StepStatus,
    pub variable: VariableRead,
    pub privilege_restoration: StepStatus,
    pub custom_curve_decoding: &'static str,
    pub firmware_write_permission: &'static str,
    pub reboot_retention: &'static str,
}

impl ProbeReport {
    pub fn new() -> Self {
        Self {
            schema_version: 1,
            probe_version: env!("CARGO_PKG_VERSION"),
            access_mode: "read_only",
            variable_name: VARIABLE_NAME,
            vendor_guid: VARIABLE_GUID,
            firmware: StepStatus::new("unknown"),
            privilege: StepStatus::new("not_requested"),
            variable: VariableRead {
                status: "not_read",
                error: None,
                metadata: None,
            },
            privilege_restoration: StepStatus::new("not_requested"),
            custom_curve_decoding: "not_implemented",
            firmware_write_permission: "not_probed",
            reboot_retention: "not_tested",
        }
    }

    #[cfg(not(windows))]
    pub fn unsupported() -> Self {
        let mut report = Self::new();
        report.firmware.status = "unsupported_platform";
        report.variable.status = "unsupported_platform";
        report
    }
}

#[derive(Serialize)]
pub struct StepStatus {
    pub status: &'static str,
    pub error: Option<ProbeError>,
}
impl StepStatus {
    pub fn new(status: &'static str) -> Self {
        Self {
            status,
            error: None,
        }
    }
}

#[derive(Serialize)]
pub struct ProbeError {
    pub operation: &'static str,
    pub win32_code: u32,
    pub message: String,
}
impl ProbeError {
    #[cfg(windows)]
    pub fn windows(operation: &'static str, code: u32) -> Self {
        Self {
            operation,
            win32_code: code,
            message: std::io::Error::from_raw_os_error(code as i32).to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct VariableRead {
    pub status: &'static str,
    pub error: Option<ProbeError>,
    pub metadata: Option<VariableMetadata>,
}

#[derive(Serialize)]
pub struct VariableMetadata {
    pub length_bytes: usize,
    pub attributes_hex: String,
    pub nonvolatile: bool,
    pub boot_service_access: bool,
    pub runtime_access: bool,
    pub additional_attribute_bits_hex: String,
    pub reference_attributes_match: bool,
    pub header: HeaderMetadata,
}

#[derive(Serialize)]
pub struct HeaderMetadata {
    pub board_image_id: Option<String>,
    pub declared_length_bytes: Option<u16>,
    pub declared_length_matches: bool,
    pub reference_header_matches: bool,
}

#[cfg(any(windows, test))]
impl VariableMetadata {
    pub fn decode(data: &[u8], attributes: u32) -> Self {
        let board_image_id = data
            .get(..8)
            .filter(|bytes| {
                bytes
                    .iter()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            })
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("checked ASCII"));
        let declared_length_bytes = data
            .get(8..10)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]));
        let declared_length_matches =
            declared_length_bytes.is_some_and(|n| usize::from(n) == data.len());
        let reference_header_matches = data.get(..8) == Some(REFERENCE_BOARD_ID.as_slice())
            && declared_length_matches
            && data.len() == REFERENCE_LENGTH;
        Self {
            length_bytes: data.len(),
            attributes_hex: format!("0x{attributes:08X}"),
            nonvolatile: attributes & 1 != 0,
            boot_service_access: attributes & 2 != 0,
            runtime_access: attributes & 4 != 0,
            additional_attribute_bits_hex: format!("0x{:08X}", attributes & !7),
            reference_attributes_match: attributes == 7,
            header: HeaderMetadata {
                board_image_id,
                declared_length_bytes,
                declared_length_matches,
                reference_header_matches,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_payload() -> Vec<u8> {
        let mut data = vec![0; REFERENCE_LENGTH];
        data[..8].copy_from_slice(REFERENCE_BOARD_ID);
        data[8..10].copy_from_slice(&(REFERENCE_LENGTH as u16).to_le_bytes());
        data
    }

    #[test]
    fn truncated_or_factory_default_header_is_not_a_reference_match() {
        for length in 0..10 {
            assert!(
                !VariableMetadata::decode(&vec![0; length], 7)
                    .header
                    .reference_header_matches
            );
        }
        let metadata = VariableMetadata::decode(&vec![0; REFERENCE_LENGTH], 7);
        assert!(metadata.header.board_image_id.is_none());
        assert!(!metadata.header.reference_header_matches);
    }

    #[test]
    fn mismatched_length_or_other_board_header_is_not_accepted() {
        let mut data = reference_payload();
        assert!(
            VariableMetadata::decode(&data, 7)
                .header
                .reference_header_matches
        );
        data.push(0);
        assert!(
            !VariableMetadata::decode(&data, 7)
                .header
                .reference_header_matches
        );
        data.pop();
        data[0] = b'9';
        assert!(
            !VariableMetadata::decode(&data, 7)
                .header
                .reference_header_matches
        );
    }

    #[test]
    fn unknown_attributes_are_preserved_and_read_does_not_claim_write_access() {
        let mut report = ProbeReport::new();
        report.variable.status = "read";
        report.variable.metadata = Some(VariableMetadata::decode(&reference_payload(), 0x27));
        let metadata = report.variable.metadata.as_ref().unwrap();
        assert!(!metadata.reference_attributes_match);
        assert_eq!(metadata.additional_attribute_bits_hex, "0x00000020");
        assert_eq!(report.firmware_write_permission, "not_probed");
        assert_eq!(report.reboot_retention, "not_tested");
    }

    #[test]
    fn exported_metadata_omits_raw_payload() {
        let mut data = reference_payload();
        let marker = b"RAW_PAYLOAD_MUST_STAY_PRIVATE";
        data[40..40 + marker.len()].copy_from_slice(marker);
        let json = serde_json::to_string(&VariableMetadata::decode(&data, 7)).unwrap();
        assert!(!json.contains(std::str::from_utf8(marker).unwrap()));
        assert!(!json.contains("payload"));
        assert!(json.contains("8AMTS010"));
    }
}
