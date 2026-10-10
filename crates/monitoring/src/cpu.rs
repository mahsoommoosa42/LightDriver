//! Service-owned CPU vendor boundary. Intel CPU SDK support can be added here.
use super::sensor;
use lightdriver_core::{Sensor, Unit};
#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "amd_cpu.rs"]
mod amd;

pub(crate) struct Cpu {
    #[cfg(all(windows, target_arch = "x86_64"))]
    amd: amd::AmdCpu,
}
impl Cpu {
    pub(crate) fn new() -> Result<Self, String> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            Ok(Self {
                amd: amd::AmdCpu::new()?,
            })
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            Err("CPU vendor SDK adapters currently require Windows x64".into())
        }
    }
    pub(crate) fn poll(&mut self, group: &str) -> Vec<Sensor> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            self.amd.poll(group)
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            unavailable(
                group,
                "CPU vendor SDK adapters currently require Windows x64",
            )
        }
    }
}
pub(crate) fn unavailable(group: &str, detail: &str) -> Vec<Sensor> {
    vec![
        sensor(
            "cpu.temperature",
            group,
            "CPU package temperature",
            Unit::Celsius,
            "CPU vendor SDK",
            true,
            Err(detail.into()),
        ),
        sensor(
            "cpu.power",
            group,
            "CPU package power",
            Unit::Watts,
            "CPU vendor SDK",
            true,
            Err(detail.into()),
        ),
    ]
}

pub(crate) fn elevation_needed() -> bool {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        amd::elevation_needed()
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        false
    }
}
