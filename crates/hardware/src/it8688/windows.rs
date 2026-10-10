//! IT8688 tachometers over shared PawnIO. No vendor identity or header guesses.
use super::{Reading, Snapshot, decode, supported_chip};
use crate::pawnio::Device;
use std::time::Instant;
fn read_slot(
    device: &Device,
    slot: u64,
    headers: [&'static str; 5],
) -> Result<Option<Snapshot>, String> {
    device.execute("ioctl_select_slot", &[slot], 0)?;
    let port = if slot == 0 { 0x2e } else { 0x4e };
    let enter = [0x87, 0x01, 0x55, if slot == 0 { 0x55 } else { 0xaa }];
    for byte in enter {
        if let Err(error) = device.out_port(port, byte) {
            let cleanup = device.exit_config();
            return Err(format!("{error}; configuration exit: {cleanup:?}"));
        }
    }
    let mut original_ldn = None;
    let result = (|| {
        original_ldn = Some(device.sio_read(7)?);
        let id = u16::from_be_bytes([device.sio_read(0x20)?, device.sio_read(0x21)?]);
        if id != 0x8688 {
            return Ok(None);
        }
        let revision = device.sio_read(0x22)? & 0x0f;
        if !supported_chip(id, revision) {
            return Err(format!(
                "IT8688 revision {revision} is unsupported by this monitor."
            ));
        }
        device.select_ldn(4)?;
        if device.sio_read(0x30)? & 1 == 0 {
            return Err("IT8688 monitor is disabled; left unchanged.".into());
        }
        let base = u16::from_be_bytes([device.sio_read(0x60)?, device.sio_read(0x61)?]) & !7;
        if !(0x100..=0xfff0).contains(&base) || (0xcf8..=0xcff).contains(&base) {
            return Err("Invalid IT8688 I/O base.".into());
        }
        // Official LpcIO validates BARs twice before allowing their I/O range.
        // It enumerates LDN selectors; the original selector is restored below.
        device.execute("ioctl_find_bars", &[], 0)?;
        device.select_ldn(4)?;
        // Do not alter bank selector just to obtain a reading.
        let bank = device.ec_read(base, 6)?;
        if bank >> 5 != 0 {
            return Err("IT8688 bank is busy/nonzero; no bank change performed.".into());
        }
        if device.ec_read(base, 0x58)? != 0x90 {
            return Err("IT8688 monitor identity check failed.".into());
        }
        let config = device.ec_read(base, 0)?;
        if config & 0x80 != 0 || config & 1 == 0 {
            return Err("IT8688 monitoring is inactive; left unchanged.".into());
        }
        let main = device.ec_read(base, 0x13)?;
        let mode = device.ec_read(base, 0x0c)?;
        let low = [0x0d, 0x0e, 0x0f, 0x80, 0x82];
        let high = [0x18, 0x19, 0x1a, 0x81, 0x83];
        let mut readings = Vec::new();
        for channel in 0..5 {
            let mut stable = None;
            // Reject an observed high-byte rollover rather than producing a spurious RPM.
            for _ in 0..3 {
                let h1 = device.ec_read(base, high[channel])?;
                let l = device.ec_read(base, low[channel])?;
                let h2 = device.ec_read(base, high[channel])?;
                if h1 == h2 {
                    stable = Some(u16::from_le_bytes([l, h2]));
                    break;
                }
            }
            let (rpm, detail) = stable
                .map(|v| decode(channel, main, mode, v))
                .unwrap_or((None, "Tachometer changed during sample; retry next poll"));
            readings.push(Reading {
                header: headers[channel],
                channel: channel as u8 + 1,
                rpm,
                detail,
            });
        }
        if device.ec_read(base, 6)? != bank {
            return Err("IT8688 bank changed during sample; readings discarded.".into());
        }
        Ok(Some(Snapshot {
            readings,
            source: format!(
                "PawnIO · IT8688 rev {revision} · I/O 0x{base:04X} · {port:#04X} · board header mapping pending physical confirmation"
            ),
            sampled_at: Instant::now(),
        }))
    })();
    let restored = original_ldn.map(|ldn| device.select_ldn(ldn)).transpose();
    let exited = device.exit_config();
    if let Err(error) = restored {
        return Err(format!("Super I/O selector restoration failed: {error}"));
    }
    if let Err(error) = exited {
        return Err(format!("Super I/O configuration exit failed: {error}"));
    }
    result
}
pub fn sample(headers: [&'static str; 5]) -> Result<Snapshot, String> {
    let (device, _lock) = crate::pawnio::open_lpcio()?;
    for slot in 0..2 {
        if let Some(snapshot) = read_slot(&device, slot, headers)? {
            return Ok(snapshot);
        }
    }
    Err("Recognized motherboard, but no supported IT8688 chip was found".into())
}
