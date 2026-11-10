//! What Demi keeps on the device: the runner measures it, applies the rules
//! the user set for it, and removes what the user asks it to.
//!
//! ```text
//!   <scratch root>/<conversation>/      a conversation's scratch directory
//!     .browser/                          cache: its browser profile
//!     .recordings/<id>/                  cache: one recording, its exports
//!     agent-<n>/<entry>                  temporary: a subagent's entries
//!     <entry>                            temporary: the root agent's
//!   <installation>, artifacts, but the scratch root    components
//! ```
//!
//! # Entries
//!
//! Each line above but the last is an entry: what a rule removes whole. An
//! entry was last used when anything in it was last written, created, or
//! moved in: the newest of each file's modification time and its status
//! change time (its creation time on Windows), since `tar`, `cp -a` and
//! `wget` give a file they write today a modification time of years ago,
//! and `mv` keeps it. Access times are not read, since macOS and most Linux
//! mounts do not keep them up to date. Its size is the bytes its files take
//! on the disk, as `du` counts them.
//!
//! The walk stays on the scratch root's file system and never follows a
//! link: a link is an entry of its own, which goes as a link, and an entry
//! that holds another file system, such as a disk image an agent mounted, is
//! neither counted past it nor removed, since its files are not Demi's.
//!
//! # The rules
//!
//! Each kind, temporary files and cache, has its own rule (`wire::StorageRule`):
//! an entry unused for its days goes, and while the kind takes more than its
//! bytes on the device, its entries go from the one used longest ago. No rule
//! removes an entry used in the last day, so a task that paused overnight
//! finds its files whole, nor a profile a browser runs on, which holds the
//! profile's lock (`crates/plugin-browser/command-chrome/src/tabs/environment.rs`),
//! nor a recording being written. A kind can therefore stay over its limit
//! while all of it is in use.
//!
//! # When
//!
//! A pass walks every entry, then applies the rules, or what the user asked.
//! The keeper makes one a minute after the backend first names the rules and
//! every 6 hours after, one at once when the volume's free space falls under
//! a tenth of it, and one for each request. Passes run one at a time, on a
//! thread of their own at the system's lowest priority: a walk of a million
//! files takes minutes and must not slow what the user runs.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use demi_runner_command_packages::ResidentHandle;
use demi_runner_protocol::wire::{
    self, CleanupCause, ConversationStorage, StorageAction, StorageCleanup, StorageReport, StorageRule,
    StorageRules,
};
use tokio::sync::{mpsc, oneshot, watch};

/// How long after the backend first names the rules the first pass runs:
/// the runner's start has work of its own to do first.
const FIRST_PASS: Duration = Duration::from_secs(60);
/// How often a pass runs: a walk costs minutes, for numbers that change
/// slowly.
const PASS_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// How recently an entry was used for no rule to remove it: long enough for a
/// task that paused overnight.
const IN_USE: Duration = Duration::from_secs(24 * 60 * 60);
/// The share of the volume under which a pass runs at once, the one at which
/// macOS and Windows warn that a disk is almost full.
const LOW_DISK_TENTHS: u64 = 10;

/// The browser profile in a conversation's scratch directory.
const PROFILE: &str = ".browser";
/// The lock a Chrome holds while it runs on the profile.
const PROFILE_LOCK: &str = "profile.lock";
/// The conversation's recordings, one directory each.
const RECORDINGS: &str = ".recordings";
/// Where what the keeper measures is.
#[derive(Clone, Debug)]
pub struct Places {
    /// The scratch root.
    pub scratch: PathBuf,
    /// The installation's directory.
    pub directory: PathBuf,
    /// The artifact cache, which a Cloud keeps outside the installation.
    pub artifacts: PathBuf,
    /// The home, whose volume the report names.
    pub home: PathBuf,
    /// The size of the home's volume, as the hello reports it; none when no
    /// volume is found for the home.
    pub disk: Option<u64>,
}

