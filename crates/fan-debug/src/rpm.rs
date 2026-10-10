//! Independent tachometer monitor. Never takes over PWM or writes a fan setting.
use std::time::Instant;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone)]
pub struct Reading {
    pub header: &'static str,
    pub channel: u8,
    pub rpm: Option<u32>,
    pub detail: &'static str,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub readings: Vec<Reading>,
    pub source: String,
    pub sampled_at: Instant,
}
pub fn sample() -> Result<Snapshot, String> {
    #[cfg(windows)]
    {
        windows::sample()
    }
    #[cfg(not(windows))]
    {
        Err("Motherboard RPM currently requires Windows and signed PawnIO.".into())
    }
}
pub fn simulated() -> Snapshot {
    Snapshot {
        readings: [
            ("CPU_FAN", 1, 1080),
            ("SYS_FAN1", 2, 920),
            ("SYS_FAN2", 3, 875),
            ("PCH_FAN", 4, 1600),
            ("CPU_OPT", 5, 1020),
        ]
        .into_iter()
        .map(|(header, channel, rpm)| Reading {
            header,
            channel,
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
    if channel < 3 && mode & (1 << channel) == 0 {
        return (None, "8-bit tachometer mode is unsupported; left unchanged");
    }
    match count {
        0 => (None, "Invalid tachometer count"),
        0xffff => (Some(0), "No tach pulses / stopped or disconnected"),
        _ => (Some(675_000 / u32::from(count)), "Live tachometer"),
    }
}
#[cfg(any(windows, test))]
fn supported_board(manufacturer: &str, product: &str) -> bool {
    manufacturer.to_ascii_lowercase().contains("gigabyte")
        && product.trim().eq_ignore_ascii_case("X570 AORUS ELITE WIFI")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sentinels_modes_and_channel_enable_bits_are_distinct() {
        assert_eq!(decode(1, 0x70, 7, 750), (Some(900), "Live tachometer"));
        assert_eq!(decode(0, 0x70, 7, 0).0, None);
        assert_eq!(decode(0, 0x70, 7, 0xffff).0, Some(0));
        assert_eq!(decode(2, 0x30, 7, 750).0, None); // disabled fan3
        assert_eq!(decode(1, 0x70, 5, 750).0, None); // 8-bit fan2
        assert_eq!(decode(3, 0, 0x10, 750).0, Some(900));
        assert_eq!(decode(4, 0, 0x20, 750).0, Some(900));
        assert_eq!(decode(5, 0x70, 0xff, 750).0, None); // unmapped
    }
    #[test]
    fn similar_board_names_do_not_inherit_header_mapping() {
        assert!(supported_board(
            "Gigabyte Technology Co., Ltd.",
            "X570 AORUS ELITE WIFI"
        ));
        assert!(!supported_board("Gigabyte", "X570 AORUS ELITE"));
        assert!(!supported_board("ASUS", "X570 AORUS ELITE WIFI"));
        assert!(!supported_board("Gigabyte", "X570 AORUS ELITE WIFI II"));
    }
}
