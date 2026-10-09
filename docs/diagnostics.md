# Diagnostics

Diagnostics is one native tab with Basic and Advanced views, backed by the local Rust service.

Basic groups CPU load/temperature/power, system memory, detected GPUs and cooling. Advanced exposes per-sensor units, source, collection failures, freshness and a selectable cached graph. Min/max cover the rolling last 30 samples; they are not lifetime statistics. CPU/memory sampling and GPU APIs run on a dedicated service thread, separately from Kraken polling. The GUI requests snapshots at 1 Hz.

## Provider coverage

| Provider | Implemented readings | Validation |
| --- | --- | --- |
| Windows / sysinfo | Aggregate CPU load, first 16 logical CPU loads, used/total memory | Automated transport and model tests; live readings can be compared with Task Manager |
| AMD ADLX | GPU temperature/hotspot, load, board/chip power, fan RPM, clocks and used memory | Rust adapter implemented; per-model coverage needs hardware evidence |
| NVIDIA NVML | GPU temperature, reported device power, load, used/total memory, clocks and fan percentage | Rust adapter implemented; physical NVIDIA validation pending |
| Intel Level Zero Sysman | GPU temperature domains, root card average power, combined engine load | Rust adapter implemented; physical Intel validation pending |
| Kraken X3 HID | Coolant temperature, pump RPM and duty | Read-only reference X53 telemetry observed |
| CPU package sensors | Temperature / package power | Native adapter pending; unavailable in live mode |
| Motherboard fan sensors | Radiator / case RPM | Adapter pending |
| Whole-PC wall power | None | Requires an external meter or compatible PSU telemetry |

GPU providers require compatible installed driver APIs. An AMD-only PC can legitimately report NVIDIA NVML and Intel Sysman as unavailable. Intel's optional loader must expose Sysman and supported devices; support is not promised for every Intel integrated/discrete GPU. No vendor DLL, driver, C/C++ helper, .NET process or Python runtime is bundled. First-party application/collector code is Rust and calls the installed native driver ABI.

DLLs are loaded from absolute installed-system paths, avoiding working-directory DLL discovery. NVIDIA also checks the standard Program Files NVSMI installation. No tuning, fan, voltage, power-limit or other hardware setter is called.

## Semantics

- Zero can be a valid reading. Missing, invalid, stale, future and offline readings never become zero or simulated data.
- Five seconds is the freshness limit. Service disconnection hides the previous values/history.
- Histories reset when a sensor disappears/fails or when a sampling gap exceeds the freshness limit.
- Maximum 48 sensors and 30 samples per sensor keep IPC and chart work bounded. Advanced CPU breakdown is limited to 16 logical CPUs; the aggregate includes all CPUs.
- Intel power is delta microjoules divided by delta microseconds, yielding interval-average watts. First samples, zero time, counter resets and read failures are unavailable.
- NVIDIA power is the NVML device-reported scope. AMD chip power and board power are separate. Values are not summed into wall power.
- Thermal-zone temperatures are not labeled as CPU package temperature.
- Demo never initializes system/vendor collectors or opens hardware.

The snapshot protocol is now version 2. Restart an older service after updating the app; version mismatches are rejected.

## Native review

```powershell
.\target\release\lightdriver-desktop.exe --demo --tab diagnostics
.\target\release\lightdriver-desktop.exe --demo --tab diagnostics --advanced
```

These options also work with `--capture` and `--size 980x720`. Screenshots use actual Iced rendering.

## Primary references

- [AMD ADLX metrics](https://gpuopen.com/manuals/adlx/programming-with-adlx/adlx-samples/rust-samples/perfgpumetrics/)
- [NVIDIA NVML API](https://docs.nvidia.com/deploy/nvml-api/index.html)
- [Intel Sysman API](https://oneapi-src.github.io/level-zero-spec/level-zero/latest/sysman/api.html)
- [Intel published ABI headers](https://github.com/oneapi-src/level-zero/tree/master/include)
- [Pinned Rust ADLX binding](https://docs.rs/adlx/0.0.0-alpha.1/adlx/)
- [Rust NVML wrapper](https://docs.rs/nvml-wrapper/0.13.0/nvml_wrapper/)
