//! Private, local firmware research snapshots and offline comparisons.
//! A valid snapshot is evidence of a read, never authorization for a write.
use super::{
    reference_layout::{self, FieldChange, Header, HeaderSettings, LevelPreview},
    report::{VARIABLE_GUID, VARIABLE_NAME},
};
pub use lightdriver_hardware::backup::{sha256, write_new};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE_BYTES: u64 = 16_384;
pub use crate::profiles::GigabyteIdentity;
pub use lightdriver_core::MotherboardIdentity as PlatformIdentity;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Live,
    Synthetic,
}

/// Deliberately omits the payload from Debug and all support-report output.
#[derive(Clone)]
pub struct Snapshot {
    data: [u8; reference_layout::PAYLOAD_LENGTH],
    attributes: u32,
    pub identity: PlatformIdentity,
    pub origin: Origin,
    pub captured_unix_ms: u128,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("identity", &self.identity)
            .field("origin", &self.origin)
            .field("captured_unix_ms", &self.captured_unix_ms)
            .field("sha256", &self.fingerprint())
            .finish()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateFile {
    schema_version: u32,
    purpose: String,
    variable_name: String,
    vendor_guid: String,
    identity: PlatformIdentity,
    origin: Origin,
    captured_unix_ms: u128,
    attributes: u32,
    payload_hex: String,
    payload_sha256: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ByteDifference {
    pub offset: usize,
    pub field: String,
    /// Available to an explicitly enabled verbose view, omitted from normal exports.
    #[serde(skip_serializing)]
    pub before: u8,
    #[serde(skip_serializing)]
    pub after: u8,
}
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub schema_version: u32,
    pub operation: &'static str,
    pub identity: PlatformIdentity,
    pub origin: Origin,
    pub baseline_unix_ms: u128,
    pub current_unix_ms: u128,
    pub baseline_sha256: String,
    pub current_sha256: String,
    pub identical: bool,
    pub changed_bytes: usize,
    pub unidentified_changed_bytes: usize,
    pub differences: Vec<ByteDifference>,
    pub interpretation: &'static str,
}
#[derive(Clone)]
pub struct OfflineCandidate {
    pub preview: LevelPreview,
    pub base_sha256: String,
    pub candidate_sha256: String,
    /// Full length patch built only in memory; no firmware transport exists.
    data: [u8; reference_layout::PAYLOAD_LENGTH],
}
impl std::fmt::Debug for OfflineCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfflineCandidate")
            .field("requested_percent", &self.preview.requested_percent)
            .field("changed_bytes", &self.changed_bytes())
            .field("base_sha256", &self.base_sha256)
            .field("candidate_sha256", &self.candidate_sha256)
            .finish()
    }
}
impl OfflineCandidate {
    pub fn matches_original(&self, snapshot: &Snapshot) -> bool {
        self.base_sha256 == snapshot.fingerprint()
    }
    pub fn changed_bytes(&self) -> usize {
        self.preview.partial_changes.len()
    }
    pub fn data_length(&self) -> usize {
        self.data.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreTarget {
    Initial,
    Factory,
}
impl std::fmt::Display for RestoreTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Initial => "Initial curve",
            Self::Factory => "Factory Normal preset",
        })
    }
}
#[derive(Clone)]
pub struct RestoreCandidate {
    pub target: RestoreTarget,
    pub header: Header,
    pub changes: Vec<FieldChange>,
    pub base_sha256: String,
    pub candidate_sha256: String,
}
impl std::fmt::Debug for RestoreCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreCandidate")
            .field("target", &self.target)
            .field("header", &self.header)
            .field("changed_bytes", &self.changes.len())
            .field("candidate_sha256", &self.candidate_sha256)
            .finish()
    }
}
impl Snapshot {
    pub(crate) fn from_read(
        data: &[u8],
        attributes: u32,
        identity: PlatformIdentity,
        origin: Origin,
    ) -> Result<Self, String> {
        reference_layout::decode(data, attributes)
            .map_err(|e| format!("Unsupported firmware snapshot format: {e:?}"))?;
        if !identity.valid() {
            return Err("Missing or invalid board/BIOS inventory; snapshot not retained".into());
        }
        Ok(Self {
            data: data.try_into().map_err(|_| "Invalid snapshot length")?,
            attributes,
            identity,
            origin,
            captured_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|t| t.as_millis())
                .unwrap_or(0),
        })
    }
    pub fn synthetic() -> Self {
        Self::from_read(
            &reference_layout::synthetic_payload(),
            7,
            PlatformIdentity {
                manufacturer: "Gigabyte Technology Co., Ltd.".into(),
                product: "X570 AORUS ELITE WIFI".into(),
                bios_version: "F41c".into(),
                bios_date: "08/18/2026".into(),
            },
            Origin::Synthetic,
        )
        .expect("synthetic snapshot")
    }
    /// Reference-only restore plan. There is no live firmware-write transport.
    /// Initial restore covers the selected 45-byte header block and two known
    /// per-header tail fields. Shared/global update semantics remain unverified.
    /// Factory restore selects Normal mode only, preserving sensor/PWM/fan-stop
    /// configuration and inactive custom fields. It is not a BIOS-wide reset.
    pub fn restore_candidate(
        &self,
        header: Header,
        target: RestoreTarget,
        initial: Option<&Self>,
    ) -> Result<RestoreCandidate, String> {
        if !self.identity.reference_match() || !Header::CASE.contains(&header) {
            return Err("No reviewed restore profile for this board/BIOS/header".into());
        }
        let mut data = self.data;
        let b = header.block();
        match target {
            RestoreTarget::Factory => {
                if reference_layout::factory_normal_graph(header).is_none() {
                    return Err("Factory reference unavailable".into());
                }
                data[b] = 0; // Reviewed Normal enum. Firmware owns preset synthesis.
            }
            RestoreTarget::Initial => {
                let initial = initial.ok_or("No protected initial snapshot")?;
                self.compare(initial)?; // Require identical inventory, attributes and origin.
                let settings = initial
                    .fields()
                    .into_iter()
                    .find(|s| s.header == header)
                    .ok_or("Unknown header")?;
                if settings.mode_raw > 3 {
                    return Err("Initial mode is outside reviewed enums".into());
                }
                data[b..b + 45].copy_from_slice(&initial.data[b..b + 45]);
                let (mode, stop) = header.tail();
                data[mode] = initial.data[mode];
                data[stop] = initial.data[stop];
            }
        }
        let changes = self
            .data
            .iter()
            .zip(data)
            .enumerate()
            .filter(|(_, (before, after))| **before != *after)
            .map(|(offset, (before, after))| FieldChange {
                offset,
                before: *before,
                after,
            })
            .collect();
        Ok(RestoreCandidate {
            target,
            header,
            changes,
            base_sha256: self.fingerprint(),
            candidate_sha256: sha256(&data),
        })
    }
    pub fn fields(&self) -> [HeaderSettings; 4] {
        reference_layout::decode(&self.data, self.attributes).expect("validated immutable snapshot")
    }
    pub fn fingerprint(&self) -> String {
        sha256(&self.data)
    }
    /// Capture a complete private backup. Never overwrite an existing file.
    /// Digest detects damaged bytes; this is not a signed or authenticated file.
    pub fn save_private(&self, path: &Path) -> Result<(), String> {
        let artifact = PrivateFile {
            schema_version: 1,
            purpose: "private_fan_firmware_research_snapshot".into(),
            variable_name: VARIABLE_NAME.into(),
            vendor_guid: VARIABLE_GUID.into(),
            identity: self.identity.clone(),
            origin: self.origin,
            captured_unix_ms: self.captured_unix_ms,
            attributes: self.attributes,
            payload_hex: self.data.iter().map(|b| format!("{b:02X}")).collect(),
            payload_sha256: self.fingerprint(),
        };
        let bytes = serde_json::to_vec_pretty(&artifact).map_err(|e| e.to_string())?;
        write_new(path, &bytes)
    }
    pub fn load_private(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("Snapshot exceeds the 16 KiB limit".into());
        }
        let file: PrivateFile =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid private snapshot: {e}"))?;
        if file.schema_version != 1
            || file.purpose != "private_fan_firmware_research_snapshot"
            || file.variable_name != VARIABLE_NAME
            || file.vendor_guid != VARIABLE_GUID
        {
            return Err("Unsupported snapshot identity or schema".into());
        }
        if file.payload_hex.len() != reference_layout::PAYLOAD_LENGTH * 2
            || !file.payload_hex.is_ascii()
        {
            return Err("Invalid snapshot payload length".into());
        }
        let mut data = [0u8; reference_layout::PAYLOAD_LENGTH];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&file.payload_hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| "Invalid snapshot hex")?;
        }
        if sha256(&data) != file.payload_sha256 {
            return Err("Snapshot digest does not match; refuse this file".into());
        }
        let mut snapshot = Self::from_read(&data, file.attributes, file.identity, file.origin)?;
        snapshot.captured_unix_ms = file.captured_unix_ms;
        Ok(snapshot)
    }
    /// Compare every byte, including undecoded fields. No firmware API is called.
    pub fn compare(&self, current: &Self) -> Result<Comparison, String> {
        if self.origin != current.origin {
            return Err("Synthetic and live snapshots cannot be compared".into());
        }
        if self.identity != current.identity || self.attributes != current.attributes {
            return Err("Board, BIOS or attributes changed; snapshots are not comparable".into());
        }
        let differences: Vec<_> = self
            .data
            .iter()
            .zip(current.data)
            .enumerate()
            .filter(|(_, (a, b))| **a != *b)
            .map(|(offset, (a, b))| ByteDifference {
                offset,
                before: *a,
                after: b,
                field: field_at(offset),
            })
            .collect();
        let unknown = differences
            .iter()
            .filter(|d| d.field.starts_with("Unidentified"))
            .count();
        Ok(Comparison {
            schema_version: 1,
            operation: "offline_full_variable_comparison",
            identity: self.identity.clone(),
            origin: self.origin,
            baseline_unix_ms: self.captured_unix_ms,
            current_unix_ms: current.captured_unix_ms,
            baseline_sha256: self.fingerprint(),
            current_sha256: current.fingerprint(),
            identical: differences.is_empty(),
            changed_bytes: differences.len(),
            unidentified_changed_bytes: unknown,
            differences,
            interpretation: "Byte comparison only. Identical storage does not prove a write, fan response, BIOS display or reboot retention.",
        })
    }
    /// Offline research patch of identified fields only. Unknown bytes are preserved.
    /// Firmware callbacks/preset semantics remain unverified; this is not a writable codec.
    pub fn candidate(&self, header: Header, percent: u8) -> Result<OfflineCandidate, String> {
        if !self.identity.reference_match() {
            return Err("No reviewed offline profile for this board/BIOS".into());
        }
        let settings = self
            .fields()
            .into_iter()
            .find(|s| s.header == header)
            .ok_or("Unknown header")?;
        let proposal = reference_layout::preview(settings, percent)?;
        let mut data = self.data;
        for FieldChange {
            offset,
            before,
            after,
        } in &proposal.partial_changes
        {
            if data[*offset] != *before {
                return Err("Candidate original bytes no longer match".into());
            }
            data[*offset] = *after;
        }
        // Re-decode checks header/length/attribute invariants; not full firmware validity.
        reference_layout::decode(&data, self.attributes)
            .map_err(|_| "Candidate violates format invariants")?;
        Ok(OfflineCandidate {
            preview: proposal,
            base_sha256: self.fingerprint(),
            candidate_sha256: sha256(&data),
            data,
        })
    }
}
pub fn field_at(offset: usize) -> String {
    if offset < 8 {
        return "Board-image identifier".into();
    }
    if offset < 10 {
        return "Declared payload length".into();
    }
    for header in Header::ALL {
        let b = header.block();
        let name = match offset.checked_sub(b) {
            Some(0) => Some("stored mode"),
            Some(1) => Some("off threshold"),
            Some(2) => Some("start threshold"),
            Some(3) => Some("full threshold"),
            Some(4) => Some("start level"),
            Some(5 | 6) => Some("base coefficient"),
            Some(7) => Some("interval"),
            Some(8) => Some("source"),
            _ => None,
        };
        if let Some(name) = name {
            return format!("{header} · {name}");
        }
        for i in 0..3 {
            let p = b + 9 + i * 4;
            if (p..p + 4).contains(&offset) {
                let name = match offset - p {
                    0 => "temperature",
                    1 => "auxiliary field",
                    _ => "coefficient delta",
                };
                return format!("{header} · segment {} {name}", i + 1);
            }
        }
        let (mode, stop) = header.tail();
        if offset == mode {
            return format!("{header} · PWM/DC");
        }
        if offset == stop {
            return format!("{header} · fan stop");
        }
    }
    "Unidentified firmware field".into()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "lightdriver-snapshot-{label}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn private_backup_round_trips_all_bytes_and_refuses_overwrite() {
        let original = Snapshot::synthetic();
        let path = path("roundtrip");
        original.save_private(&path).unwrap();
        let recovered = Snapshot::load_private(&path).unwrap();
        assert!(original.compare(&recovered).unwrap().identical);
        assert_eq!(original.captured_unix_ms, recovered.captured_unix_ms);
        assert!(original.save_private(&path).is_err());
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn corrupt_truncated_wrong_schema_and_oversized_files_are_rejected() {
        let original = Snapshot::synthetic();
        let path = path("corrupt");
        original.save_private(&path).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        for bad in [
            source.replace("\"schema_version\": 1", "\"schema_version\": 99"),
            source.replace("\"payload_hex\": \"3841", "\"payload_hex\": \"FFFF"),
            source[..30].into(),
            "x".repeat(MAX_FILE_BYTES as usize + 1),
        ] {
            std::fs::write(&path, bad).unwrap();
            assert!(Snapshot::load_private(&path).is_err());
        }
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn comparison_includes_unknown_changes_without_leaking_raw_bytes_to_reports() {
        let before = Snapshot::synthetic();
        let mut after = before.clone();
        after.data[0x68] ^= 1;
        after.data[0x230] = 199;
        let diff = before.compare(&after).unwrap();
        assert_eq!(diff.changed_bytes, 2);
        assert_eq!(diff.unidentified_changed_bytes, 1);
        assert!(diff.differences[0].field.contains("SYS_FAN1"));
        let json = serde_json::to_string(&diff).unwrap();
        assert!(
            !json.contains("\"before\"")
                && !json.contains("\"after\"")
                && !json.contains("payload_hex")
        );
        assert!(!format!("{before:?}").contains("data:"));
        after.origin = Origin::Live;
        assert!(before.compare(&after).is_err());
        after.origin = Origin::Synthetic;
        after.identity.bios_version = "F40".into();
        assert!(before.compare(&after).is_err());
    }
    #[test]
    fn restore_plans_are_header_scoped_and_factory_means_normal_preset() {
        let initial = Snapshot::synthetic();
        let mut current = initial.clone();
        current.data[0x68] = 191;
        current.data[0x85] = 99;
        current.data[0x214] = 1;
        current.data[0x95] = 180; // Other case header must remain untouched.
        let plan = current
            .restore_candidate(Header::SysFan1, RestoreTarget::Initial, Some(&initial))
            .unwrap();
        assert_eq!(plan.changes.len(), 3);
        assert!(
            plan.changes.iter().all(|c| (0x64..0x91).contains(&c.offset)
                || c.offset == 0x214
                || c.offset == 0x21f)
        );
        assert!(
            plan.changes
                .iter()
                .any(|c| c.offset == 0x85 && c.after == initial.data[0x85])
        ); // Unknown per-header field preserved in original.
        let factory = current
            .restore_candidate(Header::SysFan1, RestoreTarget::Factory, None)
            .unwrap();
        assert_eq!(
            factory.changes,
            vec![FieldChange {
                offset: 0x64,
                before: 2,
                after: 0
            }]
        );
        assert!(
            current
                .restore_candidate(Header::CpuFan, RestoreTarget::Factory, None)
                .is_err()
        );
        assert!(
            current
                .restore_candidate(Header::SysFan1, RestoreTarget::Initial, None)
                .is_err()
        );
        let mut other = initial.clone();
        other.identity.bios_version = "F40".into();
        assert!(
            current
                .restore_candidate(Header::SysFan1, RestoreTarget::Initial, Some(&other))
                .is_err()
        );
        let template = reference_layout::factory_normal_graph(Header::SysFan1).unwrap();
        assert_eq!(template.accumulated_coefficients(), [28, 28, 28, 64]);
        assert_eq!(
            reference_layout::manual_graph(template)
                .unwrap()
                .last()
                .unwrap()
                .percent,
            100.0
        );
    }
    #[test]
    fn candidate_preserves_every_byte_outside_identified_fields_and_never_mutates_original() {
        let original = Snapshot::synthetic();
        let fingerprint = original.fingerprint();
        let candidate = original.candidate(Header::SysFan2, 75).unwrap();
        assert!(candidate.matches_original(&original));
        assert_eq!(candidate.data_length(), 569);
        assert_ne!(fingerprint, candidate.candidate_sha256);
        for i in 0..original.data.len() {
            if !candidate
                .preview
                .partial_changes
                .iter()
                .any(|c| c.offset == i)
            {
                assert_eq!(candidate.data[i], original.data[i]);
            }
        }
        assert_eq!(original.fingerprint(), fingerprint);
        assert!(original.candidate(Header::CpuFan, 75).is_err());
        let mut changed = original.clone();
        changed.data[0x230] ^= 1;
        assert!(!candidate.matches_original(&changed));
        changed.identity.product = "X570 AORUS ELITE".into();
        assert!(changed.candidate(Header::SysFan2, 75).is_err());
    }
}
