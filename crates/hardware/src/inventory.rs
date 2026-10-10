//! Discovery is independent of user wiring and future vendor SDK adapters.
use lightdriver_core::{Device, DeviceStatus, motherboard_target};
pub fn demo_board() -> Device {
    let mut board = motherboard_target();
    board.name = "Motherboard · Demo".into();
    board.connection = "Simulated target".into();
    board.detail = "Simulated motherboard. No fan or ARGB backend is opened.".into();
    board
}
pub fn discover_board() -> Device {
    let mut board = motherboard_target();
    #[cfg(windows)]
    {
        let manufacturer = registry_string("BaseBoardManufacturer").ok();
        let product = registry_string("BaseBoardProduct").ok();
        if let Some(name) = product.filter(|s| !s.trim().is_empty()) {
            board.name = name;
            board.connection = format!(
                "{} · Windows board inventory",
                manufacturer.as_deref().unwrap_or("Motherboard")
            );
            board.status = DeviceStatus::Detected;
        }
    }
    board
}

#[cfg(windows)]
pub fn registry_string(name: &str) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let key = wide("HARDWARE\\DESCRIPTION\\System\\BIOS");
    let name = wide(name);
    let mut buffer = [0u16; 256];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: fixed inventory key, bounded writable UTF-16 buffer, REG_SZ only.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if code != 0
        || size == 0
        || size as usize > std::mem::size_of_val(&buffer)
        || !size.is_multiple_of(2)
    {
        return Err(format!("Board/BIOS inventory unavailable: Win32 {code}"));
    }
    let count = size as usize / 2;
    let end = buffer[..count]
        .iter()
        .position(|&c| c == 0)
        .ok_or("Unterminated inventory value")?;
    String::from_utf16(&buffer[..end]).map_err(|_| "Invalid inventory value".into())
}
#[cfg(windows)]
pub fn read_identity() -> Result<lightdriver_core::MotherboardIdentity, String> {
    Ok(lightdriver_core::MotherboardIdentity {
        manufacturer: registry_string("BaseBoardManufacturer")?,
        product: registry_string("BaseBoardProduct")?,
        bios_version: registry_string("BIOSVersion")?,
        bios_date: registry_string("BIOSReleaseDate")?,
    })
}
#[cfg(not(windows))]
pub fn read_identity() -> Result<lightdriver_core::MotherboardIdentity, String> {
    Err("Motherboard inventory currently requires Windows".into())
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
