//! What the user's plugins reach on a device.
//!
//! The module serves two things:
//!
//! - Reads of a conversation's primary device, or of a command job's device, in the
//!   form that never wakes it.
//! - The plugins' directories, which each device of the user receives in the
//!   background, once per runner connection, set and workspaces using it.
//!
//! A plugin never writes a device: it names the directories its user needs and
//! device access installs them, apart from any command: a command never waits
//! for them or fails with them, and a skill not there yet is a path that does
//! not exist for that moment.
//!
//! - A directory that exists is complete. The runner writes it beside the final
//!   one, read-only, and renames it into place, so a crash leaves at most a
//!   temporary directory, which the next installation removes.
//! - The same installation removes the plugin's directories the user's set no
//!   longer names. A command still reading one fails as with any file removed
//!   underneath it, and the model was already told the new paths.
//! - A new connection checks again, so a Cloud's new boot is covered and a
//!   directory the user deleted by hand comes back.
//! - Nothing wakes a device for a plugin's files: one that is offline
//!   receives them when it connects.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Duration;

use jush_backend_database::devices::ManagedMachine;
use futures_util::FutureExt as _;
use tokio::sync::Notify;

use jush_backend_blobs::blobs::UserBlobs;
use jush_runner_client::{DirectoryFile, DeviceSubscriber, Link, Look, LookAt, RemoteDevice, WeakLink};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use jush_device_interface::{ByteRange, FileKind, Device as _, DeviceError, DeviceErrorKind, DeviceFs as _};
use jush_runner_protocol::wire::FileHash;
use jush_plugin_interface::{DeviceDirectory, PluginId};
use jush_device_interface::{EntryKind, DeviceEntry, DeviceFile, DeviceRead};
use jush_shared_types::{B64Bytes, ReadAt};
use jush_web_api_protocol::ids::{ConversationId, DeviceId, WorkspaceId};
use tokio_util::sync::CancellationToken;

use crate::DeviceShard;
use crate::access::{Attention, DeviceAccessError, Refusal, Waits};

/// The device directories of the user's plugins: every plugin with its set,
/// an empty one for a plugin the user has off, whose directories a device
/// loses at its next installation.
pub type DirectorySets = Vec<(PluginId, Vec<DeviceDirectory>)>;

/// What the shard remembers of its users' devices' directories.
#[derive(Default)]
pub struct PluginInstalls {
    devices: RefCell<HashMap<DeviceId, Installer>>,
}

/// A device's installation, one pass at a time, so two never share a
/// temporary directory.
#[derive(Default)]
struct Installer {
    /// The connection the device was last brought to the sets on, and their
    /// revision; a new connection checks again.
    synced: Option<(WeakLink, String)>,
    /// Whether a task passes over the device now.
    running: bool,
    /// A wake for that task: a set or the device's connection changed.
    wake: Rc<Notify>,
}

impl PluginInstalls {
    fn synced(&self, device: &DeviceId, link: &Link, revision: &str) -> bool {
        self.devices
            .borrow()
            .get(device)
            .and_then(|installer| installer.synced.as_ref())
            .is_some_and(|(synced, at)| synced.is(link) && at == revision)
    }
}

/// Why a read of a conversation's device did not answer.
#[derive(Debug, thiserror::Error)]
pub enum ReadFilesError {
    /// The device is not running, and a read never wakes it.
    #[error("the conversation's device is not running")]
    NotRunning,
    /// The conversation works on another device than the one the read was
    /// asked of.
    #[error("the conversation works on another device now")]
    Moved,
    #[error(transparent)]
    Access(DeviceAccessError),
}

impl From<DeviceAccessError> for ReadFilesError {
    fn from(error: DeviceAccessError) -> Self {
        match error {
            DeviceAccessError::Refused(Refusal::Stopped) => Self::NotRunning,
            DeviceAccessError::Device(error) if not_running(&error) => Self::NotRunning,
            error => Self::Access(error),
        }
    }
}

