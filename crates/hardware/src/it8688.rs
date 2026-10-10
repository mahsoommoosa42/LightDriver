//! Reusable IT8688 tachometer monitor; header mapping is supplied by a vendor profile. Never takes over PWM or writes a fan setting.
use std::time::Instant;
#[cfg(windows)]
mod windows;

pub use lightdriver_core::{FanRpmReading as Reading, FanRpmSnapshot as Snapshot};
pub fn sample(headers: [&'static str; 5]) -> Result<Snapshot, String> {
    #[cfg(windows)]
    {
        windows::sample(headers)
    }
    #[cfg(not(windows))]
    {
        let _ = headers;
        Err("Motherboard RPM currently requires Windows and signed PawnIO.".into())
    }
}
pub fn simulated(headers: [&'static str; 5]) -> Snapshot {
    Snapshot {
        readings: headers
            .into_iter()
            .zip([1080, 920, 875, 1600, 1020])
            .enumerate()
            .map(|(channel, (header, rpm))| Reading {
                header,
                channel: channel as u8 + 1,
                rpm: Some(rpm),
                detail: "SIMULATED",
            })
            .collect(),
        source: "SIMULATED · no driver or firmware access".into(),
        sampled_at: Instant::now(),
    }
}
// Register facts and conversion verified against pinned it87 driver; no enabling writes.
#[cfg(any(windows, test))]
fn decode(channel: usize, main: u8, mode: u8, count: u16) -> (Option<u32>, &'static str) {
    let enabled = match channel {
        0..=2 => main & (1 << (channel + 4)) != 0,
        3 => mode & 0x10 != 0,
        4 => mode & 0x20 != 0,
        _ => false,
    };
    if !enabled {
        return (None, "Tachometer disabled; left unchanged");
    }
    // IT8688 has fixed 16-bit tachometers. Register 0x0C low bits are not
    // an 8/16-bit mode switch for this chip (unlike older ITE models).
    match count {
        0 => (None, "Invalid tachometer count"),
        0xffff => (Some(0), "No tach pulses / stopped or disconnected"),
        _ => (Some(675_000 / u32::from(count)), "Live tachometer"),
    }
}
#[cfg(any(windows, test))]
fn supported_chip(id: u16, revision: u8) -> bool {
    id == 0x8688 && matches!(revision, 1 | 2)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_16bit_counters_sentinels_and_enable_bits_are_distinct() {
        assert_eq!(decode(1, 0x70, 7, 750), (Some(900), "Live tachometer"));
        assert_eq!(decode(0, 0x70, 7, 0).0, None);
        assert_eq!(decode(0, 0x70, 7, 0xffff).0, Some(0));
        assert_eq!(decode(2, 0x30, 7, 750).0, None); // disabled fan3
        assert_eq!(decode(1, 0x70, 0, 750).0, Some(900)); // fixed 16-bit fan2
        assert_eq!(decode(3, 0, 0x10, 750).0, Some(900));
        assert_eq!(decode(4, 0, 0x20, 750).0, Some(900));
        assert_eq!(decode(5, 0x70, 0xff, 750).0, None); // unmapped
    }
    #[test]
    fn only_reviewed_ite_revisions_are_accepted() {
        assert!(supported_chip(0x8688, 1));
        assert!(supported_chip(0x8688, 2));
        for revision in [0, 3, 15, 255] {
            assert!(!supported_chip(0x8688, revision));
        }
        assert!(!supported_chip(0x8689, 1));
        assert!(!supported_chip(0xffff, 1));
    }
}