impl Places {
    /// Where the last cleanup is kept, so that a restart or an update of the
    /// runner still reports it.
    fn last_cleanup(&self) -> PathBuf {
        self.directory.join(LAST_CLEANUP)
    }
}

/// The file in the installation's directory that keeps the last cleanup.
const LAST_CLEANUP: &str = "storage-cleanup.json";

/// The keeper's side each connection holds.
#[derive(Clone)]
pub struct Storage {
    rules: watch::Sender<Option<StorageRules>>,
    requests: mpsc::UnboundedSender<(StorageAction, oneshot::Sender<Result<StorageReport, String>>)>,
    reports: watch::Receiver<Option<StorageReport>>,
}

impl Storage {
    /// Starts the keeper of what Demi keeps in `places`. It waits for the
    /// rules before it applies any, and measures for requests meanwhile.
    pub fn start(places: Places, residents: ResidentHandle, usage: watch::Receiver<Option<wire::Usage>>) -> Self {
        let (rules, rules_receiver) = watch::channel(None);
        let (requests, receiver) = mpsc::unbounded_channel();
        let (reports_sender, reports) = watch::channel(None);
        let last_cleanup = fs::read(places.last_cleanup()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let keeper = Keeper {
            places,
            residents,
            rules: rules_receiver,
            usage: Some(usage),
            reports: reports_sender,
            last_cleanup,
        };
        tokio::spawn(keeper.run(receiver));
        Self { rules, requests, reports }
    }

    /// The rules the backend named, which the next pass applies.
    pub fn set_rules(&self, rules: StorageRules) {
        self.rules.send_replace(Some(rules));
    }

    /// Runs `action` and a measurement after it, once the passes before it
    /// ended.
    pub fn request(&self, action: StorageAction) -> oneshot::Receiver<Result<StorageReport, String>> {
        let (answer, receiver) = oneshot::channel();
        // The keeper runs as long as the runner does.
        let _queued = self.requests.send((action, answer));
        receiver
    }

    /// The latest pass's report, and each later one; none before the first.
    pub fn reports(&self) -> watch::Receiver<Option<StorageReport>> {
        self.reports.clone()
    }
}

struct Keeper {
    places: Places,
    residents: ResidentHandle,
    rules: watch::Receiver<Option<StorageRules>>,
    /// The machine's readings, which say when the disk runs low; none once
    /// the reader stopped.
    usage: Option<watch::Receiver<Option<wire::Usage>>>,
    reports: watch::Sender<Option<StorageReport>>,
    last_cleanup: Option<StorageCleanup>,
}

/// Why a pass runs.
#[derive(Debug, Clone)]
enum Pass {
    Rules(CleanupCause),
    Asked(StorageAction),
}

impl Keeper {
    async fn run(
        mut self,
        mut requests: mpsc::UnboundedReceiver<(StorageAction, oneshot::Sender<Result<StorageReport, String>>)>,
    ) {
        let mut next: Option<tokio::time::Instant> = None;
        let mut low = false;
        loop {
            let scheduled = async {
                match next {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            let usage_changed = async {
                match self.usage.as_mut() {
                    Some(usage) => usage.changed().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = scheduled => {
                    let _report = self.pass(Pass::Rules(CleanupCause::Rules)).await;
                    next = Some(tokio::time::Instant::now() + PASS_EVERY);
                }
                request = requests.recv() => {
                    let Some((action, answer)) = request else { return };
                    // A caller that went away needs no answer.
                    let _answered = answer.send(self.pass(Pass::Asked(action)).await);
                }
                changed = self.rules.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    // New rules apply soon, not at the next of the passes 6
                    // hours apart.
                    if self.rules.borrow().is_some() {
                        let soon = tokio::time::Instant::now() + FIRST_PASS;
                        next = Some(next.map_or(soon, |at| at.min(soon)));
                    }
                }
                changed = usage_changed => {
                    if changed.is_err() {
                        // The machine's reader stopped: the scheduled passes go on.
                        self.usage = None;
                        continue;
                    }
                    let now_low = self.low_disk();
                    if now_low && !low && self.rules.borrow().is_some() {
                        let _report = self.pass(Pass::Rules(CleanupCause::LowDisk)).await;
                    }
                    low = now_low;
                }
            }
        }
    }

    /// Whether the home's volume has less than a tenth of it free, as the
    /// machine's last reading says.
    fn low_disk(&self) -> bool {
        let available = self.usage.as_ref().and_then(|usage| usage.borrow().as_ref().and_then(|usage| usage.disk_available));
        matches!((available, self.places.disk), (Some(available), Some(total)) if available.saturating_mul(LOW_DISK_TENTHS) < total)
    }

    async fn pass(&mut self, pass: Pass) -> Result<StorageReport, String> {
        // What Delete removes is what the components took before it.
        let mut components_before = None;
        if matches!(pass, Pass::Asked(StorageAction::DeleteComponents {})) {
            let places = self.places.clone();
            components_before = tokio::task::spawn_blocking(move || components(&places)).await.ok();
            self.residents.remove_unheld_artifacts().await;
        }
        let rules = *self.rules.borrow();
        let places = self.places.clone();
        let (sender, receiver) = oneshot::channel();
        let what = pass.clone();
        std::thread::Builder::new()
            .name("storage".into())
            .spawn(move || {
                lower_priority();
                let _sent = sender.send(walk_and_clean(&places, rules, &what, SystemTime::now()));
            })
            .map_err(|error| format!("the storage pass cannot start: {error}"))?;
        let (mut report, mut removed) = receiver.await.map_err(|_| "the storage pass stopped".to_owned())?;
        if let Some(before) = components_before {
            removed += before.saturating_sub(report.components);
        }
        if removed > 0 {
            let cause = match pass {
                Pass::Rules(cause) => cause,
                Pass::Asked(_) => CleanupCause::User,
            };
            tracing::info!(event = "storage.cleaned", cause = ?cause, removed_bytes = removed, "Demi's files on the device were cleaned up");
            let cleanup = StorageCleanup { at: report.measured_at, cause, removed_bytes: removed };
            let kept = serde_json::to_vec(&cleanup).map_err(io::Error::other).and_then(|bytes| fs::write(self.places.last_cleanup(), bytes));
            if let Err(error) = kept {
                tracing::warn!(event = "storage.cleanup_not_kept", %error, "the last cleanup was not kept on the device");
            }
            self.last_cleanup = Some(cleanup);
        }
        report.last_cleanup = self.last_cleanup.clone();
        self.reports.send_replace(Some(report.clone()));
        Ok(report)
    }
}

/// Lowers the calling thread to the system's lowest priority, so a walk
/// yields to everything else on the machine.
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: `setpriority` reads nothing of ours; on Linux a thread's id
    // names the thread alone.
    unsafe {
        let thread = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, thread, 19);
    }
    #[cfg(target_os = "macos")]
    // SAFETY: it changes only the calling thread's class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0);
    }
    #[cfg(windows)]
    // SAFETY: the pseudo handle names the calling thread.
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

/// What a rule weighs: temporary files or cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Temporary,
    Cache,
}

