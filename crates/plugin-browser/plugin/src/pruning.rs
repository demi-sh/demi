//! The pruning of the agents' tabs: once a conversation stops running and
//! stays stopped for [`PRUNE_AFTER`], its browser closes the tabs its agents
//! opened that the user did not make theirs (`browser.tab.prune`).
//!
//! The wait is the backend's, in memory: the device's browser knows nothing
//! of the agents' activity, and the backend decides when it goes. A restart
//! of the backend forgets the waits; the conversation's next stop sets its
//! wait again.

use std::{cell::RefCell, collections::HashMap, time::Duration};

use demi_plugin_interface::PluginPort;
use demi_web_api_protocol::ids::ConversationId;
use tokio_util::sync::CancellationToken;

/// How long a conversation stays stopped before its agents' tabs close: a
/// user who comes back to a finished turn within it still finds the pages
/// the agent left, and a conversation the user left alone does not keep a
/// tab bar full of them.
const PRUNE_AFTER: Duration = Duration::from_secs(30 * 60);

/// How long a tab a viewer watches waits before it is asked about again:
/// a page never closes while the user looks at it.
const WATCHED_RETRY: Duration = Duration::from_secs(60);

/// Each stopped conversation's wait.
#[derive(Default)]
pub(crate) struct Pruning {
    waits: RefCell<HashMap<ConversationId, CancellationToken>>,
}

impl Pruning {
    /// Follows `conversation`, which started (`running`) or stopped running:
    /// a start cancels its wait, a stop starts one, which this call holds
    /// until it ends.
    pub(crate) async fn changed(
        &self,
        conversation: &ConversationId,
        running: bool,
        port: &PluginPort,
    ) {
        let wait = CancellationToken::new();
        let previous = if running {
            self.waits.borrow_mut().remove(conversation)
        } else {
            self.waits
                .borrow_mut()
                .insert(conversation.clone(), wait.clone())
        };
        if let Some(previous) = previous {
            previous.cancel();
        }
        if running {
            return;
        }
        prune_after(port, &wait).await;
        // A wait that another change replaced was cancelled; one that was
        // not is still the conversation's.
        if !wait.is_cancelled() {
            self.waits.borrow_mut().remove(conversation);
        }
    }
}

/// Prunes the conversation's tabs after [`PRUNE_AFTER`], and every
/// [`WATCHED_RETRY`] after while a viewer watches one, unless `wait` is
/// cancelled first. A conversation whose browser has no tab to prune, as
/// most have none, asks its device nothing.
async fn prune_after(port: &PluginPort, wait: &CancellationToken) {
    let mut delay = PRUNE_AFTER;
    loop {
        tokio::select! {
            () = wait.cancelled() => return,
            () = tokio::time::sleep(delay) => {}
        }
        match crate::page::prunes_any(port).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::info!(event = "browser_tabs.prune_failed", %error, "the browser's tabs were not read");
                return;
            }
        }
        match crate::page::prune(port).await {
            Ok(pruned) if pruned.watched.is_empty() => return,
            Ok(_) => delay = WATCHED_RETRY,
            // A browser on a stopped Cloud or an offline device is out of
            // reach; its tabs wait for the conversation's next stop.
            Err(failure) => {
                tracing::info!(event = "browser_tabs.prune_failed", %failure, "the agents' tabs were not pruned");
                return;
            }
        }
    }
}
