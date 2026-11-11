//! The `mobile` kind's tabs on the backend (`crates/plugin-mobile/plugin/src/lib.rs`):
//! one tab for each simulator the conversation has open on its own device.
//!
//! - The work's tabs of the kind are the simulators the device lists as open,
//!   as one change (`PluginPort::mirror_tabs`): a tab is added when the agent
//!   or the user's Open opens the device, and removed when the device closes
//!   or is lost, that is, when the device no longer lists it as open. A
//!   resident that starts again, an update or a stopped device loses no tab: the
//!   device's resident takes its simulators back under their handles, and a stopped
//!   device's tabs stay as they were until it lists them again.
//! - Closing a tab closes its device, as `demi mobile close` does. A device
//!   that cannot hear of it then closes it when it lists the device again: the
//!   mirroring answers the listed devices whose tab the user closed.
//! - A simulator the agent opens on an attached device has no tab: the work shows
//!   the conversation's device's simulators alone.
//!
//! One conversation's work runs one piece at a time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use demi_plugin_interface::{PluginError, PluginPort};
use demi_web_api_protocol::ids::ConversationId;
use demi_web_api_protocol::work::{CreateWorkTab, WorkTab};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::page;

/// The kind whose tabs the plugin takes part in.
pub const KIND: &str = "mobile";

/// What a `mobile` tab keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabData {
    /// The device's handle, `m1`.
    device: String,
    /// Its simctl id or AVD name.
    native_id: String,
    /// Its name, which names the tab.
    name: String,
}

impl TabData {
    fn of(tab: &WorkTab) -> Option<Self> {
        (tab.kind == KIND)
            .then(|| serde_json::from_value(Value::Object(tab.data.clone())).ok())
            .flatten()
    }
}

/// The id of the tab the plugin adds for the device `device`: a handle is
/// given once in a conversation, so a closed tab is never added again.
pub(crate) fn added_id(device: &str) -> String {
    format!("mobile-{device}")
}

/// The device a tab the plugin added shows.
fn device_of(tab: &str) -> Option<&str> {
    tab.strip_prefix("mobile-")
}

/// One conversation's work at a time.
#[derive(Default)]
pub struct Work {
    locks: RefCell<HashMap<ConversationId, Rc<Mutex<()>>>>,
}

impl Work {
    fn lock(&self, conversation: &ConversationId) -> Rc<Mutex<()>> {
        self.locks
            .borrow_mut()
            .entry(conversation.clone())
            .or_default()
            .clone()
    }

    /// Closes the device of a tab its user closed. A device that cannot do it
    /// now closes it when it lists the device again.
    pub async fn removed(
        &self,
        conversation: &ConversationId,
        port: &PluginPort,
        tab: &WorkTab,
    ) -> Result<(), PluginError> {
        let lock = self.lock(conversation);
        let _turn = lock.lock().await;
        if let Some(data) = TabData::of(tab) {
            close(port, &data.device).await;
        }
        Ok(())
    }

    /// Reads the device's simulators and updates the work's tabs: a tab for each simulator
    /// this conversation has open that has none, and each tab whose device
    /// is no longer open removed.
    pub async fn sync(
        &self,
        conversation: &ConversationId,
        port: &PluginPort,
    ) -> Result<(), PluginError> {
        let lock = self.lock(conversation);
        let _turn = lock.lock().await;
        // A stopped device's simulators are as the window last showed them, until
        // it lists them again.
        let Some(listed) = page::list(port).await? else {
            return Ok(());
        };
        self.show(port, &listed.devices).await
    }

    /// Makes the work's tabs the simulators of `listed` this conversation has
    /// open, and closes each of them whose tab the user closed.
    async fn show(
        &self,
        port: &PluginPort,
        listed: &[demi_command_package_mobile_protocol::mobile::Simulator],
    ) -> Result<(), PluginError> {
        let tabs = listed
            .iter()
            .filter_map(|simulator| {
                let device = simulator.open.as_ref()?;
                let mut data = fields([
                    ("device", Value::from(device.as_str())),
                    ("nativeId", Value::from(simulator.native_id.clone())),
                    ("name", Value::from(simulator.name.clone())),
                    ("platform", Value::from(simulator.platform.to_string())),
                ]);
                // The model the driver reads, which the toolbar shows after the
                // name; a device whose driver knows none has none.
                if !simulator.model.is_empty() {
                    data.insert("model".into(), Value::from(simulator.model.clone()));
                }
                Some(CreateWorkTab {
                    id: added_id(device.as_str()),
                    kind: KIND.into(),
                    data,
                    index: None,
                })
            })
            .collect();
        for closed in port.mirror_tabs(KIND, tabs).await? {
            if let Some(device) = device_of(&closed) {
                close(port, device).await;
            }
        }
        Ok(())
    }
}

/// Closes the simulator `device`. One the device cannot close now stays
/// until the device lists it again, when the mirroring answers it.
async fn close(port: &PluginPort, device: &str) {
    if let Err(error) = page::close(port, device).await {
        tracing::info!(event = "mobile_device.close_deferred", device, %error, "a simulator whose tab was closed closes when its device lists it again");
    }
}

fn fields<const N: usize>(fields: [(&str, Value); N]) -> Map<String, Value> {
    fields
        .into_iter()
        .map(|(field, value)| (field.to_owned(), value))
        .collect()
}