impl dyn DeviceShard + '_ {
    /// Reads `reads` on the conversation's primary device, admitted as a look. A
    /// stopped Cloud or an offline device is not running and is not woken, the
    /// read is no activity, and a transition ends it instead of waiting for it.
    ///
    /// A path the device cannot read answers as unreadable. The rest answer in
    /// the order of `reads`, with the device's number before it read them
    /// (`crates/runner-device/src/subscriptions.rs`).
    pub async fn read_files(
        &self,
        id: &ConversationId,
        reads: &[DeviceRead],
        cancel: &CancellationToken,
    ) -> Result<(Vec<DeviceFile>, ReadAt), ReadFilesError> {
        self.read_files_on(id, None, reads, cancel).await
    }

    /// [`read_files`](Self::read_files), on the device of `device` alone when
    /// one is named, the primary one or one a live subagent works on:
    /// [`ReadFilesError::Moved`] once the conversation works on neither, so
    /// what a search of one device finds is never read on another.
    pub async fn read_files_on(
        &self,
        id: &ConversationId,
        device: Option<&DeviceId>,
        reads: &[DeviceRead],
        cancel: &CancellationToken,
    ) -> Result<(Vec<DeviceFile>, ReadAt), ReadFilesError> {
        if let Some(device) = device
            && !self.works_on(id, device).await?
        {
            return Err(ReadFilesError::Moved);
        }
        let access = self.admit_stream(id, device, Attention::Looks, cancel).await?;
        let waits = Waits::until_ended(cancel, &access.open.ended);
        let found = waits.wait(read_device_files(&access.admitted.device, &self.blobs(), reads)).await?;
        Ok(found.map_err(DeviceAccessError::Device)?)
    }

    /// Reads `reads` for a command's job on `device`, the conversation's
    /// primary device or an attached one, admitted as
    /// [`read_files`](Self::read_files) is; a relative path is read against
    /// `directory`, the job's, as that device resolves it (`crates/plugin-interface/src/lib.rs`).
    pub async fn read_job_files(
        &self,
        id: &ConversationId,
        device: &DeviceId,
        directory: &str,
        reads: &[DeviceRead],
        cancel: &CancellationToken,
    ) -> Result<Vec<DeviceFile>, ReadFilesError> {
        let access = self.admit_stream(id, Some(device), Attention::Looks, cancel).await?;
        let paths = access.admitted.device.identity().paths;
        let reads: Vec<DeviceRead> = reads
            .iter()
            .map(|read| DeviceRead {
                path: paths.resolve(directory, &read.path),
                ..read.clone()
            })
            .collect();
        let waits = Waits::until_ended(cancel, &access.open.ended);
        let found = waits.wait(read_device_files(&access.admitted.device, &self.blobs(), &reads)).await?;
        Ok(found.map_err(DeviceAccessError::Device)?.0)
    }

    /// A holder of subscriptions on the device of `device`
    /// (`crates/runner-device/src/subscriptions.rs`), the conversation's primary
    /// one or one a live subagent works on, admitted as
    /// [`read_files`](Self::read_files) is: a stopped Cloud or an offline
    /// device is not running and is not woken, and a device the conversation
    /// works on no longer is [`ReadFilesError::Moved`]. The holder keeps no
    /// admission, so it keeps no Cloud awake and no transition waits for it.
    pub async fn watch_files(
        &self,
        id: &ConversationId,
        device: &DeviceId,
        cancel: &CancellationToken,
    ) -> Result<(DeviceId, DeviceSubscriber), ReadFilesError> {
        if !self.works_on(id, device).await? {
            return Err(ReadFilesError::Moved);
        }
        let access = self.admit_stream(id, Some(device), Attention::Looks, cancel).await?;
        let subscriber = access
            .admitted
            .device
            .subscriber()
            .map_err(|error| ReadFilesError::from(DeviceAccessError::Device(error)))?;
        Ok((access.device, subscriber))
    }
}

/// How many directories a pass writes on a device at once: more only queue
/// on the device's one connection, where a write that has not started runs
/// out of the time its reply has (`crates/runner-client/src/remote_device.rs`).
const WRITES_AT_ONCE: usize = 8;

/// The pause before a device whose pass failed is brought to its set again,
/// doubled after each failure up to the last.
const FIRST_RETRY: Duration = Duration::from_secs(5);
const LAST_RETRY: Duration = Duration::from_secs(300);

