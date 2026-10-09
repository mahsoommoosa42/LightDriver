# Read-only milestone verification

Measured on 9 October 2026, Windows x64, Rust 1.99.0.

## Software checks

- The full Cargo workspace builds.
- `cargo fmt --all -- --check` and Clippy with `-D warnings` pass.
- 11 automated tests pass, including a real process/named-pipe lifecycle test in demo mode.
- A second service cannot claim the same per-user/mode endpoint.
- The service responds after previous clients exit; live clients cannot receive demo data.
- Invalid draft saves preserve the previous valid file; replacement saves and reloads work on Windows.
- The UI removes live values after disconnection, mode changes, and stale telemetry.

## Native UI checks

Rendered and inspected the actual Iced windows: pump profile, lighting, capability matrix, verification, recorded motherboard target, and offline state. Default layout is 1180 × 880; lighting was also checked at the supported minimum 980 × 720. Smaller windows use vertical scrolling. Demo data and local drafts are labeled. Device application is disabled.

## Reference hardware evidence

A connected Kraken X3 on the reference X53 build produced real read-only USB reports. One observed sample was **33.0 °C coolant, 2,047 RPM, 69% duty**. This is a dated observation, not a fixed expected operating point. A live service launched by the UI continued returning Kraken telemetry after the UI exited.

The read-only adapter did not send initialization, pump, lighting, or feature-report commands. The USB identity establishes the X3 family; it does not independently distinguish an X53 from X63/X73.

The Codex filesystem/USB sandbox did not expose the device to the first test process. Normal Windows access was required for the live test. Windows Smart App Control initially blocked unsigned compiler helpers; the machine owner chose to turn it off. The project does not change Windows security settings.

## Still unverified

- Pump command application, measured RPM response, device-stored profiles, and power-cycle recovery.
- Ring/logo USB lighting writes.
- Motherboard PWM/ARGB detection, control, conflict handling, and the final ARGB wiring.
- Hot unplug/replug on physical hardware, other X3 revisions, other boards, and other operating systems.
- Privileged service installation, startup behavior, packaging, and remote access.

Passing the telemetry milestone does not verify these control features. Hardware testers should report measured behavior per capability when those adapters become available.
