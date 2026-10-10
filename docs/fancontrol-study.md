# FanControl source study

Studied 10 October 2026, for LightDriver's firmware fan lab.

## What exists

[FanControl's official README](https://github.com/Rem0o/FanControl.Releases/blob/master/README.md) documents live sensors, software fan curves, profiles and a Windows boot service. V238 and later use a PawnIO build of LibreHardwareMonitor. The main FanControl application is closed source; the releases repository is not its application source.

The README's BIOS advice is to disable smart regulation and start with a fixed default. Its public [plugin contract](https://github.com/Rem0o/FanControl.Releases/wiki/Plugins) offers Update, Set(percent) and Reset, with RPM pairing and a 1 Hz sensor cycle. These are useful runtime-control interfaces; no firmware-save contract is exposed there.

We found **no implementation of saving this GIGABYTE BIOS variable** in the reviewed public backend. Running a service on Windows startup is not evidence of firmware settings retained before Windows boots. This conclusion is scoped to the public code/documentation inspected; we cannot audit FanControl's closed core.

## Public code worth studying

LibreHardwareMonitor source pinned to commit **76150732c2df9241415eec216001bec4557c9033**:

| Source | Useful details | Relationship to our goal |
| --- | --- | --- |
| [IT87XX.cs](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/76150732c2df9241415eec216001bec4557c9033/LibreHardwareMonitorLib/Hardware/Motherboard/Lpc/IT87XX.cs) | Chip differences, tachometer registers, invalid counters, ISA locking, SetControl and restoration | Relevant to RPM; its control writes runtime PWM registers, not UEFI curve storage |
| [PawnIo/LpcIO.cs](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/76150732c2df9241415eec216001bec4557c9033/LibreHardwareMonitorLib/PawnIo/LpcIO.cs) | Signed-module calls, slot/BAR selection and port transport | Confirms the transport model; PawnIO is not a firmware codec |
| [SuperIOHardware.cs](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/76150732c2df9241415eec216001bec4557c9033/LibreHardwareMonitorLib/Hardware/Motherboard/SuperIOHardware.cs) | Board sensor names and mapping | A research reference, not physical confirmation of our wiring |
| [FanControl plugin documentation](https://github.com/Rem0o/FanControl.Releases/wiki/Plugins) | Adapter lifecycle, stable sensor IDs and RPM/control pairing | Useful patterns for future Rust capability adapters |

The cached IT87XX.cs matches pinned GitHub blob cc3dfd301e46cca5a92801890c512e0af422f8e1; SHA256 A3B08A3253CA1DC2744CB13D4798019F2221C5266BA7E6A546481CB4466C2D10.

### Observed runtime control path

IT87XX.SetControl saves original PWM/output settings, can disable a GIGABYTE controller to avoid overrides, clears automatic PWM operation and writes a requested duty. Passing null invokes RestoreDefaultFanPwmControl, which restores saved registers and eventually the GIGABYTE controller state. These are not BIOS curve edits.

IT87XX.Update distinguishes automatic operation from software operation. Its 16-bit RPM conversion is 1.35e6 / (counter * 2), equivalent to 675000 / counter. It treats 0xFFFF as no pulses and additionally rejects very small counters. LightDriver's current reader follows the reviewed it87 register facts and coherence checks, with narrower board/revision recognition and no fan-setting writes.

The source headers identify MPL-2.0. This study uses protocol facts and design patterns in independent Rust code; no C# source was copied or mechanically ported into LightDriver.

## Implementation consequence

Keep these independent capabilities:

- Live RPM through the signed PawnIO transport.
- Runtime percentage control, deferred under Moosa's selected scope.
- Firmware inspection and private snapshot comparison through the standard UEFI getter.
- Firmware save and restoration, still locked pending codec/update-policy validation.
- Immediate response and retention after reboot/cold boot/resume, requiring separate observations.

FanControl does not remove the need to finish the firmware research. The lab now supports local full-variable snapshots, strict imports, complete byte comparison including unknown fields, manual-curve research plots, and in-memory fixed-level patch candidates. These tools allow controlled BIOS-change captures to validate the remaining semantics without adding a firmware setter.
