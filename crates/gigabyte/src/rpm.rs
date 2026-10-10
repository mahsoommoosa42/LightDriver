//! Board-specific channel mapping over the reusable IT8688 monitor.
use crate::{GigabyteBackend, profiles};
pub use lightdriver_core::{FanRpmReading as Reading, FanRpmSnapshot as Snapshot};
use lightdriver_core::{MotherboardBackend, MotherboardIdentity};
pub fn sample() -> Result<Snapshot, String> {
    GigabyteBackend::default().sample_fans()
}
pub fn simulated() -> Snapshot {
    lightdriver_hardware::it8688::simulated(profiles::RPM_HEADERS)
}
pub(crate) fn sample_live() -> Result<Snapshot, String> {
    sample_with(
        lightdriver_hardware::inventory::read_identity(),
        lightdriver_hardware::it8688::sample,
    )
}
fn sample_with(
    identity: Result<MotherboardIdentity, String>,
    monitor: impl FnOnce([&'static str; 5]) -> Result<Snapshot, String>,
) -> Result<Snapshot, String> {
    let identity = identity?;
    if !profiles::rpm_supported(&identity.manufacturer, &identity.product) {
        return Err(format!(
            "No recognized motherboard RPM adapter for {}. No ports were accessed.",
            identity.product
        ));
    }
    let mut snapshot = monitor(profiles::RPM_HEADERS)?;
    snapshot.source = format!("{} · {}", identity.product, snapshot.source);
    Ok(snapshot)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrecognized_board_is_rejected_without_opening_driver_or_ports() {
        let mut identity = crate::firmware::snapshot::Snapshot::synthetic().identity;
        identity.product = "X570 AORUS ELITE WIFI II".into();
        let result = sample_with(Ok(identity), |_| panic!("must not access hardware"));
        assert!(result.unwrap_err().contains("No ports were accessed"));
    }
    #[test]
    fn recognized_profile_passes_only_its_header_map_and_preserves_readings() {
        let identity = crate::firmware::snapshot::Snapshot::synthetic().identity;
        let result = sample_with(Ok(identity), |headers| {
            assert_eq!(headers, profiles::RPM_HEADERS);
            Ok(lightdriver_hardware::it8688::simulated(headers))
        })
        .unwrap();
        assert_eq!(result.readings[1].header, "SYS_FAN1");
        assert_eq!(result.readings[1].channel, 2);
        assert_eq!(result.readings[1].rpm, Some(920));
    }
}
