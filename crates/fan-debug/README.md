# Firmware fan lab

Standalone native Rust/Iced app for firmware inspection, private recovery snapshots, curve previews and optional live motherboard RPM. **Firmware writing and restoration are disabled.** The selected development route is to finish and independently validate firmware decoding before enabling writes.

## Run

Build an optimized GPU-first version:

~~~powershell
cargo build -p lightdriver-fan-debug --release --locked
.\target\release\lightdriver-fan-debug.exe
# Or double-click FanDebug.cmd.
~~~

The GUI makes one fixed-variable read on open. Windows may require administrator permissions: close the lab and run FanDebug.cmd as administrator through normal UAC. It does not request elevation, start a service or poll hardware automatically. RPM monitoring starts only when selected. In a finished Workbench integration, privileged operations belong in the local Rust service.

## Initial backup and factory reference

After the first successful recognized live read, the app anchors the complete initial snapshot in memory, saves a new private file in **%LOCALAPPDATA%\LightDriver\FirmwareBackups**, then reloads and compares it byte for byte. Later reads cannot replace that initial snapshot. If saving fails, the initial state remains in memory and **Retry initial backup** retries that original snapshot. Each new app session creates a separate file; older backups remain available through **Load baseline from path**. Demo uses an in-memory synthetic initial snapshot and creates no automatic backup.

The app also contains a declarative **F41c Normal preset reference** for SYS_FAN1 and SYS_FAN2 on the reviewed X570 AORUS ELITE WIFI profile. It was extracted from the official firmware's default/mode tables; no vendor firmware image or mostly-zero factory NVRAM blob is bundled. Other boards and BIOS versions require separately reviewed profiles.

Select a recognized case header, then use **Preview initial restore** or **Preview factory restore** in **Snapshots & comparison**. Initial restore proposes that header's original 45-byte block plus its PWM/DC and fan-stop bytes. Factory restore proposes selecting the firmware's Normal preset while preserving temperature-source, PWM/DC, fan-stop and inactive custom fields. It is a fan-curve default, not a reset of the entire BIOS or of all header configuration. CPU and chipset targets are excluded.

Both choices build offline previews only. A verified backup file is recovery data, not a tested firmware restore. **Restore initial**, **Restore factory**, and **Write + read back** remain disabled until the codec, update semantics, firmware policy and restoration procedure are validated.

## Firmware inspection and percentage previews

Use **Read firmware** to refresh. The getter temporarily enables an existing process privilege and restores it. It retains the complete recognized variable alongside identified fields and board/BIOS inventory. The separate firmware-probe CLI remains metadata-only. Firmware reads never run on the RPM timer.

Select SYS_FAN1 or SYS_FAN2 after the reference format matches. Form-derived header labels do not verify physical wiring or authorize writing. Other firmware formats return no decoded fields.

Choose 30%, 50%, 75% or 100%, or a value between 20–100%. Fixed-level preview requires stored Manual mode and sensible ordered thresholds. It reports PWM-level quantization (75% → 191/255 ≈ 74.90%) and a partial list of candidate field changes. For matching inventory, an offline full-size copy patches identified fields and preserves every other byte. Callback/derived-field semantics remain incomplete; no candidate reaches a setter. The proposal flattens identified coefficients between existing start/full thresholds, with firmware off/full-speed behavior still relevant. It does not establish fixed duty at every temperature.

In preset modes, stored Manual fields can be inactive or zero. The live F41c observations had Normal selected with zero manual fields. A controlled BIOS-made Manual curve capture is needed before preset-to-Manual conversion can be independently validated.

**Curve inspector** plots valid Manual fields using accumulated coefficients and slope/8 from the reviewed firmware GUI. A matching Normal preset uses the compiled reference. Mint shows the stored/initial estimate or factory reference; amber shows a fixed-level candidate. Below-start, source and controller behavior are excluded; the full threshold returns to 100%. This is not measured PWM/RPM.

Read/preview events are bounded to 80 in memory, with the latest 12 displayed. Raw changed-byte details appear only under **Verbose inspection**. Failed refresh clears previous current fields, selection and proposals while preserving the initial backup. No automatic upload, startup curve reapplication or continuous fan regulation exists.

## Private snapshots and comparison

The snapshot view provides **Save current snapshot**, **Use current as baseline**, and **Load baseline from path**, separately from the immutable initial backup. Read firmware again to compare current storage against the baseline. All 569 bytes are compared, including unidentified fields. Failed reads invalidate the current comparison; failed imports clear the prior baseline. Origin, exact inventory and attributes must agree.

Snapshots contain raw firmware bytes and board/BIOS inventory. Keep them private; use **.private.json**, which Git ignores. Saves sync to disk and refuse overwrite. Explicit saves require an existing parent folder; the automatic initial-backup folder is created as needed. Imports are bounded to 16 KiB, reject unknown schema/name/GUID/format/attributes and verify payload SHA256. Digests detect corruption, not authenticated provenance or write authorization.

The research profile requires X570 AORUS ELITE WIFI, F41c and release date 08/18/2026, plus strict variable format. This does not attest the installed firmware binary or physical board revision. Similar hardware does not inherit candidate support.

Write readiness separately reports inventory, current snapshot backup, initial recovery file, incomplete codec, unresolved policy and untested hardware persistence.

### CLI capture and offline comparison

Live capture requires administrator permissions. Demo needs no hardware. An explicit output is mandatory; full bytes are never printed to stdout. CLI capture does not create the GUI's automatic initial backup.

