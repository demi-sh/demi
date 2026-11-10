//! What Demi keeps on the user's devices and on the server, as Settings ›
//! Storage shows it: the rules each device applies, the reports each runner
//! sends (`crates/runner/src/storage.rs`), and what the user removes by hand.

use std::collections::HashSet;

use demi_backend_database::StorageError;
use demi_backend_device_access::root_of;
use demi_command_protocol::scratch_name;
use demi_device_interface::DeviceError;
use demi_runner_protocol::wire;
use demi_web_api_protocol::ids::DeviceId;
use demi_web_api_protocol::storage::{
    CleanupCause, ConversationFiles, DeviceStorage, DeviceStorageReport, LogRetention, ServerStorage, Storage,
    StorageAction, StorageCleanup, StorageConversation, StorageRule, StorageRules,
};

use crate::shard::Shard;

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

impl Shard {
    /// The rules the user set for their devices, the defaults until they set
    /// any.
    pub(crate) async fn storage_rules(&self) -> Result<StorageRules, StorageError> {
        let preferences = self.services().control.preferences(self.user().clone()).await?;
        Ok(preferences.storage.unwrap_or_default())
    }

    /// Keeps the user's rules for their devices and tells every device that
    /// is connected; the others hear them in their next welcome.
    pub async fn set_storage_rules(&self, rules: StorageRules) -> Result<StorageRules, StorageError> {
        self.services()
            .control
            .patch_preferences(self.user().clone(), move |mut saved| {
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
    pub(crate) fn storage_reported(&self, device: &DeviceId, report: wire::StorageReport) {
        let control = self.services().control.clone();
        let device = device.clone();
        self.tasks().spawn_local(async move {
            if let Err(error) = control.set_device_storage(device.clone(), report_of(report)).await {
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
                let owned = self.services().control.conversation(conversation.clone()).await?;
                if owned.is_none_or(|record| &record.owner != self.user()) {
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

    /// What the server and each of the user's devices keep, and the rules.
    /// An administrator sees the whole server; anyone else sees their own.
    pub async fn storage(&self, administrator: bool) -> Result<Storage, StorageError> {
        let services = self.services();
        let rules = self.storage_rules().await?;
        let reports = services.control.device_storage(self.user().clone()).await?;
        let mut records = services.control.conversations(self.user().clone(), false).await?;
        records.extend(services.control.conversations(self.user().clone(), true).await?);
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
        let measured = services.server_storage.measured(services).await;
        let own_conversations: u64 = records
            .iter()
            .filter_map(|record| measured.conversations.get(record.id.as_str()))
            .sum();
        let own_attachments = measured.attachments.get(self.user().as_str()).copied().unwrap_or(0);
        let server = match administrator {
            true => ServerStorage {
                measured_at: measured.at,
                disk_bytes: measured.disk.map(|(total, _)| total),
                disk_available_bytes: measured.disk.map(|(_, available)| available),
                conversations_bytes: measured.conversations.values().sum(),
                attachments_bytes: measured.attachments.values().sum(),
                clouds_bytes: measured.clouds,
                log_bytes: Some(measured.log),
                search_bytes: Some(measured.search),
                log: Some(services.control.log_retention().await?),
            },
            false => ServerStorage {
                measured_at: measured.at,
                disk_bytes: None,
                disk_available_bytes: None,
                conversations_bytes: own_conversations,
                attachments_bytes: own_attachments,
                clouds_bytes: None,
                log_bytes: None,
                search_bytes: None,
                log: None,
            },
        };
        let devices = reports
            .into_iter()
            .map(|(device_id, report)| DeviceStorage { device_id, report: Some(report) })
            .collect();
        Ok(Storage { server, rules, devices, conversations })
    }

    /// Keeps how long and how much of its log the server keeps, and applies
    /// it from the log's next trim.
    pub async fn set_log_retention(&self, retention: LogRetention) -> Result<LogRetention, StorageError> {
        let services = self.services();
        services.control.set_log_retention(retention).await?;
        services.log.set_limits(retention.into());
        Ok(retention)
    }

    fn conversation_running(&self, conversation: &demi_web_api_protocol::ids::ConversationId) -> bool {
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
