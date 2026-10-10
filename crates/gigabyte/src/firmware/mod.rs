//! GIGABYTE Smart Fan reference format, inspection and private recovery.
//! No setter, PWM writer or protection bypass exists.
pub mod recovery;
pub mod reference_layout;
mod report;
pub mod snapshot;
use lightdriver_hardware::{
    inventory,
    uefi::{FirmwareReader, FirmwareVariable, SystemFirmwareReader},
};
pub use report::{ProbeError, ProbeReport};
use snapshot::{GigabyteIdentity, Origin, PlatformIdentity, Snapshot};

#[derive(Debug, Clone)]
pub struct Inspection {
    pub report: ProbeReport,
    pub fields: Option<[reference_layout::HeaderSettings; 4]>,
}
#[derive(Debug, Clone)]
pub struct SnapshotInspection {
    pub report: ProbeReport,
    pub snapshot: Option<Snapshot>,
}
pub fn inspect() -> ProbeReport {
    inspect_fields().report
}
pub fn inspect_fields() -> Inspection {
    let read = inspect_snapshot();
    Inspection {
        report: read.report,
        fields: read.snapshot.as_ref().map(|s| s.fields()),
    }
}
pub fn inspect_snapshot() -> SnapshotInspection {
    inspect_with(inventory::read_identity(), &SystemFirmwareReader)
}
fn inspect_with(
    identity: Result<PlatformIdentity, String>,
    reader: &dyn FirmwareReader,
) -> SnapshotInspection {
    let mut report = ProbeReport::new();
    let Ok(identity) = identity else {
        report.variable.status = "inventory_unavailable";
        return SnapshotInspection {
            report,
            snapshot: None,
        };
    };
    // Select a reviewed board/BIOS profile BEFORE transport or privilege access.
    if !identity.reference_match() {
        report.variable.status = "unsupported_board_bios";
        return SnapshotInspection {
            report,
            snapshot: None,
        };
    }
    let result = reader.read(FirmwareVariable {
        name: crate::profiles::VARIABLE_NAME,
        guid: crate::profiles::VARIABLE_GUID,
    });
    report.firmware = result.firmware;
    report.privilege = result.privilege;
    report.variable.status = result.status;
    report.variable.error = result.error;
    report.privilege_restoration = result.restoration;
    let mut snapshot = None;
    // An injectable transport must also satisfy the restoration/read contract.
    if report.variable.status == "read"
        && report.privilege_restoration.error.is_none()
        && matches!(
            report.privilege_restoration.status,
            "restored" | "unchanged"
        )
    {
        if let Some(raw) = result.value {
            report.variable.metadata = Some(report::VariableMetadata::decode(
                raw.bytes(),
                raw.attributes,
            ));
            snapshot =
                Snapshot::from_read(raw.bytes(), raw.attributes, identity, Origin::Live).ok();
        }
        report.custom_curve_decoding = if snapshot.is_some() {
            "partial_reference_fields"
        } else {
            "unavailable"
        };
    }
    SnapshotInspection { report, snapshot }
}
#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_hardware::uefi::{AccessError, AccessStep, RawVariable, ReadOutcome};
    use std::cell::Cell;
    struct FakeReader {
        calls: Cell<usize>,
        malformed: bool,
        restore_failed: bool,
    }
    impl FirmwareReader for FakeReader {
        fn read(&self, variable: FirmwareVariable) -> ReadOutcome {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(variable.name, crate::profiles::VARIABLE_NAME);
            assert_eq!(variable.guid, crate::profiles::VARIABLE_GUID);
            let data = if self.malformed {
                vec![0; 569]
            } else {
                reference_layout::synthetic_payload().to_vec()
            };
            let mut restoration = AccessStep::new("restored");
            if self.restore_failed {
                restoration.status = "restore_failed";
                restoration.error = Some(AccessError {
                    operation: "RestoreTokenPrivilege",
                    win32_code: 5,
                    message: "test".into(),
                });
            }
            ReadOutcome {
                status: "read",
                restoration,
                value: Some(RawVariable::new(data, 7)),
                ..ReadOutcome::default()
            }
        }
    }
    fn reader() -> FakeReader {
        FakeReader {
            calls: Cell::new(0),
            malformed: false,
            restore_failed: false,
        }
    }
    #[test]
    fn unsupported_identity_is_rejected_before_any_transport_or_privilege_call() {
        let transport = reader();
        let mut identity = Snapshot::synthetic().identity;
        identity.bios_version = "F41d".into();
        let r = inspect_with(Ok(identity), &transport);
        assert_eq!(r.report.variable.status, "unsupported_board_bios");
        assert!(r.snapshot.is_none());
        assert_eq!(transport.calls.get(), 0);
        assert!(
            inspect_with(Err("no inventory".into()), &transport)
                .snapshot
                .is_none()
        );
        assert_eq!(transport.calls.get(), 0);
    }
    #[test]
    fn reference_data_survives_transport_but_never_promotes_write_or_retention_support() {
        let transport = reader();
        let r = inspect_with(Ok(Snapshot::synthetic().identity), &transport);
        assert!(r.snapshot.is_some());
        assert_eq!(transport.calls.get(), 1);
        assert_eq!(r.report.custom_curve_decoding, "partial_reference_fields");
        assert_eq!(r.report.firmware_write_permission, "not_probed");
        assert_eq!(r.report.reboot_retention, "not_tested");
    }
    #[test]
    fn corrupt_layout_or_failed_privilege_restoration_never_exposes_a_snapshot() {
        for (malformed, restore_failed) in [(true, false), (false, true)] {
            let transport = FakeReader {
                malformed,
                restore_failed,
                ..reader()
            };
            let r = inspect_with(Ok(Snapshot::synthetic().identity), &transport);
            assert!(r.snapshot.is_none());
        }
    }
}
