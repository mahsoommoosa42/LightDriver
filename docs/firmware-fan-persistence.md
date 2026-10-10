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

## Further offline decoding and write-policy investigation

Continued static analysis of the same F41c image identifies the native GUI-to-firmware codec and a firmware lock mechanism. These are reverse-engineering observations, not supported Windows APIs or evidence of write permission. All addresses below are **PE RVAs**, not file offsets.

| Module | FFS identity | SHA256 |
| --- | --- | --- |
| CoolSetupDataDxe | B7246C46-4200-4D44-BAE0-39CFDCAC6584 | 5F28C2C8F0C53005DAB302605E1082C4D2627D092081C633FF492A59D5353342 |
| CoolGUI | 3B87CD35-0D65-415E-8B35-0DDADA13F30E | 16329F50BA5040D22248127C6485E7CBBCCDF99892E7A482813FE04C6C5E5FC2 |
| NvramDxe | 1807040D-5934-41A2-A088-8E0F777F71AB | 7107DBC8E5905CD64ADA475C5E063BCDC0BFB64A712297253C19C3628B031D6C |
| NvramSmm | 447A1B58-8F3E-4658-ABAA-9E7B2280B90A | 7661AD0318D70D5A2BED7964AF25F92B150DBCA78E15F4934D9C46A6B80D255A |


**Firmware interface:** CoolSetupDataDxe installs C3E7A901-3A53-49BE-A6C2-CAF5B53F092A at RVA 0x415, with interface table at 0x3240. Entries point to form enumeration (0x1B4C), metadata (0x1BE8), a 53-byte internal-vector getter (0x1D64), setter/HII routing (0x1EEC), and whole-configuration handling (0x2370). CoolGUI locates it at 0x2C1A9 and calls the getter/setter at 0x27BF2/0x27FB9. This interface exists during firmware execution; it is not a callable Windows SDK.

**Partial representation:** metadata flags 0x198 identify three extra segments, sign-magnitude coefficient deltas and accumulation from the preceding coefficient. The 53-byte internal vector is seven primary bytes, space for nine five-byte extras and one source byte. It is not a sequence of 53 curve points or the whole 569-byte variable. CoolGUI converts this vector to/from its 0xA8-byte graph representation at 0x27B8C/0x27D94; slope calculation at 0x28690 uses truncated level delta × 8 / temperature delta, with a cap of 127 and a special zero-span case. Save flow around 0x29E36–0x29EB5 routes through these conversions. Derived update rules and the roles of auxiliary bytes still need validation.

The normal four-header forms map as follows:

| Header | Form ID | Mode byte | First manual scalar byte | PWM/DC byte | Fan-stop byte |
| --- | --- | --- | --- | --- | --- |
| CPU_FAN | 0x27B1 | 0x00A | 0x00B | 0x20E | 0x21D |
| CPU_OPT | 0x27B5 | 0x037 | 0x038 | 0x20F | 0x21E |
| SYS_FAN1 | 0x27B9 | 0x064 | 0x065 | 0x214 | 0x21F |
| SYS_FAN2 | 0x27BD | 0x091 | 0x092 | 0x215 | 0x220 |

Each identified manual block contains off/start/full temperatures, start level, base coefficient, interval, source and three extra temperature/coefficient entries. The remainder of each 0x2D-byte block is **not fully decoded**. Presets can synthesize values instead of using these manual fields. The PCH form is excluded from editing. Whole-store profile import/export uses an "SF P" marker and CRC32; that private file format does not establish a public update contract.

