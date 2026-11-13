//! The names a paired device's runner had before the product took the name
//! Jush, which an installation from before still uses until it moves.
//!
//! An earlier runner updates as it always did: it sends the release check
//! with these headers, downloads the next runner as `demi-runner`, and starts
//! it through its launcher, which sets these variables. The backend answers
//! the headers and the file name as the current ones, and the runner that
//! starts under them moves its installation to the new names
//! (`crates/runner/src/renamed.rs`).
//!
//! The names go with the other migrations when the history starts again at the
//! first formal release.

/// The directory under the user's home that held every installation.
pub const ROOT: &str = ".demi";

/// The runner executable's name, without `.exe`.
pub const RUNNER: &str = "demi-runner";

/// The variable the launcher set to the installation's directory.
pub const HOME_ENV: &str = "DEMI_HOME";

/// The variable the launcher set to the installation's release.
pub const RELEASE_ENV: &str = "DEMI_RELEASE_ID";

/// The variable the service manager's registration set on its runner.
pub const SUPERVISED: &str = "DEMI_SUPERVISED";

/// The release check's header that names the runner's release.
pub const RELEASE_HEADER: &str = "demi-runner-release";

/// The release check's header that names the runner's target.
pub const TARGET_HEADER: &str = "demi-runner-target";

/// The header that carries a paired runner's device token.
pub const TOKEN_HEADER: &str = "demi-runner-token";

/// The name of a LaunchAgent and of a scheduled task, before the
/// installation's id.
pub const LABEL_PREFIX: &str = "sh.demi.runner";

/// The name of a systemd user unit, before the installation's id.
pub const UNIT_PREFIX: &str = "demi-runner";
