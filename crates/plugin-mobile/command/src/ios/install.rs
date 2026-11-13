//! The iOS helper `jush-mobile` carries
//! (`crates/plugin-mobile/command/src/ios/mod.rs`): a tar archive compressed
//! with zstd, holding `jush-ios-sim`, the libraries it links, the guest bridge
//! and idb's license notice.
//!
//! The build embeds the archive. This module unpacks it on first use into
//! `<installation>/mobile/<sha256>/`, checked against the digest the build
//! recorded.

use std::path::PathBuf;

#[cfg(jush_ios_helper)]
use tokio_util::sync::CancellationToken;

use crate::driver::{DriverError, Result};

/// The helper's file in the archive.
#[cfg(jush_ios_helper)]
const ENTRY: &str = "jush-ios-sim";

#[cfg(jush_ios_helper)]
const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/jush-ios-sim.tar.zst"));
#[cfg(jush_ios_helper)]
const SHA256: &str = env!("JUSH_IOS_SIM_SHA256");

/// Whether this build carries the helper.
pub fn carried() -> bool {
    cfg!(jush_ios_helper)
}

/// The helper's executable, unpacked once into the installation.
pub async fn helper() -> Result<PathBuf> {
    #[cfg(not(jush_ios_helper))]
    {
        Err(DriverError::Failed("this build of jush-mobile carries no iOS helper".into()))
    }
    #[cfg(jush_ios_helper)]
    {
        let home = std::env::var_os("JUSH_HOME").ok_or_else(|| {
            DriverError::Failed("the resident was started without JUSH_HOME, where the iOS helper goes".into())
        })?;
        let root = PathBuf::from(home).join("mobile");
        let archive = jush_shared_artifacts::Archive {
            digest: jush_shared_artifacts::Digest {
                size: ARCHIVE.len() as u64,
                sha256: SHA256.to_owned(),
            },
            entry: ENTRY.to_owned(),
        };
        let cancel = CancellationToken::new();
        let failed = |error: jush_shared_artifacts::Error| {
            DriverError::Failed(format!("the iOS helper could not be unpacked: {error}"))
        };
        match jush_shared_artifacts::install_archive(&root, &archive, &cancel)
            .await
            .map_err(failed)?
        {
            jush_shared_artifacts::ArchiveInstall::Installed(entry) => Ok(entry),
            jush_shared_artifacts::ArchiveInstall::Unpack(unpacking) => {
                tokio::fs::write(unpacking.archive(), ARCHIVE)
                    .await
                    .map_err(|error| DriverError::Failed(format!("the iOS helper could not be written: {error}")))?;
                unpacking.finish(&cancel).await.map_err(failed)
            }
        }
    }
}
