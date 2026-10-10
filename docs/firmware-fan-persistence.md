# Persistent motherboard fan curves from Windows

Research date: 10 October 2026. Moosa selected **research of true BIOS persistence first**. Startup reapplication and continuous software fan regulation are outside this investigation.

## Result

Offline analysis of GIGABYTE's official X570 AORUS ELITE WIFI F41c image found a dedicated fan-settings UEFI variable. The firmware saves it with nonvolatile, boot-service and runtime-access attributes, and several firmware components read it.

The new read-only Rust probe also confirmed access to the live store from Windows after explicit UAC: expected size/header and nonvolatile/runtime attributes. This establishes a concrete candidate for persistent settings through Windows' standard UEFI variable API. It does **not** establish write permission, complete custom-curve encoding or retention of a changed curve through a real reboot. LightDriver's motherboard writes remain disabled.

The candidate does not require PawnIO for the variable transport. PawnIO may still be relevant to a separately validated immediate hardware-apply path. No public GIGABYTE API contract for these private settings was found.

## Official image and analysis

| Item | Reference |
| --- | --- |
| Board image | X570 AORUS ELITE WIFI, rev. 1.x support page |
| Vendor release | F41c, dated 18 August 2026 |
| Image filename | `X570AORUSELITEWIFI.F41c` |
| Image length | 16,777,216 bytes |
| Image SHA-256 | `2B15503189943B2BAAD7864C6A7155B2BEDB28EF1535BCDB3AB7EFC27435CE79` |
| Firmware extraction | LongSoft UEFIExtract A75, Windows x64 release |
| Setup form decoding | LongSoft IFRExtractor-RS 1.6.1, Windows release |
| Static inspection | pefile 2024.8.26 and Capstone 5.0.9; PE x64 and TE IA32 disassembly |

