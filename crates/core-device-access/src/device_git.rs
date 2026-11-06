//! Git on one of the user's paired devices, which a plugin asks for through
//! device access (`crate`'s entry comment): the device runs its own `git`,
//! as its user, with that user's keys, credential helpers and network.
//!
//! - A check reads the commit a repository's default branch points to,
//!   `git ls-remote <url> HEAD`.
//! - A fetch reads the default branch's newest commit, bare and without
//!   the files' contents where the remote can leave them out, into a
//!   directory of the device's own under `~/.demi/tmp/`. It lists the
//!   commit's tree, fetches the objects of the files of every directory a
//!   file named `marker` marks in one request, and reads them in runs of at
//!   most [`READ_MAX_BYTES`], each run's objects stored in the user's blobs
//!   before the next run starts: the commit's bytes, as the server's fetch
//!   reads them, with no checkout's line endings or filters. One request,
//!   since git would otherwise fetch each object on its own connection as
//!   it is read; runs of many objects, since each run costs a round trip to
//!   the device, and a repository may hold thousands of such files.
//!   The directory is removed afterwards, and a fetch first removes what an
//!   earlier one cut off left behind.
//!
//! `git` never waits for anyone: its standard input is closed, every way it
//! or SSH could ask for a credential is turned off, and a run that writes
//! nothing for [`SILENCE_LIMIT`] is killed, which a transfer's progress keeps
//! from happening while its bytes move. A failure answers `git`'s own words.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::StreamExt;
use demi_core_blobs::blobs::UserBlobs;
use demi_device_interface::{FileKind, Device as _, ProcessEnd, RmOptions, SpawnEnv, SpawnRequest};
use demi_runner_client::RemoteDevice;
use demi_shared_types::{BlobRef, StreamKind};
use demi_web_api_protocol::devices::DeviceKind;
use demi_web_api_protocol::ids::DeviceId;

use crate::DeviceCore;

/// The fetches this backend started, which tells two fetches' directories
/// apart within one moment.
static FETCHES: AtomicU64 = AtomicU64::new(0);

/// What a fetch's directory is named after, followed by when it started.
const DIRECTORY_PREFIX: &str = "git-fetch-";

/// How old a fetch's directory is before a later fetch removes it: a fetch
/// marks its directory every [`MARK_INTERVAL`] while it runs, so one left
/// unmarked this long belongs to a fetch that was cut off. Another
/// backend's runner on the same machine shares the directory's parent, so
/// age, not this backend's memory, tells which are left behind.
const LEFT_BEHIND: Duration = Duration::from_secs(60 * 60);

/// How often a running fetch marks its directory as in use.
const MARK_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// The most bytes of objects one run of `git cat-file --batch` reads, an
/// object larger than it in a run of its own: the backend holds a run's
/// bytes until they are stored, since a process's output waits for its
/// reader in a queue that does not bound it.
const READ_MAX_BYTES: usize = 8 * 1024 * 1024;

/// How long a check may take before its `git` is killed: checks run on a
/// schedule, and one that hangs would hold the next.
const CHECK_LIMIT: Duration = Duration::from_secs(60);

/// How long a run may write nothing before its `git` is killed as stalled:
/// a transfer whose bytes still move writes its progress far more often,
/// and a connection that cannot be made fails within it on its own, with
/// its own words, as Linux's does after about two minutes.
const SILENCE_LIMIT: Duration = Duration::from_secs(5 * 60);

/// The most bytes of a failure's words a run keeps, the last ones: git
/// ends with what failed, and a remote may write without end.
const WORDS_MAX_BYTES: usize = 64 * 1024;

/// The variables that would point `git` at another repository than the one
/// a run names, which the run's environment removes.
const ROUTING_VARIABLES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG_PARAMETERS",
];

/// A commit's tree as a fetch read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTree {
    pub commit: String,
    /// Every file of the tree, by its path in the repository.
    pub entries: Vec<GitEntry>,
    /// The files of the directories `marker` marks, each as the user's blob
    /// that holds its bytes.
    pub files: Vec<(String, BlobRef)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitEntry {
    pub path: String,
    pub executable: bool,
}

/// Why git on a device answered nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceGitError {
    /// The device is not one of the user's paired devices.
    NotFound,
    /// Its runner is not connected, or went away during the run.
    Offline,
    /// `git` or the device failed, in their words.
    Failed(String),
}

