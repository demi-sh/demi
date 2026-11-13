//! A server installed before the product took the name Jush moves to the new
//! names as the first Jush release's move begins.
//!
//! The installed `demi-server` fetches the release under the asset names of
//! then, which every release also publishes, and hands the move over to the
//! release's `bin/demi-server`, a link to `jush-server`
//! (`crates/xtask/src/server_release.rs`). Before anything else that run moves
//! the installation:
//!
//! ```text
//! demi-backend, demi-machine-manager, demi-*.path   stopped, units removed
//! /run/demi-*                                       unmounted, removed
//! /opt/demi  (Cloud store unmounted)            ->  /opt/jush, fstab rewritten
//! config/demi.env  DEMI_*, /opt/demi            ->  config/jush.env  JUSH_*, /opt/jush
//! user demi, groups demi and demi-cloud         ->  jush, jush, jush-cloud (same ids)
//! /usr/local/bin/demi-server                    ->  /usr/local/bin/jush-server
//! ```
//!
//! Each step looks at what is there before it acts, so a run that stops
//! anywhere, a failed mount or a lost power, is taken up by the next, which
//! the release's own `jush-server move` starts while anything of the earlier
//! names is left (`pending`).
//!
//! The move then goes on as every move does, from the release's own units.
//! It cannot return to the release it left, whose programs know only the
//! earlier names: a release that does not start stays, and says why.
//!
//! The Clouds' runtime leftovers under the earlier names, their interfaces
//! and firewall table, go with the machine manager's start
//! (`crates/machine-manager/src/renamed.rs`).
//!
//! It goes with the other migrations when the history starts again at the
//! first formal release.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use semver::Version;

use crate::layout::Layout;

/// The installation's directory before the rename.
const EARLIER: &str = "opt/demi";
/// The installation's directory.
const CURRENT: &str = "opt/jush";

/// The units of before the rename, path units first, so none starts a run.
const EARLIER_UNITS: [&str; 6] = [
    "demi-upgrade.path",
    "demi-prepare.path",
    "demi-upgrade.service",
    "demi-prepare.service",
    "demi-backend.service",
    "demi-machine-manager.service",
];

/// The runtime directories' prefix under `/run` before the rename.
const EARLIER_RUNTIME: &str = "demi-";

/// The machine manager's saved mount namespace, a mount of its own under
/// its runtime directory.
const EARLIER_NAMESPACE: &str = "run/demi-machine-manager/mount-namespace";

/// The configuration's file name before the rename.
const EARLIER_CONFIG: &str = "demi.env";

/// The link to the current release's program before the rename.
const EARLIER_PROGRAM: &str = "usr/local/bin/demi-server";

/// The comment above the Cloud store's `/etc/fstab` line before the rename.
const EARLIER_STORE_COMMENT: &str = "# Demi's Cloud store, which shares extents (demi-server)";

/// Whether `version`'s release runs in an installation under Jush's names:
/// one of before the rename has no `jush-server`.
pub fn runs_here(layout: &Layout, version: &Version) -> bool {
    layout.release(version).join("bin/jush-server").exists()
}

/// Whether the installation under `layout` still has anything of the names
/// of before the rename, which [`migrate`] moves, each in its turn: its
/// units, its directory, its configuration, and its program's link last.
pub fn pending(layout: &Layout) -> bool {
    let root = layout.root();
    let earlier_units = EARLIER_UNITS.iter().any(|unit| layout.units().join(unit).exists());
    earlier_units
        || root.join(EARLIER).is_dir()
        || layout.config().with_file_name(EARLIER_CONFIG).exists()
        || root.join(EARLIER_PROGRAM).symlink_metadata().is_ok()
}

/// The installation's lock, which no run handed over to this one, where the
/// installation is now: it stays the installation's lock once it moved.
pub fn lock(layout: &Layout) -> io::Result<std::fs::File> {
    let earlier = layout.root().join(EARLIER);
    let state = if earlier.is_dir() { earlier.join("server") } else { layout.state() };
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("lock"))?;
    lock.lock()?;
    Ok(lock)
}

/// Moves the installation under `layout` to the new names, for the move to
/// `to`, whose units it installs.
pub fn migrate(layout: &Layout, to: &Version) -> io::Result<()> {
    let root = layout.root();
    println!("Moving the installation to Jush's names");
    let units = layout.units();
    // The services stop as every move stops them, the manager saving every
    // running Cloud; nothing moves until they have. This run is no unit's
    // but the one an administrator's update started, which ends with it.
    for unit in EARLIER_UNITS {
        if !units.join(unit).exists() {
            continue;
        }
        if unit.ends_with(".path") {
            let _ = systemctl(&["stop", unit]);
        } else if unit != "demi-upgrade.service" {
            systemctl(&["stop", unit])?;
        }
    }
    for unit in EARLIER_UNITS {
        if units.join(unit).exists() {
            let _ = systemctl(&["disable", unit]);
            remove_if_present(&units.join(unit))?;
        }
    }
    systemctl(&["daemon-reload"])?;
    let namespace = root.join(EARLIER_NAMESPACE);
    if namespace.exists() {
        let _ = run("umount", Command::new("umount").arg("--lazy").arg(&namespace));
    }
    if let Ok(entries) = std::fs::read_dir(root.join("run")) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(EARLIER_RUNTIME) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    move_directory(root)?;
    rename_accounts()?;
    rewrite_config(layout, to)?;
    // A link into `/opt/demi`, which is gone, so it is looked at, not
    // followed.
    let installed = root.join(EARLIER_PROGRAM);
    if installed.symlink_metadata().is_ok() {
        crate::setup::link_program(layout)?;
        remove_if_present(&installed)?;
    }
    layout.install_units(to)?;
    systemctl(&["daemon-reload"])?;
    Ok(())
}

