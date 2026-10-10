//! Read-only service-owned system collectors. Demo never loads vendor libraries.
use lightdriver_core::{
    Diagnostics, Mode, Provider, SENSOR_LIMIT, Sensor, SensorSample, SensorStatus, Unit, now_ms,
};
use sysinfo::System;
#[cfg(windows)]
mod amd;
mod cpu;
mod permissions;
pub use permissions::is_administrator;
#[cfg(windows)]
mod intel;
#[cfg(windows)]
mod nvidia;
#[cfg(all(windows, target_pointer_width = "64"))]
mod windows_cpu;

pub struct Collector {
    mode: Mode,
    system: Option<System>,
    cpu: Result<cpu::Cpu, String>,
    #[cfg(all(windows, target_pointer_width = "64"))]
    windows_cpu: Result<windows_cpu::WindowsCpu, String>,
    #[cfg(not(all(windows, target_pointer_width = "64")))]
    primed: bool,
    ticks: u64,
    #[cfg(windows)]
    amd: Result<amd::Amd, String>,
    #[cfg(windows)]
    nvidia: Result<nvidia::Nvidia, String>,
    #[cfg(windows)]
    intel: Result<intel::Intel, String>,
}
impl Collector {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            system: (mode == Mode::Live).then(System::new),
            cpu: if mode == Mode::Live {
                cpu::Cpu::new()
            } else {
                Err("Demo mode".into())
            },
            #[cfg(all(windows, target_pointer_width = "64"))]
            windows_cpu: if mode == Mode::Live {
                windows_cpu::WindowsCpu::new()
            } else {
                Err("Demo mode".into())
            },
            #[cfg(not(all(windows, target_pointer_width = "64")))]
            primed: false,
            ticks: 0,
            #[cfg(windows)]
            amd: if mode == Mode::Live {
                amd::Amd::new()
            } else {
                Err("Demo mode".into())
            },
            #[cfg(windows)]
            nvidia: if mode == Mode::Live {
                nvidia::Nvidia::new()
            } else {
                Err("Demo mode".into())
            },
            #[cfg(windows)]
            intel: if mode == Mode::Live {
                intel::Intel::new()
            } else {
                Err("Demo mode".into())
            },
        }
    }
    pub fn poll(&mut self) -> Diagnostics {
        self.ticks += 1;
        let mut data = Diagnostics {
            cpu_elevation_needed: self.mode == Mode::Live && cpu::elevation_needed(),
            ..Diagnostics::default()
        };
        if self.mode == Mode::Demo {
            self.demo(&mut data);
        } else {
            let system = self.system.as_mut().expect("Live system collector");
            system.refresh_cpu_usage();
            system.refresh_memory();
            let cpu = system.cpus().first().map_or("CPU", |c| c.brand());
            let group = format!("CPU · {cpu}");
            #[cfg(all(windows, target_pointer_width = "64"))]
            let (load, clock) = match &mut self.windows_cpu {
                Ok(collector) => {
                    let (topology, load, clock) = collector.poll();
                    data.cpu_topology = topology;
                    (
                        load.ok_or_else(|| "Collecting two valid Windows CPU samples".into()),
                        clock,
                    )
                }
                Err(detail) => {
                    data.cpu_topology.detail = detail.clone();
                    (Err(detail.clone()), None)
                }
            };
            #[cfg(not(all(windows, target_pointer_width = "64")))]
            let (load, clock) = {
                let load = if self.primed {
                    Ok(f64::from(system.global_cpu_usage()))
                } else {
                    Err("Collecting a second CPU sample".into())
                };
                self.primed = true;
                data.cpu_topology.detail =
                    "Physical-core topology currently requires Windows x64".into();
                (load, None::<f64>)
            };
            data.sensors.push(sensor(
                "cpu.load",
                &group,
                "CPU load",
                Unit::Percent,
                "Windows · Processor Information",
                true,
                load,
            ));
            match &mut self.cpu {
                Ok(adapter) => {
                    let mut readings = adapter.poll(&group);
                    data.providers.push(Provider { name: "CPU vendor SDK".into(), detail:
                        if readings.iter().all(|s| s.sample.is_some()) {
                            "AMD Ryzen Master Monitoring SDK 3.1.1.5478 active · read-only · maximum 1 Hz; power is PPT".into()
                        } else { "AMD CPU SDK queried; see per-sensor errors / unsupported readings".into() } });
                    data.sensors.append(&mut readings);
                }
                Err(detail) => {
                    data.sensors.append(&mut cpu::unavailable(&group, detail));
                    data.providers.push(Provider {
                        name: "CPU vendor SDK".into(),
                        detail: detail.clone(),
                    });
                }
            }
            // Prefer the vendor's peak clock when measured, otherwise label the OS reading accurately.
            if !data
                .sensors
                .iter()
                .any(|s| s.id == "cpu.clock" && s.sample.is_some())
            {
                data.sensors.retain(|s| s.id != "cpu.clock");
                data.sensors.push(sensor(
                    "cpu.clock",
                    &group,
                    "CPU reported clock",
                    Unit::Mhz,
                    "Windows · Processor Information",
                    true,
                    clock.ok_or_else(|| "CPU frequency counter unavailable".into()),
                ));
            }
            let total = system.total_memory();
            data.sensors.push(sensor(
                "memory.used",
                "System memory",
                "Memory used",
                Unit::Gib,
                "Windows · sysinfo",
                true,
                Ok(system.used_memory() as f64 / 1_073_741_824.0),
            ));
            data.sensors.push(sensor(
                "memory.total",
                "System memory",
                "Memory total",
                Unit::Gib,
                "Windows · sysinfo",
                false,
                Ok(total as f64 / 1_073_741_824.0),
            ));
            data.providers.push(Provider { name: "Windows CPU / memory".into(),
                detail: "Read-only collection. Physical cores and SMT threads use processor-group-aware Windows counters; shared L3 groups come from OS topology.".into() });
            #[cfg(windows)]
            {
                macro_rules! poll_vendor {
                    ($field:ident, $name:expr) => {
                        match &mut self.$field {
                            Ok(adapter) => match adapter.poll() {
                                Ok(mut sensors) => {
                                    data.providers.push(Provider { name: $name.into(), detail:
                                        if sensors.is_empty() { "No compatible GPU exposed by this API".into() }
                                        else { "Adapter active · hardware coverage is tracked separately".into() } });
                                    data.sensors.append(&mut sensors);
                                },
                                Err(e) => data.providers.push(Provider { name: $name.into(), detail: e }),
                            },
                            Err(e) => data.providers.push(Provider { name: $name.into(), detail: e.clone() }),
                        }
                    }
                }
                poll_vendor!(amd, "AMD · ADLX");
                poll_vendor!(nvidia, "NVIDIA · NVML");
                poll_vendor!(intel, "Intel · Level Zero Sysman");
            }
        }
        data.sensors.push(sensor("system.wall-power", "Whole system", "Wall power", Unit::Watts,
            "External power meter", true, Err("Requires a power meter or a supported PSU; component readings do not measure wall power".into())));
        self.record(&mut data);
        data
    }
    fn record(&mut self, data: &mut Diagnostics) {
        data.sensors.truncate(SENSOR_LIMIT);
        // The Basic view needs current samples, not a history per scalar sensor.
        // Reserve transport budget for hundreds of physical cores and SMT threads.
        for sensor in &mut data.sensors {
            sensor.history.clear();
        }
    }
    fn demo(&self, data: &mut Diagnostics) {
        let wave = (self.ticks as f64 / 9.0).sin();
        for (id, group, label, unit, value, basic) in [
            (
                "cpu.load",
                "CPU · Demo",
                "CPU load",
                Unit::Percent,
                24.0 + wave * 8.0,
                true,
            ),
            (
                "cpu.temperature",
                "CPU · Demo",
                "CPU package temperature",
                Unit::Celsius,
                56.0 + wave * 3.0,
                true,
            ),
            (
                "cpu.power",
                "CPU · Demo",
                "CPU package power",
                Unit::Watts,
                64.0 + wave * 5.0,
                true,
            ),
            (
                "memory.used",
                "System memory · Demo",
                "Memory used",
                Unit::Gib,
                12.6,
                true,
            ),
            (
                "memory.total",
                "System memory · Demo",
                "Memory total",
                Unit::Gib,
                32.0,
                false,
            ),
            (
                "demo.gpu.temperature",
                "GPU · Demo",
                "GPU temperature",
                Unit::Celsius,
                49.0 + wave * 2.0,
                true,
            ),
            (
                "demo.gpu.hotspot",
                "GPU · Demo",
                "GPU hotspot",
                Unit::Celsius,
                64.0 + wave * 3.0,
                false,
            ),
            (
                "demo.gpu.power",
                "GPU · Demo",
                "GPU board power",
                Unit::Watts,
                112.0 + wave * 12.0,
                true,
            ),
            (
                "demo.gpu.load",
                "GPU · Demo",
                "GPU load",
                Unit::Percent,
                34.0 + wave * 12.0,
                true,
            ),
            (
                "demo.gpu.clock",
                "GPU · Demo",
                "GPU clock",
                Unit::Mhz,
                1650.0 + wave * 50.0,
                false,
            ),
            (
                "demo.gpu.fan",
                "GPU · Demo",
                "GPU fan speed",
                Unit::Rpm,
                950.0,
                false,
            ),
        ] {
            data.sensors.push(sensor(
                id,
                group,
                label,
                unit,
                "Simulated data",
                basic,
                Ok(value),
            ));
        }
        data.sensors.push(sensor(
            "cpu.clock",
            "CPU · Demo",
            "Fastest active core",
            Unit::Mhz,
            "Simulated data",
            true,
            Ok(4600.0 + wave * 100.0),
        ));
        data.cpu_topology = demo_topology(self.ticks);
        data.providers.push(Provider {
            name: "Demo".into(),
            detail: "Simulated readings only; no system or vendor collector is opened".into(),
        });
    }
}
pub(crate) fn sensor(
    id: &str,
    group: &str,
    label: &str,
    unit: Unit,
    source: &str,
    basic: bool,
    reading: Result<f64, String>,
) -> Sensor {
    let (status, sample, detail) = match reading {
        Ok(v) if unit.accepts(v) => (
            SensorStatus::Available,
            Some(SensorSample {
                at_ms: now_ms(),
                value: v,
            }),
            String::new(),
        ),
        Ok(_) => (
            SensorStatus::Error,
            None,
            "Provider returned an invalid measurement".into(),
        ),
        Err(e) => (SensorStatus::Unavailable, None, e),
    };
    Sensor {
        id: id.into(),
        group: group.into(),
        label: label.into(),
        unit,
        source: source.into(),
        basic,
        status,
        detail,
        sample,
        history: vec![],
    }
}
#[cfg(windows)]
pub(crate) fn system_library(name: &str) -> Result<std::path::PathBuf, String> {
    // Absolute OS path avoids DLL search through the working directory.
    let root = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable")?;
    let path = std::path::PathBuf::from(root).join("System32").join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{name} is not installed in Windows System32"))
    }
}