impl dyn DeviceCore + '_ {
    /// The commit `url`'s default branch points to, as `device` reads it.
    pub async fn device_git_head(&self, device: &DeviceId, url: &str) -> Result<String, DeviceGitError> {
        let remote = self.git_device(device).await?;
        let listed = tokio::time::timeout(CHECK_LIMIT, git(&remote, None, &["ls-remote", "--", url, "HEAD"]))
            .await
            .map_err(|_| DeviceGitError::Failed(format!("git did not answer within {} seconds", CHECK_LIMIT.as_secs())))??;
        let line = String::from_utf8_lossy(&listed);
        line.split_whitespace()
            .next()
            .map(str::to_owned)
            .ok_or_else(|| DeviceGitError::Failed("the repository holds no commit".into()))
    }

    /// The newest commit of `url`'s default branch as `device` clones it,
    /// with the files of the directories `marker` marks stored as the
    /// user's blobs.
    pub async fn device_git_fetch(&self, device: &DeviceId, url: &str, marker: &str) -> Result<GitTree, DeviceGitError> {
        let remote = self.git_device(device).await?;
        let paths = remote.identity().paths;
        let base = paths.join(&remote.identity().home_dir, ".demi/tmp");
        let base = remote
            .fs()
            .directory(&base, true)
            .await
            .map_err(|error| DeviceGitError::Failed(error.to_string()))?;
        remove_left_behind(&remote, &base).await;
        let started = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
        let number = FETCHES.fetch_add(1, Ordering::Relaxed);
        let directory = paths.join(&base, &format!("{DIRECTORY_PREFIX}{started}-{number}"));
        let blobs = self.blobs();
        let fetched = {
            let reading = clone_and_read(&remote, &blobs, url, &directory, marker);
            tokio::pin!(reading);
            let mut marks = tokio::time::interval_at(tokio::time::Instant::now() + MARK_INTERVAL, MARK_INTERVAL);
            loop {
                tokio::select! {
                    fetched = &mut reading => break fetched,
                    _ = marks.tick() => mark(&remote, &directory).await,
                }
            }
        };
        // A removal that fails, or a fetch cut off before it, leaves a
        // directory the next fetch removes.
        if let Err(error) = remote
            .fs()
            .rm(&directory, RmOptions { recursive: true, force: true })
            .await
        {
            tracing::warn!(event = "device_git.cleanup_failed", %device, %error, "a fetch's directory stayed on the device");
        }
        fetched
    }

    /// The device of the user's paired device `device`, while it is connected.
    async fn git_device(&self, device: &DeviceId) -> Result<RemoteDevice, DeviceGitError> {
        let paired = self
            .store()
            .device(device.clone())
            .await
            .map_err(|error| DeviceGitError::Failed(error.to_string()))?
            .is_some_and(|record| record.kind == DeviceKind::User);
        if !paired {
            return Err(DeviceGitError::NotFound);
        }
        if self.devices().link(device).is_none() {
            return Err(DeviceGitError::Offline);
        }
        self.devices().device_access(device).await.ok_or(DeviceGitError::Offline)
    }
}

/// Marks a running fetch's `directory` as in use, by its modification
/// time; a mark that fails leaves the next one to make.
async fn mark(device: &RemoteDevice, directory: &str) {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let Ok(now) = demi_shared_types::Timestamp::from_millisecond(i64::try_from(since).unwrap_or(i64::MAX)) else {
        return;
    };
    let _ = device.fs().utimes(directory, now, now).await;
}

/// Removes the fetches' directories in `base` older than [`LEFT_BEHIND`],
/// which fetches that were cut off left; one that fails stays for the next.
async fn remove_left_behind(device: &RemoteDevice, base: &str) {
    let Ok(entries) = device.fs().read_dir(base).await else {
        return;
    };
    let limit = i64::try_from(LEFT_BEHIND.as_millis()).unwrap_or(i64::MAX);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX));
    for entry in entries {
        if entry.kind != FileKind::Directory
            || !entry.name.starts_with(DIRECTORY_PREFIX)
            || now - entry.modified.as_millisecond() < limit
        {
            continue;
        }
        let path = device.identity().paths.join(base, &entry.name);
        let _ = device.fs().rm(&path, RmOptions { recursive: true, force: true }).await;
    }
}

