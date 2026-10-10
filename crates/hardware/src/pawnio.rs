//! Independent Rust implementation of PawnIO's documented device IOCTL interface.
//! Uses an external, hash-pinned official signed LpcIO module. No DLL search path.
use std::{ffi::c_void, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0,
    },
    Security::Cryptography::{
        BCRYPT_ALG_HANDLE, BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider,
    },
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::{
        IO::DeviceIoControl,
        Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
};
const LOAD: u32 = (41394 << 16) | (0x821 << 2);
const EXECUTE: u32 = (41394 << 16) | (0x841 << 2);
const MODULE_HASH: [u8; 32] = [
    0xb3, 0x89, 0x6a, 0x1c, 0xab, 0x0d, 0x80, 0x8f, 0xca, 0x31, 0xfe, 0x2e, 0xbc, 0xae, 0x04, 0x5d,
    0x59, 0xda, 0xc6, 0x90, 0xda, 0x87, 0xb1, 0x7c, 0x85, 0x8b, 0xb8, 0xda, 0x35, 0x7e, 0xb4, 0x5e,
];
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn last_error(operation: &str) -> String {
    // SAFETY: reads this thread's last-error code immediately after a failed API.
    format!("{operation}: Win32 {}", unsafe { GetLastError() })
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this object and closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub(crate) struct IsaLock(Handle);
impl IsaLock {
    fn acquire() -> Result<Self, String> {
        let name = wide("Global\\Access_ISABUS.HTP.Method");
        // SAFETY: valid NUL-terminated name; default security descriptor.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(last_error("ISA mutex creation"));
        }
        let owned = Handle(handle);
        // SAFETY: handle remains valid for the finite wait.
        let wait = unsafe { WaitForSingleObject(handle, 250) };
        match wait {
            WAIT_OBJECT_0 => Ok(Self(owned)),
            WAIT_ABANDONED => {
                // An abandoned owner may have left the chip's selector/config state uncertain.
                // SAFETY: WAIT_ABANDONED confers ownership, which we release before returning.
                unsafe { ReleaseMutex(handle); }
                Err("ISA mutex was abandoned. Monitoring stopped; restart other sensor utilities before retrying.".into())
            }
            _ => Err("ISA access is busy or unavailable. Close competing motherboard sensor utilities and retry.".into()),
        }
    }
}
impl Drop for IsaLock {
    fn drop(&mut self) {
        // SAFETY: this thread acquired this mutex; the Handle closes it after release.
        unsafe {
            ReleaseMutex(self.0.0);
        }
    }
}
pub(crate) struct Device(Handle);
impl Device {
    fn open() -> Result<Self, String> {
        let name = wide("\\\\?\\GLOBALROOT\\Device\\PawnIO");
        // SAFETY: stable string, no optional structures, synchronous existing-device handle.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0xc0000000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "{} · Install the official signed PawnIO driver, then run this debug app as administrator if access is denied.",
                last_error("PawnIO open")
            ));
        }
        Ok(Self(Handle(handle)))
    }
    fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<usize, String> {
        let mut written = 0;
        // SAFETY: both slice capacities match the passed lengths, owned synchronous handle.
        let ok = unsafe {
            DeviceIoControl(
                self.0.0,
                code,
                input.as_ptr().cast::<c_void>(),
                input.len() as u32,
                output.as_mut_ptr().cast::<c_void>(),
                output.len() as u32,
                &mut written,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(last_error("PawnIO request"));
        }
        if written as usize > output.len() {
            return Err("PawnIO returned an invalid response length.".into());
        }
        Ok(written as usize)
    }
    pub(crate) fn execute(
        &self,
        name: &str,
        input: &[u64],
        count: usize,
    ) -> Result<Vec<u64>, String> {
        if name.len() >= 32 || !name.is_ascii() {
            return Err("Invalid internal module function.".into());
        }
        let mut request = vec![0u8; 32 + input.len() * 8];
        request[..name.len()].copy_from_slice(name.as_bytes());
        for (i, v) in input.iter().enumerate() {
            request[32 + i * 8..40 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut output = vec![0; count * 8];
        let len = self.ioctl(EXECUTE, &request, &mut output)?;
        if len != output.len() {
            return Err(format!("{name}: incomplete module response"));
        }
        Ok(output
            .as_chunks::<8>()
            .0
            .iter()
            .map(|s| u64::from_le_bytes(*s))
            .collect())
    }
    pub(crate) fn in_port(&self, port: u16) -> Result<u8, String> {
        let value = self.execute("ioctl_pio_inb", &[u64::from(port)], 1)?[0];
        u8::try_from(value).map_err(|_| "Invalid port byte from module.".into())
    }
    pub(crate) fn out_port(&self, port: u16, value: u8) -> Result<(), String> {
        self.execute("ioctl_pio_outb", &[u64::from(port), u64::from(value)], 0)?;
        Ok(())
    }
    pub(crate) fn sio_read(&self, register: u8) -> Result<u8, String> {
        let v = self.execute("ioctl_superio_inb", &[u64::from(register)], 1)?[0];
        u8::try_from(v).map_err(|_| "Invalid Super I/O byte from module.".into())
    }
    pub(crate) fn select_ldn(&self, ldn: u8) -> Result<(), String> {
        // Register 7 selects a logical device; it does not program fan settings.
        self.execute("ioctl_superio_outb", &[7, u64::from(ldn)], 0)?;
        Ok(())
    }
    pub(crate) fn exit_config(&self) -> Result<(), String> {
        self.execute("ioctl_superio_outb", &[2, 2], 0)?;
        Ok(())
    }
    pub(crate) fn ec_read(&self, base: u16, register: u8) -> Result<u8, String> {
        // Only the address/selector port is written. The EC data port is read-only here.
        self.out_port(base + 5, register)?;
        self.in_port(base + 6)
    }
}
fn module_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    for dir in exe.parent().into_iter().flat_map(|p| p.ancestors().take(3)) {
        let candidate = dir.join("FanDebug-support").join("LpcIO.bin");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("Missing official LpcIO module. Place the pinned 0.2.11 LpcIO.bin in FanDebug-support next to the launcher (see fan-debug README).".into())
}
fn verify_module(bytes: &mut [u8]) -> Result<(), String> {
    if bytes.len() != 18076 {
        return Err("Unexpected LpcIO module size; expected release 0.2.11.".into());
    }
    let algorithm = wide("SHA256");
    let mut handle: BCRYPT_ALG_HANDLE = ptr::null_mut();
    // SAFETY: initialized out-pointer and valid NUL-terminated algorithm ID.
    let status =
        unsafe { BCryptOpenAlgorithmProvider(&mut handle, algorithm.as_ptr(), ptr::null(), 0) };
    if status < 0 {
        return Err(format!("SHA256 provider unavailable: 0x{status:08X}"));
    }
    let mut digest = [0u8; 32];
    // SAFETY: valid provider handle, no secret, byte slice and output lengths agree.
    let status = unsafe {
        BCryptHash(
            handle,
            ptr::null_mut(),
            0,
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };
    // SAFETY: owned algorithm handle, no further use.
    unsafe {
        BCryptCloseAlgorithmProvider(handle, 0);
    }
    if status < 0 {
        return Err(format!("Module digest failed: 0x{status:08X}"));
    }
    if digest != MODULE_HASH {
        return Err("LpcIO module hash does not match the reviewed official release.".into());
    }
    Ok(())
}
pub(crate) fn open_lpcio() -> Result<(Device, IsaLock), String> {
    let device = Device::open()?;
    let mut blob = std::fs::read(module_path()?).map_err(|e| e.to_string())?;
    verify_module(&mut blob)?;
    device.ioctl(LOAD, &blob, &mut [])?; // The driver verifies the module signature.
    let lock = IsaLock::acquire()?;
    Ok((device, lock))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_hash_rejects_wrong_size_and_modified_official_sized_data() {
        assert!(verify_module(&mut []).unwrap_err().contains("size"));
        assert!(
            verify_module(&mut vec![0; 18076])
                .unwrap_err()
                .contains("hash")
        );
    }
}
