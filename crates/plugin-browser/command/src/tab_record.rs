//! What a conversation's browser has open, kept in the conversation's own
//! profile so the next resident opens it again; a browser that runs on a
//! profile the conversation names keeps it there too, since a named profile
//! holds several conversations' browsers.
//!
//! A conversation's browser ends by itself when its last tab closes, when the
//! conversation is released, or when Chrome is lost; its tabs end with it. A
//! resident that ends, or hands the conversation's profile to another
//! resident (an upgrade's next release), does not end the browser's tabs: the
//! next resident to reach the conversation opens them again, under the same
//! ids and at the addresses they showed, as Chrome reopens its tabs when it
//! relaunches to update. Each page loads anew.
//!
//! ```text
//! <scratch>/<conversation>/.browser/   the conversation's profile
//!   tabs.json                          this record, while the browser has tabs
//! ```
//!
//! - The browser's owner writes the record each time the tab list changes
//!   while the browser runs, replacing the file whole, so a resident that is
//!   killed leaves the list as it last was; while the browser has no tab to
//!   open again, there is no record.
//! - The owner removes it when the browser ends by itself. The runner removes
//!   it at the conversation's release too, which reaches only the residents
//!   that run (`demi_command_protocol::browser_tab_record`).
//! - While a record waits for the next resident, the resident that left it
//!   reports nothing of the conversation's tabs: the plugin keeps showing
//!   them until the next resident lists them.
//! - A record that does not read is removed and opens nothing.

use std::path::{Path, PathBuf};

use demi_command_package_browser_chrome::tabs::environment::private_directory_all;
use demi_command_package_browser_chrome::tabs::registry::Snapshot;
use demi_command_package_browser_protocol::browser::{BrowserCreatedBy, TabId};
use demi_command_protocol::{ColorScheme, CommandLocale};
use serde::{Deserialize, Serialize};

/// A conversation browser's open tabs, in the order it made them, and how
/// its Chrome was started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TabRecord {
    pub(crate) locale: CommandLocale,
    pub(crate) color_scheme: ColorScheme,
    pub(crate) tabs: Vec<RecordedTab>,
    /// The profile its Chrome ran on, when the conversation named one; none
    /// for the conversation's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) profile: Option<PathBuf>,
    /// How many times the conversation had changed its profile then.
    #[serde(default)]
    pub(crate) revision: u64,
}

/// One tab as it is opened again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecordedTab {
    pub(crate) id: TabId,
    /// The address the tab showed: the one it loaded while a navigation the
    /// user started had not committed, otherwise its page's.
    pub(crate) url: String,
    pub(crate) created_by: BrowserCreatedBy,
    /// How many times the agent showed it, which the tab opened again keeps.
    pub(crate) shows: u64,
    /// The user acted in it, which keeps it from being pruned.
    pub(crate) used: bool,
}

impl TabRecord {
    /// The record of `listing`, a browser started in `locale` and
    /// `color_scheme` on `profile` at `revision`; none for a browser without
    /// tabs to open again. A temporary command's tab is not one: it closes as
    /// its command ends, and the next resident runs no command of this one's.
    pub(crate) fn of(
        listing: &Snapshot,
        locale: &CommandLocale,
        color_scheme: ColorScheme,
        profile: Option<PathBuf>,
        revision: u64,
    ) -> Option<Self> {
        let tabs: Vec<RecordedTab> = listing
            .tabs
            .iter()
            .filter(|listed| !matches!(listed.tab.created_by(), BrowserCreatedBy::Temporary { .. }))
            .map(|listed| RecordedTab {
                id: listed.tab.id().clone(),
                url: listed.shown().1,
                created_by: listed.tab.created_by().clone(),
                shows: listed.tab.shows(),
                used: listed.tab.used(),
            })
            .collect();
        (!tabs.is_empty()).then(|| Self {
            locale: locale.clone(),
            color_scheme,
            tabs,
            profile,
            revision,
        })
    }

    /// The record at `path`; none when there is none. One that does not read
    /// is removed.
    pub(crate) fn read(path: &Path) -> Option<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(event = "browser_tab_record.read_failed", %error, "the browser's tab record could not be read");
                return None;
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(record) => Some(record),
            Err(error) => {
                tracing::warn!(event = "browser_tab_record.read_failed", %error, "the browser's tab record does not read, and is removed");
                forget(path);
                None
            }
        }
    }

    /// Writes the record at `path`, in place of the one there, through a file
    /// beside it, so a record is never read half written. A write that fails
    /// leaves the one before and is logged.
    pub(crate) fn write(&self, path: &Path) {
        let staged = staged(path);
        let written = serde_json::to_vec(self)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                // The conversation's own profile, which no Chrome made while
                // its browser runs on a profile it names.
                if let Some(directory) = path.parent() {
                    private_directory_all(directory)?;
                }
                std::fs::write(&staged, bytes)
            })
            .and_then(|()| std::fs::rename(&staged, path));
        if let Err(error) = written {
            tracing::warn!(event = "browser_tab_record.write_failed", %error, "the browser's tab record could not be written");
        }
    }
}

/// Removes the record at `path`, if there is one.
pub(crate) fn forget(path: &Path) {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!(event = "browser_tab_record.remove_failed", %error, "the browser's tab record could not be removed");
        }
        _ => {}
    }
}

fn staged(path: &Path) -> PathBuf {
    let mut staged = path.as_os_str().to_owned();
    staged.push(".new");
    PathBuf::from(staged)
}
