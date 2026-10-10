//! Reusable operating-system transports; no motherboard vendor selection.
//! No firmware setter or fan-setting writer is exposed.
pub mod backup;
pub mod conflicts;
pub mod inventory;
pub mod it8688;
#[cfg(windows)]
mod pawnio;
pub mod uefi;
