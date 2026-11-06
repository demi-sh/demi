//! What Demi keeps on the user's devices, as Settings › Storage shows it: the
//! rules each device applies, the reports each runner sends
//! (`crates/runner/src/storage.rs`), and what the user removes by hand. What
//! the server keeps is the server's to add.

use std::collections::HashSet;

use demi_command_protocol::scratch_name;
use demi_core_device_access::root_of;
use demi_core_store::StorageError;
use demi_device_interface::DeviceError;
use demi_runner_protocol::wire;
use demi_web_api_protocol::ids::{ConversationId, DeviceId};
use demi_web_api_protocol::storage::{
    CleanupCause, ConversationFiles, DeviceStorage, DeviceStorageReport, StorageAction, StorageCleanup,
    StorageConversation, StorageRule, StorageRules,
};

use crate::Core;

/// Why a device's storage request was not done.
#[derive(Debug, thiserror::Error)]
pub enum StorageRequestError {
    #[error("the device is offline")]
    Offline,
    #[error("no such conversation")]
    UnknownConversation,
    #[error(transparent)]
    Device(#[from] DeviceError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// What the core keeps track of for Settings › Storage.
pub struct CoreStorage {
    pub rules: StorageRules,
    pub devices: Vec<DeviceStorage>,
    /// The conversations whose files a device reported.
    pub conversations: Vec<StorageConversation>,
    /// Every conversation of the core, archived or not.
    pub owned: Vec<ConversationId>,
}

impl Core {
    /// The rules the user set for their devices, the defaults until they set
    /// any.
    pub async fn storage_rules(&self) -> Result<StorageRules, StorageError> {
        let preferences = self.store().preferences().await?;
        Ok(preferences.storage.unwrap_or_default())
    }

    /// Keeps the user's rules for their devices and tells every device that
    /// is connected; the others hear them in their next welcome.
    pub async fn set_storage_rules(&self, rules: StorageRules) -> Result<StorageRules, StorageError> {
        self.store()
            .patch_preferences(move |mut saved| {
                saved.storage = Some(rules);
                saved
            })
            .await?;
        for (device, link) in self.devices().links() {
            if let Err(error) = link.set_storage_rules(wire_rules(rules)) {
                // A connection that closed hears them in its next welcome.
                tracing::info!(event = "storage_rules.unsent", device_id = %device, %error, "a device did not hear its new storage rules");
            }
        }
        Ok(rules)
    }

    /// Keeps what a device's runner measured.
    pub fn storage_reported(&self, device: &DeviceId, report: wire::StorageReport) {
        let store = self.store().clone();
        let device = device.clone();
        self.tasks().spawn_local(async move {
            if let Err(error) = store.set_device_storage(device.clone(), report_of(report)).await {
                tracing::warn!(event = "device_storage.store_failed", device_id = %device, error = &error as &dyn std::error::Error, "a device's storage report was not kept");
            }
        });
    }

    /// Has `device` remove what `action` names and measure again; its new
    /// report, which its runner's answer also keeps.
    pub async fn device_storage(
        &self,
        device: &DeviceId,
        action: StorageAction,
    ) -> Result<DeviceStorageReport, StorageRequestError> {
        let action = match action {
            StorageAction::Measure => wire::StorageAction::Measure {},
            StorageAction::CleanUpTemporary => wire::StorageAction::CleanUpTemporary {},
            StorageAction::ClearCache => wire::StorageAction::ClearCache {},
            StorageAction::DeleteComponents => wire::StorageAction::DeleteComponents {},
            StorageAction::Conversation { conversation, empty } => {
                if self.store().conversation(conversation.clone()).await?.is_none() {
                    return Err(StorageRequestError::UnknownConversation);
                }
                // A conversation an agent works in loses only what it did not
                // use in the last day.
                let empty = empty && !self.conversation_running(&conversation);
                wire::StorageAction::Conversation { conversation: conversation.to_string(), empty }
            }
        };
        let link = self.devices().link(device).ok_or(StorageRequestError::Offline)?;
        Ok(report_of(link.storage(action).await?))
    }

    /// What each of the user's devices keeps, the rules, and the
    /// conversations.
    pub async fn storage(&self) -> Result<CoreStorage, StorageError> {
        let rules = self.storage_rules().await?;
        let reports = self.store().device_storage().await?;
        let mut records = self.store().conversations(false).await?;
        records.extend(self.store().conversations(true).await?);
        let named: HashSet<&str> = reports
            .iter()
            .flat_map(|(_, report)| report.conversations.iter().map(|files| files.name.as_str()))
            .collect();
        let conversations = records
            .iter()
            .filter(|record| named.contains(scratch_name(record.id.as_str()).as_str()))
            .map(|record| StorageConversation {
                name: scratch_name(record.id.as_str()),
                id: record.id.clone(),
                title: record.title.clone().unwrap_or_default(),
                archived: record.archived_at.is_some(),
                running: self.conversation_running(&record.id),
            })
            .collect();
        let devices = reports
            .into_iter()
            .map(|(device_id, report)| DeviceStorage { device_id, report: Some(report) })
            .collect();
        let owned = records.into_iter().map(|record| record.id).collect();
        Ok(CoreStorage { rules, devices, conversations, owned })
    }

    fn conversation_running(&self, conversation: &ConversationId) -> bool {
        self.agent().tree(&root_of(conversation)).is_some_and(|tree| tree.is_running())
    }
}

/// The rules as the runner wire carries them.
pub(crate) fn wire_rules(rules: StorageRules) -> wire::StorageRules {
    let rule = |rule: StorageRule| wire::StorageRule { unused_days: rule.unused_days, max_bytes: rule.max_bytes };
    wire::StorageRules { temporary: rule(rules.temporary), cache: rule(rules.cache) }
}

fn report_of(report: wire::StorageReport) -> DeviceStorageReport {
    let time = |time: wire::Timestamp| {
        demi_shared_types::Timestamp::from_millisecond(time.0).unwrap_or(demi_shared_types::Timestamp::UNIX_EPOCH)
    };
    DeviceStorageReport {
        measured_at: time(report.measured_at),
        disk_bytes: report.disk,
        disk_available_bytes: report.disk_available,
        components_bytes: report.components,
        conversations: report
            .conversations
            .into_iter()
            .map(|files| ConversationFiles {
                name: files.name,
                temporary_bytes: files.temporary,
                cache_bytes: files.cache,
                last_used_at: files.last_used.map(time),
            })
            .collect(),
        last_cleanup: report.last_cleanup.map(|cleanup| StorageCleanup {
            at: time(cleanup.at),
            cause: match cleanup.cause {
                wire::CleanupCause::Rules => CleanupCause::Rules,
                wire::CleanupCause::LowDisk => CleanupCause::LowDisk,
                wire::CleanupCause::User => CleanupCause::User,
            },
            removed_bytes: cleanup.removed_bytes,
        }),
    }
}
