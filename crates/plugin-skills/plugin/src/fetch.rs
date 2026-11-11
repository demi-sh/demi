//! A source's fetch and the check of its commit.
//!
//! - The fetch is a shallow fetch of the repository's default branch into a
//!   temporary directory, the server's or a device's. Either answers a
//!   [`Tree`]: the commit it pins, every file's path, and the files of each
//!   directory a `SKILL.md` marks, stored as the user's blobs as they are
//!   read, so the fetch holds one file at a time however large the source.
//!   [`skills`] reads the skills of a tree by the same rules for both.
//! - The check finds the commit that branch points to now.
//!
//! The server's fetch and check run here, with gix, and block, so they run on
//! the blocking pool. A device's run there, with its own `git`
//! (`crates/backend-device-access/src/device_git.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use demi_plugin_interface::PluginPort;
use demi_shared_types::{B64Bytes, BlobRef};
use gix::bstr::ByteSlice;

use crate::skill::{self, Parsed, SkipProblem};

/// The skills of a fetched commit.
#[derive(Debug)]
pub(crate) struct Fetched {
    pub(crate) commit: String,
    pub(crate) skills: Vec<FetchedSkill>,
    pub(crate) skipped: Vec<Skipped>,
}

/// A skill of the commit, with its files as the user's blobs.
#[derive(Debug)]
pub(crate) struct FetchedSkill {
    pub(crate) parsed: Parsed,
    /// Its directory in the repository, empty at the root.
    pub(crate) directory: String,
    pub(crate) files: Vec<FetchedFile>,
}

#[derive(Debug)]
pub(crate) struct FetchedFile {
    /// Relative to the skill's directory.
    pub(crate) path: String,
    pub(crate) executable: bool,
    /// The user's blob that holds its bytes.
    pub(crate) blob: BlobRef,
}

/// A `SKILL.md` that is not a skill.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// Its path in the repository.
    pub path: String,
    /// Why, in English.
    pub reason: String,
    /// Why, as the page words it; none in a value a fetch before codes
    /// recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<SkipProblem>,
}

/// Why a fetch gave no skills, as the page words it: one of Demi's finds, or
/// a failure in git's or the system's words, which the failure's message
/// keeps.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum FetchProblem {
    NoCommit,
    NoSkill,
    Failed,
}

/// Why a fetch gave no skills.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FetchError {
    /// The instance ended, as with the backend's shutdown: nothing is
    /// recorded.
    #[error("the fetch was stopped")]
    Stopped,
    #[error("{message}")]
    Failed {
        message: String,
        problem: FetchProblem,
    },
}

impl FetchError {
    /// A failure in git's or the system's words.
    fn failed(error: impl std::fmt::Display) -> Self {
        Self::problem(FetchProblem::Failed, error.to_string())
    }

    fn problem(problem: FetchProblem, message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
            problem,
        }
    }
}

/// A commit's tree as a fetch read it.
pub(crate) struct Tree {
    pub(crate) commit: String,
    /// Every file of the tree.
    pub(crate) entries: Vec<TreeFile>,
    /// The files of the directories a `SKILL.md` marks, by path, each as the
    /// user's blob that holds its bytes.
    pub(crate) files: BTreeMap<String, BlobRef>,
}

/// Fetches `url` on the server, in a directory of its own under `scratch`,
/// and stores the files of the directories a `SKILL.md` marks as the
/// user's blobs as they are read; `stop` ends the fetch, as the instance's
/// end does.
pub(crate) async fn on_server(
    port: &PluginPort,
    url: String,
    scratch: PathBuf,
    stop: Arc<AtomicBool>,
) -> Result<Tree, FetchError> {
    // The clone's repository cannot move between threads, so one blocking
    // task clones and reads it, and hands the objects over one at a time.
    let (sender, mut objects) = tokio::sync::mpsc::channel(1);
    let reading = tokio::task::spawn_blocking(move || clone_and_read(&url, &scratch, &stop, &sender));
    let mut stored: HashMap<gix::ObjectId, BlobRef> = HashMap::new();
    while let Some((object, bytes)) = objects.recv().await {
        let blob = port
            .put_blob(B64Bytes::new(bytes))
            .await
            .map_err(|error| FetchError::failed(format!("the files could not be stored: {error}")))?;
        stored.insert(object, blob);
    }
    let read = reading
        .await
        .map_err(|error| FetchError::failed(format!("the fetch ended: {error}")))??;
    let mut files = BTreeMap::new();
    for (path, object) in read.wanted {
        let blob = stored
            .get(&object)
            .ok_or_else(|| FetchError::failed(format!("the fetch read no {path}")))?;
        files.insert(path, blob.clone());
    }
    Ok(Tree {
        commit: read.commit,
        entries: read.entries,
        files,
    })
}

