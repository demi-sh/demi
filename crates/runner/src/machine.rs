//! The machine the runner runs on: what it is, which the hello carries once,
//! and how busy it is, which a thread of its own reads and each connection
//! reports while it is online.
//!
//! ```text
//!   start ── operating_system, hardware, booted_at ──> hello
//!   usage thread ── 1 s, then every 10 s ──> watch ──> each online connection
//!       └─ the home's volume, read every 5 minutes, rides along
//! ```
//!
//! The intervals are each value's own: the CPU's share is averaged over the
//! 10 seconds between reports, and the volumes take tens of milliseconds to
//! read, which is worth doing only as often as free space changes.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use demi_runner_protocol::wire;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::watch;

/// How long after the runner starts its first usage is read: long enough for
/// the CPU's share to mean something.
const FIRST_REPORT: Duration = Duration::from_secs(1);
const REPORT_EVERY: Duration = Duration::from_secs(10);
const DISK_EVERY: Duration = Duration::from_secs(5 * 60);

/// What a GPU that WSL passes through from Windows is called: Linux sees
/// only Microsoft's virtual device, and the Windows device names it.
#[cfg(target_os = "linux")]
const WSL_GPU: &str = "GPU shared by Windows";

/// The device's operating system as the hello names it (`crates/runner/src/connection/mod.rs`),
/// a Linux under WSL marked as such, which `kernel`'s release tells. A
/// release the system does not tell leaves its name alone.
pub fn operating_system(kernel: Option<&wire::Kernel>) -> wire::OperatingSystem {
    let named = |name: String, release: Option<String>| match release {
        Some(release) => format!("{name} {release}"),
        None => name,
    };
    // Darwin's own name is the kernel's; the product version is macOS's.
    #[cfg(target_os = "macos")]
    let name = named("macOS".to_owned(), System::os_version());
    // The product name, such as `Windows 11 Pro`, with its build number.
    #[cfg(windows)]
    let name = named(
        System::long_os_version().unwrap_or_else(|| "Windows".to_owned()),
        System::kernel_version().map(|build| format!("(build {build})")),
    );
    #[cfg(any(target_os = "macos", windows))]
    let distribution = None;
    // The distribution's name and version from os-release, such as
    // `Ubuntu 26.04`, and its `ID`, such as `ubuntu`.
    #[cfg(not(any(target_os = "macos", windows)))]
    let name = named(
        System::name().unwrap_or_else(|| "Linux".to_owned()),
        System::os_version(),
    );
    #[cfg(not(any(target_os = "macos", windows)))]
    let distribution = Some(System::distribution_id()).filter(|id| !id.is_empty());
    let wsl = kernel.is_some_and(|kernel| is_wsl(&kernel.release));
    wire::OperatingSystem {
        name: if wsl { format!("{name} (WSL)") } else { name },
        distribution,
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// Whether a kernel release is one of Microsoft's WSL kernels, such as
/// `6.6.87.2-microsoft-standard-WSL2`, or WSL 1's `4.4.0-19041-Microsoft`.
fn is_wsl(release: &str) -> bool {
    release.to_ascii_lowercase().contains("microsoft")
}

/// What the machine is: its processor, memory, the size of the volume that
/// holds `home`, and its GPUs.
pub fn hardware(home: &Path) -> wire::Hardware {
    let system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|brand| !brand.is_empty());
    let disks = Disks::new_with_refreshed_list();
    wire::Hardware {
        cpu,
        cores: u32::try_from(system.cpus().len()).unwrap_or(u32::MAX),
        memory: system.total_memory(),
        disk: home_volume(&disks, home).map(sysinfo::Disk::total_space),
        gpus: gpus(),
    }
}

/// When the machine started; now when the system does not tell.
pub fn booted_at() -> wire::Timestamp {
    let seconds = i64::try_from(System::boot_time()).unwrap_or(0);
    if seconds > 0 {
        return wire::Timestamp(seconds.saturating_mul(1000));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX));
    wire::Timestamp(now)
}

/// Starts the thread that reads how busy the machine is, one second after
/// the runner starts and every 10 seconds after, and answers where each
/// reading appears; none before the first.
pub fn report_usage(home: PathBuf) -> watch::Receiver<Option<wire::Usage>> {
    let (sender, receiver) = watch::channel(None);
    let started = std::thread::Builder::new()
        .name("machine-usage".into())
        .spawn(move || read_usage(&home, &sender));
    if let Err(error) = started {
        // The devices list shows the machine without its usage.
        tracing::warn!(event = "machine_usage.start_failed", %error, "the machine's usage is not read");
    }
    receiver
}

