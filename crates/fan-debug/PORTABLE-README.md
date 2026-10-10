# LightDriver standalone fan debugger

This archive is a separate application. Its Rust backend modules are built into the executables; no additional LightDriver DLL or Workbench service is required.

Firmware writing and restoration remain unavailable. Percentages and restore actions are previews only. This version recognizes GIGABYTE X570 AORUS ELITE WIFI with BIOS F41c (18 August 2026) for firmware inspection; other RGB/RPM support is independently gated.

## Start

Extract the whole archive to a folder. For live firmware reads, right-click **lightdriver-fan-debug.exe** and select **Run as administrator**, accepting normal Windows UAC. If UAC is cancelled, repeat the launch. `FanDebug.cmd` performs an ordinary launch and does not elevate.

Use **Snapshots & comparison** to read and save a private before snapshot. Follow [the complete BIOS capture steps](docs/firmware-test-guide.md) before changing anything. Existing private backups remain importable. After a reboot, a new session's Initial backup is the post-change state: retain the explicitly saved before file.

Optional live RPM requires the official signed PawnIO driver and separately obtained, hash-pinned LpcIO 0.2.11 module. Put that module in **FanDebug-support/LpcIO.bin** next to the executable; see [full usage/dependency details](crates/fan-debug/README.md). Neither third-party dependency is bundled. Firmware capture does not require PawnIO.

For an entirely synthetic preview, run `FanDebug.cmd --demo`. The optional `lightdriver-firmware-probe.exe --inspect` provides a metadata-only report without exporting raw bytes.

[Shared adapter architecture](docs/motherboard-adapters.md) explains how the service and debugger use the same GIGABYTE implementation. There is no background fan regulation, automatic reboot, firmware setter or protection bypass.
