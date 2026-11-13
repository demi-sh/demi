//! What a Cloud image holds for the runner
//! (`cloud-guest-image/rootfs/packages.txt`): the image build writes it, the
//! manager's image check expects it, and the runner reads it
//! (`crates/runner-command-packages/src/cache.rs`).

/// Where an image embeds each command package's executable: alone in a
/// directory named by its SHA-256, under the name its release gives it, such
/// as `/opt/jush/artifacts/<sha256>/jush-file`.
pub const ARTIFACTS_PATH: &str = "/opt/jush/artifacts";