fn read_usage(home: &Path, sender: &watch::Sender<Option<wire::Usage>>) {
    let mut system = System::new();
    system.refresh_cpu_usage();
    let mut disk_available = None;
    let mut disk_read: Option<Instant> = None;
    let mut wait = FIRST_REPORT;
    loop {
        std::thread::sleep(wait);
        wait = REPORT_EVERY;
        system.refresh_cpu_usage();
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        if disk_read.is_none_or(|read| read.elapsed() >= DISK_EVERY) {
            let disks = Disks::new_with_refreshed_list();
            disk_available = home_volume(&disks, home).map(sysinfo::Disk::available_space);
            disk_read = Some(Instant::now());
        }
        let cpu = system.global_cpu_usage();
        let usage = wire::Usage {
            cpu_percent: if cpu.is_finite() { cpu.clamp(0.0, 100.0) } else { 0.0 },
            // `sysinfo`'s own "used" and "available" overlap on macOS; the
            // total less what is available is what Linux's `free` counts.
            memory_used: system.total_memory().saturating_sub(system.available_memory()),
            disk_available,
        };
        // The runner keeps a receiver as long as it runs.
        if sender.send(Some(usage)).is_err() {
            return;
        }
    }
}

/// The volume `home` is on: the one mounted at the longest path that holds
/// it.
fn home_volume<'a>(disks: &'a Disks, home: &Path) -> Option<&'a sysinfo::Disk> {
    disks
        .iter()
        .filter(|disk| home.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
}

/// The machine's GPUs: the PCI display devices, named by the system's
/// `pci.ids`, or by NVIDIA's driver when it runs one.
#[cfg(target_os = "linux")]
fn gpus() -> Vec<wire::Gpu> {
    use std::fs;
    let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") else {
        return Vec::new();
    };
    let mut devices: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    devices.sort();
    let read = |device: &Path, name: &str| {
        fs::read_to_string(device.join(name)).ok().map(|text| text.trim().to_owned())
    };
    let id = |device: &Path, name: &str| {
        read(device, name).and_then(|text| u16::from_str_radix(text.trim_start_matches("0x"), 16).ok())
    };
    let mut found = Vec::new();
    for device in devices {
        // The display controllers' class, `0x03` and a subclass.
        if !read(&device, "class").is_some_and(|class| class.starts_with("0x03")) {
            continue;
        }
        let driver = fs::read_link(device.join("driver"))
            .ok()
            .and_then(|driver| driver.file_name().map(|name| name.to_string_lossy().into_owned()));
        if driver.as_deref() == Some("dxgkrnl") {
            found.push(Found::Named(wire::Gpu { name: WSL_GPU.to_owned(), memory: None }));
            continue;
        }
        let memory = read(&device, "mem_info_vram_total").and_then(|bytes| bytes.parse().ok());
        let address = device.file_name().map(|name| name.to_string_lossy().into_owned());
        let named = address.and_then(|address| nvidia_model(&address));
        match (named, id(&device, "vendor"), id(&device, "device")) {
            (Some(name), _, _) => found.push(Found::Named(wire::Gpu { name, memory })),
            (None, Some(vendor), Some(device)) => found.push(Found::Ids { vendor, device, memory }),
            (None, _, _) => {}
        }
    }
    let wanted: Vec<(u16, u16)> = found
        .iter()
        .filter_map(|found| match found {
            Found::Ids { vendor, device, .. } => Some((*vendor, *device)),
            Found::Named(_) => None,
        })
        .collect();
    let names = if wanted.is_empty() { PciNames::default() } else { PciNames::read(&wanted) };
    found
        .into_iter()
        .map(|found| match found {
            Found::Named(gpu) => gpu,
            Found::Ids { vendor, device, memory } => wire::Gpu {
                name: names.name(vendor, device),
                memory,
            },
        })
        .collect()
}

#[cfg(target_os = "linux")]
enum Found {
    Named(wire::Gpu),
    Ids { vendor: u16, device: u16, memory: Option<u64> },
}

/// The model NVIDIA's own driver names the GPU at a PCI address by, such as
/// `NVIDIA GeForce RTX 4090`; none without that driver.
#[cfg(target_os = "linux")]
fn nvidia_model(address: &str) -> Option<String> {
    let information = std::fs::read_to_string(format!("/proc/driver/nvidia/gpus/{address}/information")).ok()?;
    information
        .lines()
        .find_map(|line| line.strip_prefix("Model:"))
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
}

/// The names `pci.ids` gives the vendors and devices asked for.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct PciNames {
    vendors: std::collections::HashMap<u16, String>,
    devices: std::collections::HashMap<(u16, u16), String>,
}

#[cfg(target_os = "linux")]
impl PciNames {
    /// Where distributions keep `pci.ids`: hwdata's place first, then
    /// pciutils' own.
    const FILES: [&str; 3] = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"];

