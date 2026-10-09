//! Optional Intel Level Zero Sysman collector. Native C ABI, read-only getters.
//! Structure layouts and units follow Intel's zes_api.h / ze_api.h specification.
use super::{counter_rate, sensor, system_library};
use libloading::Library;
use lightdriver_core::{Sensor, Unit};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    hash::{Hash, Hasher},
    ptr,
};
type Handle = *mut c_void;
type ResultCode = u32;
type Enumerate = unsafe extern "system" fn(Handle, *mut u32, *mut Handle) -> ResultCode;
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Counter {
    value: u64,
    timestamp: u64,
}
#[repr(C)]
struct TemperatureProperties {
    stype: u32,
    next: *mut c_void,
    kind: u32,
    subdevice: u32,
    subdevice_id: u32,
    max_c: f64,
    critical: u32,
    threshold1: u32,
    threshold2: u32,
}
#[repr(C)]
struct EngineProperties {
    stype: u32,
    next: *mut c_void,
    kind: u32,
    subdevice: u32,
    subdevice_id: u32,
}
#[repr(C)]
struct CoreProperties {
    stype: u32,
    next: *mut c_void,
    kind: u32,
    vendor: u32,
    device: u32,
    flags: u32,
    subdevice: u32,
    clock: u32,
    allocation: u64,
    contexts: u32,
    priority: u32,
    threads: u32,
    simd: u32,
    eus: u32,
    subslices: u32,
    slices: u32,
    resolution: u64,
    timestamp_bits: u32,
    kernel_bits: u32,
    uuid: [u8; 16],
    name: [c_char; 256],
}
#[repr(C)]
struct DeviceProperties {
    stype: u32,
    next: *mut c_void,
    core: CoreProperties,
    subdevices: u32,
    strings: [[c_char; 64]; 6],
}
pub struct Intel {
    library: Library,
    previous: HashMap<String, (u64, u64)>,
}
fn checked(code: u32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("Level Zero returned 0x{code:08x}"))
    }
}
impl Intel {
    pub fn new() -> Result<Self, String> {
        let path = system_library("ze_loader.dll")?;
        // SAFETY: installed system library, typed symbols matching the published Sysman ABI.
        unsafe {
            let library =
                Library::new(path).map_err(|_| "Unable to load Intel Level Zero".to_string())?;
            let init = library
                .get::<unsafe extern "system" fn(u32) -> ResultCode>(b"zesInit\0")
                .map_err(|_| "Sysman initialization is not exposed by this loader")?;
            checked(init(0))?;
            Ok(Self {
                library,
                previous: HashMap::new(),
            })
        }
    }
    unsafe fn enumerate(&self, name: &[u8], parent: Handle) -> Result<Vec<Handle>, String> {
        // SAFETY: callers supply a valid parent handle for the named SDK enumeration API.
        unsafe {
            let f = self
                .library
                .get::<Enumerate>(name)
                .map_err(|_| "Sysman enumeration API unavailable")?;
            let mut count = 0;
            checked(f(parent, &mut count, ptr::null_mut()))?;
            if count > 64 {
                return Err("Sysman resource count exceeds collector limit".into());
            }
            let mut handles = vec![ptr::null_mut(); count as usize];
            if count > 0 {
                checked(f(parent, &mut count, handles.as_mut_ptr()))?;
            }
            handles.truncate((count as usize).min(handles.len()));
            handles.retain(|h| !h.is_null());
            Ok(handles)
        }
    }
    fn delta(
        &mut self,
        key: &str,
        reading: Result<Counter, String>,
        scale: f64,
    ) -> Result<f64, String> {
        match reading {
            Ok(counter) => {
                let current = (counter.value, counter.timestamp);
                let previous = self.previous.insert(key.into(), current);
                counter_rate(previous, current)
                    .map(|v| v * scale)
                    .ok_or_else(|| {
                        "Collecting two valid counter samples; counter reset or zero interval"
                            .into()
                    })
            }
            Err(e) => {
                self.previous.remove(key);
                Err(e)
            }
        }
    }
    unsafe fn read_counter(&self, api: &[u8], handle: Handle) -> Result<Counter, String> {
        // SAFETY: SDK counter structs contain exactly two uint64 fields; valid live handle.
        unsafe {
            let f = self
                .library
                .get::<unsafe extern "system" fn(Handle, *mut Counter) -> ResultCode>(api)
                .map_err(|_| "Sysman counter API unavailable")?;
            let mut counter = Counter::default();
            checked(f(handle, &mut counter))?;
            Ok(counter)
        }
    }
    pub fn poll(&mut self) -> Result<Vec<Sensor>, String> {
        let mut sensors = vec![];
        // SAFETY: handles are enumerated by this initialized loader; buffers and structs
        // have the published repr(C) layouts, counts are bounded, no setter API is loaded.
        unsafe {
            let driver_get = self
                .library
                .get::<unsafe extern "system" fn(*mut u32, *mut Handle) -> ResultCode>(
                    b"zesDriverGet\0",
                )
                .map_err(|_| "Sysman driver enumeration unavailable")?;
            let mut count = 0;
            checked(driver_get(&mut count, ptr::null_mut()))?;
            if count > 16 {
                return Err("Too many Sysman drivers".into());
            }
            let mut drivers = vec![ptr::null_mut(); count as usize];
            if count > 0 {
                checked(driver_get(&mut count, drivers.as_mut_ptr()))?;
            }
            drivers.truncate((count as usize).min(drivers.len()));
            for driver in drivers.into_iter().filter(|h| !h.is_null()) {
                for device in self
                    .enumerate(b"zesDeviceGet\0", driver)?
                    .into_iter()
                    .take(8)
                {
                    let get = self.library.get::<unsafe extern "system" fn(Handle, *mut DeviceProperties) -> ResultCode>(b"zesDeviceGetProperties\0")
                        .map_err(|_| "Sysman device properties unavailable")?;
                    let mut properties: DeviceProperties = std::mem::zeroed();
                    properties.stype = 1; // ZES_STRUCTURE_TYPE_DEVICE_PROPERTIES
                    checked(get(device, &mut properties))?;
                    if properties.core.vendor != 0x8086 {
                        continue;
                    }
                    let bytes: Vec<u8> = properties
                        .core
                        .name
                        .iter()
                        .take_while(|v| **v != 0)
                        .map(|v| *v as u8)
                        .collect();
                    let name = String::from_utf8_lossy(&bytes);
                    let group = if name.is_empty() { "Intel GPU" } else { &name };
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    properties.core.uuid.hash(&mut hash);
                    (device as usize).hash(&mut hash);
                    let id = format!("intel.{:x}", hash.finish());
                    let mut got_temperature = false;
                    if let Ok(domains) =
                        self.enumerate(b"zesDeviceEnumTemperatureSensors\0", device)
                    {
                        for (i, domain) in domains.into_iter().enumerate() {
                            let prop_get = self.library.get::<unsafe extern "system" fn(
                                Handle,
                                *mut TemperatureProperties,
                            )
                                -> ResultCode>(
                                b"zesTemperatureGetProperties\0"
                            );
                            let state_get = self.library.get::<unsafe extern "system" fn(
                                Handle,
                                *mut f64,
                            )
                                -> ResultCode>(
                                b"zesTemperatureGetState\0"
                            );
                            if let (Ok(prop_get), Ok(state_get)) = (prop_get, state_get) {
                                let mut prop: TemperatureProperties = std::mem::zeroed();
                                prop.stype = 0x14;
                                if prop_get(domain, &mut prop) != 0 {
                                    continue;
                                }
                                let label = match prop.kind {
                                    0 => "GPU maximum sensor temperature",
                                    1 => "GPU temperature",
                                    2 => "GPU memory temperature",
                                    _ => "GPU temperature domain",
                                };
                                let mut value = 0.0;
                                let reading = checked(state_get(domain, &mut value)).map(|_| value);
                                let basic = prop.kind == 1 && prop.subdevice == 0;
                                got_temperature |= basic;
                                sensors.push(sensor(
                                    &format!("{id}.temperature.{i}"),
                                    group,
                                    label,
                                    Unit::Celsius,
                                    "Intel · Level Zero Sysman",
                                    basic,
                                    reading,
                                ));
                            }
                        }
                    }
                    if !got_temperature {
                        sensors.push(sensor(
                            &format!("{id}.temperature"),
                            group,
                            "GPU temperature",
                            Unit::Celsius,
                            "Intel · Level Zero Sysman",
                            true,
                            Err("Root GPU temperature is not exposed by this driver".into()),
                        ));
                    }
                    let power_read = (|| {
                        let get = self
                            .library
                            .get::<unsafe extern "system" fn(Handle, *mut Handle) -> ResultCode>(
                                b"zesDeviceGetCardPowerDomain\0",
                            )
                            .map_err(|_| "Card power API unavailable".to_string())?;
                        let mut power = ptr::null_mut();
                        checked(get(device, &mut power))?;
                        if power.is_null() {
                            return Err("Card power domain is unavailable".into());
                        }
                        self.read_counter(b"zesPowerGetEnergyCounter\0", power)
                    })();
                    let power = self.delta(&format!("{id}.power"), power_read, 1.0);
                    sensors.push(sensor(
                        &format!("{id}.power"),
                        group,
                        "GPU card power (interval average)",
                        Unit::Watts,
                        "Intel · Level Zero Sysman",
                        true,
                        power,
                    ));
                    let mut load_read = Err("Combined GPU engine activity is not exposed".into());
                    if let Ok(engines) = self.enumerate(b"zesDeviceEnumEngineGroups\0", device)
                        && let Ok(get) = self.library.get::<unsafe extern "system" fn(
                            Handle,
                            *mut EngineProperties,
                        )
                            -> ResultCode>(
                            b"zesEngineGetProperties\0"
                        )
                    {
                        for engine in engines {
                            let mut p: EngineProperties = std::mem::zeroed();
                            p.stype = 5;
                            if get(engine, &mut p) == 0 && p.kind == 0 && p.subdevice == 0 {
                                load_read = self.read_counter(b"zesEngineGetActivity\0", engine);
                                break;
                            }
                        }
                    }
                    let load = self.delta(&format!("{id}.load"), load_read, 100.0);
                    sensors.push(sensor(
                        &format!("{id}.load"),
                        group,
                        "GPU load (combined engines)",
                        Unit::Percent,
                        "Intel · Level Zero Sysman",
                        true,
                        load,
                    ));
                }
            }
        }
        Ok(sensors)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_layouts_match_windows_x64_headers() {
        assert_eq!(std::mem::size_of::<Counter>(), 16);
        assert_eq!(std::mem::size_of::<TemperatureProperties>(), 56);
        assert_eq!(std::mem::offset_of!(CoreProperties, name), 112);
        assert_eq!(std::mem::size_of::<CoreProperties>(), 368);
        assert_eq!(std::mem::size_of::<DeviceProperties>(), 776);
        assert_eq!(std::mem::size_of::<EngineProperties>(), 32);
    }
}
