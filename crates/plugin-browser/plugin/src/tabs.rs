//! The `browser` kind's tabs on the backend: the work shows exactly the
//! browser tabs the conversation's primary device lists.
//!
//! ```text
//!  close, + , a link's Open --call--> browser package on the device
//!                                            | the tabs, whole
//!                                            v
//!  work tabs <-- mirror_tabs <-- plugin <-- kept list (page::keep)
//! ```
//!
//! - Each list the browser package reports becomes the kind's work tabs, as
//!   one change (`PluginPort::mirror_tabs`): a work tab for each browser tab,
//!   its data as the list says it, and no work tab for a browser tab the list
//!   does not name; a tab the work has no room for waits for a later list.
//!   Nothing else adds or removes one; the page opens a tab by asking the
//!   device (`Work::open`), which answers once the work holds the tab.
//! - A work tab the user closes closes its browser tab. A device that cannot
//!   hear of it then closes it when it lists the tab again: the mirroring
//!   answers the listed tabs whose work tab the user closed.
//! - A device that cannot be reached changes no tab: the backend keeps its
//!   last list, which the work shows until the next one.
//! - When the package's state on the device ended unsaid (`package_lost`), as
//!   when the runner restarted, the last list stays. A look on the device
//!   brings the conversation to the next resident, which opens the browser's
//!   tabs again and lists them.
//!
//! # Rules
//!
//! - A tab id is used once per conversation, so a tab the user closed never
//!   comes back.
//! - A page's tab goes right after its opener, behind the tabs that opener
//!   opened before, as Chrome places them; the others go at the end.
//! - `shows` is the count of the times the agent, or the user's own Open,
//!   asked that the user see the tab, which the page applies once per rise.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use jush_command_package_browser_protocol::browser::{BrowserCreatedBy, BrowserTab};
use jush_plugin_interface::{PluginError, PluginPort};
use jush_web_api_protocol::ids::{ConversationId, DeviceId};
use jush_web_api_protocol::work::{CreateWorkTab, WorkTab};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::{Mutex, watch};

use crate::page::{self, TabOpened};
use crate::reason::ControlReason;

/// The kind whose tabs the plugin takes part in.
pub const KIND: &str = "browser";

/// What a `browser` tab keeps: the browser tab it shows, as the device last
/// listed it, so the strip names it before the page reads the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabData {
    /// The id of the conversation browser's tab it shows.
    tab: String,
    url: String,
    title: String,
    /// How many times the agent, or the user's own Open, asked that the
    /// user see it; absent while none did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shows: Option<u64>,
    /// The work tab whose page opened it, by a link or script of the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opened_by: Option<String>,
}

impl TabData {
    /// The data of `tab`; none for one that does not fit the kind.
    fn of(tab: &WorkTab) -> Option<Self> {
        serde_json::from_value(Value::Object(tab.data.clone())).ok()
    }
}

/// The id of the work tab for the browser's tab `tab`: a tab the user closed
/// is never added again, since an id is used once.
fn work_id(tab: &str) -> String {
    format!("browser-{tab}")
}

/// The browser's tab a work tab the plugin made shows.
fn browser_tab(work_tab: &str) -> Option<&str> {
    work_tab.strip_prefix("browser-")
}

/// One conversation's work at a time.
#[derive(Default)]
pub struct Work {
    locks: RefCell<HashMap<ConversationId, Rc<Mutex<()>>>>,
    /// Each conversation's last list the work followed, which an open waits
    /// on.
    followed: RefCell<HashMap<ConversationId, watch::Sender<Followed>>>,
}

/// A list of the primary device's the work followed: its number and the
/// browser tabs it named.
#[derive(Default)]
struct Followed {
    list: u64,
    tabs: HashSet<String>,
}

impl Work {
    fn lock(&self, conversation: &ConversationId) -> Rc<Mutex<()>> {
        self.locks
            .borrow_mut()
            .entry(conversation.clone())
            .or_default()
            .clone()
    }