/// What [`clone_and_read`] read of a commit.
struct Read {
    commit: String,
    entries: Vec<TreeFile>,
    /// The files of the directories a `SKILL.md` marks, each with its object.
    wanted: Vec<(String, gix::ObjectId)>,
}

/// Clones `url`'s default branch's newest commit, bare, into a temporary
/// directory under `scratch`, lists its tree, and sends `objects` the bytes
/// of each file of a directory a `SKILL.md` marks, each object once; `stop`
/// ends it. It blocks, so it runs on the blocking pool.
fn clone_and_read(
    url: &str,
    scratch: &Path,
    stop: &Arc<AtomicBool>,
    objects: &tokio::sync::mpsc::Sender<(gix::ObjectId, Vec<u8>)>,
) -> Result<Read, FetchError> {
    std::fs::create_dir_all(scratch)
        .map_err(|error| FetchError::failed(format!("no temporary directory: {error}")))?;
    let directory = tempfile::tempdir_in(scratch)
        .map_err(|error| FetchError::failed(format!("no temporary directory: {error}")))?;
    let failed = |error: &dyn std::fmt::Display| {
        if stop.load(Ordering::Relaxed) {
            FetchError::Stopped
        } else {
            FetchError::failed(error)
        }
    };
    // Isolated: no configuration of the machine's, so no credential either.
    let mut prepare = gix::clone::PrepareFetch::new(
        url,
        directory.path(),
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        gix::open::Options::isolated(),
    )
    .map_err(|error| failed(&error))?
    .with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
        NonZeroU32::new(1).expect("one is not zero"),
    ));
    // gix writes the pack it receives to a file in the clone as it arrives.
    let (repository_on_disk, _) = prepare
        .fetch_only(gix::progress::Discard, stop)
        .map_err(|error| failed(&error))?;
    let commit = repository_on_disk
        .head_commit()
        .map_err(|_| FetchError::problem(FetchProblem::NoCommit, "the repository holds no commit"))?;
    let tree = commit.tree().map_err(|error| failed(&error))?;
    let mut recorder = gix::traverse::tree::Recorder::default();
    tree.traverse()
        .breadthfirst(&mut recorder)
        .map_err(|error| failed(&error))?;
    // Each file by its path as the skills read it, which a name that is
    // not UTF-8 reaches in its lossy form.
    let (entries, objects_of): (Vec<TreeFile>, Vec<gix::ObjectId>) = recorder
        .records
        .iter()
        .filter(|entry| entry.mode.is_blob())
        .map(|entry| {
            let file = TreeFile {
                path: entry.filepath.to_str_lossy().into_owned(),
                executable: entry.mode.is_executable(),
            };
            (file, entry.oid)
        })
        .unzip();
    let directories = skill_directories(&entries);
    let wanted: Vec<(String, gix::ObjectId)> = entries
        .iter()
        .zip(objects_of)
        .filter(|(file, _)| directories.iter().any(|directory| within(directory, &file.path).is_some()))
        .map(|(file, object)| (file.path.clone(), object))
        .collect();
    let mut sent = HashSet::new();
    for (_, object) in &wanted {
        if !sent.insert(*object) {
            continue;
        }
        if stop.load(Ordering::Relaxed) {
            return Err(FetchError::Stopped);
        }
        let bytes = repository_on_disk
            .find_object(*object)
            .map_err(FetchError::failed)?
            .detach()
            .data;
        // A receiver that is gone gave up on the fetch, and says why.
        if objects.blocking_send((*object, bytes)).is_err() {
            return Err(FetchError::Stopped);
        }
    }
    Ok(Read {
        commit: commit.id.to_string(),
        entries,
        wanted,
    })
}

/// The commit `url`'s default branch points to now, read from the remote's
/// ref advertisement without fetching any object. It blocks, so it runs on
/// the blocking pool.
pub(crate) fn remote_head(url: &str) -> Result<String, String> {
    // A remote needs a repository; an empty one, isolated as a fetch's is.
    let directory =
        tempfile::tempdir().map_err(|error| format!("no temporary directory: {error}"))?;
    let repository = gix::ThreadSafeRepository::init_opts(
        directory.path(),
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        gix::open::Options::isolated(),
    )
    .map_err(|error| error.to_string())?
    .to_thread_local();
    let remote = repository
        .remote_at(url)
        .map_err(|error| error.to_string())?
        .with_fetch_tags(gix::remote::fetch::Tags::None);
    let head = gix::refspec::parse("HEAD".into(), gix::refspec::parse::Operation::Fetch)
        .expect("HEAD is a refspec")
        .to_owned();
    let (refs, _) = remote
        .connect(gix::remote::Direction::Fetch)
        .map_err(|error| error.to_string())?
        .ref_map(
            gix::progress::Discard,
            gix::remote::ref_map::Options {
                extra_refspecs: vec![head],
                ..Default::default()
            },
        )
        .map_err(|error| error.to_string())?;
    refs.remote_refs
        .iter()
        .find_map(|advertised| {
            let (name, target, peeled) = advertised.unpack();
            (name == "HEAD").then(|| peeled.or(target)).flatten()
        })
        .map(|commit| commit.to_string())
        .ok_or_else(|| "the repository holds no commit".to_owned())
}