impl dyn DeviceShard + '_ {
    /// Brings `device` to the user's directory sets in the background, unless
    /// it is there already on its current connection, when it connects or a
    /// set changes: a pass waiting to try again after a failure goes at once.
    pub fn install_on(&self, device: &DeviceId) {
        self.wake_installer(device, true);
    }

    /// [`Self::install_on`] each device whose runner is connected, after a
    /// set changed.
    pub fn install_everywhere(&self) {
        for (device, _) in self.devices().links() {
            self.install_on(&device);
        }
    }

    /// Checks that `device` holds the user's directory sets as a
    /// conversation is about to use it, when a message arrives or a command
    /// starts there, which never waits for it: a pass waiting to try again
    /// after a failure keeps its pause.
    pub fn check_installed(&self, device: &DeviceId) {
        self.wake_installer(device, false);
    }

    /// [`Self::check_installed`] each device whose runner is connected.
    pub fn check_installed_everywhere(&self) {
        for (device, _) in self.devices().links() {
            self.check_installed(&device);
        }
    }

    /// Starts `device`'s installer unless it runs, and, when `now`, ends the
    /// pause of one that waits to try again.
    fn wake_installer(&self, device: &DeviceId, now: bool) {
        let installs = self.plugin_installs();
        let wake = {
            let mut devices = installs.devices.borrow_mut();
            let installer = devices.entry(device.clone()).or_default();
            if installer.running {
                if now {
                    installer.wake.notify_one();
                }
                return;
            }
            installer.running = true;
            installer.wake.clone()
        };
        let shard = self.this();
        let device = device.clone();
        self.tasks()
            .spawn_local(async move { install_until_done(shard, device, wake).await });
    }
}

/// Passes over `device` until one brings it to the sets, it goes offline or
/// the shard closes; a failed pass is tried again after a pause, or at the
/// next wake.
async fn install_until_done(shard: Rc<dyn DeviceShard>, device: DeviceId, wake: Rc<Notify>) {
    let mut pause = FIRST_RETRY;
    loop {
        // A wake before this pass is one it answers.
        let _ = wake.notified().now_or_never();
        match pass(&*shard, &device).await {
            Ok(()) => pause = FIRST_RETRY,
            Err(error) => {
                tracing::warn!(
                    event = "plugin_directories.pass_failed",
                    device_id = %device,
                    %error,
                    retry_ms = pause.as_millis() as u64,
                    "the plugins' directories were not all installed on a device; it tries again"
                );
                tokio::select! {
                    () = tokio::time::sleep(pause) => {}
                    () = wake.notified() => {}
                    () = shard.closed() => break,
                }
                pause = (pause * 2).min(LAST_RETRY);
                continue;
            }
        }
        // A wake during the pass may have come for a set it did not see.
        if wake.notified().now_or_never().is_none() {
            break;
        }
    }
    if let Some(installer) = shard.plugin_installs().devices.borrow_mut().get_mut(&device) {
        installer.running = false;
    }
}

/// Brings `device` to the user's directory sets once per runner connection,
/// set and workspaces using it:
///
/// 1. It lists every plugin's directory on the device with one request.
/// 2. It removes what the sets no longer name with one more request.
/// 3. It writes each directory missing that a command on the device may
///    read, every conversation's and those of the workspaces whose
///    conversations run on the device, [`WRITES_AT_ONCE`] at a time.
///
/// A device that is offline, or the Claude Code machine, which runs no
/// command, passes at once. A directory that fails leaves the others to go
/// on, and fails the pass.
async fn pass(shard: &dyn DeviceShard, device: &DeviceId) -> Result<(), String> {
    let Some(link) = shard.devices().link(device) else {
        return Ok(());
    };
    let record = shard
        .control()
        .device(device.clone())
        .await
        .map_err(|error| error.to_string())?;
    if record.is_none_or(|record| record.machine == Some(ManagedMachine::ClaudeCode)) {
        return Ok(());
    }
    let sets = shard.directory_sets().await?;
    let workspaces = shard
        .control()
        .device_workspaces(device.clone())
        .await
        .map_err(|error| error.to_string())?;
    let revision = revision_of(&sets, &workspaces);
    let installs = shard.plugin_installs();
    if installs.synced(device, &link, &revision) {
        return Ok(());
    }
    let Some(remote) = shard.devices().device_access(device).await else {
        return Ok(());
    };
    let home = shard
        .devices()
        .home(device)
        .ok_or("the device reported no home directory for the plugins' files")?;
    let started = std::time::Instant::now();
    // A failed pass is logged by its caller, with what failed and was written.
    let written = sync_directories(&remote, &home, &sets, &workspaces, &shard.blobs()).await?;
    tracing::info!(
        event = "plugin_directories.installed",
        device_id = %device,
        written,
        ms = started.elapsed().as_millis() as u64,
        "a device was brought to the plugins' directories"
    );
    if let Some(installer) = installs.devices.borrow_mut().get_mut(device) {
        installer.synced = Some((link.downgrade(), revision));
    }
    Ok(())
}

