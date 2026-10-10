# Motherboard fan curves from Windows

Investigation updated 10 October 2026. Scope: edit a curve in LightDriver, apply it once, and let the motherboard regulate its fans using its own temperature inputs. A continuous software temperature-to-duty loop is outside this feature.

## Conclusion

There is a credible path to autonomous hardware curves on recognized controllers, but LightDriver cannot claim it works on the reference board yet. Windows curve editing is established in GIGABYTE's own software; third-party curve programming, exit behavior and reboot persistence need separate verification.

Recommend a controller-resident curve with an optional **reapply at Windows startup/resume** setting. This is a proposal, not an implemented or accepted architecture decision. It avoids continuous duty updates while accounting for firmware reinitialization. It does not change the curve used before Windows starts.

A true **save into BIOS** feature is not currently supported by an identified vendor API. Neither a saved LightDriver profile nor a successful register write establishes firmware persistence.

## Evidence and limits

| Finding | Evidence | Limit |
| --- | --- | --- |
| Windows fan-curve editing exists | Official X570 guide, System Information Viewer / Smart Fan 5 Advanced | No third-party API, process-exit guarantee or persistent BIOS-save contract documented there |
| The reference board has a candidate ITE layout | Maintained it87 board catalog associates X570 AORUS ELITE / ELITE WIFI with IT8688 and SIV ID 0x40050607 | Catalog evidence; exact chip revision, firmware identity and wiring have not been read locally |
| Hardware automatic regulation exists | it87 defines newer automatic PWM thresholds, starting duty, slope and hysteresis | A simple slope is not equivalent to an arbitrary multi-point BIOS curve |
| Revision matters | it87 handles additional banked vectors for IT8688 revision 2 and IT8689 | Editing just the primary curve fields may leave other vectors active |
| Full BIOS-style vector editing remains incomplete upstream | it87 README lists vector support as unfinished; its code restores firmware snapshots when leaving manual overrides | Its working manual control does not prove complete curve programming |
| PawnIO provides a possible Windows transport | Released LpcIO module exposes permitted port access; IsaBridgeEC exposes MMIO access | No curve abstraction or BIOS persistence operation; applicable access path still needs verification |

The it87 source and catalog were inspected at revision `bd7a566c8a60fe0308d67e26761e16260368191a`. A repeated direct download of the pinned source matched the local file hash. Runtime chip detection remains necessary.

## Candidate board mapping

The pinned catalog gives this mapping for SIV ID 0x40050607. These are reference labels, not proof of the current machine's detected channels.

| Controller channel | Catalog label | Initial writable scope |
| --- | --- | --- |
| 1 | CPU_FAN | Only after matching hardware and validation |
| 2 | SYS_FAN1 | Only after matching hardware and validation |
| 3 | SYS_FAN2 | Only after matching hardware and validation |
| 4 | PCH_FAN | Exclude chipset cooling from the initial editor |
| 5 | CPU_OPT | Only after matching hardware and validation |
| 6 | Unmapped | Unsupported |

Catalog temperature labels are System 1, System 2, CPU, PCIEX16, VRM MOS and PCH. The adapter must separately verify which inputs each fan channel can actually select. An onboard CPU input may differ from the AMD SDK reading displayed in Diagnostics.

For a passive PWM hub, the curve belongs to its connected motherboard fan header. Fans sharing that PWM signal share its curve; one reported tachometer does not establish the speed of every connected fan. The ARGB header assignment is separate from PWM fan wiring.

## What applying a curve would require

1. Match board identity, chip ID/revision and runtime access path to an explicit supported profile. Never infer writable support from the motherboard name alone.
2. Read the current automatic mode, complete primary and extra-vector state, sensor selection, hysteresis and PWM/DC configuration. Preserve an original snapshot for rollback.
3. Describe the controller's actual capabilities to the UI: available sensors, curve representation, point/segment limits, quantization, fan-stop support and persistence evidence.
4. Validate a draft, encode it into supported thresholds/slopes/vectors, and preview the representable curve. Reject unsupported requests or show approximation for user acceptance.
5. Apply through the privileged Rust service with serialized access. Preserve unrelated bits, follow a validated write sequence and leave regulation in automatic mode.
6. Read back hardware state and compare decoded values, mode and selected sensor. Report requested versus accepted values and a clear failure if verification is incomplete.
7. Once physically validated, leave the successfully applied automatic curve active on normal GUI/service exit. A failed transaction should restore the original automatic configuration and verify restoration.

The newer primary it87 fields include fan-off, start and full-speed temperatures, starting PWM, a slope in fractional PWM units and hysteresis. Full vector semantics, segment interpolation, limits and safe sequencing remain unresolved for the exact reference revision. A register list is not enough to enable writes.

GIGABYTE's other controller generations can use H2RAM vector tables and different transports. Those layouts must have separate adapters; addresses or rules for IT879x must not be reused for IT8688.

## Exit and persistence

