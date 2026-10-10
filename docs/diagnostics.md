# Diagnostics

Diagnostics is a single native view for **CPU and GPU load, temperature and clock speed**. CPU and GPU load use cached radial bars. Missing readings show a neutral track and a dash; valid zero remains 0%. Power, memory and cooling measurements can remain in diagnostic exports, but are not summary cards in this view.

**Show cores** expands a responsive CPU diagram. Each tile represents one physical core, with a separate load bar for each SMT thread and the OS reported clock. Rows adapt to the available width; large diagrams scroll vertically.

## CPU topology

The service reads Windows `GetLogicalProcessorInformationEx` relationships. Tiles are grouped by socket and shared L3 cache affinity. Shared L3 groups are useful for CCX-like layouts; they are not claimed to be vendor CCX IDs or inferred from core counts. Missing or ambiguous cache relationships remain ungrouped. Hybrid efficiency classes are shown when Windows reports differing classes, without assuming a universal P/E classification.

Thread addresses contain both processor group and processor number. Load and frequency use language-neutral Windows **Processor Information** counters, including systems with more than 64 logical processors. A physical core's load is the mean of its measured SMT thread loads; an incomplete set of thread samples has no aggregate. Rate counters require two valid samples. The diagram is bounded to 256 physical cores and 512 threads; any truncation is explicit. Static topology is refreshed on service restart; live CPU hotplug has not been validated.

Temperature appears at CPU package level only. Individual-core temperatures are outside the current scope because reliable vendor-to-core sensor mapping is not established. Core tiles focus on measured load, SMT threads and reported clock. Demo readings and topology are explicitly simulated.

## AMD CPU sensors

Install the optional [AMD Ryzen Master Monitoring SDK](https://www.amd.com/en/developer/ryzen-master-monitoring-sdk.html). The independent Rust adapter currently supports the exact **3.1.1.5478 Windows x64 ABI** and checks both runtime file versions before loading them. No AMD SDK headers, binaries or driver are redistributed by LightDriver.

Select **Enable CPU sensors** in the CPU card. This explicitly asks Windows UAC to start the local service with administrator permissions while the UI stays at normal permissions. The ordinary live service relinquishes its endpoint; demo and already-elevated services reject that handoff. Canceling UAC restores ordinary monitoring. This is a per-session background process, not an installed Windows Service or startup task. Use the same Windows account for elevation. Named-pipe permissions explicitly allow the current Windows account, administrators and SYSTEM; the ordinary UI can connect to the elevated service. Remote pipe clients are disabled. Only read APIs are exposed; no hardware setting changes are enabled.

The adapter reads package temperature, peak active-core clock and PPT power at a maximum of 1 Hz. **Fastest active core** means the highest clock across active cores in the current SDK sample, not a session maximum. When unavailable, the summary falls back to a clearly labeled Windows reported clock; this is not an effective clock adjusted for core sleep time.

The runtime is discovered through an explicit absolute `LIGHTDRIVER_AMD_CPU_SDK_DIR`, AMD's `AMDRMMONITORSDKPATH`, the SDK installation registry value, or its standard Program Files directory. Discovery is read-only. Version mismatch, initialization failure, unsupported readings and invalid measurements are surfaced. Installing/updating the SDK requires restarting the service.

## Provider coverage

| Provider | Implemented readings | Validation |
| --- | --- | --- |
| Windows performance counters / topology | Aggregate load, physical cores, SMT loads, reported clocks, sockets, shared L3 and efficiency classes | Live reference topology/read paths observed; synthetic multi-group, SMT and hybrid fixtures |
| AMD Ryzen Master Monitoring SDK | CPU package temperature, fastest active-core clock, PPT power | Live elevated read paths observed |
| AMD ADLX | GPU temperature/hotspot, load, board/chip power, fan RPM, clocks and used memory | Live temperature/load/board-power read paths observed; broader coverage pending |
| NVIDIA NVML | GPU temperature, device power, load, memory, clocks and fan percentage | Rust adapter implemented; physical validation pending |
| Intel Level Zero Sysman | GPU temperature domains, root-card average power, combined-engine load and GPU frequency | Rust adapter and x64 layout checks implemented; physical validation pending |
| Intel CPU vendor SDK | Package temperature | Adapter pending; Windows load/topology/reported clocks remain usable |
| Kraken X3 HID | Coolant temperature, pump RPM and duty | Read-only reference X53 telemetry observed |
| Motherboard fan sensors | Radiator / case RPM | Adapter pending |

GPU readings depend on compatible installed driver APIs. A machine can legitimately lack APIs for other GPU vendors. All first-party runtime code is Rust; the optional SDKs use installed native libraries. No C++ helper, .NET process or Python runtime is shipped. DLLs are loaded from absolute installed paths; the AMD CPU loader restricts dependency search to the SDK directory and Windows System32.

## Semantics and bounds

- Missing, invalid, stale, future and offline values never become simulated readings or measured zeros.
- Readings expire after five seconds; disconnection hides previous values.
- The UI polls snapshots at 1 Hz. System/vendor collection runs on a dedicated service thread, separate from Kraken polling.
- Current scalar readings replace the former Advanced sensor-history view. Up to 48 scalar sensors plus the bounded CPU frame fit the existing 128 KiB IPC limit; maximum-inventory transport tests cover this budget.
- Intel power uses delta microjoules / delta microseconds. First samples, counter resets and invalid intervals remain unavailable.
- Component power is labeled by scope and is never summed into whole-PC wall power; a wall meter or supported PSU is required for that measurement.
- Demo never initializes system/vendor collectors or opens hardware.

Protocol version **3** carries CPU topology and the explicit unprivileged-service handoff. Restart older services after upgrading; incompatible versions are rejected.

## Native previews

```powershell
.\target\release\lightdriver-desktop.exe --demo --tab diagnostics
.\target\release\lightdriver-desktop.exe --demo --tab diagnostics --cores
# Review a larger simulated topology; this does not modify hardware.
$env:LIGHTDRIVER_DEMO_CORES='64'
# Restart the demo service before capturing a different topology.
```

These options also work with `--capture` and `--size 980x720`. See the [overview](diagnostics-basic.png) and [core diagram](diagnostics-cores.png); both are native release captures with simulated data.

## Primary references

- [AMD CPU Monitoring SDK](https://www.amd.com/en/developer/ryzen-master-monitoring-sdk.html)
- [Windows processor relationships](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getlogicalprocessorinformationex)
- [Windows performance counter formatting](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcountervalue)
- [AMD ADLX metrics](https://gpuopen.com/manuals/adlx/programming-with-adlx/adlx-samples/rust-samples/perfgpumetrics/)
- [NVIDIA NVML API](https://docs.nvidia.com/deploy/nvml-api/index.html)
- [Intel Sysman API](https://oneapi-src.github.io/level-zero-spec/level-zero/latest/sysman/api.html)
- [Intel published ABI headers](https://github.com/oneapi-src/level-zero/tree/master/include)
