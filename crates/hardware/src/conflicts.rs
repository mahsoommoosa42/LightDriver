#[cfg(windows)]
pub fn competing_controller() -> Option<String> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.processes().values().find_map(|p| {
        let name=p.name().to_string_lossy().to_ascii_lowercase();
        ["nzxt cam.exe", "nzxt cam helper.exe", "cam.exe", "liquidctl.exe", "openrgb.exe", "signalrgb.exe", "rgbfusion.exe", "rgbfusion2.exe", "gcc.exe"].contains(&name.as_str()).then_some("Another cooling/lighting controller is running. Close it before enabling or applying device control.".into())
    })
}