| Event | Expected behavior of the proposed hardware-curve design | Verification status |
| --- | --- | --- |
| Close the Workbench GUI | Controller continues regulating | Plausible; untested |
| Stop all LightDriver processes | Controller continues if the curve uses onboard inputs and firmware does not replace it | Plausible; untested |
| Restart or crash the service | Applied curve remains until controller reset/overwrite | Untested; service must read hardware rather than silently restore an old draft |
| Sleep / resume | Firmware may restore its own configuration; optionally reapply after detecting resume and revalidating identity | Untested |
| Windows restart | Assume firmware reinitializes the controller; optionally reapply the saved profile when the startup service runs | Planning assumption, not a measured result |
| Full shutdown and cold power-on | Treat runtime state as nonpersistent unless exact hardware evidence proves otherwise | Untested |
| Boot BIOS setup, another OS, or Windows without LightDriver startup | Only a verified firmware-persistent curve could guarantee the new profile there | No supported persistence path established |

App exit and service exit are separate tests. The current application already leaves its local service running after the GUI closes, so closing the GUI alone cannot prove autonomous hardware regulation.

Boot reapplication means **the profile takes effect again after Windows starts**. It does not mean the controller retained it across reboot, or that BIOS settings changed. Until then, the motherboard's existing firmware curve governs cooling.

## Rust architecture proposal

Keep curve modeling and validation in the Rust core, board/chip encoding in device adapters, privileged access and apply transactions in the Rust service, and the draft editor in Iced.

Expose distinct capabilities for reading a hardware curve, applying an autonomous curve, selectable onboard inputs and verified persistence. Keep unsupported/unknown separate from supported and from physically tested. New boards can add profiles and codecs without changing the editor or assuming identical point counts.

An optional startup/resume path would store an explicitly enabled profile, validate the board/controller identity again and program the hardware once per initialization event. The monitoring service can remain active for diagnostics, but fan regulation would not depend on its polling loop. No periodic duty updates or hidden software-curve fallback.

The existing service is not installed as an automatically starting Windows service. Creating startup behavior is a separate implementation decision and setup action; it has not been added by this investigation.

## Transport and source boundaries

The selected signed PawnIO driver remains an external dependency. The released module reference is `52a7e536dff3e53c96917a28caac5e0fa6510696` (0.2.11). Its LpcIO access functions mask register values to a byte; banked runtime addressing still requires the correct chip protocol. IsaBridgeEC is a separate MMIO option, not proof that every it87 firmware workaround is available on Windows.

Shared register-index/bank operations need serialized access and the established ISA access mutex. Coordination with other monitoring/control applications and firmware is part of validation; a user-mode mutex alone cannot prevent firmware activity.

LibreHardwareMonitor's manual-duty implementation and default restoration are useful access references, but do not implement this requested curve workflow. The interrupted manual takeover implementation remains archived outside shipped source.

Upstream source was inspected for protocol facts; no upstream C/Pawn code was copied into a Rust adapter or shipped by this investigation. No fan registers were probed or written, no driver was installed, and no reboot was performed.

## Validation gates for later hardware testing

1. Read-only identity, revision, header/sensor mapping and complete curve snapshot.
2. Offline codec checks using documented captures: round trips, quantization, boundaries and rejected unsupported curves.
3. An explicitly approved apply on a noncritical, identified fan header; verify registers and actual temperature-dependent RPM behavior.
4. Repeat after GUI exit, then after stopping every LightDriver process. Readback alone does not prove that the fan follows the curve.
5. Test resume, ordinary restart and full cold power-on separately. Record whether values were retained, reset or reapplied; account for Windows Fast Startup.
6. Verify rollback and conflicts with vendor software before advertising writable support for that exact board/controller profile.

Physical testing is deferred. Motherboard fan-curve programming remains unimplemented.

## Sources

- [GIGABYTE X570 SIV/Smart Fan 5 guide, page 22](https://download.gigabyte.com/FileList/Manual/mb_manual_x570-features.pdf).
- [Maintained it87 controller implementation, pinned revision](https://github.com/frankcrawford/it87/blob/bd7a566c8a60fe0308d67e26761e16260368191a/it87.c).
- [it87 capabilities and unfinished vector support, pinned README](https://github.com/frankcrawford/it87/blob/bd7a566c8a60fe0308d67e26761e16260368191a/README).
- [it87 AMD GIGABYTE board/header catalog, pinned revision](https://github.com/frankcrawford/it87/blob/bd7a566c8a60fe0308d67e26761e16260368191a/Sensors%20configs/Gigabyte/configs/gigabyte-it87-amd.conf).
- [PawnIO official signed distribution](https://pawnio.eu/), [released LpcIO interface](https://github.com/namazso/PawnIO.Modules/blob/52a7e536dff3e53c96917a28caac5e0fa6510696/LpcIO.p) and [released IsaBridgeEC interface](https://github.com/namazso/PawnIO.Modules/blob/52a7e536dff3e53c96917a28caac5e0fa6510696/IsaBridgeEC.p).
- [LibreHardwareMonitor IT87XX reference, pinned revision](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/76150732c2df9241415eec216001bec4557c9033/LibreHardwareMonitorLib/Hardware/Motherboard/Lpc/IT87XX.cs).
