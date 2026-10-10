//! Read-only Windows topology and processor-group-aware performance counters.
use lightdriver_core::{
    CPU_CORE_LIMIT, CPU_THREAD_LIMIT, CpuCluster, CpuThread, CpuTopology, PhysicalCore, Unit,
    now_ms,
};
use std::{collections::HashSet, ptr};
use windows_sys::Win32::System::{
    Performance::{
        PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
        PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW,
    },
    SystemInformation::{GetLogicalProcessorInformationEx, RelationAll},
};
type Address = (u16, u8);
fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or("Truncated topology record")?
            .try_into()
            .unwrap(),
    ))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or("Truncated topology record")?
            .try_into()
            .unwrap(),
    ))
}
fn affinities(record: &[u8], offset: usize, count: usize) -> Result<Vec<Address>, String> {
    if count > 256 {
        return Err("Too many processor groups".into());
    }
    let mut addresses = vec![];
    for i in 0..count {
        let at = offset + i * 16;
        let mask = u64::from_le_bytes(
            record
                .get(at..at + 8)
                .ok_or("Truncated affinity")?
                .try_into()
                .unwrap(),
        );
        let group = u16_at(record, at + 8)?;
        for index in 0..64 {
            if mask & (1u64 << index) != 0 {
                addresses.push((group, index));
            }
        }
    }
    Ok(addresses)
}
fn contains_all(outer: &[Address], inner: &[Address]) -> bool {
    inner.iter().all(|v| outer.contains(v))
}
fn unique_group(groups: &[Vec<Address>], threads: &[Address]) -> Option<u16> {
    let mut matching = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| contains_all(g, threads));
    let id = matching.next()?.0;
    matching.next().is_none().then_some(id as u16)
}
/// Parse variable-sized x64 records with bounds checks, including pre-Windows-11 caches.
fn parse(bytes: &[u8]) -> Result<CpuTopology, String> {
    let mut cores = vec![];
    let mut packages = vec![];
    let mut caches = vec![];
    let mut at = 0;
    while at < bytes.len() {
        let relation = u32_at(bytes, at)?;
        let size = u32_at(bytes, at + 4)? as usize;
        if size < 8 {
            return Err("Invalid topology record size".into());
        }
        let record = bytes
            .get(at..at.checked_add(size).ok_or("Topology size overflow")?)
            .ok_or("Truncated topology buffer")?;
        match relation {
            0 | 3 => {
                let class = *record.get(9).ok_or("Missing efficiency class")?;
                let threads = affinities(record, 32, u16_at(record, 30)? as usize)?;
                if threads.is_empty() {
                    return Err("Empty processor affinity".into());
                }
                if relation == 0 {
                    cores.push((threads, class));
                } else {
                    packages.push(threads);
                }
            }
            2 if *record.get(8).ok_or("Missing cache level")? == 3
                && matches!(u32_at(record, 16)?, 0 | 2) =>
            {
                let count = usize::from(u16_at(record, 38)?).max(1);
                let threads = affinities(record, 40, count)?;
                if !threads.is_empty() {
                    caches.push((threads, u32_at(record, 12)?));
                }
            }
            _ => {}
        }
        if cores.len() > 4096 || packages.len() > 256 || caches.len() > 4096 {
            return Err("Topology exceeds collection limits".into());
        }
        at += size;
    }
    if cores.is_empty() {
        return Err("Windows returned no physical cores".into());
    }
    cores.sort_by_key(|(threads, _)| threads[0]);
    packages.sort_by_key(|threads| threads[0]);
    caches.sort_by_key(|(threads, _)| threads[0]);
    let mut seen = HashSet::new();
    for (threads, _) in &cores {
        for address in threads {
            if !seen.insert(*address) {
                return Err("Overlapping physical-core affinities".into());
            }
        }
    }
    let total_cores = cores.len();
    let total_threads = seen.len();
    let cache_groups: Vec<_> = caches.iter().map(|(threads, _)| threads.clone()).collect();
    let mut used_threads = 0;
    let cores: Vec<_> = cores
        .into_iter()
        .enumerate()
        .take(CPU_CORE_LIMIT)
        .take_while(|(_, (threads, _))| {
            used_threads += threads.len();
            used_threads <= CPU_THREAD_LIMIT
        })
        .map(|(id, (threads, class))| PhysicalCore {
            id: id as u16,
            package: unique_group(&packages, &threads),
            cluster: unique_group(&cache_groups, &threads),
            efficiency_class: class,
            threads: threads
                .into_iter()
                .map(|(group, index)| CpuThread {
                    group,
                    index,
                    load: None,
                })
                .collect(),
            temperature: None,
            clock_mhz: None,
        })
        .collect();
    let clusters = caches
        .into_iter()
        .enumerate()
        .filter(|(id, _)| cores.iter().any(|c| c.cluster == Some(*id as u16)))
        .map(|(id, (_, l3_bytes))| CpuCluster {
            id: id as u16,
            l3_bytes,
        })
        .collect();
    let detail = format!(
        "Windows physical-core / shared-L3 topology. Per-core temperatures await validated vendor-to-OS identity mapping.{}",
        if cores.len() < total_cores {
            " Display limited to 256 physical cores / 512 threads."
        } else {
            ""
        }
    );
    Ok(CpuTopology {
        cores,
        clusters,
        total_cores,
        total_threads,
        sampled_at_ms: 0,
        detail,
    })
}
fn inventory() -> Result<CpuTopology, String> {
    let mut size = 0;
    // SAFETY: standard size query, followed by aligned storage for the OS output.
    unsafe {
        GetLogicalProcessorInformationEx(RelationAll, ptr::null_mut(), &mut size);
    }
    for _ in 0..3 {
        if size == 0 || size > 1024 * 1024 {
            return Err("Windows topology buffer unavailable or too large".into());
        }
        let mut storage = vec![0u64; (size as usize).div_ceil(8)];
        let capacity = storage.len() * 8;
        let success = unsafe {
            GetLogicalProcessorInformationEx(RelationAll, storage.as_mut_ptr().cast(), &mut size)
        };
        if success != 0 {
            if size as usize > capacity {
                return Err("Windows topology output exceeded capacity".into());
            }
            let bytes =
                unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), size as usize) };
            return parse(bytes);
        }
    }
    Err("Windows topology query failed".into())
}
struct ThreadCounters {
    load: Option<PDH_HCOUNTER>,
    clock: Option<PDH_HCOUNTER>,
}
pub(crate) struct WindowsCpu {
    topology: CpuTopology,
    query: PDH_HQUERY,
    total: Option<PDH_HCOUNTER>,
    counters: Vec<Vec<ThreadCounters>>,
    primed: bool,
}
impl WindowsCpu {
    pub(crate) fn new() -> Result<Self, String> {
        let topology = inventory()?;
        let mut query = ptr::null_mut();
        if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
            return Err("Windows performance query unavailable".into());
        }
        let mut collector = Self {
            topology,
            query,
            total: None,
            counters: vec![],
            primed: false,
        };
        collector.total = collector.add(r"\Processor Information(_Total)\% Processor Time");
        for core in &collector.topology.cores {
            collector.counters.push(
                core.threads
                    .iter()
                    .map(|thread| {
                        let instance = format!("{},{}", thread.group, thread.index);
                        ThreadCounters {
                            load: collector.add(&format!(
                                r"\Processor Information({instance})\% Processor Time"
                            )),
                            clock: collector.add(&format!(
                                r"\Processor Information({instance})\Processor Frequency"
                            )),
                        }
                    })
                    .collect(),
            );
        }
        Ok(collector)
    }
    fn add(&self, path: &str) -> Option<PDH_HCOUNTER> {
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut counter = ptr::null_mut();
        (unsafe { PdhAddEnglishCounterW(self.query, path.as_ptr(), 0, &mut counter) } == 0)
            .then_some(counter)
    }
    fn value(counter: Option<PDH_HCOUNTER>, unit: Unit) -> Option<f64> {
        let mut value: PDH_FMT_COUNTERVALUE = unsafe { std::mem::zeroed() };
        // NOCAP100 is essential for MHz, which commonly exceed 100.
        let code = unsafe {
            PdhGetFormattedCounterValue(
                counter?,
                PDH_FMT_DOUBLE | 0x8000,
                ptr::null_mut(),
                &mut value,
            )
        };
        let reading = unsafe { value.Anonymous.doubleValue };
        (code == 0
            && value.CStatus <= 1
            && unit.accepts(reading)
            && (unit != Unit::Mhz || reading > 0.0))
            .then_some(reading)
    }
    pub(crate) fn poll(&mut self) -> (CpuTopology, Option<f64>, Option<f64>) {
        let collected = unsafe { PdhCollectQueryData(self.query) } == 0;
        let ready = collected && self.primed;
        self.primed = collected;
        let total = ready
            .then(|| Self::value(self.total, Unit::Percent))
            .flatten();
        for (core, counters) in self.topology.cores.iter_mut().zip(&self.counters) {
            let mut clocks = vec![];
            for (thread, counter) in core.threads.iter_mut().zip(counters) {
                thread.load = ready
                    .then(|| Self::value(counter.load, Unit::Percent))
                    .flatten()
                    .map(|v| v as f32);
                if collected && let Some(clock) = Self::value(counter.clock, Unit::Mhz) {
                    clocks.push(clock);
                }
            }
            core.clock_mhz = (!clocks.is_empty())
                .then(|| (clocks.iter().sum::<f64>() / clocks.len() as f64) as f32);
        }
        self.topology.sampled_at_ms = now_ms();
        let clocks: Vec<_> = self
            .topology
            .cores
            .iter()
            .filter_map(|c| c.clock_mhz)
            .collect();
        let clock =
            (!clocks.is_empty()).then(|| f64::from(clocks.iter().copied().fold(0.0, f32::max)));
        (self.topology.clone(), total, clock)
    }
}
impl Drop for WindowsCpu {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn processor(relation: u32, class: u8, masks: &[(u16, u64)]) -> Vec<u8> {
        let mut bytes = vec![0u8; 32 + masks.len() * 16];
        bytes[0..4].copy_from_slice(&relation.to_le_bytes());
        let len = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&len.to_le_bytes());
        bytes[9] = class;
        bytes[30..32].copy_from_slice(&(masks.len() as u16).to_le_bytes());
        for (i, (group, mask)) in masks.iter().enumerate() {
            let at = 32 + i * 16;
            bytes[at..at + 8].copy_from_slice(&mask.to_le_bytes());
            bytes[at + 8..at + 10].copy_from_slice(&group.to_le_bytes());
        }
        bytes
    }
    fn cache(group: u16, mask: u64) -> Vec<u8> {
        let mut bytes = vec![0u8; 56];
        bytes[0..4].copy_from_slice(&2u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&56u32.to_le_bytes());
        bytes[8] = 3;
        bytes[12..16].copy_from_slice(&(32u32 * 1024 * 1024).to_le_bytes());
        // Zero group count represents legacy Windows' single GroupMask union.
        bytes[40..48].copy_from_slice(&mask.to_le_bytes());
        bytes[48..50].copy_from_slice(&group.to_le_bytes());
        bytes
    }
    #[test]
    fn smt_groups_l3_and_hybrid_classes_are_preserved() {
        let records = [
            processor(0, 0, &[(0, 3)]),
            processor(0, 1, &[(1, 3)]),
            processor(3, 0, &[(0, 3), (1, 3)]),
            cache(0, 3),
            cache(1, 3),
        ]
        .concat();
        let t = parse(&records).unwrap();
        assert_eq!(t.total_cores, 2);
        assert_eq!(t.total_threads, 4);
        assert_eq!(t.cores[0].package, Some(0));
        assert_eq!(t.cores[1].package, Some(0));
        assert_eq!(t.cores[1].threads[0].group, 1);
        assert_eq!(t.cores[1].efficiency_class, 1);
        assert_eq!(t.cores[0].cluster, Some(0));
        assert_eq!(t.cores[1].cluster, Some(1));
        assert_eq!(
            parse(&processor(0, 0, &[(0, 1)])).unwrap().cores[0].cluster,
            None
        );
    }
    #[test]
    fn corrupt_records_and_duplicate_affinities_are_rejected() {
        for bytes in [
            vec![0; 8],
            processor(0, 0, &[(0, 1)])[..47].to_vec(),
            [processor(0, 0, &[(0, 1)]), processor(0, 0, &[(0, 1)])].concat(),
        ] {
            assert!(parse(&bytes).is_err());
        }
    }
}