    fn read(wanted: &[(u16, u16)]) -> Self {
        let text = Self::FILES.iter().find_map(|file| std::fs::read_to_string(file).ok());
        text.map(|text| Self::parse(&text, wanted)).unwrap_or_default()
    }

    /// Reads the vendors' lines (`10de  NVIDIA Corporation`) and, under
    /// each, its devices' (`\t2684  AD102 [GeForce RTX 4090]`), up to the
    /// device classes that end the list.
    fn parse(text: &str, wanted: &[(u16, u16)]) -> Self {
        let mut names = Self::default();
        let mut vendor: Option<u16> = None;
        let entry = |line: &str| {
            let (id, name) = line.split_once("  ")?;
            Some((u16::from_str_radix(id, 16).ok()?, name.trim().to_owned()))
        };
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if line.starts_with("C ") {
                break;
            }
            if let Some(device) = line.strip_prefix('\t') {
                if device.starts_with('\t') {
                    continue;
                }
                if let (Some(vendor), Some((device, name))) = (vendor, entry(device))
                    && wanted.contains(&(vendor, device))
                {
                    names.devices.insert((vendor, device), name);
                }
                continue;
            }
            vendor = None;
            if let Some((id, name)) = entry(line)
                && wanted.iter().any(|(wanted, _)| *wanted == id)
            {
                names.vendors.insert(id, name);
                vendor = Some(id);
            }
        }
        names
    }

    /// Such as `NVIDIA GeForce RTX 4090`: the vendor's short name and the
    /// marketing name in the device's brackets, or the device's own name,
    /// which names its vendor itself when it starts with the vendor's first
    /// word, as `ASPEED Graphics Family` does; the ids where `pci.ids` has
    /// no name.
    fn name(&self, vendor: u16, device: u16) -> String {
        let brand = self.vendors.get(&vendor).map(|name| short_vendor(name));
        match (self.devices.get(&(vendor, device)), brand) {
            (Some(name), Some(brand)) => {
                let model = bracketed(name).unwrap_or(name);
                let first = brand.split(' ').next().unwrap_or(brand);
                if model.starts_with(first) {
                    model.to_owned()
                } else {
                    format!("{brand} {model}")
                }
            }
            (Some(name), None) => bracketed(name).unwrap_or(name).to_owned(),
            (None, Some(brand)) => format!("{brand} {vendor:04x}:{device:04x}"),
            (None, None) => format!("GPU {vendor:04x}:{device:04x}"),
        }
    }
}

/// A vendor's name as people say it: `AMD` for `Advanced Micro Devices,
/// Inc. [AMD/ATI]`, `NVIDIA` for `NVIDIA Corporation`, `Red Hat` for `Red
/// Hat, Inc.`: the short name in brackets, or the name without its company
/// form.
#[cfg(target_os = "linux")]
fn short_vendor(name: &str) -> &str {
    const FORMS: [&str; 9] = [
        ", Inc.", " Inc.", ", Inc", " Corporation", " Corp.", ", Ltd.", " Ltd.", " Co.", " GmbH",
    ];
    if let Some(short) = bracketed(name) {
        return short.split('/').next().unwrap_or(short);
    }
    let mut short = name.trim();
    while let Some(form) = FORMS.iter().find(|form| short.ends_with(*form)) {
        short = short[..short.len() - form.len()].trim_end_matches([',', ' ']);
    }
    if short.is_empty() { name } else { short }
}

/// What the last brackets of a `pci.ids` name hold, the product's
/// marketing name.
#[cfg(target_os = "linux")]
fn bracketed(name: &str) -> Option<&str> {
    let (_, rest) = name.rsplit_once('[')?;
    rest.strip_suffix(']').filter(|inner| !inner.is_empty())
}

