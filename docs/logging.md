# Live logging

Open **Live logs** in the sidebar. Logs belong to the local Rust service and use separate, bounded IPC responses, independent of telemetry snapshots. The default is **Info**.

| Verbosity | Included |
| --- | --- |
| Error | Device read failures and invalid sensor results |
| Warning | Errors, rejected commands, disconnected controllers and unavailable sensors |
| Info | Warnings plus startup/configuration, discovery, command outcomes and adapter status |
| Debug | Info plus command reception and sensor availability transitions |
| Verbose | Debug plus actual received HID bytes and decoded scalar sensor samples |

The levels are cumulative. **Raw incoming packets are collected and shown only in Verbose.** Reducing verbosity stops capture and removes raw entries from the service buffer and the UI. Re-enabling Verbose does not recover discarded packets. UI level changes are serialized; stale responses from another live/demo session cannot restore raw data. A changing level is marked pending until the service confirms it.

The service logs bytes at the HID receive boundary, before parsing. Kraken status/initialization reports and ITE identity responses are covered. No extra hardware queries are sent just to create logs. ITE native effects have no continuous inbound stream; their raw identity response appears when control is explicitly enabled. SDK sensor values are labeled **decoded**; they are not presented as native SDK structure dumps. Demo packet data is explicitly marked **SIMULATED**.

## Configuration for tester reports

Info events include the OS/app/protocol description, processor identities, physical/logical-core and shared-L3 counts, RAM total, detected controllers, and adapter status. Every fetched batch also carries a current system summary. **Show system configuration** displays it alongside the renderer/graphics adapter and configured lighting routes.

**Export log** writes a new local JSON file containing:

- App/protocol and OS information, CPU/GPU identities and topology, memory, controller identities, exposed outputs, and firmware when explicitly queried.
- Sensor adapter availability and errors, including the AMD CPU SDK version requirement.
- Live/demo mode, renderer, current verbosity, service-session marker and timestamps.
- Saved wiring and retained entries permitted by the selected level.

Standard event/context generation does not include hostnames, usernames, executable paths, USB paths or serial numbers. Verbose exports include received packet bytes; they are explicit exports and should be reviewed before sharing. The application never uploads reports automatically.

## Retention and controls

The service retains at most **512 entries**. Each response contains at most **64 entries**, messages at most **512 UTF-8 bytes**, and sources at most **64 bytes**. Context is bounded to 32 lines. These limits fit the existing 128 KiB frame bound even with escaped text.

The UI retains 512 entries for export and renders the latest 128 entries matching the search, keeping rendering work bounded. Older entries expire; the viewer indicates a buffer gap. **Pause view** pauses fetching; the service keeps collecting at its selected verbosity. **Follow** scrolls to the newest received entries. **Clear view** clears the local display; system context remains available. It does not delete previous exports.

Logs stay in memory until an explicit export and disappear when the service exits. Verbosity applies to the current service session; new UI connections request their selected level, with Info as the default. Live and demo have separate buffers. A service restart resets the cursor and retained UI stream.

## Command-line access

With the service already running:

~~~powershell
.\target\release\lightdriver-service.exe --logs
.\target\release\lightdriver-service.exe --logs --log-level verbose
.\target\release\lightdriver-service.exe --logs --log-level info
~~~

A foreground service can start with `--log-level debug`. The desktop also accepts `--tab logs --log-level verbose` and `--log-summary` for native review captures. Demo runs never open hardware.