**Firmware locking:** NvramDxe contains the EDK II variable-lock protocol GUID CD3D0A05-9E24-437C-A891-1EE053DB7638. Its interface at 0xE270 points to callback 0x1C2C; installation occurs at 0x23DB. The callback sends lock registration through SMM operation 4. NvramSmm dispatches that operation at 0x7160–0x71C5 to registration at 0x5D48. Matching namespace/name checks at 0x5E2C–0x5EEA return EFI_WRITE_PROTECTED; write paths call the checker at 0x1821 and 0x545C. This is consistent with the [EDK II variable-lock definition](https://github.com/tianocore/edk2/blob/master/MdeModulePkg/Include/Protocol/VariableLock.h).

Whether **GcSensorVarName itself is registered in that lock list has not been established**. Neither read success nor an incomplete literal-GUID scan proves external writes are allowed. Other AMI policies may also apply; [EDK II variable-policy definitions](https://github.com/tianocore/edk2/blob/master/MdeModulePkg/Include/Protocol/VariablePolicy.h) explain why variable policy is separate from storage format. No SMM payload, lock bypass, firmware flash, Windows setter or reboot was attempted.

## Debug tool milestone

The standalone [Firmware fan lab](../crates/fan-debug/README.md) shares the fixed-variable getter and a pure Rust partial decoder. It displays identified scalars and case-header previews, including full-length offline patches; no candidate is a validated writable payload. A 75% preview quantizes to 191/255 (~74.90%) between existing start/full thresholds; it does not prove fixed output at all temperatures.

Write/readback and restore controls are disabled by construction. The current milestone implements Moosa's selected **continue decoding before enabling writes** route.

Optional live RPM uses a separate read-only PawnIO tachometer adapter, not the UEFI store. After installation, the signed driver and an elevated getter returned changing nonzero RPM from the reference board's IT8688 revision 1; physical header mapping and independent accuracy remain outstanding. The firmware-buffer data is never used as a fabricated live-speed reading.

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

1. **Completed:** build and run the narrowly scoped, read-only Rust probe for this exact variable. UEFI mode, read access, attributes, length, header and process privilege restoration were confirmed after explicit UAC. The metadata CLI has no setter, variable enumeration, raw capture or firmware flashing. The lab's separately authorized private snapshot interface is described below.
2. **Partially completed:** the live size/header/attributes match the reference format. Complete board/BIOS identity gates and custom-curve layout validation before any writable adapter is considered. A matching board name or header alone is insufficient.
3. Complete the custom-curve codec and derived-field rules through offline analysis and controlled before/after captures of BIOS-made changes. Preserve every unknown byte. Verify point limits, encoding, selected onboard sensor, interpolation, hysteresis, fan stop and PWM/DC relationships.
4. Investigate firmware write policy. Read success does not prove write success. If the standard API is denied, report that limit and seek a supported vendor interface; do not bypass firmware protection.
5. Only after the codec and recovery procedure are established, explicitly approve a change on an identified noncritical fan header. Back up the original store, check it has not changed, submit one supported update and read back the result. A preceding comparison does not make the firmware API atomic; concurrent vendor utilities must be excluded during validation.
6. Verify BIOS display, real temperature-dependent fan response, GUI and service exit, restart, cold boot and resume separately. Account for Fast Startup and verify that no application silently reapplies the curve. Test restoration of the original settings.

Fan-response, write-permission and reboot-persistence tests remain deferred. Live metadata and private snapshot reads are documented below; no curve is writable or advertised as persistent yet.

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



## Firmware lab snapshot tooling — 10 October 2026

Read-only registry/CIM inventory reports X570 AORUS ELITE WIFI, BIOS F41c, released 18 August 2026. This matches the analysed image but does not attest the entire installed binary or board revision.

Further CoolGUI inspection shows graph reconstruction at RVAs 0x28D71–0x28E91 using accumulated coefficients, 7-bit bounds, slope/8 and clamping. The graph save path calls the integer slope calculation at 0x29E98 before conversion through 0x27D94. The lab offers a labelled research estimate for valid Manual fields; exact rounding, preset synthesis and all callback effects are still not independently validated.

The lab retains a full private snapshot through its snapshot interface and compares every byte including unidentified fields. Its GUI automatically reads once on open and saves/verifies an immutable initial backup after the first successful read, as requested by Moosa. Additional captures and imports are explicit. The metadata CLI still excludes payloads. Fixed-level candidates are full-length offline copies with identified start-level/coefficients patched and all other bytes preserved; no candidate reaches a firmware setter.

There is no live-write transport, restoration command or protection bypass. A snapshot provides recovery data but is not a tested restoration method. Import digests detect corruption, not authenticated hardware provenance. Getter/comparison tools can check equal storage and controlled BIOS changes; they cannot prove write acceptance or persistence alone.

[FanControl's public-source study](fancontrol-study.md) found no UEFI/BIOS-save path for these curves. Its open backend provides useful RPM/runtime references and does not complete this codec.



## Initial recovery and factory reference — 10 October 2026

Each GUI session anchors its first successful live snapshot before disk I/O, writes a new private file in %LOCALAPPDATA%\LightDriver\FirmwareBackups, and reloads it for full byte-for-byte and timestamp verification. Subsequent refreshes cannot replace the initial state. Failed disk saves retain the original in memory and can be retried. Prior sessions' files remain available; a new launch does not overwrite the previous baseline.

The official F41c CoolSetupDataDxe default/mode tables at RVAs 0x3460–0x3604 provide the Normal reference for SYS_FAN1 and SYS_FAN2: off/start/full 0/20/60 °C, initial level 70/255, base coefficient 28, interval 3, source 0; additional thresholds 30/40/50 °C with coefficient deltas 0/0/36. The factory stored mode is 0 (Normal). These facts were independently rechecked against the offline extracted image; they are bundled as a declarative Rust reference, not as vendor firmware or a factory NVRAM blob.

Factory restore preview selects Normal on the chosen case header and preserves source, PWM/DC, fan-stop and inactive custom fields. Initial restore preview proposes that header's original 45-byte block plus two known PWM/DC/fan-stop bytes. Both preserve other headers. This defines a fan-curve restore target, not a whole-BIOS reset or proof that shared callback effects are decoded. Actual restore remains disabled.

Two elevated live captures produced identical 569-byte snapshots with zero identified or unknown changed bytes. Both case headers had Normal selected and zero inactive manual fields. This confirms current preset storage, not a ready Manual encoder. A controlled BIOS-made Manual capture/diff remains necessary. No firmware write or restart occurred.