/// One entry of a conversation's scratch directory, as the walk found it.
#[derive(Debug)]
struct Entry {
    path: PathBuf,
    conversation: String,
    kind: Kind,
    bytes: u64,
    last_used: Option<SystemTime>,
    /// A profile a browser runs on, a recording being written, or an entry
    /// that holds another file system.
    busy: bool,
    profile: bool,
}

/// Walks `places`, removes what `pass` says, and reports what stays, with
/// the bytes it removed.
fn walk_and_clean(places: &Places, rules: Option<StorageRules>, pass: &Pass, now: SystemTime) -> (StorageReport, u64) {
    let mut entries = entries(&places.scratch);
    let recent = now.checked_sub(IN_USE).unwrap_or(UNIX_EPOCH);
    let old = |entry: &Entry| !entry.busy && entry.last_used.is_none_or(|used| used < recent);
    let mut doomed: Vec<usize> = Vec::new();
    match pass {
        Pass::Rules(_) => {
            if let Some(rules) = rules {
                doomed = by_rules(&entries, &rules, now, &old);
            }
        }
        Pass::Asked(action) => {
            for (index, entry) in entries.iter().enumerate() {
                let chosen = match action {
                    StorageAction::Measure {} | StorageAction::DeleteComponents {} => false,
                    StorageAction::CleanUpTemporary {} => entry.kind == Kind::Temporary && old(entry),
                    StorageAction::ClearCache {} => entry.kind == Kind::Cache && !entry.busy,
                    StorageAction::Conversation { conversation, empty } => {
                        entry.conversation == demi_command_protocol::scratch_name(conversation)
                            && if *empty { !entry.busy } else { old(entry) }
                    }
                };
                if chosen {
                    doomed.push(index);
                }
            }
        }
    }
    let mut removed = 0;
    doomed.sort_unstable();
    for index in doomed.into_iter().rev() {
        let entry = entries.remove(index);
        if remove(&entry) {
            removed += entry.bytes;
        } else {
            entries.push(entry);
        }
    }
    (report(places, &entries, now), removed)
}