    fn followed(&self, conversation: &ConversationId) -> watch::Sender<Followed> {
        self.followed
            .borrow_mut()
            .entry(conversation.clone())
            .or_insert_with(|| watch::channel(Followed::default()).0)
            .clone()
    }

    /// Opens a tab of the conversation's browser for the user, on `url` or
    /// blank, and answers it once the work holds it: once the device listed
    /// it and the work followed. Its opening on the page ends on every path:
    /// a tab the work has no room for closes again and is refused, and one
    /// that closed before the work held it is refused as gone.
    pub async fn open(
        &self,
        conversation: &ConversationId,
        port: &PluginPort,
        url: Option<String>,
    ) -> Result<TabOpened, PluginError> {
        // Followed from before the device opens the tab: a list the work
        // followed earlier may be of a browser before the one it opens in,
        // numbered apart from it.
        let mut followed = self.followed(conversation).subscribe();
        let opened = page::new_tab(port, url).await?;
        let list = opened.list.ok_or_else(|| {
            PluginError::failed("the device did not number the list its new tab is in")
        })?;
        let tab = opened.tab.to_string();
        loop {
            if followed.changed().await.is_err() {
                return Err(PluginError::failed("the plugin stopped following the tabs"));
            }
            let named = {
                let now = followed.borrow_and_update();
                if now.tabs.contains(&tab) {
                    true
                } else if now.list >= list {
                    false
                } else {
                    continue;
                }
            };
            if !named {
                return Err(page::refusal(
                    ControlReason::TabNotFound,
                    "the tab closed before the work held it".into(),
                ));
            }
            let id = work_id(&tab);
            if port.work_tabs().await?.tabs.iter().any(|held| held.id == id) {
                return Ok(TabOpened { tab, list });
            }
            // The list named it and the work left it out: a tab the user
            // cannot see does not stay open.
            close(port, &tab).await;
            return Err(page::refusal(
                ControlReason::WorkFull,
                "the conversation holds the most tabs it can".into(),
            ));
        }
    }

    /// Closes the browser's tab of a work tab its user removed. A device that
    /// cannot do it now closes it when it lists the tab again.
    pub async fn removed(
        &self,
        conversation: &ConversationId,
        port: &PluginPort,
        tab: &WorkTab,
    ) -> Result<(), PluginError> {
        let lock = self.lock(conversation);
        let _turn = lock.lock().await;
        if let Some(data) = TabData::of(tab) {
            close(port, &data.tab).await;
        }
        Ok(())
    }

    /// The package's state on a device ended unsaid. The primary device's
    /// last list stays; while it names tabs, a look on the device brings the
    /// conversation to the next resident, which opens them again and lists
    /// them. A device that cannot be reached now does when the next resident
    /// first hears of the conversation.
    pub async fn lost(&self, port: &PluginPort) -> Result<(), PluginError> {
        let Some(kept) = page::reported(port).await? else {
            return Ok(());
        };
        if kept.tabs.is_empty() {
            return Ok(());
        }
        if let Err(error) = page::look(port).await {
            tracing::info!(event = "browser_tabs.look_failed", %error, "the device did not take the conversation's tabs back now");
        }
        Ok(())
    }

