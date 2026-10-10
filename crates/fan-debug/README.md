# Firmware fan lab

Standalone native Rust/Iced debug app for firmware-settings inspection and **optional live motherboard RPM**. Fixed-level proposals are previews. **Write + read back and Restore original stay disabled:** Moosa chose complete firmware decoding before enabling writes, rather than temporary PWM control.

## Run

Build an optimized GPU-first version:

```powershell
cargo build -p lightdriver-fan-debug --release --locked
.\target\release\lightdriver-fan-debug.exe
# Or double-click FanDebug.cmd.
```

No firmware read or hardware polling starts automatically. The tool starts no service and makes no automatic elevation request. For this standalone lab, privileged operations run in its own explicitly elevated process; a finished Workbench integration belongs in the local privileged Rust service.

## Live fan speeds

Install the [official signed PawnIO driver](https://pawnio.eu/) using its normal installer. The unrestricted/unsigned edition is not the supported setup. Obtain **LpcIO.bin from official [PawnIO.Modules 0.2.11](https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11)** and place it in **FanDebug-support/LpcIO.bin**, next to FanDebug.cmd or the executable. The support directory is ignored by Git. Driver and module are external dependencies, not shipped in this repository.

The module must be exactly 18,076 bytes, SHA256 **B3896A1CAB0D808FCA31FE2EBCAE045D59DAC690DA87B17C858BB8DA357EB45E**. Rust checks this digest before sending it to the driver. The official driver also verifies its module signature. There is no PawnIOLib DLL loading or automatic installer execution.

Close competing motherboard tools such as SIV while testing. Right-click FanDebug.cmd and choose **Run as administrator** if driver access is denied. Click **Start RPM monitor**. It polls once per second asynchronously, with one sample in flight, a stop action, source/chip information, per-channel status and sample age. Samples over three seconds old display as stale. Errors clear the previous readings and stop the monitor; restart explicitly after fixing the cause. Late results from stopped/replaced sessions cannot become current.

Initial recognition requires **X570 AORUS ELITE WIFI + IT8688 revision 2**, active hardware monitoring, plausible I/O base and matching monitor vendor identity. Similar board names and unknown chips do not inherit support. Channel names are research-derived labels **pending physical confirmation**:

| Chip channel | Proposed board label |
| --- | --- |
| 1 | CPU_FAN |
| 2 | SYS_FAN1 |
| 3 | SYS_FAN2 |
| 4 | PCH_FAN |
| 5 | CPU_OPT |

Live RPM uses a separate tachometer path from the UEFI store. Disabled tachometers and unsupported 8-bit modes remain unchanged and show unavailable. A zero counter is invalid; 0xFFFF means no tach pulses and is shown as 0 RPM, which cannot distinguish a stopped fan from a disconnected tach wire. A passive hub usually forwards one fan's tach signal; this is not individual monitoring of every fan connected to the hub.

The reader acquires the shared global ISA mutex, enters/exits Super I/O configuration mode, selects logical devices, and writes register **selectors** required for reads. The official module enumerates BARs through logical-device selection; the original selection is restored on success and failure. It never writes PWM duty, fan mode, tach enable, thresholds, curves or the EC data port. Nonzero bank selection is rejected rather than changed. This is read-only with respect to fan settings; it is not zero port-output traffic. The mutex cannot coordinate firmware or tools which ignore it. No general-purpose port API is exposed to the UI.

The code builds and its conversion, identity, module-hash and session-state tests pass. **Physical RPM validation remains pending:** PawnIO was absent during development, so real tachometers and proposed header mapping have not yet been verified.

## Firmware inspection and percentage previews

Click **Read firmware**. Windows may require elevation for the getter; close the app, launch FanDebug.cmd as administrator and read again. The shared getter temporarily enables an existing process privilege, restores it and exposes only metadata and identified scalar fields. The full variable buffer is discarded, never exported. Firmware reads do not run on the RPM timer.

Select SYS_FAN1 or SYS_FAN2 after the reference format matches. The labels come from analyzed firmware forms; they do not identify physical wiring or constitute a writable board/BIOS allowlist. CPU and chipset targets are excluded from proposed edits. Other firmware formats return no decoded fields.

Choose 30%, 50%, 75% or 100%, or a value between 20–100%. Preview requires stored Manual mode and sensible ordered thresholds. It reports PWM-level quantization (75% → 191/255 ≈ 74.90%) and a **partial** list of candidate field changes; it does not construct a writable payload. The candidate flattens identified coefficients between existing start/full thresholds, with firmware off/full-speed behavior still relevant. It does not establish a fixed duty at every temperature. Preset synthesis and other derived/update semantics remain incomplete.

Stored scalars are separate from live RPM. In preset modes firmware may synthesize values instead of using stored manual fields. A future variable write/readback would establish storage acceptance only; the firmware may wait until next boot to program the controller. Fan response and reboot/cold-boot retention need separate observations.

Read/preview events are bounded to 80 entries in memory; the latest 12 are displayed. No raw firmware payload, automatic export/upload, startup reapplication or continuous fan regulation exists. Failed refresh clears old fields, selection and proposal.

## Demo and review

```powershell
.\target\release\lightdriver-fan-debug.exe --demo
.\target\release\lightdriver-fan-debug.exe --demo --capture .data\fan-lab.png
.\target\release\lightdriver-fan-debug.exe --demo --size 920x680 --capture .data\fan-lab-small.png
```

Demo fields and RPM are **synthetic** and call no firmware or driver API. Capture is accepted only in demo mode, saves a rendered native screenshot and exits. The destination parent must exist.

## Before firmware writes can be enabled

1. Complete and independently validate encoding, preset handling and derived fields; preserve unrelated bytes.
2. Establish exact board/BIOS identity gates and a complete private original snapshot for recovery.
3. Resolve firmware lock behavior through the standard API without bypassing protection.
4. Add one-header save, byte-for-byte variable readback and verified restoration; reject concurrent changes and report uncertain outcomes without blind retries.
5. Verify actual response, BIOS display, reboot, cold boot and resume separately.

See [firmware findings](../../docs/firmware-fan-persistence.md). This debug app has not been integrated into Workbench or its service.

## Primary implementation references

- [PawnIO user-mode interface, pinned source](https://github.com/namazso/PawnIO/blob/9d52965895588b0ffa3703b72eec75ba19f4ccc0/PawnIO/include/pawnio_um.h): device IOCTL protocol; the adapter is independently implemented in Rust.
- [Official LpcIO module, reviewed source](https://github.com/namazso/PawnIO.Modules/blob/0.2.11/LpcIO.p): allowed ports, BAR enumeration and shared-mutex requirement. The external module is LGPL-2.1-or-later; its source/license are available from that repository.
- [it87 driver, pinned source](https://github.com/frankcrawford/it87/blob/bd7a566c8a60fe0308d67e26761e16260368191a/it87.c): IT8688 identity, tachometer register facts, mode/enable bits and 16-bit conversion 675000/count. Linux initialization writes were deliberately not implemented.