/// The entries `rules` remove: those unused for their kind's days, then,
/// while a kind is over its bytes, the rest from the one used longest ago.
/// Only those `old` says may go are among them.
fn by_rules(entries: &[Entry], rules: &StorageRules, now: SystemTime, old: &dyn Fn(&Entry) -> bool) -> Vec<usize> {
    let mut doomed = Vec::new();
    for (kind, rule) in [(Kind::Temporary, rules.temporary), (Kind::Cache, rules.cache)] {
        let StorageRule { unused_days, max_bytes } = rule;
        let cutoff = unused_days.and_then(|days| now.checked_sub(Duration::from_secs(u64::from(days) * 24 * 60 * 60)));
        let mut total: u64 = entries.iter().filter(|entry| entry.kind == kind).map(|entry| entry.bytes).sum();
        let mut left: Vec<usize> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if entry.kind != kind || !old(entry) {
                continue;
            }
            let unused = cutoff.is_some_and(|cutoff| entry.last_used.is_none_or(|used| used < cutoff));
            if unused {
                total = total.saturating_sub(entry.bytes);
                doomed.push(index);
            } else {
                left.push(index);
            }
        }
        if let Some(max) = max_bytes {
            left.sort_by_key(|&index| entries[index].last_used);
            for index in left {
                if total <= max {
                    break;
                }
                total = total.saturating_sub(entries[index].bytes);
                doomed.push(index);
            }
        }
    }
    doomed
}

/// Every entry of every conversation's scratch directory under `root`.
fn entries(root: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Some(device) = device_of(root) else { return entries };
    // Only a directory of the root's file system is gone into: a link, even
    // to a directory, is an entry of its own.
    let walked = |path: &Path| directory_on(path, device);
    for (conversation, directory) in children(root).filter(|(_, path)| walked(path)) {
        for (name, path) in children(&directory) {
            if name == PROFILE && walked(&path) {
                // A profile a cleanup emptied keeps only its lock: nothing to
                // count or remove.
                if children(&path).any(|(name, _)| name != PROFILE_LOCK) {
                    let busy = profile_busy(&path);
                    entries.push(entry(&path, &conversation, Kind::Cache, busy, true, device));
                }
            } else if name == RECORDINGS && walked(&path) {
                for (_, recording) in children(&path) {
                    let busy = demi_shared_recording_protocol::written(&recording);
                    entries.push(entry(&recording, &conversation, Kind::Cache, busy, false, device));
                }
            } else if is_agent_folder(&name) && walked(&path) {
                for (_, inner) in children(&path) {
                    entries.push(entry(&inner, &conversation, Kind::Temporary, false, false, device));
                }
            } else {
                entries.push(entry(&path, &conversation, Kind::Temporary, false, false, device));
            }
        }
    }
    entries
}

