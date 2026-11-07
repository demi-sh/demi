//! A device's machine as `demi device list` and `demi device show` tell it:
//! what it is, which the backend keeps from its runner's hello, and how busy
//! it is, which the connection holds from the runner's latest report and
//! which goes with the connection.

use demi_backend_database::devices::DeviceRecord;
use demi_runner_client::Link;
use demi_shared_types::Timestamp;
use schemars::JsonSchema;
use serde::Serialize;

/// What a device's machine is and how busy it is now.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Machine {
    /// Its operating system, such as `Ubuntu 24.04 (WSL)` or `macOS 26.5`;
    /// null before its runner first connected.
    pub system: Option<String>,
    /// The processor architecture, such as `x86_64` or `aarch64`.
    pub arch: Option<String>,
    /// The processor's name; null when the system does not tell it, or
    /// before its runner first reported it.
    pub cpu: Option<String>,
    /// Logical cores, which `usage.cpu_percent` is a share of.
    pub cores: Option<u32>,
    pub memory_bytes: Option<u64>,
    /// The size of the volume that holds the device's home.
    pub disk_bytes: Option<u64>,
    pub gpus: Vec<MachineGpu>,
    /// How busy it is, as its runner reported in the last 10 seconds; null
    /// for a device that is not connected.
    pub usage: Option<MachineUsage>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MachineGpu {
    pub name: String,
    /// Its own memory; null for a GPU that shares the system's.
    pub memory_bytes: Option<u64>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MachineUsage {
    /// The whole machine's processor use, 0 to 100, over the last 10
    /// seconds.
    pub cpu_percent: f32,
    pub memory_used_bytes: u64,
    /// Free space on the home's volume, read every 5 minutes.
    pub disk_available_bytes: Option<u64>,
    /// Seconds since the machine started.
    pub up_seconds: u64,
}

impl Machine {
    /// `device`'s machine, with its usage while `link` serves it.
    pub fn of(device: &DeviceRecord, link: Option<&Link>, now: Timestamp) -> Self {
        let hardware = device.hardware.as_ref();
        let usage = link.and_then(|link| {
            let usage = link.usage()?;
            let up = now.as_millisecond().saturating_sub(link.booted_at().0) / 1000;
            Some(MachineUsage {
                cpu_percent: usage.cpu_percent,
                memory_used_bytes: usage.memory_used,
                disk_available_bytes: usage.disk_available,
                up_seconds: u64::try_from(up).unwrap_or(0),
            })
        });
        Self {
            system: device.os.as_ref().map(|os| os.name.clone()),
            arch: device.os.as_ref().map(|os| os.arch.clone()),
            cpu: hardware.and_then(|hardware| hardware.cpu.clone()),
            cores: hardware.map(|hardware| hardware.cores),
            memory_bytes: hardware.map(|hardware| hardware.memory),
            disk_bytes: hardware.and_then(|hardware| hardware.disk),
            gpus: hardware
                .map(|hardware| {
                    hardware
                        .gpus
                        .iter()
                        .map(|gpu| MachineGpu {
                            name: gpu.name.clone(),
                            memory_bytes: gpu.memory,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            usage,
        }
    }

    /// The system with its architecture, such as `Ubuntu 24.04 x86_64`.
    pub fn system_cell(&self) -> Option<String> {
        let system = self.system.as_deref()?;
        Some(match &self.arch {
            Some(arch) => format!("{system} {arch}"),
            None => system.to_owned(),
        })
    }

    /// Such as `32 cores 7%`, or `32 cores` without usage.
    pub fn cpu_cell(&self) -> Option<String> {
        let cores = cores(self.cores?);
        Some(match &self.usage {
            Some(usage) => format!("{cores} {}", percent(usage.cpu_percent)),
            None => cores,
        })
    }

    /// Such as `21.3 of 63.8 GiB`, or `63.8 GiB` without usage.
    pub fn memory_cell(&self) -> Option<String> {
        let total = self.memory_bytes?;
        Some(match &self.usage {
            Some(usage) => used_of(usage.memory_used_bytes, total),
            None => bytes(total),
        })
    }

    /// Such as `1.2 of 1.8 TiB`, used of the volume's size, or its size
    /// without usage.
    pub fn disk_cell(&self) -> Option<String> {
        let total = self.disk_bytes?;
        Some(match self.usage.as_ref().and_then(|usage| usage.disk_available_bytes) {
            Some(available) => used_of(total.saturating_sub(available), total),
            None => bytes(total),
        })
    }

    /// `show`'s `System:`, such as `Windows 11 Pro (build 26100), x86_64`.
    pub fn system_line(&self) -> Option<String> {
        let system = self.system.as_deref()?;
        Some(match &self.arch {
            Some(arch) => format!("{system}, {arch}"),
            None => system.to_owned(),
        })
    }

    /// `show`'s `CPU:`, such as `AMD Ryzen 9 7950X, 32 cores, 7% busy`.
    pub fn cpu_line(&self) -> Option<String> {
        let parts: Vec<String> = [
            self.cpu.clone(),
            self.cores.map(cores),
            self.usage.as_ref().map(|usage| format!("{} busy", percent(usage.cpu_percent))),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// `show`'s `GPU:` lines, such as `NVIDIA GeForce RTX 4090, 24.0 GiB`.
    pub fn gpu_lines(&self) -> Vec<String> {
        self.gpus
            .iter()
            .map(|gpu| match gpu.memory_bytes {
                Some(memory) => format!("{}, {}", gpu.name, bytes(memory)),
                None => gpu.name.clone(),
            })
            .collect()
    }

    /// `show`'s `Memory:`, such as `21.3 of 63.8 GiB used`.
    pub fn memory_line(&self) -> Option<String> {
        let cell = self.memory_cell()?;
        Some(if self.usage.is_some() { format!("{cell} used") } else { cell })
    }

    /// `show`'s `Disk:`, such as `1.2 of 1.8 TiB used`.
    pub fn disk_line(&self) -> Option<String> {
        let cell = self.disk_cell()?;
        let used = self.usage.as_ref().is_some_and(|usage| usage.disk_available_bytes.is_some());
        Some(if used { format!("{cell} used") } else { cell })
    }

    /// `show`'s `Up:`, such as `3 days 4 hours`.
    pub fn up_line(&self) -> Option<String> {
        self.usage.as_ref().map(|usage| duration(usage.up_seconds))
    }
}

fn cores(count: u32) -> String {
    if count == 1 { "1 core".to_owned() } else { format!("{count} cores") }
}

fn percent(share: f32) -> String {
    format!("{share:.0}%")
}

const KIB: f64 = 1024.0;

/// The unit a total is told in, which an amount used of it shares: the
/// largest of GiB and TiB it fills one of, and MiB below a GiB.
fn unit(total: u64) -> (&'static str, f64) {
    let total = total as f64;
    if total >= KIB.powi(4) {
        ("TiB", KIB.powi(4))
    } else if total >= KIB.powi(3) {
        ("GiB", KIB.powi(3))
    } else {
        ("MiB", KIB.powi(2))
    }
}

/// An amount in a unit: one decimal below 100, whole above, as `21.3` and
/// `1007`, decided on the amount as rounded, so 99.97 is `100`.
fn number(amount: u64, scale: f64) -> String {
    let value = amount as f64 / scale;
    if (value * 10.0).round() < 1000.0 { format!("{value:.1}") } else { format!("{value:.0}") }
}

fn bytes(total: u64) -> String {
    let (unit, scale) = unit(total);
    format!("{} {unit}", number(total, scale))
}

fn used_of(used: u64, total: u64) -> String {
    let (unit, scale) = unit(total);
    format!("{} of {} {unit}", number(used, scale), number(total, scale))
}

/// Such as `3 days 4 hours`, `5 hours 12 minutes` or `8 minutes`.
fn duration(seconds: u64) -> String {
    let plural = |count: u64, word: &str| {
        if count == 1 { format!("1 {word}") } else { format!("{count} {word}s") }
    };
    let (days, hours, minutes) = (seconds / 86_400, seconds % 86_400 / 3600, seconds % 3600 / 60);
    if days > 0 {
        format!("{} {}", plural(days, "day"), plural(hours, "hour"))
    } else if hours > 0 {
        format!("{} {}", plural(hours, "hour"), plural(minutes, "minute"))
    } else {
        plural(minutes, "minute")
    }
}
