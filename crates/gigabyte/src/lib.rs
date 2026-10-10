//! Consolidated GIGABYTE motherboard backend used by the service and fan lab.
//! Capability discovery is passive; RPM/firmware reads are explicit operations.
pub mod firmware;
pub mod profiles;
pub mod rgb;
pub mod rpm;
use lightdriver_core::{
    CommandReceipt, ControlRequest, Device, DeviceAdapter, FanRpmSnapshot, LogStore,
    MotherboardBackend, MotherboardIdentity, MotherboardSupport,
};

#[derive(Default)]
pub struct GigabyteBackend {
    #[cfg(windows)]
    rgb: rgb::GigabyteAdapter,
}
impl MotherboardBackend for GigabyteBackend {
    type FirmwareInspection = firmware::SnapshotInspection;
    fn support(&self, identity: &MotherboardIdentity) -> MotherboardSupport {
        profiles::support(identity)
    }
    fn inspect_firmware(&self) -> Self::FirmwareInspection {
        firmware::inspect_snapshot()
    }
    fn sample_fans(&self) -> Result<FanRpmSnapshot, String> {
        rpm::sample_live()
    }
}
impl DeviceAdapter for GigabyteBackend {
    fn set_logger(&mut self, logger: LogStore) {
        #[cfg(windows)]
        {
            self.rgb.set_logger(logger);
        }
        #[cfg(not(windows))]
        {
            let _ = logger;
        }
    }
    fn poll(&mut self) -> Vec<Device> {
        // Service polling remains RGB discovery only. Do not silently start
        // privileged firmware reads or PawnIO monitoring on a one-second timer.
        #[cfg(windows)]
        {
            self.rgb.poll()
        }
        #[cfg(not(windows))]
        {
            vec![lightdriver_hardware::inventory::discover_board()]
        }
    }
    fn control(&mut self, request: &ControlRequest) -> CommandReceipt {
        #[cfg(windows)]
        {
            self.rgb.control(request)
        }
        #[cfg(not(windows))]
        {
            CommandReceipt::rejected(request, "Live GIGABYTE RGB requires Windows")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backend_construction_and_capability_discovery_are_passive_and_read_only() {
        let backend = GigabyteBackend::default();
        let identity = firmware::snapshot::Snapshot::synthetic().identity;
        let support = backend.support(&identity);
        assert_eq!(support.vendor_id, "gigabyte");
        assert!(support.rgb_layout.is_some() && support.rpm_profile.is_some());
        assert!(support.firmware_inspection_profile.is_some());
        assert!(
            !support.firmware_save
                && !support.immediate_apply
                && !support.reboot_retention_verified
        );
    }
}