/// Renames the installation's directory with the Cloud store unmounted,
/// rewrites the store's `/etc/fstab` line for the new place, and mounts the
/// store there unless it is mounted: a store that would not unmount moved
/// with the directory.
fn move_directory(root: &Path) -> io::Result<()> {
    let fstab = root.join("etc/fstab");
    let earlier = format!("/{EARLIER}/");
    let current = format!("/{CURRENT}/");
    let table = match std::fs::read_to_string(&fstab) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        read => read?,
    };
    let at = |path: &str| root.join(path.trim_start_matches('/'));
    if root.join(EARLIER).is_dir() && !root.join(CURRENT).exists() {
        for (_, point) in mounts(&table, &earlier) {
            let _ = run("umount", Command::new("umount").arg(at(&point)));
        }
        std::fs::rename(root.join(EARLIER), root.join(CURRENT))?;
    }
    // The link to the current release names its place in full, so that the
    // programs, and a run that takes this one up, are found at the new one.
    let link = root.join(CURRENT).join("current");
    if let Ok(target) = std::fs::read_link(&link)
        && let Ok(rest) = target.strip_prefix(format!("/{EARLIER}"))
    {
        let staged = link.with_extension("jush");
        let _ = std::fs::remove_file(&staged);
        std::os::unix::fs::symlink(Path::new("/").join(CURRENT).join(rest), &staged)?;
        std::fs::rename(&staged, &link)?;
    }
    let rewritten: String = table
        .lines()
        .map(|line| {
            if line == EARLIER_STORE_COMMENT {
                crate::store::FSTAB_COMMENT.to_owned()
            } else if mounts(line, &earlier).next().is_some() {
                line.replace(&earlier, &current)
                    .replace("demi-machine-manager.service", "jush-machine-manager.service")
            } else {
                line.to_owned()
            }
        })
        .map(|line| line + "\n")
        .collect();
    if rewritten != table {
        let staged = fstab.with_extension("jush");
        std::fs::write(&staged, &rewritten)?;
        std::fs::rename(&staged, &fstab)?;
        systemctl(&["daemon-reload"])?;
    }
    for (device, point) in mounts(&rewritten, &current) {
        if Command::new("mountpoint").arg("-q").arg(at(&point)).status().is_ok_and(|status| status.success()) {
            continue;
        }
        // The store keeps its contents; its label follows the name.
        if device.starts_with(&current) {
            let _ = run("xfs_admin", Command::new("xfs_admin").args(["-L", "jush-cloud"]).arg(at(&device)));
        }
        run("mount", Command::new("mount").arg(at(&point)))?;
    }
    Ok(())
}

/// The device and mount point of each line of `table` that mounts under
/// `directory`, as the Cloud store's does; a comment mounts nothing.
fn mounts<'a>(table: &'a str, directory: &'a str) -> impl Iterator<Item = (String, String)> + 'a {
    table.lines().filter_map(move |line| {
        if line.trim_start().starts_with('#') {
            return None;
        }
        let mut fields = line.split_whitespace();
        let device = fields.next()?;
        let point = fields.next()?;
        point.starts_with(directory).then(|| (device.to_owned(), point.to_owned()))
    })
}

/// Writes the configuration under its new name: each `DEMI_` setting as
/// `JUSH_`, and each path of before the rename at its new place. The earlier
/// file stays beside it as `jush.env.before-<version>`, as a migration of
/// the configuration keeps it, and goes once the new one is whole.
fn rewrite_config(layout: &Layout, to: &Version) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let config = layout.config();
    let earlier = config.with_file_name(EARLIER_CONFIG);
    let text = match std::fs::read_to_string(&earlier) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        read => read?,
    };
    let rewritten: String = text
        .lines()
        .map(|line| {
            let line = match line.strip_prefix("DEMI_") {
                Some(rest) => format!("JUSH_{rest}"),
                None => line.to_owned(),
            };
            line.replace("/opt/demi", "/opt/jush").replace("/run/demi-", "/run/jush-") + "\n"
        })
        .collect();
    let kept = config.with_file_name(format!("jush.env.before-{to}"));
    if !kept.exists() {
        std::fs::copy(&earlier, &kept)?;
    }
    // A copy of the earlier file keeps its mode, and is given its owner.
    let staged = config.with_extension("env.new");
    std::fs::copy(&earlier, &staged)?;
    let metadata = std::fs::metadata(&earlier)?;
    std::os::unix::fs::chown(&staged, Some(metadata.uid()), Some(metadata.gid()))?;
    std::fs::write(&staged, rewritten)?;
    std::fs::rename(&staged, &config)?;
    std::fs::remove_file(&earlier)
}

/// Renames the services' user and groups, which keep their ids, so every
/// file keeps its owner.
fn rename_accounts() -> io::Result<()> {
    let exists = |kind: &str, name: &str| {
        Command::new("getent").args([kind, name]).output().is_ok_and(|output| output.status.success())
    };
    if exists("passwd", "demi") && !exists("passwd", "jush") {
        run("usermod", Command::new("usermod").args(["--login", "jush", "--home", "/opt/jush", "demi"]))?;
    }
    for (earlier, current) in [("demi", "jush"), ("demi-cloud", "jush-cloud")] {
        if exists("group", earlier) && !exists("group", current) {
            run("groupmod", Command::new("groupmod").args(["--new-name", current, earlier]))?;
        }
    }
    Ok(())
}

fn systemctl(args: &[&str]) -> io::Result<()> {
    run("systemctl", Command::new("systemctl").args(args))
}

fn run(program: &str, command: &mut Command) -> io::Result<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn remove_if_present(path: &PathBuf) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
