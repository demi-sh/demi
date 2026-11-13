//! The Clouds of before the product took the name Jush, which keep their
//! home and move to the new names.
//!
//! - A manager of before the rename leaves its slots' interfaces, `demih*`,
//!   which hold the pool's routes, and its firewall table, `demi_cloud`, when
//!   the upgrade stops it. The manager's start removes them, before it checks
//!   that no host route overlaps the pool.
//! - A Cloud's home volume, the sandbox's `/home`, holds its user's directory
//!   as `demi`, where the sandbox now looks for `jush`, and in it the
//!   runner's plugin files and scratch as `.demi`, where the runner now looks
//!   for `.jush`: each boot renames them before the sandbox starts.
//! - A Cloud whose system lies on a base of before the rename, whose user is
//!   `demi` and whose programs lie in `/opt/demi`, is reset to the configured
//!   base at its next wake, keeping its home, as a base whose format a release
//!   ends is (`jush_machine_manager_protocol::image`): what was installed
//!   outside the home goes.
//!
//! It goes with the other migrations when the history starts again at the
//! first formal release.

use std::{io, path::Path};

/// The prefix of the host interfaces of a slot of before the rename.
pub const HOST_INTERFACES: &str = "demih";

/// The firewall table of before the rename.
pub const TABLE: &str = "demi_cloud";

/// The operation that resets a Cloud on a base of before the rename, once.
pub const RESET: &str = "jush-names";

/// The user's directory in a home volume of before the rename.
const EARLIER_HOME: &str = "demi";

/// The user's directory in a home volume.
const HOME: &str = "jush";

/// The runner's directory in the user's directory before the rename, and now.
const EARLIER_DATA: &str = ".demi";
const DATA: &str = ".jush";

/// Renames the user's directory in the home volume whose root is `home`, and
/// the runner's directory in it, each when it has the name of before the
/// rename and none of the new name.
pub fn move_home(home: &Path) -> io::Result<()> {
    let user = home.join(HOME);
    for (earlier, current) in [(home.join(EARLIER_HOME), user.clone()), (user.join(EARLIER_DATA), user.join(DATA))] {
        if earlier.is_dir() && !current.exists() {
            fs_err::rename(&earlier, &current)?;
            tracing::info!(event = "cloud.home_renamed", path = %current.display(), "a Cloud's home took Jush's name");
        }
    }
    Ok(())
}

/// Whether the base in `base`, a base's directory, is one of before the
/// rename.
pub fn is_earlier_base(base: &Path) -> bool {
    let root = base.join("rootfs/opt");
    !root.join(HOME).exists() && root.join(EARLIER_HOME).exists()
}
