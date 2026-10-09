//! ADLX read-only metrics, using pinned Rust ABI bindings and the installed driver DLL.
use super::{sensor, system_library};
use adlx::{GpuList, Interface, PerformanceMonitoringServices, ffi};
use lightdriver_core::{Sensor, Unit};
use std::ptr;

pub struct Amd {
    system: *mut ffi::IADLXSystem,
    terminate: unsafe extern "C" fn() -> ffi::ADLX_RESULT,
    _library: libloading::Library,
}
impl Amd {
    pub fn new() -> Result<Self, String> {
        let path = system_library("amdadlx64.dll")?;
        // SAFETY: an absolute installed-driver path is used. Symbols use the SDK C ABI.
        unsafe {
            let library = libloading::Library::new(path)
                .map_err(|_| "Unable to load AMD ADLX".to_string())?;
            let init: unsafe extern "C" fn(u64, *mut *mut ffi::IADLXSystem) -> ffi::ADLX_RESULT =
                *library
                    .get(b"ADLXInitialize\0")
                    .map_err(|_| "ADLXInitialize is missing")?;
            let terminate = *library
                .get::<unsafe extern "C" fn() -> ffi::ADLX_RESULT>(b"ADLXTerminate\0")
                .map_err(|_| "ADLXTerminate is missing")?;
            let version = (u64::from(ffi::ADLX_VER_MAJOR) << 48)
                | (u64::from(ffi::ADLX_VER_MINOR) << 32)
                | (u64::from(ffi::ADLX_VER_RELEASE) << 16)
                | u64::from(ffi::ADLX_VER_BUILD_NUM);
            let mut system = ptr::null_mut();
            let result = init(version, &mut system);
            if result != 0 || system.is_null() {
                return Err(format!("ADLX initialization returned {result}"));
            }
            Ok(Self {
                system,
                terminate,
                _library: library,
            })
        }
    }
    pub fn poll(&mut self) -> Result<Vec<Sensor>, String> {
        // SAFETY: system lives until Drop; successful getters transfer owned interface
        // references into Rust wrappers, all dropped before ADLX termination/library unload.
        let (gpus, perf) = unsafe {
            let vtable = &*(*self.system).pVtbl;
            let mut gpus = ptr::null_mut();
            let mut perf = ptr::null_mut();
            let get_gpus = vtable.GetGPUs.ok_or("ADLX GPU enumeration unavailable")?;
            if get_gpus(self.system, &mut gpus) != 0 || gpus.is_null() {
                return Err("ADLX GPU enumeration failed".into());
            }
            let gpus = GpuList::from_raw(gpus);
            let get_perf = vtable
                .GetPerformanceMonitoringServices
                .ok_or("ADLX performance API unavailable")?;
            if get_perf(self.system, &mut perf) != 0 || perf.is_null() {
                return Err("ADLX performance API failed".into());
            }
            (gpus, PerformanceMonitoringServices::from_raw(perf))
        };
        let mut sensors = vec![];
        for i in 0..gpus.size().min(8) {
            let gpu = gpus.at(i).map_err(|e| format!("ADLX GPU {i}: {e:?}"))?;
            let vendor = gpu.vendor_id().unwrap_or("");
            if !vendor.trim_start_matches("0x").eq_ignore_ascii_case("1002") {
                continue;
            }
            let name = gpu.name().unwrap_or("AMD GPU");
            let id = format!("amd.{}", gpu.unique_id().unwrap_or(i as i32));
            let metrics = perf
                .current_gpu_metrics(&gpu)
                .map_err(|e| format!("ADLX metrics: {e:?}"))?;
            let support = perf
                .supported_gpu_metrics(&gpu)
                .map_err(|e| format!("ADLX support: {e:?}"))?;
            macro_rules! metric {
                ($key:expr, $label:expr, $unit:expr, $basic:expr, $supported:ident, $read:ident) => {
                    sensors.push(sensor(
                        &format!("{id}.{}", $key),
                        name,
                        $label,
                        $unit,
                        "AMD · ADLX",
                        $basic,
                        match support.$supported() {
                            Ok(true) => metrics
                                .$read()
                                .map(f64::from)
                                .map_err(|e| format!("ADLX read: {e:?}")),
                            Ok(false) => Err("Not supported by this GPU / driver".into()),
                            Err(e) => Err(format!("ADLX support query: {e:?}")),
                        },
                    ));
                };
            }
            metric!(
                "temperature",
                "GPU temperature",
                Unit::Celsius,
                true,
                is_supported_gpu_temperature,
                temperature
            );
            metric!(
                "hotspot",
                "GPU hotspot",
                Unit::Celsius,
                false,
                is_supported_gpu_hotspot_temperature,
                hotspot_temperature
            );
            metric!(
                "load",
                "GPU load",
                Unit::Percent,
                true,
                is_supported_gpu_usage,
                usage
            );
            metric!(
                "board-power",
                "GPU board power",
                Unit::Watts,
                true,
                is_supported_gpu_total_board_power,
                total_board_power
            );
            metric!(
                "chip-power",
                "GPU chip power",
                Unit::Watts,
                false,
                is_supported_gpu_power,
                power
            );
            metric!(
                "fan",
                "GPU fan speed",
                Unit::Rpm,
                false,
                is_supported_gpu_fan_speed,
                fan_speed
            );
            metric!(
                "clock",
                "GPU clock",
                Unit::Mhz,
                false,
                is_supported_gpu_clock_speed,
                clock_speed
            );
            metric!(
                "memory-clock",
                "GPU memory clock",
                Unit::Mhz,
                false,
                is_supported_gpu_vram_clock_speed,
                vram_clock_speed
            );
            let vram = match support.is_supported_gpu_vram() {
                Ok(true) => metrics
                    .vram()
                    .map(|v| f64::from(v) / 1024.0)
                    .map_err(|e| format!("{e:?}")),
                _ => Err("GPU memory usage is unavailable".into()),
            };
            sensors.push(sensor(
                &format!("{id}.memory"),
                name,
                "GPU memory used",
                Unit::Gib,
                "AMD · ADLX",
                false,
                vram,
            ));
        }
        Ok(sensors)
    }
}
impl Drop for Amd {
    fn drop(&mut self) {
        // SAFETY: this collector owns one successful initialization, with no outstanding interfaces.
        unsafe {
            (self.terminate)();
        }
    }
}
