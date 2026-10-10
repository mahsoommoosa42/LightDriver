# Motherboard fan curves from Windows

Scope clarified on 10 October 2026: edit a motherboard fan curve in LightDriver, apply it to the motherboard, and leave temperature-to-fan regulation with the motherboard controller. Manual-duty takeover and a continuous software curve loop are outside this feature.

## What is established

GIGABYTE's X570 documentation describes SIV Smart Fan 5 Advanced for editing temperature-dependent fan behavior from Windows. The X570 AORUS ELITE WIFI product page also advertises Smart Fan 5 and SIV. Therefore Windows-side curve editing exists in the vendor's supported software.

This does not document an API for third-party curve programming, prove that SIV exits without a background dependency, or guarantee that a Windows-applied curve updates persistent BIOS settings.

## Desired flow

1. Discover a recognized board/controller and enumerate actual fan headers and supported onboard temperature sources.
2. Read the current automatic curve and controller limits.
3. Edit a draft that respects the controller's point count, slope/threshold format, PWM/DC mode and sensor choices.
4. Apply with explicit hardware permission, verify readback, and leave the controller in automatic mode.
5. Explain persistence accurately: session/controller memory, reapply after startup, or firmware-persistent, only after each behavior is verified.

The app should not require an active GUI or a continuous user-mode temperature loop to run an applied hardware curve. GPU-derived or other software-only temperature sources are outside this autonomous motherboard mode.

## Driver boundary

The user selected a Rust service with the externally installed signed PawnIO driver. That remains a possible transport for supported register access. PawnIO itself does not define motherboard fan curves, provide a vendor curve API, or save settings to BIOS.

LibreHardwareMonitor's IT87XX manual-duty implementation is a reference for chip access and original-setting restoration, but it does not satisfy this clarified requirement. The interrupted manual-duty/lease implementation was archived outside the shipped source; the application remains at the previously working protocol and control feature set. No live fan settings were changed and no PawnIO driver was installed.

The Linux it87 implementation shows hardware automatic-PWM mechanisms with chip-dependent temperature thresholds, hysteresis, starting PWM and slopes. That is a useful protocol reference, not proof that the host's exact IT8688E revision implements the same layout. Do not copy a related chip's register table into a writable adapter.

## Remaining investigation

- Identify the exact Super I/O/EC revision and the connector-to-channel mapping without changing fan regulation.
- Obtain the exact automatic-curve register/command format and its onboard sensor mapping.
- Determine whether firmware or a second EC overwrites those registers.
- Verify readback, continued autonomous operation after application/service exit, reboot behavior and power-cycle behavior separately.
- Implement only verified board profiles with a capability-based adapter boundary; unknown boards remain unsupported without guessed writes.
- Avoid BIOS flashing and generic UEFI-variable writes as a workaround.

Physical write testing is deferred. This document records feasibility and requirements; motherboard curve programming is not implemented yet.

## Sources

- [GIGABYTE X570 SIV/Smart Fan 5 guide](https://download.gigabyte.com/FileList/Manual/mb_manual_x570-features.pdf).
- [X570 AORUS ELITE WIFI features](https://www.gigabyte.com/FileUpload/Global/KeyFeature/1238/index.html).
- [PawnIO official signed distribution](https://pawnio.eu/) and [official modules](https://github.com/namazso/PawnIO.Modules).
- [LibreHardwareMonitor IT87XX](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/76150732c2df9241415eec216001bec4557c9033/LibreHardwareMonitorLib/Hardware/Motherboard/Lpc/IT87XX.cs), inspected at this pinned revision.
- [Linux it87 automatic PWM implementation](https://github.com/torvalds/linux/blob/master/drivers/hwmon/it87.c); consult an exact revision before implementation.