/// Brings the plugins' directories on `device` to `sets`: every plugin's
/// directory listed with one request, what no set names removed with one,
/// and each directory missing that every conversation or one of
/// `workspaces` reads written with one, [`WRITES_AT_ONCE`] at a time.
/// Answers how many it wrote, or why one or more failed.
async fn sync_directories(
    device: &RemoteDevice,
    home: &str,
    sets: &DirectorySets,
    workspaces: &[WorkspaceId],
    blobs: &UserBlobs,
) -> Result<usize, String> {
    let paths = device.identity().paths;
    let base = |plugin: &PluginId| paths.join(home, &format!(".jush/plugins/{plugin}"));
    let listed: Vec<LookAt> = sets
        .iter()
        .map(|(plugin, _)| LookAt {
            path: base(plugin),
            limit: 0,
        })
        .collect();
    let found = if listed.is_empty() {
        Vec::new()
    } else {
        device.look(&listed).await.map_err(|error| error.message)?.0
    };
    let mut removed = Vec::new();
    let mut missing = Vec::new();
    for ((plugin, set), look) in sets.iter().zip(found) {
        let present: Vec<String> = match look {
            Look::Directory(entries) => entries.into_iter().map(|entry| entry.name).collect(),
            Look::Missing => Vec::new(),
            Look::Unreadable(error) => return Err(error.message),
            Look::File { .. } | Look::Other => return Err(format!("{} is not a directory", base(plugin))),
        };
        let wanted: Vec<(String, &DeviceDirectory)> = set
            .iter()
            .map(|directory| (directory.device_name(), directory))
            .collect();
        let names: BTreeSet<&str> = wanted.iter().map(|(name, _)| name.as_str()).collect();
        removed.extend(
            present
                .iter()
                .filter(|name| !names.contains(name.as_str()))
                .map(|name| paths.join(&base(plugin), name)),
        );
        missing.extend(
            wanted
                .into_iter()
                .filter(|(_, directory)| directory.workspace.as_ref().is_none_or(|workspace| workspaces.contains(workspace)))
                .filter(|(name, _)| !present.contains(name))
                .map(|(name, directory)| (paths.join(&base(plugin), &name), directory)),
        );
    }
    let (mut written, mut failed) = (0, 0);
    // One that cannot be removed, such as a directory a process still uses,
    // keeps no other directory from being written.
    if !removed.is_empty()
        && let Err(error) = device.remove_all(&removed).await
    {
        failed += 1;
        tracing::warn!(
            event = "plugin_directory.remove_failed",
            paths = removed.len(),
            error = %error.message,
            "plugin directories no set names were not all removed from a device"
        );
    }
    let mut writes = futures_util::stream::iter(missing)
        .map(|(path, directory)| async move {
            let written = install(device, blobs, &path, directory).await;
            (path, written)
        })
        .buffer_unordered(WRITES_AT_ONCE);
    while let Some((path, outcome)) = writes.next().await {
        match outcome {
            Ok(()) => written += 1,
            Err(error) => {
                failed += 1;
                tracing::warn!(
                    event = "plugin_directory.install_failed",
                    path,
                    error = %error.message,
                    "a plugin's directory was not written on a device"
                );
            }
        }
    }
    if failed > 0 {
        return Err(format!("{failed} of the plugins' changes failed, and {written} directories were written"));
    }
    Ok(written)
}

/// Writes `directory` at `path` with one request: its files read-only,
/// each keeping its executable bit, and every directory in it read-only
/// too, so a directory that exists is complete.
async fn install(
    device: &RemoteDevice,
    blobs: &UserBlobs,
    path: &str,
    directory: &DeviceDirectory,
) -> Result<(), DeviceError> {
    let mut files = Vec::with_capacity(directory.files.len());
    for file in &directory.files {
        let bytes = blobs
            .get(&file.blob)
            .await
            .map_err(|error| DeviceError::failed(None, error.to_string()))?
            .ok_or_else(|| {
                DeviceError::failed(
                    None,
                    format!("the file {} of {path} is not stored", file.path),
                )
            })?;
        files.push(DirectoryFile {
            path: file.path.clone(),
            mode: if file.executable { 0o555 } else { 0o444 },
            bytes,
        });
    }
    device.write_directory(path, files, 0o555).await
}

