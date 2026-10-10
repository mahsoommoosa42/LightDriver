# GIGABYTE backend

`GigabyteBackend` implements the shared `DeviceAdapter` and `MotherboardBackend` interfaces. The local service and standalone firmware fan debugger consume this crate.

- `profiles`: independent RGB/RPM/firmware identity gates and reviewed channel maps.
- `rgb`: ITE 8297 HID protocol and explicit RGB control sessions.
- `rpm`: board-specific mapping over the shared IT8688/PawnIO reader.
- `firmware`: exact-profile read, partial reference codec, private snapshots/comparison and offline restoration/recovery previews.

All persistent fan-save capabilities remain false. See [the architecture](../../docs/motherboard-adapters.md) for expansion, validation and distribution responsibilities.
