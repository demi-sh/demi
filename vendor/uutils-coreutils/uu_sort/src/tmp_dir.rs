// This file is part of the uutils coreutils package.
// See the distributed LICENSE for copyright and license information.

use std::path::PathBuf;
#[cfg(unix)]
use std::{fs::Permissions, os::unix::fs::OpenOptionsExt, os::unix::fs::PermissionsExt};
use tempfile::TempDir;
use uucore::context::fs::{File, OpenOptions};
use uucore::error::UResult;
use crate::SortError;

/// Owns one invocation's temporary directory and removes it on drop.
/// The runner owns the enclosing job directory for forced cancellation cleanup.
pub struct TmpDirWrapper {
    temp_dir: Option<TempDir>,
    parent_path: PathBuf,
    size: usize,
}

impl TmpDirWrapper {
    pub fn new(path: PathBuf) -> Self {
        Self {
            parent_path: uucore::context::resolve(&path),
            size: 0,
            temp_dir: None,
        }
    }

    pub fn next_file(&mut self) -> UResult<(File, PathBuf)> {
        if self.temp_dir.is_none() {
            let mut builder = tempfile::Builder::new();
            builder.prefix("uutils_sort");
            // The chunks hold a copy of the input, so keep them out of reach of other
            // users instead of relying on whatever umask the process inherited.
            #[cfg(unix)]
            builder.permissions(Permissions::from_mode(0o700));
            self.temp_dir = Some(builder.tempdir_in(&self.parent_path).map_err(|_| {
                SortError::TmpFileCreationFailed {
                    path: self.parent_path.clone(),
                }
            })?);
        }
        let path = self.temp_dir.as_ref().unwrap().path().join(self.size.to_string());
        self.size += 1;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // Restrict the chunks too, so a directory whose mode is later relaxed
        // doesn't expose them.
        #[cfg(unix)]
        options.mode(0o600);
        Ok((
            options
                .open(&path)
                .map_err(|error| SortError::OpenTmpFileFailed { error })?,
            path,
        ))
    }
}

impl Drop for TmpDirWrapper {
    fn drop(&mut self) {
        if let Some(directory) = self.temp_dir.take() {
            if let Err(error) = directory.close() {
                uucore::show_error!("failed to remove sort temporary directory: {error}");
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::TmpDirWrapper;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Restores the process umask on drop, so a panic in the test cannot leak the
    /// value into the rest of the binary.
    struct UmaskGuard(libc::mode_t);

    impl UmaskGuard {
        fn set(mask: libc::mode_t) -> Self {
            // SAFETY: umask(2) has no failure mode; it returns the previous value.
            Self(unsafe { libc::umask(mask) })
        }
    }

    impl Drop for UmaskGuard {
        fn drop(&mut self) {
            unsafe { libc::umask(self.0) };
        }
    }

    #[test]
    fn tmp_files_are_private_regardless_of_umask() {
        // Pin a permissive umask: under 0077 the umask alone would produce 0700 and
        // 0600, so the assertions would hold for a broken implementation too. The
        // guard restores it, and the only other test here that creates files sets
        // the modes it cares about explicitly.
        let _umask = UmaskGuard::set(0o022);

        let parent = tempfile::tempdir().unwrap();
        let mut wrapper = TmpDirWrapper::new(parent.path().to_owned());
        let (_file, path) = wrapper.next_file().unwrap();

        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
    }
}
