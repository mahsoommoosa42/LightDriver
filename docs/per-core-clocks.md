# Per-core clock investigation

Research checkpoint: 10 October 2026. No new live hardware probe or CPU stress test was performed.

## Current implementation

The physical-core tiles already read Windows `Processor Information(group,logical_processor)\Processor Frequency` counters. Available SMT-thread values are averaged into a physical-core tile; the summary fallback uses the maximum tile value. These are Windows-reported clocks, not a measured sleep-adjusted effective frequency. The topology preserves processor groups, physical cores, shared-L3 groups and efficiency classes.

Keep the label explicit. Do not use invariant TSC or QueryPerformanceCounter frequency as a core clock.

## AMD SDK findings

The installed Ryzen Master Monitoring SDK 3.1.1.5478 includes `ICPUEx::GetCPUParameters` and `EffectiveFreqData`:

- `uLength`: array length.
- `dFreq`: effective MHz array.
- `dState`: C0 residency percentage array.
- `dCurrentFreq`: current-frequency array; documentation identifies support starting with Matisse.
- A temperature pointer is also present, but per-core temperatures remain excluded by the product decision.

The bundled sample queries the output structure without allocating these arrays, reads the returned pointers, and does not free them in the inspected code. This is evidence for provider-managed output, not a complete documented lifetime/ownership contract. The existing Rust ABI already reserves these fields but intentionally consumes only package temperature, package power and peak active clock.

The sample skips zero current-frequency slots and assigns a separate consecutive display number. It does not establish a correspondence between SDK slots and Windows physical-core IDs, processor groups, APIC IDs, CCDs or CCXs. Array length equality is insufficient to prove that mapping. Shared L3 membership cannot resolve unspecified SDK ordering.

## Recommended implementation sequence

1. Preserve the existing Windows-reported fallback and its source label.
2. Extend the pinned AMD reader to copy bounded current/effective/C0 arrays immediately after a successful call, once pointer lifetime is established. Preserve SDK slot IDs and original timestamps; never compact away a zero slot or interpret zero alone as proof of sleep.
3. Establish and test the SDK-slot-to-physical-core mapping. Reject incomplete or ambiguous mappings, including disabled cores and multiple CCDs. Keep processor-group and SMT addressing intact.
4. Display current MHz as the main per-core clock, with a clearly labeled effective MHz option where supported. Transport source and measurement kind so fallbacks are visible.
5. Retain the user's shared-L3 physical-core grouping and SMT load bars. Do not restore per-core temperature.

This is an investigation result, not a claim that AMD measured per-core clocks have been enabled or validated.

## Intel

Intel PCM is a primary reference for hardware performance-counter monitoring. Its Windows setup requires an MSR-access driver; it is not a drop-in ordinary-permission Rust API. Do not add another driver, test-signing setting, or C++ runtime solely for clocks without a separate architecture decision. The selected PawnIO fan architecture may eventually supply an approved Intel MSR module, but that still needs counter semantics, per-processor affinity, model support and physical validation.

Windows clocks remain the common fallback. Do not derive effective frequency by multiplying unrelated load and frequency samples and present it as a measured vendor value.

## Evidence and follow-up

- [AMD Ryzen Master Monitoring SDK](https://www.amd.com/en/developer/ryzen-master-monitoring-sdk.html). Detailed findings above come from the installed 3.1.1.5478 `IDevice.h`, `ICPUEx.h`, generated API documentation and `sampleApp.cpp`; vendor files are not redistributed here.
- [Microsoft performance-counter guidance](https://learn.microsoft.com/en-us/windows/win32/perfctrs/about-performance-counters): use a diagnostic sampling cadence around one second.
- [Microsoft processor power information](https://learn.microsoft.com/en-us/windows/win32/power/processor-power-information-str): its CurrentMhz represents specified clock multiplied by throttle and is not an effective-clock substitute.
- [Intel PCM Windows setup](https://github.com/intel/pcm/blob/master/doc/WINDOWS_HOWTO.md).

Done criteria: valid bounded reads, documented or experimentally verified core identity mapping, explicit current/effective/source labels, stale/missing values kept unavailable, and physical checks on single/multiple CCD, SMT, disabled-core and processor-group configurations. Physical testing remains deferred.
