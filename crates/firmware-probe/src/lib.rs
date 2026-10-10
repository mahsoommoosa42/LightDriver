//! Narrow firmware getter and partial offline layout research.
//! No firmware setter or writable support is exposed. The explicit firmware-lab
//! snapshot API can retain full bytes for private local research; CLI reports remain metadata-only.
pub mod reference_layout;
mod report;
pub mod snapshot;
#[cfg(windows)]
mod windows;

pub use report::{ProbeError, ProbeReport};

/// Metadata only. Does not request elevation.
pub fn inspect() -> ProbeReport {
    inspect_fields().report
}

/// Reads the same fixed variable and returns only identified scalar fields.
/// A format match is not a board/BIOS allowlist or permission to write.
pub fn inspect_fields() -> Inspection {
    #[cfg(windows)]
    {
        windows::inspect_fields()
    }
    #[cfg(not(windows))]
    {
        Inspection {
            report: ProbeReport::unsupported(),
            fields: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Inspection {
    pub report: ProbeReport,
    pub fields: Option<[reference_layout::HeaderSettings; 4]>,
}

#[derive(Debug, Clone)]
pub struct SnapshotInspection {
    pub report: ProbeReport,
    pub snapshot: Option<snapshot::Snapshot>,
}
/// Explicit firmware-lab capture: retains a full reference variable in memory.
/// No file is written and no firmware setter exists.
pub fn inspect_snapshot() -> SnapshotInspection {
    #[cfg(windows)]
    {
        windows::inspect_snapshot()
    }
    #[cfg(not(windows))]
    {
        SnapshotInspection {
            report: ProbeReport::unsupported(),
            snapshot: None,
        }
    }
}
