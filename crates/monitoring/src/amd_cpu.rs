//! Independent Rust bindings to Ryzen Master Monitoring SDK 3.1.1.5478, MSVC x64.
//! ABI verified against AMD's public headers and an MSVC layout/assembly probe.
//! No vendor headers/binaries, C++ shim, registry writes or driver installer are bundled.
use super::super::sensor;
use libloading::os::windows::{
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
};
use lightdriver_core::{Sensor, Unit};
use std::{
    ffi::{OsString, c_char},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{self, NonNull},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    },
    System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW},
};

const SDK_VERSION: (u32, u32) = ((3 << 16) | 1, (1 << 16) | 5478);
const SOURCE: &str = "AMD · Ryzen Master Monitoring SDK";
#[repr(C)]
struct Interface<V> {
    vtable: *const V,
}
type Platform = Interface<PlatformVtable>;
type Manager = Interface<ManagerVtable>;
type Cpu = Interface<CpuVtable>;
#[repr(C)]
struct PlatformVtable {
    init: unsafe extern "system" fn(*mut Platform, *const c_char, u8) -> u8,
    uninit: unsafe extern "system" fn(*mut Platform) -> u8,
    manager: unsafe extern "system" fn(*mut Platform) -> *mut Manager,
}
#[repr(C)]
struct ManagerVtable {
    unused: [usize; 3],
    // MSVC puts GetDevice(type,index) in slot 3, after GetDevice(index).
    // Declaration order alone is insufficient for these overloaded methods.
    device_by_type: unsafe extern "system" fn(*mut Manager, i32, u32) -> *mut Cpu,
}
#[repr(C)]
struct CpuVtable {
    unused: [usize; 7],
    device_type: unsafe extern "system" fn(*mut Cpu) -> i32,
    unused_more: [usize; 9], // Includes the virtual destructor in slot 9.
    parameters: unsafe extern "system" fn(*mut Cpu, *mut CpuParameters) -> i32,
}
#[repr(C)]
struct EffectiveFrequency {
    length: u32,
    effective_mhz: *mut f64,
    active_percent: *mut f64,
    current_mhz: *mut f64,
    temperature: *mut f64,
}
#[repr(C)]
struct CpuParameters {
    mode: u32,
    frequency: EffectiveFrequency,
    peak_voltage: f64,
    peak_voltage_1: f64,
    soc_voltage: f64,
    temperature: f64,
    average_voltage: f64,
    average_voltage_1: f64,
    peak_mhz: f64,
    metrics: [f32; 21], // PPT limit/value, currents, limits, rails; exact SDK order.
}
impl CpuParameters {
    fn empty() -> Self {
        Self {
            mode: 0,
            frequency: EffectiveFrequency {
                length: 0,
                effective_mhz: ptr::null_mut(),
                active_percent: ptr::null_mut(),
                current_mhz: ptr::null_mut(),
                temperature: ptr::null_mut(),
            },
            peak_voltage: f64::NAN,
            peak_voltage_1: f64::NAN,
            soc_voltage: f64::NAN,
            temperature: f64::NAN,
            average_voltage: f64::NAN,
            average_voltage_1: f64::NAN,
            peak_mhz: f64::NAN,
            metrics: [f32::NAN; 21],
        }
    }
}
struct Reader {
    cpu: NonNull<Cpu>,
    last_query: Option<Instant>,
    cached: Vec<Sensor>,
}
impl Reader {
    fn poll_at(&mut self, group: &str, now: Instant) -> Vec<Sensor> {
        if self.last_query.is_some_and(|previous| {
            now.checked_duration_since(previous)
                .is_none_or(|elapsed| elapsed < Duration::from_secs(1))
        }) {
            // Preserve original timestamps; an early poll never makes old data fresh.
            return self.cached.clone();
        }
        self.last_query = Some(now);
        let mut values = CpuParameters::empty();
        // SAFETY: the pinned SDK owns this CPU and vtable until platform UnInit.
        // MSVC x64 slot/struct layouts are verified below. This is an output struct;
        // returned SDK-owned array pointers are neither dereferenced nor freed here.
        let status =
            unsafe { ((*self.cpu.as_ref().vtable).parameters)(self.cpu.as_ptr(), &mut values) };
        let reading = |value| match status {
            0 => Ok(value),
            4 => Err("CPU parameters are unsupported by this CPU / SDK".into()),
            other => Err(format!(
                "GetCPUParameters returned {other}; check SDK setup and permissions"
            )),
        };
        self.cached = vec![
            sensor(
                "cpu.temperature",
                group,
                "CPU package temperature",
                Unit::Celsius,
                SOURCE,
                true,
                reading(values.temperature),
            ),
            sensor(
                "cpu.power",
                group,
                "CPU package power (PPT)",
                Unit::Watts,
                SOURCE,
                true,
                reading(f64::from(values.metrics[1])),
            ),
            sensor(
                "cpu.clock",
                group,
                "Fastest active core",
                Unit::Mhz,
                SOURCE,
                true,
                reading(values.peak_mhz),
            ),
        ];
        self.cached.clone()
    }
}
pub(crate) struct AmdCpu {
    platform: NonNull<Platform>,
    reader: Reader,
    // Drop keeps Platform loaded through UnInit and unloads it before Device.
    _platform_library: Library,
    _device_library: Library,
}
impl AmdCpu {
    pub(crate) fn new() -> Result<Self, String> {
        // SAFETY: CPUID leaf 0 is available on x64; this does not access hardware sensors.
        #[allow(unused_unsafe)] // __cpuid is unsafe on the declared Rust 1.88 MSRV.
        let id = unsafe { std::arch::x86_64::__cpuid(0) };
        let vendor: Vec<_> = [id.ebx, id.edx, id.ecx]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        if vendor != b"AuthenticAMD" {
            return Err("AMD CPU SDK requires an AMD CPU; Intel CPU SDK adapter is pending".into());
        }
        let root = sdk_root()?;
        let platform_path = root.join("bin").join("Platform.dll");
        let device_path = root.join("bin").join("Device.dll");
        verify_version(&platform_path)?;
        verify_version(&device_path)?;
        if !super::super::is_administrator()? {
            return Err("Enable CPU sensors to allow the local service to read AMD's SDK with administrator permissions".into());
        }
        // SAFETY: absolute SDK paths, exact file-version gate, bounded DLL dependency
        // search. Preload Device explicitly; no process-wide DLL search changes.
        unsafe {
            let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
            let device_library = Library::load_with_flags(device_path, flags)
                .map_err(|_| "Unable to load SDK Device.dll; verify the AMD SDK installation")?;
            let platform_library = Library::load_with_flags(platform_path, flags)
                .map_err(|_| "Unable to load SDK Platform.dll; verify the AMD SDK installation")?;
            let get_platform = *platform_library
                .get::<unsafe extern "system" fn() -> *mut Platform>(b"GetPlatform\0")
                .map_err(|_| "SDK GetPlatform export is missing")?;
            let platform = NonNull::new(get_platform()).ok_or("SDK returned no platform")?;
            if platform.as_ref().vtable.is_null() {
                return Err("SDK returned no platform interface".into());
            }
            let table = &*platform.as_ref().vtable;
            if (table.init)(platform.as_ptr(), ptr::null(), 1) != 1 {
                return Err("AMD CPU SDK initialization failed; install the SDK and check its driver / permissions".into());
            }
            // After successful Init, every failure must release the platform.
            let cpu = (|| {
                let manager = NonNull::new((table.manager)(platform.as_ptr()))
                    .ok_or("SDK returned no device manager")?;
                if manager.as_ref().vtable.is_null() {
                    return Err("SDK returned no manager interface");
                }
                let cpu = NonNull::new(((*manager.as_ref().vtable).device_by_type)(
                    manager.as_ptr(),
                    0,
                    0,
                ))
                .ok_or("SDK did not expose a compatible CPU")?;
                if cpu.as_ref().vtable.is_null()
                    || ((*cpu.as_ref().vtable).device_type)(cpu.as_ptr()) != 0
                {
                    return Err("SDK CPU interface is invalid");
                }
                Ok(cpu)
            })();
            let cpu = match cpu {
                Ok(cpu) => cpu,
                Err(error) => {
                    (table.uninit)(platform.as_ptr());
                    return Err(error.into());
                }
            };
            Ok(Self {
                platform,
                reader: Reader {
                    cpu,
                    last_query: None,
                    cached: vec![],
                },
                _platform_library: platform_library,
                _device_library: device_library,
            })
        }
    }
    pub(crate) fn poll(&mut self, group: &str) -> Vec<Sensor> {
        self.reader.poll_at(group, Instant::now())
    }
}
impl Drop for AmdCpu {
    fn drop(&mut self) {
        // SAFETY: exactly one successful Init, no borrowed SDK data outlives the poll,
        // and both libraries remain loaded until after this Drop completes.
        unsafe {
            ((*self.platform.as_ref().vtable).uninit)(self.platform.as_ptr());
        }
    }
}
// Detection is read-only; demo bypasses this path entirely.
pub(crate) fn elevation_needed() -> bool {
    #[allow(unused_unsafe)]
    let id = unsafe { std::arch::x86_64::__cpuid(0) };
    let vendor: Vec<_> = [id.ebx, id.edx, id.ecx]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    vendor == b"AuthenticAMD"
        && super::super::is_administrator().is_ok_and(|admin| !admin)
        && sdk_root().is_ok_and(|root| {
            root.join("bin/Platform.dll").is_file() && root.join("bin/Device.dll").is_file()
        })
}
fn sdk_root() -> Result<PathBuf, String> {
    let configured = std::env::var_os("LIGHTDRIVER_AMD_CPU_SDK_DIR")
        .or_else(|| std::env::var_os("AMDRMMONITORSDKPATH"))
        .or_else(registry_sdk_root);
    let root = if let Some(root) = configured {
        PathBuf::from(root)
    } else {
        PathBuf::from(
            std::env::var_os("ProgramW6432")
                .or_else(|| std::env::var_os("ProgramFiles"))
                .ok_or("Windows Program Files directory is unavailable")?,
        )
        .join("AMD")
        .join("RyzenMasterMonitoringSDK")
    };
    if !root.is_absolute() {
        return Err("AMD CPU SDK directory must be an absolute path".into());
    }
    root.canonicalize().map_err(|_| "AMD Ryzen Master Monitoring SDK is not installed; install SDK 3.1.1.5478 and restart the service".into())
}
fn registry_sdk_root() -> Option<OsString> {
    let key: Vec<_> = r"Software\AMD\RyzenMasterMonitoringSDK"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value: Vec<_> = "InstallationPath".encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0u16; 4096];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: read-only registry query with fixed capacity and null-terminated names.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 || bytes == 0 || bytes as usize > std::mem::size_of_val(&buffer) {
        return None;
    }
    let length = buffer.iter().position(|v| *v == 0)?;
    (length > 0).then(|| OsString::from_wide(&buffer[..length]))
}
fn verify_version(path: &Path) -> Result<(), String> {
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: bounded Win32 version-resource reads, with returned pointer checked
    // against the owning buffer before copying the fixed information unaligned.
    unsafe {
        let mut ignored = 0;
        let bytes = GetFileVersionInfoSizeW(wide.as_ptr(), &mut ignored);
        if bytes == 0 || bytes > 1_048_576 {
            return Err(format!(
                "{filename} is missing or has no SDK version metadata"
            ));
        }
        let mut buffer = vec![0u8; bytes as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, bytes, buffer.as_mut_ptr().cast()) == 0 {
            return Err(format!("Cannot read {filename} SDK version"));
        }
        let mut info = ptr::null_mut();
        let mut length = 0;
        if VerQueryValueW(
            buffer.as_ptr().cast(),
            [b'\\' as u16, 0].as_ptr(),
            &mut info,
            &mut length,
        ) == 0
        {
            return Err(format!("{filename} SDK version is unavailable"));
        }
        let begin = buffer.as_ptr() as usize;
        let offset = (info as usize)
            .checked_sub(begin)
            .ok_or("Invalid SDK version metadata")?;
        let end = offset
            .checked_add(std::mem::size_of::<VS_FIXEDFILEINFO>())
            .ok_or("Invalid SDK version metadata")?;
        if length < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32 || end > buffer.len() {
            return Err("Invalid SDK version metadata".into());
        }
        let info = ptr::read_unaligned(info.cast::<VS_FIXEDFILEINFO>());
        if info.dwSignature != 0xFEEF04BD
            || (info.dwFileVersionMS, info.dwFileVersionLS) != SDK_VERSION
        {
            return Err(format!(
                "{filename} version is unsupported; this adapter requires SDK 3.1.1.5478"
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use lightdriver_core::SensorStatus;
    #[test]
    fn abi_matches_sdk_3_1_1_msvc_x64() {
        assert_eq!(std::mem::size_of::<CpuParameters>(), 192);
        assert_eq!(std::mem::size_of::<EffectiveFrequency>(), 40);
        assert_eq!(std::mem::offset_of!(CpuParameters, frequency), 8);
        assert_eq!(std::mem::offset_of!(CpuParameters, temperature), 72);
        assert_eq!(std::mem::offset_of!(CpuParameters, metrics) + 4, 108);
        assert_eq!(std::mem::offset_of!(ManagerVtable, device_by_type), 24);
        assert_eq!(std::mem::offset_of!(CpuVtable, parameters), 136);
    }
    #[repr(C)]
    struct FakeCpu {
        interface: Cpu,
        calls: u32,
        result: i32,
        fill_power: bool,
    }
    unsafe extern "system" fn fake_type(_: *mut Cpu) -> i32 {
        0
    }
    unsafe extern "system" fn fake_parameters(cpu: *mut Cpu, output: *mut CpuParameters) -> i32 {
        // SAFETY: tests supply their own repr(C) FakeCpu and writable output.
        unsafe {
            let fake = &mut *cpu.cast::<FakeCpu>();
            fake.calls += 1;
            (*output).temperature = 0.0;
            (*output).peak_mhz = 0.0;
            if fake.fill_power {
                (*output).metrics[1] = 0.0;
            }
            fake.result
        }
    }
    #[test]
    fn virtual_dispatch_throttle_missing_fields_and_failures() {
        let table = CpuVtable {
            unused: [0; 7],
            device_type: fake_type,
            unused_more: [0; 9],
            parameters: fake_parameters,
        };
        let mut fake = FakeCpu {
            interface: Cpu { vtable: &table },
            calls: 0,
            result: 0,
            fill_power: true,
        };
        let mut reader = Reader {
            cpu: NonNull::from(&mut fake.interface),
            last_query: None,
            cached: vec![],
        };
        let start = Instant::now();
        let first = reader.poll_at("CPU", start);
        assert!(
            first
                .iter()
                .all(|s| s.sample.as_ref().is_some_and(|v| v.value == 0.0))
        );
        let cached = reader.poll_at("CPU", start + Duration::from_millis(999));
        assert_eq!(fake.calls, 1);
        assert_eq!(
            cached[0].sample.as_ref().unwrap().at_ms,
            first[0].sample.as_ref().unwrap().at_ms
        );
        fake.fill_power = false;
        let partial = reader.poll_at("CPU", start + Duration::from_secs(1));
        assert_eq!(partial[0].status, SensorStatus::Available);
        assert_eq!(partial[1].status, SensorStatus::Error);
        assert!(partial[1].sample.is_none());
        fake.result = -1;
        let failed = reader.poll_at("CPU", start + Duration::from_secs(2));
        assert!(
            failed
                .iter()
                .all(|s| s.sample.is_none() && s.status == SensorStatus::Unavailable)
        );
        assert_eq!(fake.calls, 3);
    }
}
