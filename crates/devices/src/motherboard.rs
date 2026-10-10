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
        let manufacturer = registry_string("BaseBoardManufacturer");
        let product = registry_string("BaseBoardProduct");
        if let Some(name) = product.filter(|s| !s.trim().is_empty()) {
            board.name = name;
            board.connection = format!(
                "{} · Windows board inventory",
                manufacturer.as_deref().unwrap_or("Motherboard")
            );
            board.status = DeviceStatus::Detected;
            if manufacturer
                .as_deref()
                .is_some_and(|v| v.to_ascii_lowercase().contains("gigabyte"))
            {
                board.capabilities[1].detail = "GIGABYTE board detected. Case RGB awaits a supported vendor SDK with documented channels; the legacy 2019 x86 SDK is not loaded by this x64 service.".into();
            }
        }
    }
    board
}
#[cfg(windows)]
fn registry_string(name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let key = wide("HARDWARE\\DESCRIPTION\\System\\BIOS");
    let name = wide(name);
    let mut bytes = [0u16; 256];
    let mut size = std::mem::size_of_val(&bytes) as u32;
    // SAFETY: read-only registry value, fixed UTF-16 buffer and matching byte capacity.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 || size as usize > std::mem::size_of_val(&bytes) {
        return None;
    }
    let end = bytes.iter().position(|&v| v == 0).unwrap_or(bytes.len());
    Some(String::from_utf16_lossy(&bytes[..end]))
}
