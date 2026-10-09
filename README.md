# LightDriver

A native Rust workbench for understanding, controlling, and verifying PC cooling and lighting. The desktop uses **Iced**; an independent local Rust service owns device access.

**Early development: the current milestone is read-only.** Pump curves and lighting are local drafts/previews. Hardware application, motherboard fan control, and case ARGB control are still pending.

![Native Iced Workbench with simulated telemetry](docs/workbench.png)

The screenshot is the running native application in demo mode. See the [verification record](docs/verification.md) for measured hardware evidence and remaining limits.

## Current support

| Path | Current behavior |
| --- | --- |
| NZXT Kraken X3 USB HID (X53 target) | Discovery and read-only coolant temperature, pump RPM, and pump duty |
| Kraken pump curve | Editable example curves; validated local drafts; device application disabled |
| Kraken ring and logo | Static local preview; device lighting writes pending |
| Radiator fans | Motherboard path; the X53 does not expose fan speed control |
| Lian Li LANCOOL 216 stock hub | Reference wiring only; motherboard PWM/ARGB adapters pending |
| Verification | Current connection/telemetry evidence and shareable diagnostics; untested controls stay pending |

X53, X63, and X73 share a USB identity, so discovery identifies the **Kraken X3 family**, rather than claiming to distinguish the radiator model. Both known X3 product IDs are recognized; support beyond the reference X53 needs hardware testing.

The stock case hub is a PWM/ARGB distribution path. Independent per-fan speed or RPM control is not assumed. The reference build records a Gigabyte X570 AORUS ELITE WIFI, CPU_FAN for radiator fans, and SYS_FAN1 for the case hub. The exact ARGB header still needs confirmation. This is recorded wiring, not motherboard auto-detection.

## Run on Windows

Install stable Rust with the MSVC toolchain and Visual Studio Build Tools with **Desktop development with C++**. Then:

```powershell
cargo build --workspace --locked
.\target\debug\lightdriver-desktop.exe --demo
```

Omit `--demo` for live read-only discovery:

```powershell
.\target\debug\lightdriver-desktop.exe
```

The desktop connects to the existing service or starts `lightdriver-service.exe` from the same directory. Build the whole workspace so both executables are present. The service runs without a console window when the UI starts it, and continues after the UI closes. No Windows startup task or system service is installed.

For a foreground service with visible diagnostics:

```powershell
.\target\debug\lightdriver-service.exe
# Ctrl+C stops it.
```

For a background service started by the UI, stop its process using Task Manager when finished. Live and demo services use separate local endpoints; demo never opens hardware.

Drafts and exported reports are saved under `%LOCALAPPDATA%\LightDriver`. Use `--data-dir <directory>` to select another location. Drafts are validated when loaded and saved. Saving a draft does not apply it to a device.

### Read-only diagnostics

```powershell
# Poll once without leaving a service running.
.\target\debug\lightdriver-service.exe --once
# Read a running service.
.\target\debug\lightdriver-service.exe --snapshot
# Same operations with simulated data.
.\target\debug\lightdriver-service.exe --demo --once
```

The read-only adapter does not initialize the Kraken or send USB commands. A detected device may therefore have no active telemetry stream. Read/open errors are surfaced without changing device settings or stopping another controller. Samples older than five seconds are not displayed as live readings.

## Architecture

```text
Iced desktop ── bounded, versioned local IPC ── Rust service
                                                 │
                                           Rust adapters
                                                 │
                                             USB / board
```

| Crate | Responsibility |
| --- | --- |
| `lightdriver-core` | Device/capability models, telemetry freshness, curve/draft validation |
| `lightdriver-ipc` | Versioned length-prefixed JSON over local sockets; Windows named pipes; bounded frames/timeouts |
| `lightdriver-devices` | Read-only Windows Kraken HID adapter and isolated demo adapter |
| `lightdriver-service` | Single owner per user/mode, polling and local snapshot requests |
| `lightdriver-desktop` | Native Workbench, local draft editing, capability evidence, diagnostic export |

All first-party runtime code is Rust. Windows HID uses `hidapi`'s native Windows backend; the UI renders with Iced's Tiny Skia backend. The API currently contains snapshot requests only. Future control commands belong in the service, alongside validation and device-specific capability checks.

The initial service is a **per-user background process**, not a privileged Windows Service. Remote control, privileged motherboard drivers, automatic startup, and packaging need separate design decisions. Local IPC currently relies on OS default permissions; authenticated control commands and explicit access policy must be designed before enabling writes or remote access.

## Development

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Windows CI runs these checks and builds both executables. Tests cover wire decoding and faults, telemetry freshness, curve constraints, draft replacement, demo isolation, bounded IPC, service lifecycle, and UI disconnect states. Tests use synthetic data and do not open hardware. Live hardware support currently targets Windows; other platforms can use the demo adapter but have not been validated.

Native rendering can be captured for review:

```powershell
.\target\debug\lightdriver-desktop.exe --demo --capture .data\workbench.png
.\target\debug\lightdriver-desktop.exe --demo --tab lighting --capture .data\lighting.png
```

`--tab` also accepts `capabilities` and `verification`. The app captures its native window after three seconds and exits; the independent service remains running.

For rendering checks, `--size 980x720` selects the minimum supported window size, `--device motherboard` selects the recorded board target, and `--no-autostart` prevents the initial connection attempt from launching a service.

## Next milestones

1. Validate Kraken telemetry and device lifecycle on the reference X53.
2. Design and measure pump/RGB command behavior, conflict handling, and recovery before enabling Apply.
3. Confirm case wiring and choose the motherboard PWM/ARGB integration approach.
4. Add opt-in hardware test reports so support labels reflect measured evidence.

## Protocol references

The Kraken status layout and identifiers were checked against the [Linux Kraken 3 driver](https://github.com/torvalds/linux/blob/master/drivers/hwmon/nzxt-kraken3.c) and [liquidctl X3 documentation](https://github.com/liquidctl/liquidctl/blob/main/docs/kraken-x3-z3-guide.md). The Rust parser is independently implemented. These references are not runtime dependencies.

## License

A project license has not yet been selected.
