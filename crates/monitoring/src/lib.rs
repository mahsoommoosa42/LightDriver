//! Read-only service-owned system collectors. Demo never loads vendor libraries.
use lightdriver_core::{
    Diagnostics, Mode, Provider, SENSOR_HISTORY_LIMIT, SENSOR_LIMIT, Sensor, SensorSample,
    SensorStatus, Unit, now_ms,
};
use std::collections::HashMap;
use sysinfo::System;
#[cfg(windows)]
mod amd;
#[cfg(windows)]
mod intel;
#[cfg(windows)]
mod nvidia;

pub struct Collector {
    mode: Mode,
    system: Option<System>,
    primed: bool,
    ticks: u64,
    history: HashMap<String, Vec<SensorSample>>,
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
            primed: false,
            ticks: 0,
            history: HashMap::new(),
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
        let mut data = Diagnostics::default();
        if self.mode == Mode::Demo {
            self.demo(&mut data);
        } else {
            let system = self.system.as_mut().expect("Live system collector");
            system.refresh_cpu_usage();
            system.refresh_memory();
            let cpu = system.cpus().first().map_or("CPU", |c| c.brand());
            let group = format!("CPU · {cpu}");
            data.sensors.push(sensor(
                "cpu.load",
                &group,
                "CPU load",
                Unit::Percent,
                "Windows · sysinfo",
                true,
                if self.primed {
                    Ok(f64::from(system.global_cpu_usage()))
                } else {
                    Err("Collecting a second CPU sample".into())
                },
            ));
            for (i, cpu) in system.cpus().iter().take(16).enumerate() {
                data.sensors.push(sensor(
                    &format!("cpu.core.{i}"),
                    &group,
                    &format!("Logical CPU {i} load"),
                    Unit::Percent,
                    "Windows · sysinfo",
                    false,
                    if self.primed {
                        Ok(f64::from(cpu.cpu_usage()))
                    } else {
                        Err("Collecting a second CPU sample".into())
                    },
                ));
            }
            self.primed = true;
            data.sensors.push(sensor("cpu.temperature", &group, "CPU package temperature", Unit::Celsius,
                "CPU sensor adapter", true, Err("A native CPU sensor adapter is required; ACPI thermal zones are not CPU package temperature".into())));
            data.sensors.push(sensor(
                "cpu.power",
                &group,
                "CPU package power",
                Unit::Watts,
                "CPU sensor adapter",
                true,
                Err("A native CPU power adapter is required".into()),
            ));
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
                detail: "Read-only collection. Advanced shows up to 16 logical CPUs; aggregate includes all CPUs.".into() });
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
        let now = now_ms();
        for s in &mut data.sensors {
            if s.fresh_value_at(now).is_some() {
                let history = self.history.entry(s.id.clone()).or_default();
                if history
                    .last()
                    .is_some_and(|v| now.saturating_sub(v.at_ms) > lightdriver_core::STALE_AFTER_MS)
                {
                    history.clear();
                }
                if let Some(sample) = &s.sample {
                    history.push(sample.clone());
                }
                if history.len() > SENSOR_HISTORY_LIMIT {
                    history.remove(0);
                }
                s.history = history.clone();
            } else {
                self.history.remove(&s.id);
                s.history.clear();
            }
        }
        self.history
            .retain(|id, _| data.sensors.iter().any(|s| &s.id == id));
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
            assert!(
                data.sensors
                    .iter()
                    .all(|s| s.history.len() <= SENSOR_HISTORY_LIMIT)
            );
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
        };
        collector.record(&mut data);
        assert!(data.sensors[0].history.is_empty());
        assert!(!collector.history.contains_key("cpu.load"));
    }
}
