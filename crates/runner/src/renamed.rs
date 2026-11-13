//! An installation from before the product took the name Jush moves to the
//! new names as its runner starts.
//!
//! The runner of the release that brings the rename starts as the earlier
//! runner left the installation (`jush_runner_protocol::renamed`): as
//! `<installation>/demi-runner`, from a launcher that names the installation
//! in `DEMI_HOME`, by the registration `sh.demi.runner.<id>`. It then:
//!
//! ```text
//! ~/.demi/instances/<id>/demi-runner      started by sh.demi.runner.<id>
//!   1. rename the directory               ~/.jush/instances/<id>/
//!      and move the rest of ~/.demi,       ~/.jush/plugins/, ~/.jush/browsers/
//!      leaving links for the others
//!   2. settle: link jush-runner, write     (crate::update)
//!      the launcher, start jush-runner
//! ~/.jush/instances/<id>/jush-runner      no manager's: it registers
//!   3. hand over                          ai.jush.runner.<id> and hands over
//! ~/.jush/instances/<id>/jush-runner      the manager's
//!   4. finish: remove the registration    sh.demi.runner.<id>, demi-runner,
//!      and what is left of ~/.demi         the link; with the last one,
//!                                          ~/.demi
//! ```
//!
//! Only an installation in one of the places runners make, `~/.demi/runner/`
//! or `~/.demi/instances/`, moves; one that `DEMI_HOME` put elsewhere stays.
//! The rest of `~/.demi`, such as the downloaded browsers and the plugins'
//! files, is every installation's on the device, and the others may still
//! run the earlier release: on Unix the first to move takes it and leaves a
//! link at each earlier path, which the last to leave removes. Windows makes
//! no link without the right to, so there the rest moves with the last
//! installation, and those that moved before it start without it.
//!
//! A job runs on through the move. On Unix a directory renames while a job's
//! shell writes in it: the shell keeps its open files, its endpoint lies in
//! the system's temporary directory, and the next runner takes the job over
//! in the new place. The old path then stays as a link to the new one, so a
//! job that names it goes on, until a start finds no job's shell. Windows
//! refuses to rename a directory in which a process holds a file or works,
//! as a job's shell does: the runner then settles where it is and stays
//! there. The move is tried only at this first start, whose settling and
//! handing over register the runner where it now lies; a later one would
//! leave the running supervisor starting the launcher at its earlier path.
//!
//! It goes with the other migrations when the history starts again at the
//! first formal release.

use std::{
    io,
    path::{Path, PathBuf},
};

use jush_runner_protocol::renamed;

/// Takes the installation and the release a launcher from before the rename
/// names under its variables as the current ones name them, and drops the
/// earlier registration's mark of its runner: this runner is no runner of
/// the registration that will start it, so it settles and hands over.
///
/// # Safety
///
/// No other thread may read or write the environment meanwhile: it runs
/// first in `main`.
pub unsafe fn adopt_environment() {
    use jush_runner_protocol::{installation::HOME_ENV, release::RELEASE_ENV};
    for (earlier, current) in [(renamed::HOME_ENV, HOME_ENV), (renamed::RELEASE_ENV, RELEASE_ENV)] {
        if let Some(value) = std::env::var_os(earlier) {
            if std::env::var_os(current).is_none() {
                // SAFETY: the caller's.
                unsafe { std::env::set_var(current, value) };
            }
            // SAFETY: the caller's.
            unsafe { std::env::remove_var(earlier) };
        }
    }
    // SAFETY: the caller's.
    unsafe { std::env::remove_var(renamed::SUPERVISED) };
}

/// The directory of before the rename that held every installation.
fn old_root(home: &Path) -> PathBuf {
    home.join(renamed::ROOT)
}

/// The directory that holds every installation.
fn new_root(home: &Path) -> PathBuf {
    home.join(jush_runner_protocol::installation::ROOT[0])
}

/// What became of an installation from before the rename at this start,
/// which the runner logs once its log is open.
pub enum Move {
    /// It moved to this directory.
    Moved { from: PathBuf, to: PathBuf },
    /// It stays where it is, for this reason.
    Kept { from: PathBuf, reason: String },
}

impl Move {
    /// The installation's directory from here on, when it moved.
    pub fn moved_to(&self) -> Option<&Path> {
        match self {
            Self::Moved { to, .. } => Some(to),
            Self::Kept { .. } => None,
        }
    }

    pub fn log(&self) {
        match self {
            Self::Moved { from, to } => {
                tracing::info!(event = "rename.installation_moved", from = %from.display(), to = %to.display(), "the installation moved to its new name");
            }
            Self::Kept { from, reason } => {
                tracing::warn!(event = "rename.installation_kept", from = %from.display(), %reason, "the installation stays under its earlier name");
            }
        }
    }
}

/// Moves the installation in `directory`, when it is one of `~/.demi`'s and
/// this runner starts as its earlier runner, to the same place under
/// `~/.jush`, and with it the rest of `~/.demi`; none when it is no such
/// installation.
pub async fn move_installation(directory: &Path, home: &Path) -> Option<Move> {
    let started = std::env::current_exe().ok()?;
    if started.file_name()? != std::ffi::OsStr::new(&earlier_runner()) {
        return None;
    }
    let old = old_root(home);
    let relative = directory.strip_prefix(&old).ok()?.to_owned();
    let mut parts = relative.components();
    let one_of_ours = matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(holder), Some(_), None) if is_installations(holder.as_os_str())
    );
    if !one_of_ours {
        return None;
    }
    let from = directory.to_owned();
    let new = new_root(home).join(&relative);
    if tokio::fs::symlink_metadata(&new).await.is_ok() {
        let reason = format!("{} is taken", new.display());
        return Some(Move::Kept { from, reason });
    }
    if let Err(error) = rename(directory, &new).await {
        return Some(Move::Kept { from, reason: error.to_string() });
    }
    // A job that names the earlier path goes on through it.
    #[cfg(unix)]
    if let Err(error) = tokio::fs::symlink(&new, directory).await {
        tracing::warn!(event = "rename.link_failed", %error, "the earlier path of the installation leads nowhere");
    }
    move_rest(&old, &new_root(home)).await;
    Some(Move::Moved { from, to: new })
}