~~~powershell
New-Item -ItemType Directory -Path .data -Force
Start-Process -Wait -FilePath .\target\release\lightdriver-fan-debug.exe -ArgumentList @('--firmware-snapshot','--output','.data\before.private.json')
# After an independently chosen BIOS change or reboot:
Start-Process -Wait -FilePath .\target\release\lightdriver-fan-debug.exe -ArgumentList @('--firmware-snapshot','--output','.data\after.private.json')
# Entirely offline; no firmware or PawnIO access:
Start-Process -Wait -FilePath .\target\release\lightdriver-fan-debug.exe -ArgumentList @('--compare-firmware','.data\before.private.json','.data\after.private.json','--output','.data\comparison.json')
~~~

Comparison reports include hashes, identity, changed offsets and field names, excluding raw before/after bytes and payloads. Equal storage does not establish a performed write, fan response or reboot. Record these observations separately. For codec validation, capture a baseline, change only one case header's Manual curve through BIOS, capture again and record the exact BIOS changes. Restore through BIOS after the experiment. The app never changes BIOS settings or initiates a restart.

## Live fan speeds

Install the [official signed PawnIO driver](https://pawnio.eu/) normally. The unrestricted/unsigned edition is not supported. Obtain **LpcIO.bin from official [PawnIO.Modules 0.2.11](https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11)** and place it in **FanDebug-support/LpcIO.bin** beside the launcher or executable. Driver/module are external dependencies, not shipped in the repository.

The module must be 18,076 bytes, SHA256 **B3896A1CAB0D808FCA31FE2EBCAE045D59DAC690DA87B17C858BB8DA357EB45E**. Rust checks the digest before driver submission; the signed driver verifies the module signature. No PawnIOLib DLL or automatic installer is used.

Close competing motherboard tools such as SIV while testing. Run the lab as administrator and select **Start RPM monitor**. It polls asynchronously at 1 Hz with one sample in flight, a stop action, source/chip information and freshness. Readings over three seconds old are stale. Errors clear readings and stop monitoring; late results from replaced/stopped sessions cannot become current.

Recognition requires **X570 AORUS ELITE WIFI + IT8688 revision 1 or 2**, active monitoring, plausible I/O base and matching monitor vendor. Labels remain pending physical confirmation:

| Chip channel | Proposed board label |
| --- | --- |
| 1 | CPU_FAN |
| 2 | SYS_FAN1 |
| 3 | SYS_FAN2 |
| 4 | PCH_FAN |
| 5 | CPU_OPT |

The separate tachometer path uses fixed 16-bit counters for IT8688; older ITE mode-switch rules are not applied. Disabled tachometers remain unchanged. Zero count is invalid; 0xFFFF means no pulses (0 RPM), which cannot distinguish a stopped fan from a disconnected tach wire. Passive hubs usually forward one fan's tach signal, not a reading for every attached fan.

The adapter acquires the shared ISA mutex, uses configuration/register-selector port outputs and restores logical-device selection. It never changes PWM duty, mode, tach enable, thresholds, curves or EC data. Nonzero bank selection is rejected. This is read-only for fan settings, not zero port-output traffic. The mutex cannot coordinate tools/firmware that ignore it. No general port API is exposed.

Live checks on 10 October 2026 observed signed PawnIO 2.2.0, IT8688 revision 1, base 0x0A40, three changing nonzero readings and two no-pulse readings. Normal-token access was denied; elevated access succeeded. Header wiring, independent RPM accuracy and physical revision-2 support remain unverified.

### Bounded RPM support check

Request 1–5 samples (default three, one second apart), with no GUI or firmware read. It stops on error and refuses to replace existing output. Reports contain decoded values/status/source, not raw port or firmware dumps.

~~~powershell
Start-Process -Wait -FilePath .\target\release\lightdriver-fan-debug.exe -ArgumentList @('--rpm-check','--samples','3','--output','.data\rpm-check.json')
# Add --demo for synthetic readings.
~~~

Without --output, JSON goes to inherited stdout. Wait for this Windows GUI executable before reading its output file. It never requests elevation automatically.

## Demo and UI review

~~~powershell
.\target\release\lightdriver-fan-debug.exe --demo
.\target\release\lightdriver-fan-debug.exe --demo --page snapshots --capture .data\snapshots.png
.\target\release\lightdriver-fan-debug.exe --demo --restore-preview factory --size 1080x1750 --capture .data\factory.png
.\target\release\lightdriver-fan-debug.exe --demo --size 920x680 --capture .data\small.png
~~~

Demo fields/RPM are clearly synthetic and call no firmware or driver API. Capture and restore-preview flags are demo review options; screenshots exit after rendering. The destination parent must exist. --page also accepts curve and events.

## Gates before firmware writes

1. Independently validate full encoding, presets, callbacks and derived fields through controlled BIOS captures; preserve unknown bytes.
2. Establish exact writable board/BIOS identity gates and verified recovery, beyond saved backup data.
3. Resolve firmware lock policy through the standard API without bypassing protection.
4. Add one-header save, full variable readback, concurrent-change rejection and tested restoration; handle uncertain outcomes without blind retries.
5. Verify physical response, BIOS display, app exit, reboot, cold boot and resume separately.

See [firmware findings](../../docs/firmware-fan-persistence.md) and [FanControl study](../../docs/fancontrol-study.md). The lab is separate from Workbench and its service.

## Primary implementation references

- [PawnIO user-mode interface, pinned source](https://github.com/namazso/PawnIO/blob/9d52965895588b0ffa3703b72eec75ba19f4ccc0/PawnIO/include/pawnio_um.h): independently implemented Rust transport.
- [Official LpcIO module, reviewed source](https://github.com/namazso/PawnIO.Modules/blob/0.2.11/LpcIO.p): module entry points and ISA locking.
- [it87 driver, pinned source](https://github.com/frankcrawford/it87/blob/bd7a566c8a60fe0308d67e26761e16260368191a/it87.c): identities, register facts and 675000/count conversion; no Linux initialization writes were implemented.