/// What each of `reads` finds on `device`, a read by content's file
/// ([`DeviceRead::content`]) as the bytes `blobs` hold of it, or one read of
/// them into `blobs` (`UserBlobs::fetch_once`): the device hashes the file, and
/// its bytes travel only when they are new, so twenty commands that read the
/// same image send it once. The rest is [`look_files`].
pub async fn read_device_files(
    device: &RemoteDevice,
    blobs: &UserBlobs,
    reads: &[DeviceRead],
) -> Result<(Vec<DeviceFile>, ReadAt), DeviceError> {
    let looks: Vec<DeviceRead> = reads
        .iter()
        .map(|read| if read.content { DeviceRead::new(read.path.clone(), 0) } else { read.clone() })
        .collect();
    let (mut files, read_at) = look_files(device, &looks).await?;
    let contents = reads.iter().zip(&files).enumerate().filter_map(|(index, (read, file))| match file {
        DeviceFile::File { size, .. } if read.content && *size <= read.limit => Some(async move {
            (index, content(device, blobs, &read.path, read.limit).await)
        }),
        _ => None,
    });
    for (index, read) in futures_util::future::join_all(contents).await {
        files[index] = match read {
            Ok(bytes) => DeviceFile::File {
                size: bytes.len() as u64,
                bytes: B64Bytes::new(bytes),
            },
            Err(message) => DeviceFile::Unreadable { message },
        };
    }
    Ok((files, read_at))
}

/// The bytes of the file at `path` on `device`, of at most `limit`: the ones
/// `blobs` hold under its hash, or a read of them into `blobs`.
async fn content(device: &RemoteDevice, blobs: &UserBlobs, path: &str, limit: u64) -> Result<Bytes, String> {
    let (sha256, size) = match device.hash_file(path, limit).await.map_err(|error| error.message)? {
        FileHash::Hashed { sha256, size, .. } => (sha256, size),
        FileHash::TooLarge { size } => return Err(format!("the file grew to {size} bytes while it was read")),
    };
    // Only the bytes hashed: a file replaced since by a larger one is read
    // no further, and fails as changed.
    let read = || async move {
        let range = ByteRange {
            offset: 0,
            length: Some(size),
        };
        let mut stream = device.read_stream(path, range).await?;
        let mut bytes = BytesMut::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
            if bytes.len() as u64 > size {
                return Err(DeviceError::interrupted("the runner sent more bytes than it was asked for"));
            }
        }
        Ok::<_, DeviceError>(bytes.freeze())
    };
    blobs.fetch_once(&sha256, read).await.map_err(|error| error.to_string())
}

/// What each of `reads` finds on `device`, with one request to the device
/// (`crates/runner-device/src/lib.rs`).
///
/// A failure on the device about a path answers for the path. Any other failure,
/// such as the connection's end, fails the read.
pub async fn look_files(device: &RemoteDevice, reads: &[DeviceRead]) -> Result<(Vec<DeviceFile>, ReadAt), DeviceError> {
    let paths: Vec<LookAt> = reads
        .iter()
        .map(|read| LookAt {
            path: read.path.clone(),
            limit: read.limit,
        })
        .collect();
    let (found, read_at) = device.look(&paths).await?;
    let files = found
        .into_iter()
        .map(|look| match look {
            Look::Missing => DeviceFile::Missing,
            Look::Directory(entries) => DeviceFile::Directory {
                entries: entries
                    .into_iter()
                    .map(|entry| DeviceEntry {
                        name: entry.name,
                        kind: entry_kind(entry.kind),
                    })
                    .collect(),
            },
            Look::File { bytes, size } => DeviceFile::File {
                bytes: B64Bytes::new(bytes),
                size,
            },
            Look::Other => DeviceFile::Other,
            Look::Unreadable(error) => DeviceFile::Unreadable {
                message: error.message,
            },
        })
        .collect();
    Ok((files, read_at))
}

fn entry_kind(kind: FileKind) -> EntryKind {
    match kind {
        FileKind::File => EntryKind::File,
        FileKind::Directory => EntryKind::Directory,
        FileKind::Symlink => EntryKind::Symlink,
        FileKind::CharacterDevice | FileKind::Fifo | FileKind::Other => EntryKind::Other,
    }
}

/// Whether `error` says the device is not reachable now.
fn not_running(error: &DeviceError) -> bool {
    matches!(
        error.kind,
        DeviceErrorKind::Offline | DeviceErrorKind::Unavailable
    )
}

/// The revision of the user's sets for a device that `workspaces` use, which
/// a device is brought to once per connection: each plugin with the names its
/// device directory holds and the workspace each is for, since a directory
/// that moves to another workspace keeps its name.
fn revision_of(sets: &DirectorySets, workspaces: &[WorkspaceId]) -> String {
    let mut revision = String::new();
    for workspace in workspaces {
        revision.push_str(workspace.as_str());
        revision.push(' ');
    }
    revision.push('\n');
    for (plugin, set) in sets {
        revision.push_str(plugin.as_str());
        for directory in set {
            revision.push(' ');
            revision.push_str(&directory.device_name());
            if let Some(workspace) = &directory.workspace {
                revision.push('@');
                revision.push_str(workspace.as_str());
            }
        }
        revision.push('\n');
    }
    revision
}
