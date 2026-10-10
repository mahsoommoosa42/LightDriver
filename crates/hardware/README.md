# Shared hardware access

Vendor-neutral operating-system transports and chip-level tachometer reading. This crate contains Windows inventory/conflict detection, bounded getter-only UEFI access with serialized privilege restoration, hash-pinned PawnIO/LpcIO access, IT8688 decoding and explicit private file/hash primitives.

Vendor adapters select reviewed board profiles before calling transports. No firmware setter, fan PWM/configuration writer, automatic elevation, raw debug logging or vendor defaults live here. External driver/module installation remains explicit. See [the architecture](../../docs/motherboard-adapters.md).