The image was downloaded from the official [F41c archive](https://download.gigabyte.com/FileList/BIOS/mb_bios_x570-aorus-elite-wifi_8amts010_f41c.zip?v=ea95cdfd7da9a6fc7230db81b8d920bd) linked on the [board support page](https://www.gigabyte.com/Motherboard/X570-AORUS-ELITE-WIFI-rev-1x/Support/support#Support-Bios). Analysis inspected files only; the image was never flashed and firmware code was never executed.

UEFIExtract completed with the warning `the last VTF appears inside compressed item`. The extracted modules, form metadata and variable entry agree on the variable identity and size, but this is not a claim that every part of the image parsed without warnings.

Vendor images, extracted modules, SDK files and analysis caches are excluded from public version control.

## Identified settings store

| Field | Offline observation |
| --- | --- |
| UEFI variable name | `GcSensorVarName` |
| Vendor GUID | `5489A3E4-C745-11EA-87D0-0242AC130003` |
| Setup VarStore ID | `0x18`, an identifier inside this extracted form package |
| Payload length | `0x239` = 569 bytes |
| Factory NVAR entry | Two matching extracted copies, both 569 bytes |
| Factory payload SHA-256 | `8021181728389471a19bea61f1749a467c9613c4bde0eaa2d8eb69031685ec42` |
| NVAR parser attributes | `0x83`: Runtime, AsciiName, Valid |
| Observed UEFI save attributes | `0x7`: Nonvolatile, Boot-service access, Runtime access |

NVAR's internal `0x83` flags and UEFI's `0x7` attributes are different encodings. They must not be passed interchangeably to Windows APIs.

The factory payload starts with zero bytes. A traced initialization path stamps the first eight bytes with the image's board identifier `8AMTS010`, and bytes 8–9 with little-endian `0x239`, before saving. Factory defaults are therefore **not a valid replacement for a live settings snapshot**.

The store also contains configuration beyond a curve, including fan mode, temperature source, interval, PWM/DC selection and fan-stop settings. Replacing it with a mostly zero blob could change unrelated settings.

### Verified form offsets

These are observations from this image's setup forms, not a writable public ABI.

| Header | Speed-control enum | Temperature input | Temperature interval | PWM/DC enum | Fan stop |
| --- | --- | --- | --- | --- | --- |
| CPU_FAN | `0x0A` | `0x12` | `0x11` | `0x20E` | `0x21D` |
| CPU_OPT | `0x37` | `0x3F` | `0x3E` | `0x20F` | `0x21E` |
| SYS_FAN1 | `0x64` | `0x6C` | `0x6B` | `0x214` | `0x21F` |
| SYS_FAN2 | `0x91` | `0x99` | `0x98` | `0x215` | `0x220` |

The speed enum is Normal = 0, Silent = 1, Manual = 2, Full Speed = 3. IFR's `Size: 8` here means eight bits, or one byte. CPU_FAN's temperature-input question permits only the CPU input in this form; other headers have separate choices.

Custom graph points are not fully described by these simple IFR questions. CoolSetupDataDxe contains tables linking internal control IDs to additional payload offsets and derived settings. Its configuration handlers also perform transformations and controller updates. A successful byte write would not prove that those transformations were reproduced correctly.

## Firmware code evidence

Addresses below are module-relative RVAs from the extracted executable sections.

| Component | FFS GUID | Relevant observation |
| --- | --- | --- |
| Setup | `899407D7-99FE-43D8-9A21-79EC328CAC21` | IFR declares the named store, GUID and length; fan questions use it |
| GenericComponentDxeEntry | `8835D63E-7F59-485A-99B8-18D6C70E8FF5` | Reads and saves this variable with attributes `0x7` |
| CoolSetupDataDxe | `B7246C46-4200-4D44-BAE0-39CFDCAC6584` | HII configuration routing and fan-data transformations; not a Windows API |
| ThermalFanCtrlDxe | `2FD10E90-C379-45CE-BD79-EEFB1600DEAC` | Reads the 569-byte store and applies selected thermal settings through controller I/O |
| ThermalFanCtrSmm | `471D888A-AB93-4663-A24D-24FA32F3FC82` | Reads the same store; firmware participates in hardware configuration |
| ThermalFanCtrlPei | `487FF6B4-2147-40F7-8352-7A1266EDBCE3` | Early-boot code requests the same variable and size |

In GenericComponentDxeEntry, the runtime-services pointer is initialized from the UEFI system table's `+0x58` member. The inspected save call has the named variable at RVA `0x20408`, its GUID at `0x1EE50`, and a size constant of `0x239` at `0x1EE60`.

At RVA `0x12C17`, it loads attributes `7` into R8D. At `0x12C24`, R9 receives the size. At `0x12C2B`, the data pointer becomes the fifth argument. At `0x12C37`, it calls runtime-services `SetVariable` through the table's `+0x58` slot. This matches the x64 calling convention and corroborates the persistent/runtime attribute interpretation.

ThermalFanCtrlDxe calls runtime `GetVariable` at RVA `0x4D1`. The IA32 TE module ThermalFanCtrlPei assembles the same GUID, requests length `0x239`, and calls the read-only variable PPI at RVA `0x3E0`.

These paths show firmware ownership and boot-time consumption of the store. They do not yet prove the complete custom-curve path, when all derived values are recomputed, or whether firmware restores or rejects changed data.

## Windows access and remaining uncertainty

Microsoft documents [GetFirmwareEnvironmentVariableExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfirmwareenvironmentvariableexw) and [SetFirmwareEnvironmentVariableExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setfirmwareenvironmentvariableexw). The flags `1 | 2 | 4` describe nonvolatile storage and access during boot and OS runtime. Access requires the process's `SeSystemEnvironmentPrivilege` and a UEFI boot environment.

The firmware's own save path runs before or within firmware-controlled contexts. It is **not evidence of permission for Windows to perform the same save**. Firmware variable policies, write protection, authentication requirements, boot phase and version changes can restrict access even when runtime reads are permitted. An elevated Windows process cannot override a firmware refusal through this API.

A live read through the [standalone Rust probe](../crates/firmware-probe/README.md) succeeded on the reference machine after explicit Windows UAC. It returned 569 bytes, attributes `0x7`, the expected board-image identifier and a matching declared length. The probe restored its original process privilege before exit. The normal-permission run correctly reported a missing token privilege (Win32 1300), rather than claiming the variable was absent.

Only metadata was exported to a private local report; no raw payload, usernames, paths or serials were included, and nothing was uploaded. The compiled probe's firmware imports were audited: only `GetFirmwareType` and `GetFirmwareEnvironmentVariableExW`, with no firmware setter or driver-I/O import.

Live read access and reference-header/attribute matching are now verified for that observation. The complete custom-curve layout, firmware write policy and changed-curve retention remain unverified. A matching header does not identify every firmware version or prove writable support.

## Meaning of persistence

| Event or action | What the candidate could provide | Current evidence |
| --- | --- | --- |
| Save for next boot | Change the firmware's persistent fan settings | Dedicated store and persistent save path identified; external writes untested |
| Apply immediately | Update the running controller to the new curve | Requires a separately understood apply path; saving the variable alone does not prove this |
| Close GUI or stop all app processes | Saved firmware data remains without an app running | Expected for a successfully saved nonvolatile variable; hardware behavior untested |
| Restart or cold power-on | Firmware reads the saved settings during boot | Boot readers identified; custom curve acceptance and actual fan response untested |
| Sleep/resume | Firmware/controller restores or retains the intended curve | Unknown; boot consumption does not establish resume behavior |
| BIOS reset or firmware update | May clear, migrate or replace settings | Do not promise retention |

The intended finished feature saves firmware-owned curves that work before Windows starts and without LightDriver running. If saving only takes effect after reboot, the UI must say **Save for next boot**. **Apply now** must remain a separate capability until verified. No automatic reboot is implied.

## Next validation gates

1. **Completed:** build and run the narrowly scoped, read-only Rust probe for this exact variable. UEFI mode, read access, attributes, length, header and process privilege restoration were confirmed after explicit UAC. The tool has no setter, variable enumeration, raw capture or firmware flashing.
2. **Partially completed:** the live size/header/attributes match the reference format. Complete board/BIOS identity gates and custom-curve layout validation before any writable adapter is considered. A matching board name or header alone is insufficient.
3. Complete the custom-curve codec and derived-field rules through offline analysis and controlled before/after captures of BIOS-made changes. Preserve every unknown byte. Verify point limits, encoding, selected onboard sensor, interpolation, hysteresis, fan stop and PWM/DC relationships.
4. Investigate firmware write policy. Read success does not prove write success. If the standard API is denied, report that limit and seek a supported vendor interface; do not bypass firmware protection.
5. Only after the codec and recovery procedure are established, explicitly approve a change on an identified noncritical fan header. Back up the original store, check it has not changed, submit one supported update and read back the result. A preceding comparison does not make the firmware API atomic; concurrent vendor utilities must be excluded during validation.
6. Verify BIOS display, real temperature-dependent fan response, GUI and service exit, restart, cold boot and resume separately. Account for Fast Startup and verify that no application silently reapplies the curve. Test restoration of the original settings.

Fan-response, write-permission and reboot-persistence tests remain deferred. The metadata read above is the only live firmware operation performed; no curve is writable or advertised as persistent yet.

## Expandable Rust design, if validation succeeds

Use a Rust UEFI transport behind firmware-specific codecs in the local privileged service. Describe **firmware save**, **immediate hardware apply**, **runtime autonomous regulation** and **verified reboot retention** as separate capabilities. The Iced editor can share curve validation while each adapter declares its own limits and supported onboard sensors.

Support starts with exact validated board/firmware profiles. Other versions and vendors can add independent layouts or supported vendor APIs; they must not inherit these offsets because they have a similar name or ITE chip. Store access and commands should be restricted to the recognized fan variable rather than exposing arbitrary NVRAM writes through IPC.

This is a research direction, not an implemented architecture change. The service remains as currently shipped, and startup reapplication has not been added.

## Sources

- [Official board BIOS support](https://www.gigabyte.com/Motherboard/X570-AORUS-ELITE-WIFI-rev-1x/Support/support#Support-Bios), and the F41c download linked above. Binary findings are our own static analysis.
- [GIGABYTE X570 SIV / Smart Fan 5 guide](https://download.gigabyte.com/FileList/Manual/mb_manual_x570-features.pdf), page 22: existing Windows curve editing, without a third-party persistence contract.
- [Microsoft firmware-variable read API](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfirmwareenvironmentvariableexw) and [write API](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setfirmwareenvironmentvariableexw).
- [Microsoft documented UEFI attribute constants](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-exgetfirmwareenvironmentvariable).
- [UEFI 2.11 runtime variable services](https://uefi.org/specs/UEFI/2.11/08_Services_Runtime_Services.html): variable lifetime and runtime access rules.
- [UEFIExtract A75](https://github.com/LongSoft/UEFITool/releases/tag/A75) and [IFRExtractor-RS 1.6.1](https://github.com/LongSoft/IFRExtractor-RS/releases/tag/v1.6.1): offline analysis tools.