/// The file system `path` is on; none when it cannot be read. Windows has
/// no such number the standard library gives, and mounts a volume only where
/// a junction stands, which the walk never follows.
fn device_of(path: &Path) -> Option<u64> {
    let metadata = fs::symlink_metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.dev())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Some(0)
    }
}

/// Whether `path` is a directory itself, not a link to one, on the file
/// system `device`.
fn directory_on(path: &Path, device: u64) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) && device_of(path) == Some(device)
}

/// A subagent's folder in a conversation's scratch directory, `agent-<n>`
/// (`demi_command_protocol::CommandCaller::scratch_folder`).
fn is_agent_folder(name: &str) -> bool {
    name.strip_prefix("agent-").is_some_and(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The entries of `directory` by name, none when it cannot be read.
fn children(directory: &Path) -> impl Iterator<Item = (String, PathBuf)> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path()))
}

fn entry(path: &Path, conversation: &str, kind: Kind, busy: bool, profile: bool, device: u64) -> Entry {
    let measured = measure(path, None, device);
    Entry {
        path: path.to_owned(),
        conversation: conversation.to_owned(),
        kind,
        bytes: measured.bytes,
        last_used: measured.last_used,
        busy: busy || measured.mounts,
        profile,
    }
}

/// What a walk of a path found.
struct Measured {
    /// The bytes its files take on the disk.
    bytes: u64,
    /// When anything in it was last written, created or moved in.
    last_used: Option<SystemTime>,
    /// It holds a directory of another file system.
    mounts: bool,
}

/// Walks `path` on the file system `device`, `except` a directory not
/// counted. Links are counted as links, never followed, and a directory of
/// another file system is neither counted nor gone into; what cannot be read
/// counts nothing.
fn measure(path: &Path, except: Option<&Path>, device: u64) -> Measured {
    let mut measured = Measured { bytes: 0, last_used: None, mounts: false };
    let mut pending = vec![path.to_owned()];
    while let Some(path) = pending.pop() {
        if except.is_some_and(|except| path == except) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else { continue };
        if metadata.is_dir() && device_of(&path) != Some(device) {
            measured.mounts = true;
            continue;
        }
        measured.bytes += allocated(&metadata);
        if let Some(changed) = changed(&metadata) {
            measured.last_used = Some(measured.last_used.map_or(changed, |newest| newest.max(changed)));
        }
        if metadata.is_dir() {
            pending.extend(fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
        }
    }
    measured
}

/// When a file was last written, created or moved: the newer of its
/// modification time, which a program may set to any time, and its status
/// change time, which only the system sets, or its creation time on Windows.
fn changed(metadata: &fs::Metadata) -> Option<SystemTime> {
    let modified = metadata.modified().ok();
    #[cfg(unix)]
    let other = {
        use std::os::unix::fs::MetadataExt;
        u64::try_from(metadata.ctime()).ok().map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
    };
    #[cfg(not(unix))]
    let other = metadata.created().ok();
    modified.max(other)
}

/// The bytes the components take: the installation's directory but the
/// scratch root, and the artifact cache where it is kept apart.
fn components(places: &Places) -> u64 {
    let Some(device) = device_of(&places.directory) else { return 0 };
    let mut bytes = measure(&places.directory, Some(&places.scratch), device).bytes;
    if !places.artifacts.starts_with(&places.directory)
        && let Some(device) = device_of(&places.artifacts)
    {
        bytes += measure(&places.artifacts, Some(&places.scratch), device).bytes;
    }
    bytes
}

/// The bytes a file takes on the disk: its blocks where the system counts
/// them, else its length.
fn allocated(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        metadata.len()
    }
}

