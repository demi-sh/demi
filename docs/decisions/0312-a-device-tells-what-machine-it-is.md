---
status: accepted
---

# A device tells what machine it is and how busy it is

## Problem

Alex has a Windows desktop and the Ubuntu that runs inside it under WSL.
Both are paired, and both take the desktop's name, so the agent reads:

```text
NAME             ID                                    STATUS  ROLE  DIRECTORY
alex-desktop     5f0c9e1a-2b7d-4c3e-9a61-0d8e4b2f7a10  online  -     -
alex-desktop     c81b3d44-7e29-4f05-b6aa-91e2d03c5e87  online  -     -
build-box        0a7e6c25-93d1-4b8f-a4c2-6f1d8e9b3a54  online  -     -
```

Nothing tells which one is Windows, which one has the GPU, or which one is
busy or nearly out of disk. Each runner already reports its system and
architecture, and the backend keeps them, but the list shows neither, and
nothing reads the processor, the GPUs or the disk at all. The agent has to
run a probe on each device to choose one, and Alex has to remember which id
is which.

## Decision

`jush device list` names each device's system and machine, and how busy it
is now; `jush device show <device>` tells all of it, as `kubectl get nodes`
and `kubectl describe node` do:

```text
$ jush device list
NAME          ID     STATUS   ROLE     SYSTEM                     CPU          MEMORY            DISK              DIRECTORY
Cloud         3d1f…  online   primary  Ubuntu 26.04 x86_64        4 cores 12%  1.9 of 3.8 GiB    21.0 of 79.0 GiB  /home/jush
alex-desktop  5f0c…  online   -        Windows 11 Pro (build      32 cores 7%  21.3 of 63.8 GiB  1.2 of 1.8 TiB    -
                                       26100) x86_64
alex-desktop  c81b…  online   -        Ubuntu 24.04 (WSL) x86_64  32 cores 3%  4.1 of 31.9 GiB   80.0 of 1007 GiB  -
alex-mac      77e2…  offline  -        macOS 26.5 aarch64         18 cores     48.0 GiB          1.8 TiB           -

$ jush device show alex-desktop
Name: alex-desktop
ID: 5f0c9e1a-2b7d-4c3e-9a61-0d8e4b2f7a10
Status: online
Role: -
Directory: -
System: Windows 11 Pro (build 26100), x86_64
CPU: AMD Ryzen 9 7950X 16-Core Processor, 32 cores, 7% busy
GPU: NVIDIA GeForce RTX 4090, 24.0 GiB
GPU: Intel(R) UHD Graphics 770
Memory: 21.3 of 63.8 GiB used
Disk: 1.2 of 1.8 TiB used
Up: 3 days 4 hours
Runner: 0.1.5900
```

The disk is the volume that holds the device's home, where its
conversations' files go. An amount is told in its total's unit, with one
decimal below 100.

The ids in the sketch are cut for width; the command prints them whole.
`--json` of either prints every field. Both answer at once from what the
backend holds; neither asks a device anything. A device that is offline, or
a Cloud that is stopped, shows its machine as last reported and no usage: no
percentage, no amount used, no uptime. Its `show` adds `Last seen:`, when
its runner's connection last began or ended, as Settings shows it. Listing
never wakes a stopped Cloud.

A Linux under WSL is named as such everywhere the system's name is shown:
in these commands, in the agent's context block, and in Settings.

### What a runner reports, and how often

Each value is reported as often as it changes:

| Value | Reported | Why |
| --- | --- | --- |
| System, CPU name and cores, memory, GPUs, disk size, when the machine started | Once, in the hello | They change only across a restart or a hardware change; the uptime is counted from the start |
| CPU use, memory in use | Every 10 seconds | The CPU's share is the average over those 10 seconds, which says how busy the machine is better than an instant does |
| Free disk space | Read every 5 minutes, sent with the next report | It changes slowly, and reading the volumes takes tens of milliseconds |