/// The machine's GPUs: IOKit's accelerators, each named by its `model`, its
/// own or its PCI device's, with the memory a discrete GPU has.
#[cfg(target_os = "macos")]
fn gpus() -> Vec<wire::Gpu> {
    use objc2_core_foundation::{CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType};
    use objc2_io_kit::{
        IOIteratorNext, IOObjectRelease, IORegistryEntrySearchCFProperty, IOServiceGetMatchingServices,
        IOServiceMatching, io_iterator_t, kIORegistryIterateParents, kIORegistryIterateRecursively,
        kIOReturnSuccess, kIOServicePlane,
    };

    // The accelerator itself, then its parents up to its PCI device.
    let search = |entry, key: &'static str| -> Option<CFRetained<CFType>> {
        let key = CFString::from_static_str(key);
        // SAFETY: `entry` is a live registry entry and the plane's name a
        // NUL-terminated string of the length IOKit's names take.
        unsafe {
            IORegistryEntrySearchCFProperty(
                entry,
                kIOServicePlane.as_ptr().cast_mut().cast(),
                Some(&key),
                None,
                kIORegistryIterateRecursively | kIORegistryIterateParents,
            )
        }
    };
    let mut gpus = Vec::new();
    // SAFETY: the matching dictionary is IOKit's own, which the call
    // consumes, and every object the iterator gives is released.
    unsafe {
        let Some(matching) = IOServiceMatching(c"IOAccelerator".as_ptr()) else {
            return gpus;
        };
        let matching = CFRetained::<CFDictionary>::from(&matching);
        let mut iterator: io_iterator_t = 0;
        // 0 is the default main port.
        if IOServiceGetMatchingServices(0, Some(matching), &mut iterator) != kIOReturnSuccess {
            return gpus;
        }
        loop {
            let entry = IOIteratorNext(iterator);
            if entry == 0 {
                break;
            }
            let name = search(entry, "model").and_then(|model| {
                if let Some(text) = model.downcast_ref::<CFString>() {
                    return Some(text.to_string());
                }
                let bytes = model.downcast_ref::<CFData>()?.to_vec();
                Some(String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_owned())
            });
            let memory = search(entry, "VRAM,totalMB")
                .and_then(|megabytes| megabytes.downcast_ref::<CFNumber>()?.as_i64())
                .and_then(|megabytes| u64::try_from(megabytes).ok())
                .map(|megabytes| megabytes * 1024 * 1024);
            IOObjectRelease(entry);
            if let Some(name) = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()) {
                gpus.push(wire::Gpu { name, memory });
            }
        }
        IOObjectRelease(iterator);
    }
    gpus
}

/// The machine's GPUs: the display devices present whose hardware id is a
/// PCI one, which leaves out the virtual displays of remote desktops and
/// streaming apps, each with the memory its driver key records.
#[cfg(windows)]
fn gpus() -> Vec<wire::Gpu> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DICS_FLAG_GLOBAL, DIGCF_PRESENT, DIREG_DRV, GUID_DEVCLASS_DISPLAY, SP_DEVINFO_DATA, SPDRP_DEVICEDESC,
        SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW, SetupDiOpenDevRegKey,
    };
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Registry::{KEY_READ, RegCloseKey, RegQueryValueExW};

    let mut gpus = Vec::new();
    // SAFETY: each call gets the set SetupAPI made, the device data it
    // filled and buffers of the sizes passed; the set and the key are
    // closed once read.
    unsafe {
        let set = SetupDiGetClassDevsW(&GUID_DEVCLASS_DISPLAY, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT);
        if set == -1 {
            return gpus;
        }
        // The first string of a property, which a multi-string's first is.
        let property = |data: &SP_DEVINFO_DATA, property| {
            let mut buffer = [0u16; 512];
            let size = u32::try_from(std::mem::size_of_val(&buffer)).unwrap_or(u32::MAX);
            let read = SetupDiGetDeviceRegistryPropertyW(
                set,
                data,
                property,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                size,
                std::ptr::null_mut(),
            );
            (read != 0).then(|| {
                let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
                String::from_utf16_lossy(&buffer[..end])
            })
        };
        let value_name: Vec<u16> = "HardwareInformation.qwMemorySize\0".encode_utf16().collect();
        for index in 0.. {
            let mut data = SP_DEVINFO_DATA {
                cbSize: u32::try_from(std::mem::size_of::<SP_DEVINFO_DATA>()).unwrap_or(u32::MAX),
                ..std::mem::zeroed()
            };
            if SetupDiEnumDeviceInfo(set, index, &mut data) == 0 {
                break;
            }
            let pci = property(&data, SPDRP_HARDWAREID).is_some_and(|id| id.to_ascii_uppercase().starts_with("PCI\\"));
            let Some(name) = property(&data, SPDRP_DEVICEDESC).filter(|name| pci && !name.trim().is_empty()) else {
                continue;
            };
            let key = SetupDiOpenDevRegKey(set, &data, DICS_FLAG_GLOBAL, 0, DIREG_DRV, KEY_READ);
            let mut memory = None;
            if !key.is_null() && key != INVALID_HANDLE_VALUE {
                let mut bytes = 0u64;
                let mut size = u32::try_from(std::mem::size_of::<u64>()).unwrap_or(0);
                let read = RegQueryValueExW(
                    key,
                    value_name.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    (&raw mut bytes).cast(),
                    &mut size,
                );
                if read == 0 && size == 8 && bytes > 0 {
                    memory = Some(bytes);
                }
                RegCloseKey(key);
            }
            gpus.push(wire::Gpu { name: name.trim().to_owned(), memory });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    gpus
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn gpus() -> Vec<wire::Gpu> {
    Vec::new()
}