async fn clone_and_read(
    device: &RemoteDevice,
    blobs: &UserBlobs,
    url: &str,
    directory: &str,
    marker: &str,
) -> Result<GitTree, DeviceGitError> {
    // Bare and without the files' contents, which the remote sends only for
    // the objects read below, in the remote's object format, which a clone
    // follows. Not quiet, which would hide the meter of the objects being
    // received, by which a transfer that moves shows it has not stalled;
    // the line that names the clone's directory, "Cloning into…" in the
    // device's language, says nothing of a failure and leaves its words.
    let clone = [
        "clone", "--progress", "--bare", "--depth", "1", "--single-branch", "--no-tags", "--filter=blob:none", "--",
        url, directory,
    ];
    git(device, None, &clone).await.map_err(|error| match error {
        DeviceGitError::Failed(words) => {
            let words: Vec<&str> = words.lines().filter(|line| !line.contains(directory)).collect();
            DeviceGitError::Failed(if words.is_empty() { "git failed".into() } else { words.join("\n") })
        }
        error => error,
    })?;
    let commit = String::from_utf8_lossy(&git(device, Some(directory), &["rev-parse", "HEAD"]).await?)
        .trim()
        .to_owned();
    if commit.is_empty() {
        return Err(DeviceGitError::Failed("the repository holds no commit".into()));
    }
    let listing = git(device, Some(directory), &["ls-tree", "-r", "-z", "HEAD"]).await?;
    let mut entries = Vec::new();
    let mut objects = Vec::new();
    for record in listing.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
        let record = String::from_utf8_lossy(record);
        // `<mode> <type> <object>\t<path>`
        let Some((head, path)) = record.split_once('\t') else {
            continue;
        };
        let mut fields = head.split_whitespace();
        let (Some(mode), Some(kind), Some(object)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        if kind != "blob" || !(mode == "100644" || mode == "100755") {
            continue;
        }
        entries.push(GitEntry { path: path.to_owned(), executable: mode == "100755" });
        objects.push(object.to_owned());
    }
    let marked: BTreeSet<&str> = entries
        .iter()
        .filter_map(|entry| match entry.path.rsplit_once('/') {
            Some((parent, name)) if name == marker => Some(parent),
            None if entry.path == marker => Some(""),
            _ => None,
        })
        .collect();
    let below = |path: &str| {
        marked
            .iter()
            .any(|parent| parent.is_empty() || path.strip_prefix(parent).is_some_and(|rest| rest.starts_with('/')))
    };
    let wanted: Vec<(&GitEntry, &String)> = entries
        .iter()
        .zip(&objects)
        .filter(|(entry, _)| below(&entry.path))
        .collect();
    let promisor = git(device, Some(directory), &["config", "--get", "remote.origin.promisor"])
        .await
        .is_ok_and(|value| String::from_utf8_lossy(&value).trim() == "true");
    if promisor && !wanted.is_empty() {
        let fetch = [
            "-c", "fetch.negotiationAlgorithm=noop", "fetch", "--progress", "--no-tags", "--no-write-fetch-head",
            "--recurse-submodules=no", "--filter=blob:none", "--stdin", "origin",
        ];
        run(device, Some(directory), &fetch, Some(list(wanted.iter().map(|(_, object)| object.as_str())))).await?;
    }
    // The objects' own bytes: what the commit holds, as the server reads it,
    // each object once, though several paths may hold it.
    let unique: BTreeSet<&str> = wanted.iter().map(|(_, object)| object.as_str()).collect();
    let mut stored: BTreeMap<String, BlobRef> = BTreeMap::new();
    for run_objects in read_runs(device, directory, &unique).await? {
        let objects = list(run_objects.iter().map(String::as_str));
        let output = run(device, Some(directory), &["cat-file", "--batch"], Some(objects)).await?;
        for (object, bytes) in read_batch(Bytes::from(output))? {
            let blob = blobs
                .put(bytes)
                .await
                .map_err(|error| DeviceGitError::Failed(format!("the files could not be stored: {error}")))?;
            stored.insert(object, blob);
        }
    }
    let mut files = Vec::with_capacity(wanted.len());
    for (entry, object) in wanted {
        let blob = stored
            .get(object.as_str())
            .cloned()
            .ok_or_else(|| DeviceGitError::Failed(format!("git read no object {object}")))?;
        files.push((entry.path.clone(), blob));
    }
    Ok(GitTree { commit, entries, files })
}