/// A file of a commit's tree.
pub(crate) struct TreeFile {
    /// In the repository, with `/` between its parts.
    pub(crate) path: String,
    pub(crate) executable: bool,
}

/// The skills of `tree`, read by the same rules whichever fetcher read it,
/// and the `SKILL.md` files that are not skills; `repository` names a skill
/// at the root whose front matter names none.
pub(crate) async fn skills(port: &PluginPort, tree: Tree, repository: &str) -> Result<Fetched, FetchError> {
    // Each SKILL.md is read and parsed before the next, so one at a time
    // is held.
    let mut manifests = Vec::new();
    for directory in skill_directories(&tree.entries) {
        let path = manifest(&directory);
        let blob = stored(&tree.files, &path)?;
        let bytes = port
            .blob(blob.clone())
            .await
            .map_err(FetchError::failed)?
            .ok_or_else(|| FetchError::failed(format!("{path} is not stored")))?;
        let name = directory.rsplit('/').next().filter(|name| !name.is_empty());
        let parsed = skill::parse(name.unwrap_or(repository), &bytes.into_bytes());
        manifests.push((directory, parsed));
    }
    // Every file of the tree against every skill: the blocking pool's, as a
    // tree may hold any number of either.
    tokio::task::spawn_blocking(move || skills_of(tree, manifests))
        .await
        .map_err(|error| FetchError::failed(format!("the fetch ended: {error}")))?
}

/// The directories of `files`, a commit's whole tree, that hold a
/// `SKILL.md`, the root as the empty one.
fn skill_directories(files: &[TreeFile]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|file| match file.path.rsplit_once('/') {
            Some((directory, "SKILL.md")) => Some(directory.to_owned()),
            None if file.path == "SKILL.md" => Some(String::new()),
            _ => None,
        })
        .collect()
}

/// The path of the `SKILL.md` of `directory`.
fn manifest(directory: &str) -> String {
    if directory.is_empty() {
        "SKILL.md".to_owned()
    } else {
        format!("{directory}/SKILL.md")
    }
}

/// The blob that holds the file at `path`, which the fetch read.
fn stored<'a>(blobs: &'a BTreeMap<String, BlobRef>, path: &str) -> Result<&'a BlobRef, FetchError> {
    blobs
        .get(path)
        .ok_or_else(|| FetchError::failed(format!("the fetch read no {path}")))
}

/// The skills of `tree`, each directory of `manifests` with what its
/// `SKILL.md` parsed as, and the `SKILL.md` files that are not skills. It
/// blocks.
fn skills_of(tree: Tree, manifests: Vec<(String, Result<Parsed, skill::NotASkill>)>) -> Result<Fetched, FetchError> {
    let directories: BTreeSet<&str> = manifests.iter().map(|(directory, _)| directory.as_str()).collect();
    let mut skills = Vec::new();
    let mut skipped = Vec::new();
    for (directory, parsed) in &manifests {
        let parsed = match parsed {
            Ok(parsed) => parsed.clone(),
            Err(not) => {
                skipped.push(Skipped {
                    path: manifest(directory),
                    reason: not.reason.clone(),
                    problem: Some(not.problem.clone()),
                });
                continue;
            }
        };
        let mut skill_files = Vec::new();
        for file in &tree.entries {
            let path = &file.path;
            let Some(relative) = within(directory, path) else {
                continue;
            };
            // A directory below that is a skill of its own keeps its files.
            let nested = directories.iter().any(|other| {
                other != directory && within(directory, other).is_some() && within(other, path).is_some()
            });
            if nested {
                continue;
            }
            skill_files.push(FetchedFile {
                path: relative.to_owned(),
                executable: file.executable,
                blob: stored(&tree.files, path)?.clone(),
            });
        }
        skills.push(FetchedSkill {
            parsed,
            directory: directory.clone(),
            files: skill_files,
        });
    }
    if skills.is_empty() {
        return Err(FetchError::problem(FetchProblem::NoSkill, "the repository holds no skill"));
    }
    Ok(Fetched {
        commit: tree.commit,
        skills,
        skipped,
    })
}

/// `path` relative to `directory`, if it is below it; every path is below
/// the root, the empty directory.
fn within<'a>(directory: &str, path: &'a str) -> Option<&'a str> {
    if directory.is_empty() {
        return Some(path);
    }
    path.strip_prefix(directory)?.strip_prefix('/')
}
