# Device control and expandable support

Monitoring and device setting changes are separate actions. A local draft never represents stored settings read back from hardware. Profiles load as drafts; startup, reconnection and service handoff never apply them automatically.

## Kraken X3

The Windows HID adapter identifies the X3 family from supported USB IDs rather than inferring the radiator size. Explicit Enable control opens a service-owned handle, requests firmware/channel information and initializes status reports. It does not apply a curve or color.

Pump curves contain 2–16 ascending coolant-temperature points, within 20–59 °C. Duty is 20–100%, never decreases, and the last point must reach 100%. The service interpolates the 40 hardware duty entries; the 59 °C entry is always 100%. The device executes the coolant curve, so it does not depend on a GUI animation loop.

Apply curve requires fresh, noncritical coolant telemetry. The explicit Pump 100% action remains available when the handle is enabled even if telemetry is missing or critical. Ring/logo lighting is independent of coolant telemetry.

Ring and logo can be selected separately or together. Static, breathing, spectrum and off are encoded independently in Rust. Brightness scales single colors; spectrum is a controller-native effect with no separate brightness adjustment in this version.

Release closes the control handle. The last device settings remain; previous controller settings cannot be restored because stored-setting readback is unavailable.

## GIGABYTE ITE 8297

This is a native Rust USB HID adapter; neither OpenRGB nor a GIGABYTE SDK is a runtime dependency. The initial board-layout allowlist is X570 AORUS ELITE, ELITE WIFI, PRO, PRO WIFI and ULTRA. Detection requires one matching 048D:8297 interface with usage page FF89 and usage CC. Unknown/ambiguous layouts or interfaces remain unavailable for writes.

Enable RGB control queries the controller identity and checks its report, IT8297 product string and primary controller instance. It sends no all-zone reset. Static, breathing, color cycle and off use hardware effect packets followed by a commit mask for only the chosen header.

The adapter exposes D_LED1 and D_LED2 as named capabilities. Each user assigns a passive hub to an actual header. The assignment is bound to the detected board name, and no header is a global default. Changing the selection requires saving it before applying. Other board layouts and controller generations require separate validated mapping data.

Only controller-native effects are implemented. Switching a controller left in a different direct-stream mode by another application is not handled. The adapter does not change global mode masks, LED counts, calibration, firmware or flash storage. Sleep/power-cycle persistence is unverified and automatic restoration is outside this milestone.

Motherboard fan curves remain in BIOS in this version. A passive case hub follows its PWM/ARGB inputs; individual fan speed, RPM and lighting identity cannot be inferred from the hub.

## Service and evidence

Commands use bounded, versioned local IPC with an exact target and a short deadline. Polling and writes share a serialized device actor. Live/demo mismatch, invalid payloads, unknown targets and expired requests are rejected. Duplicate request identifiers return the original receipt; conflicting payloads do not replace it. The bounded cache rejects new work instead of evicting still-valid commands.

Known competing cooling/lighting processes block Enable and Apply. The service does not stop them. This is a conservative process check, not a guarantee of exclusive Windows HID access.

USB submission means the OS accepted a command. It is not device-setting readback, visible-response confirmation or a verified-support badge. Errors may follow a partial lighting sequence; the app reports uncertainty and does not retry automatically. Local tester reports record selected-device observations without automatic uploads or verification claims.

## Source research

The old RGB Fusion SDK B19.0311.1 contains an x86 motherboard DLL and documents older controller/board support. It is not loaded by this x64 application. Vendor SDK adapters remain an option for other supported hardware.

Protocol and layout facts were checked against OpenRGB commit [b190d3e](https://github.com/CalcProgrammer1/OpenRGB/tree/b190d3e5afb6bd68b4bfa3e948947878d2bf1ba9/Controllers/GigabyteRGBFusion2USBController):

- [HID detection](https://github.com/CalcProgrammer1/OpenRGB/blob/b190d3e5afb6bd68b4bfa3e948947878d2bf1ba9/Controllers/GigabyteRGBFusion2USBController/GigabyteRGBFusion2USBControllerDetect.cpp)
- [Board layouts](https://github.com/CalcProgrammer1/OpenRGB/blob/b190d3e5afb6bd68b4bfa3e948947878d2bf1ba9/Controllers/GigabyteRGBFusion2USBController/GigabyteFusion2USB_Devices.cpp)
- [USB packet reference](https://github.com/CalcProgrammer1/OpenRGB/blob/b190d3e5afb6bd68b4bfa3e948947878d2bf1ba9/Controllers/GigabyteRGBFusion2USBController/GigabyteRGBFusion2USBController.h)

OpenRGB's research source remains outside version control. The first-party runtime is independently implemented in Rust. Its constructor's all-zone reset, calibration, direct streaming and flash operations were not adopted. The project license remains undecided.

Kraken wire behavior was checked against [liquidctl's Kraken 3 driver](https://github.com/liquidctl/liquidctl/blob/main/liquidctl/driver/kraken3.py). These projects are source references, not application dependencies.
