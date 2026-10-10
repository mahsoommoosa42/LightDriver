use crate::report::{
    ProbeError, ProbeReport, StepStatus, VARIABLE_GUID, VARIABLE_NAME, VariableMetadata,
};
use std::{mem, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID, SetLastError},
    Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::{
        SystemInformation::{
            FirmwareTypeBios as FIRMWARE_TYPE_BIOS, FirmwareTypeUefi as FIRMWARE_TYPE_UEFI,
            GetFirmwareType,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
        WindowsProgramming::GetFirmwareEnvironmentVariableExW,
    },
};

const READ_BUFFER_BYTES: usize = 65_536;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn last_error(operation: &'static str) -> ProbeError {
    // SAFETY: reads the calling thread's error immediately after a failed API call.
    ProbeError::windows(operation, unsafe { GetLastError() })
}

/// Owns the process token and restores only the privilege this probe changed.
/// It does not add account privileges or change Windows policy.
struct PrivilegeGuard {
    token: HANDLE,
    previous: TOKEN_PRIVILEGES,
    restore_pending: bool,
}

impl PrivilegeGuard {
    fn enable() -> Result<Self, ProbeError> {
        let mut token = ptr::null_mut();
        // SAFETY: valid output handle, pseudo-handle for this process, documented flags.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                &mut token,
            )
        } == 0
        {
            return Err(last_error("OpenProcessToken"));
        }
        let mut guard = Self {
            token,
            // SAFETY: zero is valid for TOKEN_PRIVILEGES and its count/attributes.
            previous: unsafe { mem::zeroed() },
            restore_pending: false,
        };
        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        let privilege = wide("SeSystemEnvironmentPrivilege");
        // SAFETY: null selects this system; name is NUL-terminated and output is valid.
        if unsafe { LookupPrivilegeValueW(ptr::null(), privilege.as_ptr(), &mut luid) } == 0 {
            return Err(last_error("LookupPrivilegeValueW"));
        }
        let requested = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // SAFETY: one requested privilege, correctly sized output for at most one
        // changed privilege, live owned token. The API reports partial assignment
        // through GetLastError even when its BOOL result is true.
        let mut returned_length = 0;
        let (success, code) = unsafe {
            SetLastError(0);
            let success = AdjustTokenPrivileges(
                token,
                0,
                &requested,
                mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                &mut guard.previous,
                &mut returned_length,
            );
            (success, GetLastError())
        };
        if success == 0 {
            return Err(ProbeError::windows("AdjustTokenPrivileges", code));
        }
        guard.restore_pending = guard.previous.PrivilegeCount != 0;
        if code != 0 {
            return Err(ProbeError::windows("AdjustTokenPrivileges", code));
        }
        Ok(guard)
    }

    fn restore(&mut self) -> Result<&'static str, ProbeError> {
        if !self.restore_pending {
            return Ok("unchanged");
        }
        // SAFETY: PreviousState is the exact single-privilege state returned by
        // the successful enable operation. No unrelated privileges are disabled.
        let (success, code) = unsafe {
            SetLastError(0);
            let success = AdjustTokenPrivileges(
                self.token,
                0,
                &self.previous,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            (success, GetLastError())
        };
        if success == 0 || code != 0 {
            return Err(ProbeError::windows("RestoreTokenPrivilege", code));
        }
        self.restore_pending = false;
        Ok("restored")
    }
}

impl Drop for PrivilegeGuard {
    fn drop(&mut self) {
        let _ = self.restore();
        // SAFETY: this handle was obtained once from OpenProcessToken and is owned here.
        unsafe { CloseHandle(self.token) };
    }
}

pub fn inspect_fields() -> crate::Inspection {
    let inspection = inspect_snapshot();
    crate::Inspection {
        report: inspection.report,
        fields: inspection.snapshot.as_ref().map(|s| s.fields()),
    }
}

