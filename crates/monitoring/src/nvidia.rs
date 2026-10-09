//! NVML telemetry only. No tuning, clock, fan or power setters.
use super::{sensor, system_library};
use lightdriver_core::{Sensor, Unit};
use nvml_wrapper::{
    Nvml,
    enum_wrappers::device::{Clock, TemperatureSensor},
};
use std::hash::{Hash, Hasher};

pub struct Nvidia {
    nvml: Nvml,
}
impl Nvidia {
    pub fn new() -> Result<Self, String> {
        let path = system_library("nvml.dll").or_else(|_| {
            let root = std::env::var_os("ProgramW6432").ok_or("NVIDIA NVML is not installed")?;
            let path = std::path::PathBuf::from(root)
                .join("NVIDIA Corporation")
                .join("NVSMI")
                .join("nvml.dll");
            if path.is_file() {
                Ok(path)
            } else {
                Err("NVIDIA NVML is not installed".to_string())
            }
        })?;
        Nvml::builder()
            .lib_path(path.as_os_str())
            .init()
            .map(|nvml| Self { nvml })
            .map_err(|e| format!("NVML initialization: {e}"))
    }
    pub fn poll(&mut self) -> Result<Vec<Sensor>, String> {
        let count = self
            .nvml
            .device_count()
            .map_err(|e| format!("NVML enumeration: {e}"))?;
        let mut sensors = vec![];
        for i in 0..count.min(8) {
            let gpu = self
                .nvml
                .device_by_index(i)
                .map_err(|e| format!("NVML GPU {i}: {e}"))?;
            let name = gpu.name().unwrap_or_else(|_| format!("NVIDIA GPU {i}"));
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            gpu.uuid()
                .unwrap_or_else(|_| format!("{name}.{i}"))
                .hash(&mut hash);
            let id = format!("nvidia.{:x}", hash.finish());
            macro_rules! metric {
                ($key:expr, $label:expr, $unit:expr, $basic:expr, $reading:expr) => {
                    sensors.push(sensor(
                        &format!("{id}.{}", $key),
                        &name,
                        $label,
                        $unit,
                        "NVIDIA · NVML",
                        $basic,
                        $reading.map_err(|e| format!("NVML: {e}")),
                    ));
                };
            }
            metric!(
                "temperature",
                "GPU temperature",
                Unit::Celsius,
                true,
                gpu.temperature(TemperatureSensor::Gpu).map(f64::from)
            );
            metric!(
                "power",
                "GPU power (NVML device scope)",
                Unit::Watts,
                true,
                gpu.power_usage().map(|mw| f64::from(mw) / 1000.0)
            );
            metric!(
                "load",
                "GPU load",
                Unit::Percent,
                true,
                gpu.utilization_rates().map(|u| f64::from(u.gpu))
            );
            metric!(
                "memory",
                "GPU memory used",
                Unit::Gib,
                false,
                gpu.memory_info().map(|m| m.used as f64 / 1_073_741_824.0)
            );
            metric!(
                "memory-total",
                "GPU memory total",
                Unit::Gib,
                false,
                gpu.memory_info().map(|m| m.total as f64 / 1_073_741_824.0)
            );
            metric!(
                "clock",
                "GPU clock",
                Unit::Mhz,
                false,
                gpu.clock_info(Clock::Graphics).map(f64::from)
            );
            metric!(
                "memory-clock",
                "GPU memory clock",
                Unit::Mhz,
                false,
                gpu.clock_info(Clock::Memory).map(f64::from)
            );
            metric!(
                "fan",
                "GPU fan speed",
                Unit::Percent,
                false,
                gpu.fan_speed(0).map(f64::from)
            );
        }
        Ok(sensors)
    }
}