/// `objects` in the runs `git cat-file --batch` reads them in: as many as
/// fit [`READ_MAX_BYTES`] together, by the sizes one
/// `git cat-file --batch-check` gives, an object larger than it alone.
async fn read_runs(device: &RemoteDevice, directory: &str, objects: &BTreeSet<&str>) -> Result<Vec<Vec<String>>, DeviceGitError> {
    if objects.is_empty() {
        return Ok(Vec::new());
    }
    let checked = run(
        device,
        Some(directory),
        &["cat-file", "--batch-check=%(objectname) %(objectsize)"],
        Some(list(objects.iter().copied())),
    )
    .await?;
    let mut runs: Vec<Vec<String>> = Vec::new();
    let mut room = 0usize;
    for line in String::from_utf8_lossy(&checked).lines() {
        let (object, size) = line
            .split_once(' ')
            .ok_or_else(|| DeviceGitError::Failed("git cat-file wrote what it should not".into()))?;
        let Ok(size) = size.parse::<usize>() else {
            return Err(DeviceGitError::Failed(format!("git has no object {object}")));
        };
        match runs.last_mut() {
            Some(run) if size <= room => {
                run.push(object.to_owned());
                room -= size;
            }
            _ => {
                runs.push(vec![object.to_owned()]);
                room = READ_MAX_BYTES.saturating_sub(size);
            }
        }
    }
    Ok(runs)
}

/// `objects`, one name per line, as git reads a list on its standard input.
fn list<'a>(objects: impl Iterator<Item = &'a str>) -> Bytes {
    let mut list = String::new();
    for object in objects {
        list.push_str(object);
        list.push('\n');
    }
    Bytes::from(list)
}

/// The objects `git cat-file --batch` wrote, by name: each a header line,
/// `<object> <type> <size>`, its bytes and a newline, or `<object> missing`
/// for one the clone lacks.
fn read_batch(output: Bytes) -> Result<BTreeMap<String, Bytes>, DeviceGitError> {
    let malformed = || DeviceGitError::Failed("git cat-file wrote what it should not".into());
    let mut objects = BTreeMap::new();
    let mut at = 0;
    while at < output.len() {
        let end = output[at..].iter().position(|byte| *byte == b'\n').ok_or_else(malformed)? + at;
        let header = std::str::from_utf8(&output[at..end]).map_err(|_| malformed())?;
        let mut fields = header.split(' ');
        let (Some(object), Some(kind)) = (fields.next(), fields.next()) else {
            return Err(malformed());
        };
        if kind == "missing" {
            return Err(DeviceGitError::Failed(format!("git has no object {object}")));
        }
        let size: usize = fields.next().and_then(|size| size.parse().ok()).ok_or_else(malformed)?;
        let start = end + 1;
        let stop = start.checked_add(size).filter(|stop| *stop < output.len()).ok_or_else(malformed)?;
        if output[stop] != b'\n' {
            return Err(malformed());
        }
        objects.insert(object.to_owned(), output.slice(start..stop));
        at = stop + 1;
    }
    Ok(objects)
}

/// Runs `git args` on `device`, in `directory` or the device's default, and
/// answers its standard output; a run that writes nothing for
/// [`SILENCE_LIMIT`] is killed, and a nonzero exit fails with its words
/// ([`Words`]), or its output when it has none.
async fn git(device: &RemoteDevice, directory: Option<&str>, args: &[&str]) -> Result<Vec<u8>, DeviceGitError> {
    run(device, directory, args, None).await
}

