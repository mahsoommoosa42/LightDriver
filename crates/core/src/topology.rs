//! Physical cores and shared-cache topology. OS addresses remain processor-group aware.
use crate::{STALE_AFTER_MS, Unit};
use serde::{Deserialize, Serialize};
pub const CPU_CORE_LIMIT: usize = 256;
pub const CPU_THREAD_LIMIT: usize = 512;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuThread {
    pub group: u16,
    pub index: u8,
    pub load: Option<f32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalCore {
    pub id: u16,
    pub package: Option<u16>,
    pub cluster: Option<u16>,
    pub efficiency_class: u8,
    pub threads: Vec<CpuThread>,
    pub temperature: Option<f32>,
    pub clock_mhz: Option<f32>,
}
impl PhysicalCore {
    /// Do not report an aggregate if even one SMT thread has no valid measurement.
    pub fn load(&self) -> Option<f32> {
        if self.threads.is_empty() {
            return None;
        }
        let sum = self.threads.iter().try_fold(0.0, |sum, t| {
            t.load
                .filter(|v| Unit::Percent.accepts(f64::from(*v)))
                .map(|v| sum + v)
        })?;
        Some(sum / self.threads.len() as f32)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCluster {
    pub id: u16,
    pub l3_bytes: u32,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuTopology {
    pub cores: Vec<PhysicalCore>,
    pub clusters: Vec<CpuCluster>,
    pub total_cores: usize,
    pub total_threads: usize,
    pub sampled_at_ms: u64,
    pub detail: String,
}
impl CpuTopology {
    pub fn is_fresh_at(&self, now: u64) -> bool {
        self.sampled_at_ms <= now && now - self.sampled_at_ms <= STALE_AFTER_MS
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_smt_readings_are_not_an_aggregate() {
        let mut core = PhysicalCore {
            id: 0,
            package: Some(0),
            cluster: None,
            efficiency_class: 0,
            threads: vec![
                CpuThread {
                    group: 0,
                    index: 0,
                    load: Some(20.0),
                },
                CpuThread {
                    group: 1,
                    index: 0,
                    load: None,
                },
            ],
            temperature: None,
            clock_mhz: None,
        };
        assert_eq!(core.load(), None);
        core.threads[1].load = Some(80.0);
        assert_eq!(core.load(), Some(50.0));
        core.threads[1].load = Some(101.0);
        assert_eq!(core.load(), None);
        let topology = CpuTopology {
            sampled_at_ms: 1000,
            ..CpuTopology::default()
        };
        assert!(!topology.is_fresh_at(999));
        assert!(topology.is_fresh_at(6000));
        assert!(!topology.is_fresh_at(6001));
    }
}