/// Renames the installation's directory `from` to `to`. On Windows the
/// runner that started this one, whose files the directory holds, may still
/// be ending, and an antivirus may still read the new program: each refuses
/// the rename for a moment, with an error whose kind varies, so it is tried
/// for a while.
async fn rename(from: &Path, to: &Path) -> io::Result<()> {
    /// How long Windows' refusals are waited out, and how often it is asked.
    const FOR: std::time::Duration = std::time::Duration::from_secs(20);
    const EVERY: std::time::Duration = std::time::Duration::from_millis(500);
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let deadline = tokio::time::Instant::now() + FOR;
    loop {
        match tokio::fs::rename(from, to).await {
            Err(_) if cfg!(windows) && tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(EVERY).await;
            }
            result => return result,
        }
    }
}

/// Moves each entry of `old` but the directories of installations to `new`,
/// where none of its name is yet: the downloaded browsers, the plugins'
/// files and the like, which the device's installations share. On Unix each
/// leaves a link for the installations still under `old`; on Windows, which
/// makes none, an entry moves only once none is. An entry held open stays
/// for a later start.
async fn move_rest(old: &Path, new: &Path) {
    if cfg!(windows) && others_remain(old).await {
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(old).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let is_link = entry.file_type().await.is_ok_and(|kind| kind.is_symlink());
        if is_installations(&name) || is_link {
            continue;
        }
        let target = new.join(&name);
        if tokio::fs::symlink_metadata(&target).await.is_ok() {
            continue;
        }
        if let Err(error) = tokio::fs::rename(entry.path(), &target).await {
            tracing::warn!(event = "rename.entry_kept", entry = %entry.path().display(), %error, "an entry stays under the earlier name");
            continue;
        }
        #[cfg(unix)]
        if let Err(error) = tokio::fs::symlink(&target, entry.path()).await {
            tracing::warn!(event = "rename.link_failed", %error, "an entry's earlier path leads nowhere");
        }
    }
}

/// Whether an installation is still in `old`: a directory, not the link one
/// that moved leaves.
async fn others_remain(old: &Path) -> bool {
    for holder in [jush_runner_protocol::installation::ROOT[1], "instances"] {
        let Ok(mut entries) = tokio::fs::read_dir(old.join(holder)).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.is_ok_and(|kind| kind.is_dir()) {
                return true;
            }
        }
    }
    false
}

/// Removes the links [`move_rest`] left in `old` once no installation is
/// there to follow them.
async fn remove_links(old: &Path) {
    if others_remain(old).await {
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(old).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let is_link = entry.file_type().await.is_ok_and(|kind| kind.is_symlink());
        if is_link && !is_installations(&entry.file_name()) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Whether `name` in `~/.demi` holds installations, which each moves itself.
fn is_installations(name: &std::ffi::OsStr) -> bool {
    let runner = jush_runner_protocol::installation::ROOT[1];
    name == runner || name == "instances"
}

/// Ends the move of the installation in `directory`, which runs from its one
/// path under the new names: removes the earlier registration and runner,
/// the link at the earlier path once no job's shell runs, and `~/.demi` once
/// it holds nothing. Each runner's start tries what is left.
pub async fn finish(directory: &Path, home: &Path) {
    let earlier = directory.join(earlier_runner());
    if tokio::fs::symlink_metadata(&earlier).await.is_ok() {
        match crate::autostart::Registration::before_rename(directory, home) {
            Ok(registration) if registration.exists().await => registration.remove().await,
            Ok(_) => {}
            Err(error) => tracing::warn!(event = "rename.registration_kept", %error, "the earlier registration stays"),
        }
        if let Err(error) = tokio::fs::remove_file(&earlier).await {
            tracing::warn!(event = "rename.runner_kept", %error, "the earlier runner stays");
        }
    }
    let old = old_root(home);
    if tokio::fs::symlink_metadata(&old).await.is_err() {
        return;
    }
    let Ok(relative) = directory.strip_prefix(new_root(home)) else {
        return;
    };
    let link = old.join(relative);
    let is_link = tokio::fs::symlink_metadata(&link).await.is_ok_and(|metadata| metadata.is_symlink());
    if is_link && !shells_run(directory).await {
        let _ = tokio::fs::remove_file(&link).await;
    }
    move_rest(&old, &new_root(home)).await;
    remove_links(&old).await;
    // Only what is empty goes: another installation's directory stays.
    for name in jush_runner_protocol::installation::ROOT[1..].iter().copied().chain(["instances"]) {
        let _ = tokio::fs::remove_dir(old.join(name)).await;
    }
    let _ = tokio::fs::remove_dir(&old).await;
}

/// Whether a job's shell that a runner started may still run: the runner
/// records each in `shells/` until it has ended.
async fn shells_run(directory: &Path) -> bool {
    match tokio::fs::read_dir(directory.join("shells")).await {
        Ok(mut entries) => matches!(entries.next_entry().await, Ok(Some(_))),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The earlier runner's file name on this platform.
pub fn earlier_runner() -> String {
    if cfg!(windows) {
        format!("{}.exe", renamed::RUNNER)
    } else {
        renamed::RUNNER.to_owned()
    }
}
