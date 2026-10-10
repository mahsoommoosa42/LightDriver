# Read-only firmware fan-settings probe

Research utility, separate from the desktop and service. It reads only `GcSensorVarName` under GUID `5489A3E4-C745-11EA-87D0-0242AC130003`. It has no firmware setter, driver I/O, raw-data export, automatic elevation or startup installation.

A successful read does not prove write permission or working fan-curve persistence. A matching header is not a verified writable board/BIOS profile.

## Build and run

From the repository root on Windows:

```powershell
cargo build -p lightdriver-firmware-probe --release --locked
.\target\release\lightdriver-firmware-probe.exe --inspect
```

Without `--inspect`, it prints help and does not query firmware. Unknown arguments and custom variable names are rejected.

Windows requires `SeSystemEnvironmentPrivilege` for this read. A normal account token may return Win32 error 1300. To continue, open an administrator PowerShell window through Windows UAC and run the same executable with `--inspect`. Elevation is an explicit user action; the probe does not request it itself.

The probe enables that one existing privilege on its own process token, performs one fixed variable read using a 64 KiB buffer, restores its previous privilege state and exits. It does not change account privileges, policy, BIOS settings or controller registers.

## Report

JSON contains boot mode, privilege/read/restoration status, error codes, returned length, attributes and the eight-byte board-image identifier plus declared payload length. Header/attribute comparisons refer only to the analyzed F41c image's format.

The entire payload is excluded from stdout and files. Usernames, machine names, paths, serial numbers and fan-curve bytes are not included. Nothing is uploaded.

An optional `--output <new-json-file>` writes the same metadata to a new file and refuses to overwrite an existing capture:

```powershell
.\target\release\lightdriver-firmware-probe.exe --inspect --output .data\firmware-read.json
```

The parent directory must already exist. Keep machine reports private; `.data` is ignored by Git. No raw-capture option exists in this version.

Exit codes: **0** = variable read and no privilege-restoration error; **1** = unavailable/failed; **2** = argument or export error. The metadata's header flags must still be checked after exit 0. Unknown firmware or mismatched layouts are observations, not writable support.

## Validation

Tests use synthetic payloads for truncated/default headers, differing boards/lengths, unknown attributes and raw-data exclusion. A Windows test exercises process-token enable/restore without touching firmware. No automated test reads or writes a firmware variable.

An import audit of the optimized Windows executable found only `GetFirmwareType` and `GetFirmwareEnvironmentVariableExW` among firmware imports, with no firmware setter or driver-I/O import.

The reference-machine read succeeded after explicit UAC: expected size/header and attributes `0x7`; process privilege restoration succeeded. The actual report remains local. Write permission, custom-curve encoding and reboot retention remain untested.

A shared Rust library now also exposes the identified scalar fields to the separate [native Firmware fan lab](../fan-debug/README.md). Its strict format check is read-only and does not establish a writable BIOS identity. CLI JSON remains metadata-only, without raw capture or a setter. The library's separate inspect_snapshot API now retains the full reference variable and inventory for the fan lab's explicit private local save/comparison workflow; inspect/inspect_fields discard full bytes. Calls serialize temporary process-token privilege changes.

See [the persistence investigation](../../docs/firmware-fan-persistence.md) for evidence and remaining gates.

## Consolidated implementation

The existing CLI and import path are retained as a compatibility facade. All firmware profiles/decoding/private snapshot logic now lives in `lightdriver-gigabyte`, using the bounded read-only transport in `lightdriver-hardware`. Live inspection requires the exact reviewed board/BIOS/date profile before transport access; unsupported identity reports `unsupported_board_bios` and performs no UEFI read or privilege change. Service and standalone debugger share the vendor backend; service discovery does not automatically invoke this privileged getter.
