//! Explicit Windows UAC handoff. The Iced UI remains at ordinary permissions.
pub async fn enable() -> Result<lightdriver_core::Snapshot, String> {
    let binary = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("lightdriver-service.exe");
    let binary = binary.canonicalize().map_err(|_| {
        "Build the workspace so the service executable is beside the app".to_string()
    })?;
    if !lightdriver_ipc::stop_unprivileged_live_service(lightdriver_core::Mode::Live)
        .await
        .map_err(|e| format!("CPU sensor handoff failed: {e}"))?
    {
        return Err(
            "The current service cannot be handed off; reconnect to its existing readings".into(),
        );
    }
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    tokio::task::spawn_blocking(move || elevate(&binary))
        .await
        .map_err(|e| e.to_string())??;
    for _ in 0..30 {
        if let Ok(snapshot) = lightdriver_ipc::snapshot(lightdriver_core::Mode::Live).await {
            return Ok(snapshot);
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    Err("Elevated CPU sensor service did not connect; retry Enable CPU sensors".into())
}
#[cfg(windows)]
fn elevate(binary: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError},
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::Shell::{
            SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
        },
    };
    let verb: Vec<_> = "runas".encode_utf16().chain(Some(0)).collect();
    let file: Vec<_> = binary.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: stable null-terminated strings; fully initialized Win32 structure.
    // ShellExecute runs only the known sibling service, without a command shell,
    // arbitrary arguments, registry writes, startup installation or device setters.
    unsafe {
        let com = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            nShow: 0,
            ..SHELLEXECUTEINFOW::default()
        };
        let success = ShellExecuteExW(&mut info);
        let error = GetLastError();
        if !info.hProcess.is_null() {
            CloseHandle(info.hProcess);
        }
        if com >= 0 {
            CoUninitialize();
        }
        if success != 0 {
            Ok(())
        } else if error == 1223 {
            Err("CPU sensor permission request canceled; ordinary monitoring restored".into())
        } else {
            Err(format!(
                "Windows could not start the CPU sensor service (error {error})"
            ))
        }
    }
}
#[cfg(not(windows))]
fn elevate(_: &std::path::Path) -> Result<(), String> {
    Err("CPU sensor elevation currently requires Windows".into())
}
