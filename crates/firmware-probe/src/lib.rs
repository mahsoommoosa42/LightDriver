//! Narrow firmware getter and partial offline layout research.
//! No firmware setter, full-payload export, or writable support is exposed.
pub mod reference_layout;
mod report;
#[cfg(windows)]
mod windows;

pub use report::ProbeReport;

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