/// [`git`], with `input` written to its standard input before it closes.
async fn run(
    remote: &RemoteDevice,
    directory: Option<&str>,
    args: &[&str],
    input: Option<Bytes>,
) -> Result<Vec<u8>, DeviceGitError> {
    let mut command: Vec<String> = Vec::new();
    if let Some(directory) = directory {
        command.extend(["-C".to_owned(), directory.to_owned()]);
    }
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    let mut env: std::collections::BTreeMap<String, Option<String>> = [
        // No prompt: git's own, SSH's, or a credential manager's.
        ("GIT_TERMINAL_PROMPT", Some("0")),
        ("GCM_INTERACTIVE", Some("never")),
        ("SSH_ASKPASS_REQUIRE", Some("never")),
        ("GIT_ASKPASS", None),
        ("SSH_ASKPASS", None),
        // A read of an object the fetch did not bring fails at once rather
        // than fetching it alone; a git before 2.44 fetches it all the same.
        ("GIT_NO_LAZY_FETCH", Some("1")),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.map(str::to_owned)))
    .collect();
    env.extend(ROUTING_VARIABLES.iter().map(|name| ((*name).to_owned(), None)));
    let request = SpawnRequest {
        command: "git".into(),
        args: command,
        cwd: None,
        env: SpawnEnv::Overlay(env),
        descriptors: Vec::new(),
        retained: false,
        // A kill takes the helpers git started, such as `git-remote-https`,
        // which would otherwise hold a stalled connection on.
        process_group: true,
    };
    let mut process = remote
        .process()
        .spawn(request)
        .await
        .map_err(|error| DeviceGitError::Failed(error.to_string()))?;
    if let Some(input) = input {
        process
            .control
            .write_stdin(input)
            .await
            .map_err(|error| DeviceGitError::Failed(error.to_string()))?;
    }
    // Nothing answers a question git asks on its standard input.
    let _ = process.control.close_stdin().await;
    let mut stdout = Vec::new();
    let mut words = Words::default();
    // Dropping the process on an early return kills it.
    loop {
        let Ok(chunk) = tokio::time::timeout(SILENCE_LIMIT, process.output.next()).await else {
            let stalled = format!("git wrote nothing for {} seconds", SILENCE_LIMIT.as_secs());
            let words = words.text();
            return Err(DeviceGitError::Failed(if words.is_empty() { stalled } else { format!("{words}\n{stalled}") }));
        };
        let Some(chunk) = chunk else {
            break;
        };
        match chunk.stream {
            StreamKind::Stdout => stdout.extend_from_slice(&chunk.bytes),
            StreamKind::Stderr => words.push(&chunk.bytes),
        }
    }
    match process.exit.await {
        ProcessEnd::Exited(0) => Ok(stdout),
        ProcessEnd::Exited(_) | ProcessEnd::Signalled(_) => {
            let words = words.text();
            let words = if words.is_empty() { String::from_utf8_lossy(&stdout).trim().to_owned() } else { words };
            Err(DeviceGitError::Failed(if words.is_empty() { "git failed".into() } else { words }))
        }
        ProcessEnd::NotStarted(error) => Err(DeviceGitError::Failed(format!("git did not start: {error:?}"))),
        ProcessEnd::Lost(reason) => {
            tracing::info!(event = "device_git.lost", %reason, "a device went away during git");
            Err(DeviceGitError::Offline)
        }
    }
}

/// The words of a failure: what `git` wrote to its standard error after the
/// last line it redrew, ending it with a carriage return alone, as it draws
/// a progress meter. What came before is progress too, such as the remote's
/// counts. A carriage return before a newline ends a line, as Windows ends
/// them. The last [`WORDS_MAX_BYTES`] of the whole lines, and as many of the
/// line being written.
#[derive(Default)]
struct Words {
    /// The lines since the last redrawn one.
    lines: Vec<u8>,
    /// The line being written.
    line: Vec<u8>,
    /// The last byte was a carriage return, which the next one tells apart.
    carriage: bool,
}

impl Words {
    fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.carriage && byte != b'\n' {
                self.lines.clear();
                self.line.clear();
            }
            self.carriage = byte == b'\r';
            match byte {
                b'\r' => {}
                b'\n' => {
                    self.lines.append(&mut self.line);
                    self.lines.push(b'\n');
                }
                byte => self.line.push(byte),
            }
        }
        for kept in [&mut self.lines, &mut self.line] {
            if kept.len() > WORDS_MAX_BYTES {
                kept.drain(..kept.len() - WORDS_MAX_BYTES);
            }
        }
    }

    fn text(&self) -> String {
        if self.carriage {
            return String::new();
        }
        let mut bytes = self.lines.clone();
        bytes.extend_from_slice(&self.line);
        String::from_utf8_lossy(&bytes).trim().to_owned()
    }
}