fn registry(name: &str) -> Result<String, String> {
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
            ptr::null_mut(),
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
fn identity() -> Result<crate::snapshot::PlatformIdentity, String> {
    Ok(crate::snapshot::PlatformIdentity {
        manufacturer: registry("BaseBoardManufacturer")?,
        product: registry("BaseBoardProduct")?,
        bios_version: registry("BIOSVersion")?,
        bios_date: registry("BIOSReleaseDate")?,
    })
}
pub fn inspect_snapshot() -> crate::SnapshotInspection {
    // Privilege changes affect the process token; serialize concurrent callers.
    static READ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _read_lock = READ_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut report = ProbeReport::new();
    let mut snapshot = None;
    let mut firmware_type = 0;
    // SAFETY: valid enum output; this API does not modify firmware.
    if unsafe { GetFirmwareType(&mut firmware_type) } == 0 {
        report.firmware.status = "query_failed";
        report.firmware.error = Some(last_error("GetFirmwareType"));
        return crate::SnapshotInspection { report, snapshot };
    }
    report.firmware.status = match firmware_type {
        FIRMWARE_TYPE_UEFI => "uefi",
        FIRMWARE_TYPE_BIOS => "legacy_bios",
        _ => "unknown",
    };
    if firmware_type != FIRMWARE_TYPE_UEFI {
        report.variable.status = "unsupported_boot_mode";
        return crate::SnapshotInspection { report, snapshot };
    }
    let mut privilege = match PrivilegeGuard::enable() {
        Ok(guard) => guard,
        Err(error) => {
            report.privilege.status = if error.win32_code == ERROR_NOT_ALL_ASSIGNED {
                "not_assigned"
            } else {
                "enable_failed"
            };
            report.privilege.error = Some(error);
            report.variable.status = "privilege_unavailable";
            return crate::SnapshotInspection { report, snapshot };
        }
    };
    report.privilege.status = if privilege.restore_pending {
        "enabled_temporarily"
    } else {
        "already_enabled"
    };
    let name = wide(VARIABLE_NAME);
    let guid = wide(VARIABLE_GUID);
    let mut data = vec![0u8; READ_BUFFER_BYTES];
    let mut attributes = 0;
    // SAFETY: fixed, NUL-terminated name/GUID; bounded writable buffer and attributes.
    // This is the only firmware-variable API called by the probe. It is a getter.
    let (length, error_code) = unsafe {
        SetLastError(0);
        let length = GetFirmwareEnvironmentVariableExW(
            name.as_ptr(),
            guid.as_ptr(),
            data.as_mut_ptr().cast(),
            READ_BUFFER_BYTES as u32,
            &mut attributes,
        );
        (length as usize, GetLastError())
    };
    if length == 0 {
        report.variable.status = "read_failed";
        report.variable.error = Some(ProbeError::windows(
            "GetFirmwareEnvironmentVariableExW",
            error_code,
        ));
    } else if length > data.len() {
        report.variable.status = "invalid_returned_length";
    } else {
        report.variable.status = "read";
        report.variable.metadata = Some(VariableMetadata::decode(&data[..length], attributes));
        snapshot = identity().ok().and_then(|identity| {
            crate::snapshot::Snapshot::from_read(
                &data[..length],
                attributes,
                identity,
                crate::snapshot::Origin::Live,
            )
            .ok()
        });
        report.custom_curve_decoding = if snapshot.is_some() {
            "partial_reference_fields"
        } else {
            "unavailable"
        };
    }
    // Full bytes escape only the explicitly requested snapshot interface.
    // inspect_fields/inspect discard them; no automatic file export exists.
    data.fill(0);
    match privilege.restore() {
        Ok(status) => report.privilege_restoration = StepStatus::new(status),
        Err(error) => {
            report.privilege_restoration.status = "restore_failed";
            report.privilege_restoration.error = Some(error);
        }
    }
    if report.privilege_restoration.error.is_some() {
        snapshot = None;
    }
    crate::SnapshotInspection { report, snapshot }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_enable_uses_valid_output_buffers_and_restores_any_change() {
        // Process-local privilege only: no firmware getter or setter runs here.
        match PrivilegeGuard::enable() {
            Ok(mut guard) => assert!(guard.restore().is_ok()),
            Err(error) => assert!(
                matches!(error.win32_code, 5 | 1300 | 1314),
                "unexpected token API failure: {} ({})",
                error.operation,
                error.win32_code
            ),
        }
    }
}