/// Whether a browser runs on the profile at `path`: its Chrome holds the
/// profile's lock.
fn profile_busy(path: &Path) -> bool {
    match File::open(path.join(PROFILE_LOCK)) {
        Ok(lock) => matches!(lock.try_lock(), Err(fs::TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// Removes `entry`, and says whether it went. A profile keeps its lock file
/// and stays held while its files go, so a browser that starts meanwhile
/// waits for it.
fn remove(entry: &Entry) -> bool {
    let removed = if entry.profile {
        remove_profile(&entry.path)
    } else {
        remove_path(&entry.path)
    };
    match removed {
        Ok(()) => true,
        Err(error) => {
            // The next pass tries again.
            tracing::warn!(event = "storage.remove_failed", path = %entry.path.display(), %error, "a file of Demi's on the device could not be removed");
            false
        }
    }
}

fn remove_profile(path: &Path) -> io::Result<()> {
    let lock = File::options().create(true).truncate(false).write(true).open(path.join(PROFILE_LOCK))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Err(io::Error::other("a browser runs on the profile")),
        Err(fs::TryLockError::Error(error)) => return Err(error),
    }
    for (name, child) in children(path) {
        if name == PROFILE_LOCK {
            continue;
        }
        remove_path(&child)?;
    }
    Ok(())
}

/// Removes the file, link or directory at `path`. A directory its owner
/// made read-only, as Go's module cache makes each of its own, is made
/// writable first: removing what is in a directory takes writing to it.
fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => match fs::remove_dir_all(path) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                make_writable(path);
                fs::remove_dir_all(path)
            }
            removed => removed,
        },
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Lets the owner write every directory under `path`, which is a directory
/// itself; links are not followed.
fn make_writable(path: &Path) {
    let mut pending = vec![path.to_owned()];
    while let Some(directory) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&directory) else { continue };
        if !metadata.is_dir() {
            continue;
        }
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() | 0o700);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        // A directory that stays as it was fails the removal, which says why.
        let _changed = fs::set_permissions(&directory, permissions);
        pending.extend(fs::read_dir(&directory).into_iter().flatten().flatten().map(|entry| entry.path()));
    }
}

/// What stays, by conversation, with the components and the volume.
fn report(places: &Places, entries: &[Entry], now: SystemTime) -> StorageReport {
    let mut conversations: HashMap<String, ConversationStorage> = HashMap::new();
    for (name, path) in children(&places.scratch).filter(|(_, path)| fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())) {
        // The directory's own entry counts as temporary, as in `du`.
        let own = fs::symlink_metadata(&path).map(|metadata| allocated(&metadata)).unwrap_or(0);
        conversations.insert(name.clone(), ConversationStorage { name, temporary: own, cache: 0, last_used: None });
    }
    // A directory with nothing left in it, such as one a cleanup emptied, is
    // no conversation's files: Storage would list it with nothing to remove.
    let holding: HashSet<&str> = entries.iter().map(|entry| entry.conversation.as_str()).collect();
    conversations.retain(|name, _| holding.contains(name.as_str()));
    for entry in entries {
        let Some(conversation) = conversations.get_mut(&entry.conversation) else { continue };
        match entry.kind {
            Kind::Temporary => conversation.temporary += entry.bytes,
            Kind::Cache => conversation.cache += entry.bytes,
        }
        if let Some(used) = entry.last_used.map(timestamp)
            && conversation.last_used.is_none_or(|last| last.0 < used.0)
        {
            conversation.last_used = Some(used);
        }
    }
    let mut conversations: Vec<ConversationStorage> = conversations.into_values().collect();
    conversations.sort_by_key(|conversation| std::cmp::Reverse(conversation.temporary + conversation.cache));
    let components = components(places);
    let volume = crate::machine::volume(&places.home);
    StorageReport {
        measured_at: timestamp(now),
        disk: volume.map(|(total, _)| total),
        disk_available: volume.map(|(_, available)| available),
        components,
        conversations,
        last_cleanup: None,
    }
}

fn timestamp(time: SystemTime) -> wire::Timestamp {
    let millis = time.duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX));
    wire::Timestamp(millis)
}