A runner reads its first usage one second after it starts, the CPU's share
over that second, and each connection sends the latest reading as soon as it
opens, so a device that just came online is not listed without usage for
long.

The backend keeps the hello's machine in the database beside the system, so
an offline device still tells it. It keeps the usage only in memory, with
the device's connection: a value 10 seconds old is worth nothing once the
device is gone, and writing it would put a write per device every 10
seconds on the database. When the connection ends, its usage goes with it.

A usage report is no activity: the Cloud's idle stop counts the
conversations that use it, its running jobs and the operations that hold
it, none of which a report is, so a Cloud that reports still stops when
idle.

```rust
// crates/runner-protocol/src/wire/messages.rs
pub struct RunnerInfo {
    // ...
    pub os: OperatingSystem,
    /// Replaces `memory`, which the idle Claude Code processes read from
    /// `hardware.memory` now.
    pub hardware: Hardware,
    /// When the machine started, which the uptime is counted from; kept
    /// with the connection, since it means nothing once the device is gone.
    pub booted_at: Timestamp,
}

/// What a device's machine is, read once when its runner starts.
pub struct Hardware {
    /// The processor's name, such as `Apple M4 Pro`; none when the system
    /// does not tell it.
    pub cpu: Option<String>,
    /// Logical cores, which a CPU percentage is a share of.
    pub cores: u32,
    /// Physical memory in bytes.
    pub memory: u64,
    /// The size in bytes of the volume that holds the home; none when no
    /// volume is found for it.
    pub disk: Option<u64>,
    pub gpus: Vec<Gpu>,
}

pub struct Gpu {
    /// Such as `NVIDIA GeForce RTX 4090`, or the vendor's name with the PCI
    /// ids when the system has no name for it.
    pub name: String,
    /// Its own memory in bytes; none for one that shares the system's.
    pub memory: Option<u64>,
}

// Outbound::Usage { usage }, when a connection opens and every 10 seconds, a
// message of the connection's own: a report the drop lost is not sent
// again, the next one replaces it.
/// How busy a device's machine is.
pub struct Usage {
    /// The whole machine's processor use, 0 to 100, averaged since the
    /// report before.
    pub cpu_percent: f32,
    /// Physical memory in use: the total less what the system says is
    /// available.
    pub memory_used: u64,
    /// The home's volume's free bytes, as the system's file manager counts
    /// them, as last read; none when no volume is found for the home.
    pub disk_available: Option<u64>,
}
```