/// Energy (microjoules) / time (microseconds) gives average watts over the interval.
#[cfg(any(windows, test))]
pub(crate) fn counter_rate(previous: Option<(u64, u64)>, current: (u64, u64)) -> Option<f64> {
    let (value, time) = previous?;
    let dt = current.1.checked_sub(time)?;
    if dt == 0 {
        return None;
    }
    Some(current.0.checked_sub(value)? as f64 / dt as f64)
}
fn demo_topology(ticks: u64) -> lightdriver_core::CpuTopology {
    use lightdriver_core::{CPU_CORE_LIMIT, CpuCluster, CpuThread, CpuTopology, PhysicalCore};
    let count = std::env::var("LIGHTDRIVER_DEMO_CORES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(16)
        .clamp(1, CPU_CORE_LIMIT);
    let cores = (0..count)
        .map(|i| {
            let load = 24.0 + ((ticks as f32 / 9.0) + (i as f32 * 0.7)).sin() * 20.0;
            PhysicalCore {
                id: i as u16,
                package: Some(0),
                cluster: Some((i / 8) as u16),
                efficiency_class: 0,
                threads: (0..2)
                    .map(|t| {
                        let index = i * 2 + t;
                        CpuThread {
                            group: (index / 64) as u16,
                            index: (index % 64) as u8,
                            load: Some(load + t as f32 * 7.0),
                        }
                    })
                    .collect(),
                clock_mhz: Some(4500.0 + load * 3.0),
            }
        })
        .collect();
    CpuTopology {
        cores,
        clusters: (0..count.div_ceil(8))
            .map(|id| CpuCluster {
                id: id as u16,
                l3_bytes: 32 * 1024 * 1024,
            })
            .collect(),
        total_cores: count,
        total_threads: count * 2,
        sampled_at_ms: now_ms(),
        detail: "Simulated shared-L3 clusters, loads and clocks".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counter_units_reset_and_first_sample_are_handled() {
        assert_eq!(
            counter_rate(Some((1_000_000, 100)), (51_000_000, 1_000_100)),
            Some(50.0)
        );
        assert_eq!(counter_rate(None, (1, 1)), None);
        assert_eq!(counter_rate(Some((10, 10)), (9, 20)), None);
        assert_eq!(counter_rate(Some((10, 10)), (11, 10)), None);
        assert_eq!(counter_rate(Some((10, 10)), (11, 9)), None);
    }
    #[test]
    fn demo_is_bounded_and_failures_clear_history() {
        let mut collector = Collector::new(Mode::Demo);
        for _ in 0..100 {
            let data = collector.poll();
            assert!(data.sensors.len() <= SENSOR_LIMIT);
            assert!(data.sensors.iter().all(|s| s.history.is_empty()));
            assert!(
                data.sensors
                    .iter()
                    .filter(|s| s.sample.is_some())
                    .all(|s| s.source == "Simulated data")
            );
        }
        let mut data = Diagnostics {
            sensors: vec![sensor(
                "cpu.load",
                "CPU",
                "Load",
                Unit::Percent,
                "test",
                true,
                Err("disconnected".into()),
            )],
            providers: vec![],
            ..Diagnostics::default()
        };
        collector.record(&mut data);
        assert!(data.sensors[0].history.is_empty());
    }
}