    /// Keeps the browser's tabs `listed`, as the package on the device
    /// `device` reported them at `revision`, and makes the work's tabs those
    /// of the conversation's primary device: a work tab for each, placed
    /// beside its opener (`after_opened`) or at the end, and none for a tab
    /// the list does not name. A listed tab whose work tab the user closed is
    /// closed on the device.
    pub async fn sync(
        &self,
        conversation: &ConversationId,
        port: &PluginPort,
        device: &DeviceId,
        revision: u64,
        listed: Vec<BrowserTab>,
    ) -> Result<(), PluginError> {
        let lock = self.lock(conversation);
        let _turn = lock.lock().await;
        // Another device's report waits in the store: the work shows the
        // primary device's browser, and a conversation moved back finds it.
        if !page::keep(port, device, revision, listed.clone()).await? {
            return Ok(());
        }
        let work = port.work_tabs().await?;
        // The kind's tabs in order, as the mirroring leaves them.
        let mut order: Vec<Placed> = work.tabs.iter().map(Placed::of).collect();
        let shown: HashSet<String> = order.iter().filter_map(|placed| placed.shows.clone()).collect();
        let mut tabs = Vec::with_capacity(listed.len());
        for tab in &listed {
            let id = work_id(tab.id.as_str());
            let opened_by = opener(tab).map(work_id);
            let index = if shown.contains(tab.id.as_str()) {
                None
            } else {
                let index = opener_of(&order, tab).map(|at| after_opened(&order, at));
                let placed = Placed {
                    id: id.clone(),
                    shows: Some(tab.id.to_string()),
                    opened_by: opened_by.clone(),
                };
                order.insert(index.unwrap_or(order.len()), placed);
                index
            };
            tabs.push(CreateWorkTab {
                id,
                kind: KIND.into(),
                data: data(tab, opened_by),
                index,
            });
        }
        let closed = port.mirror_tabs(KIND, tabs).await?;
        self.followed(conversation).send_replace(Followed {
            list: revision,
            tabs: listed.iter().map(|tab| tab.id.to_string()).collect(),
        });
        for closed in closed {
            if let Some(tab) = browser_tab(&closed) {
                close(port, tab).await;
            }
        }
        Ok(())
    }
}

/// Closes the browser's tab `tab`. One the device cannot close now stays
/// until the device lists it again, when the mirroring answers it.
async fn close(port: &PluginPort, tab: &str) {
    if let Err(error) = page::close(port, tab).await {
        tracing::info!(event = "browser_tab.close_deferred", tab, %error, "a browser tab whose work tab was closed closes when its device lists it again");
    }
}

/// A work tab as the sync places others beside it: the browser tab it
/// shows, and the work tab that opened it.
struct Placed {
    id: String,
    shows: Option<String>,
    opened_by: Option<String>,
}

impl Placed {
    fn of(tab: &WorkTab) -> Self {
        let data = (tab.kind == KIND).then(|| TabData::of(tab)).flatten();
        Self {
            id: tab.id.clone(),
            shows: data.as_ref().map(|data| data.tab.clone()),
            opened_by: data.and_then(|data| data.opened_by),
        }
    }
}

/// The browser's tab whose page opened `tab`; none for a tab no page
/// opened.
fn opener(tab: &BrowserTab) -> Option<&str> {
    match &tab.created_by {
        BrowserCreatedBy::Page { opener } => Some(opener.as_str()),
        _ => None,
    }
}

/// Where in `order` the work tab stands that shows the browser tab whose
/// page opened `tab`; none for a tab no page opened, or whose opener the
/// work does not show.
fn opener_of(order: &[Placed], tab: &BrowserTab) -> Option<usize> {
    let opener = opener(tab)?;
    order
        .iter()
        .position(|placed| placed.shows.as_deref() == Some(opener))
}

/// Where a tab the work tab at `at` opened goes: right after it, behind the
/// tabs it opened before that still stand right after it, as Chrome places
/// the tabs a link opens.
fn after_opened(order: &[Placed], at: usize) -> usize {
    let opener = order[at].id.as_str();
    let run = order[at + 1..]
        .iter()
        .take_while(|placed| placed.opened_by.as_deref() == Some(opener))
        .count();
    at + 1 + run
}

/// The data of the work tab for the browser's `tab`, opened by the work tab
/// `opened_by`'s page.
fn data(tab: &BrowserTab, opened_by: Option<String>) -> Map<String, Value> {
    let data = TabData {
        tab: tab.id.to_string(),
        url: tab.url.clone(),
        title: tab.title.clone(),
        shows: (tab.shows > 0).then_some(tab.shows),
        opened_by,
    };
    let Ok(Value::Object(data)) = serde_json::to_value(data) else {
        unreachable!("a browser tab's data serializes to an object")
    };
    data
}