`OperatingSystem.name` gains ` (WSL)` when the kernel's release names
Microsoft's WSL kernel, as `uname -r` does there (`5.15.167.4-microsoft-
standard-WSL2`).

The backend stores `hardware` in a new `devices.hardware` column, written
by each hello as `os` is. A change of hardware, unlike a change of system,
does not advance the conversations' context: the context block names only
the system. A device whose runner has not said hello since this release has
none, which the commands show as `-`.

### Where each value comes from

| | macOS | Windows | Linux |
| --- | --- | --- | --- |
| CPU name, cores, memory, disk, usage | `sysinfo` | `sysinfo` | `sysinfo` |
| Which GPUs | IOKit's `IOAccelerator` services | SetupAPI's present display devices whose hardware id is a PCI one | `/sys/bus/pci/devices` of the display class (`0x03`) |
| GPU name | the service's `model` | the device's description | the system's `pci.ids` by vendor and device id |
| GPU memory | none (shared) | the driver key's `HardwareInformation.qwMemorySize` | `mem_info_vram_total` where the driver gives it |

A display device that is not a PCI one, such as a remote desktop's or a
streaming app's virtual display, is no GPU. Under WSL, Linux sees each of
Windows's GPUs as a Microsoft virtual device that tells no model; each is
listed as `GPU shared by Windows`, and the Windows device names them.

## Spike

- **Cost on macOS.** On an Apple silicon Mac, `sysinfo` read the processor
  and memory in 0.15 ms and the home's volume in 70 ms, which is why the
  disk is read every 5 minutes and not every 10 seconds. IOKit gave
  `Apple M5 Max` and a core count for the GPU at once.
- **macOS's disk.** The home is on the Data volume, which `sysinfo` reports
  as `/`: the same APFS container, so the same size. Its available space
  counts purgeable space as Finder does, and so is larger than `df`'s.
- **macOS's memory.** `sysinfo`'s own "used" and "available" add up to more
  than the total, so "used" is the total less "available", as Linux's
  `free` counts it.
- **Windows's GPUs.** A desktop with an NVIDIA card, an Intel integrated GPU
  and four virtual displays (remote desktop and game streaming) has six
  display adapters in the registry. Only the two physical ones have a PCI
  hardware id. The NVIDIA card's driver key gives 25,757,220,864 bytes in
  `qwMemorySize`, its 24 GiB; the old 32-bit `MemorySize` caps at 4 GiB, as
  WMI's `AdapterRAM` does, so neither is read. The Intel GPU has no
  `qwMemorySize`: it shares the system's memory. The registry's class key
  can also keep adapters that are no longer present, which Device Manager
  shows only as hidden devices (not seen on this desktop), so the list comes
  from SetupAPI's present devices, which name their driver key.
- **Linux's GPUs.** A DRM card is not one per GPU: a server's management
  chip (ASPEED) has no DRM driver and shows only as a framebuffer, and a
  virtual machine without a GPU has no card, while both show their PCI
  display devices, or none, in `/sys/bus/pci/devices`. Ubuntu and Debian
  both ship `pci.ids` at `/usr/share/misc/pci.ids`.
- **WSL's GPUs.** The Linux of a desktop with two GPUs sees two PCI devices
  `1414:008e` of the class `0x0302`, driven by `dxgkrnl`, and no DRM card
  for them. Only DirectX's own library, `libdxcore.so`, could name them,
  through COM interfaces; the Windows device already does.

## Prior art

- `kubectl get nodes` lists a summary and `-o wide` adds the system and
  kernel; `kubectl describe node` tells the capacity, which the kubelet
  reports when it registers, and its status every 10 seconds by default;
  usage is `kubectl top`, from a metrics server that samples every 15
  seconds by default. Jush's tiers follow the same split: what the machine
  is once, how busy it is often.
- Coder's [agent metadata](https://coder.com/docs/templates/agent-metadata)
  has each workspace agent run a script per value at that value's own
  interval, and warns that every report is a database write. Jush's
  intervals are fixed, and its usage is never written.
- [`coder stat`](https://coder.com/docs/cli/stat) shows the host's CPU,
  memory and home disk as a table or JSON, in the "used of total" form this
  list takes.
- [fastfetch](https://github.com/fastfetch-cli/fastfetch/wiki/Modules-GPU)
  names Linux GPUs from `/sys` and `pci.ids`, and loses only the model's name
  when `pci.ids` is missing, as Jush does.
- `tailscale status` shows each machine's system beside its name.

## Alternatives

- **WSL names its GPUs through `libdxcore.so`.** Loading DirectX's library
  and calling its COM interfaces from Rust is the most code of any source
  here, for names the Windows device on the same machine already lists.
- **Everything in `list`.** Every field in one table makes it far wider than
  a terminal, and a device with several GPUs does not fit a cell.
- **The backend asks each device when the list runs.** No device would send
  anything while nobody looks, but the list would wait for the slowest
  device, at least the 200 ms the shortest CPU sample takes (`sysinfo`'s
  `MINIMUM_CPU_UPDATE_INTERVAL`), and a device that does not answer would
  need a timeout. An instant's CPU share also says less than an average.
- **One interval for everything.** The machine's description would be sent
  again every 10 seconds for nothing, or the CPU's use would be minutes old.
- **Only the system in the list.** It tells the two devices of the same name
  apart, but not which machine can run a build or has room for it.

## Open questions

None: the user chose the list with a `show`, usage reported at these
intervals, GPU names and memory without their use, and the WSL mark in
Settings too.
