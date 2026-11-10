//! `/api/storage`: what Demi keeps on the server and on each of the caller's
//! devices, and the rules that clean it up.

use demi_shared_types::{MAX_SAFE_INTEGER, Nullable, Timestamp};
use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{ConversationId, DeviceId};

/// `GET /storage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub server: ServerStorage,
    /// The rules for what Demi keeps on the caller's devices, one set for all
    /// of them, each applied on each device on its own.
    pub rules: StorageRules,
    /// Each of the caller's devices, its Cloud included.
    pub devices: Vec<DeviceStorage>,
    /// The caller's conversations the devices' reports name, by the name of
    /// their scratch directory.
    pub conversations: Vec<StorageConversation>,
}

/// What the server keeps. An administrator sees all of it and the log's
/// rules; anyone else sees only their own conversations and attachments,
/// and null for the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
#[garde(allow_unvalidated)]
pub struct ServerStorage {
    pub measured_at: Timestamp,
    /// The size and the free bytes of the volume that holds the server's
    /// data.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub disk_bytes: Option<u64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub disk_available_bytes: Option<u64>,
    /// The conversations' databases.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub conversations_bytes: u64,
    /// The attachments and the media made in conversations.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub attachments_bytes: u64,
    /// The Clouds' disks and the generations kept of them, which share their
    /// blocks, so one Cloud's part is no number of its own; null on a server
    /// without Clouds.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub clouds_bytes: Option<u64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub log_bytes: Option<u64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub search_bytes: Option<u64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<LogRetention>")]
    pub log: Option<LogRetention>,
}

/// How long the server keeps its log, and how much of it at most: `PUT
/// /storage/log`, which only an administrator may.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogRetention {
    #[garde(range(min = 1, max = 365))]
    pub keep_days: u16,
    #[garde(range(min = 1, max = MAX_SAFE_INTEGER))]
    pub max_bytes: u64,
}

impl Default for LogRetention {
    fn default() -> Self {
        Self { keep_days: 14, max_bytes: 2 * 1024 * 1024 * 1024 }
    }
}

/// The rules for what Demi keeps on a user's devices: `PUT /storage/rules`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageRules {
    /// What agents put in their scratch directories.
    #[garde(dive)]
    pub temporary: StorageRule,
    /// The conversations' browser profiles and recordings.
    #[garde(dive)]
    pub cache: StorageRule,
}

impl Default for StorageRules {
    fn default() -> Self {
        // A GB as the page shows sizes, 2^30 bytes.
        const GB: u64 = 1 << 30;
        Self {
            temporary: StorageRule { unused_days: Some(30), max_bytes: Some(20 * GB) },
            cache: StorageRule { unused_days: Some(30), max_bytes: Some(10 * GB) },
        }
    }
}

/// When one kind's files are removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageRule {
    /// A file unused for this many days goes; null keeps it however old.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u16>")]
    #[garde(inner(range(min = 1)))]
    pub unused_days: Option<u16>,
    /// The kind's files go from the one used longest ago while the kind
    /// takes more than this on a device; null sets no limit.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(min = 1, max = MAX_SAFE_INTEGER))]
    pub max_bytes: Option<u64>,
}

/// One device and what its runner last measured on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
#[garde(allow_unvalidated)]
pub struct DeviceStorage {
    pub device_id: DeviceId,
    /// Null before its runner first measured.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<DeviceStorageReport>")]
    pub report: Option<DeviceStorageReport>,
}

/// What Demi keeps on a device, as its runner last measured it. Sizes are the
/// bytes the files take on the disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
#[garde(allow_unvalidated)]
pub struct DeviceStorageReport {
    pub measured_at: Timestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub disk_bytes: Option<u64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub disk_available_bytes: Option<u64>,
    /// What Demi downloaded to run, its logs and its other state.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub components_bytes: u64,
    /// Each conversation's files, largest first.
    pub conversations: Vec<ConversationFiles>,
    /// The last cleanup that removed anything since the runner started.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<StorageCleanup>")]
    pub last_cleanup: Option<StorageCleanup>,
}

/// One conversation's files on a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
#[garde(allow_unvalidated)]
pub struct ConversationFiles {
    /// The name of its scratch directory, which [`Storage::conversations`]
    /// resolves.
    pub name: String,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub temporary_bytes: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub cache_bytes: u64,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<Timestamp>")]
    pub last_used_at: Option<Timestamp>,
}

/// A cleanup that removed something from a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(rename_all = "camelCase")]
#[garde(allow_unvalidated)]
pub struct StorageCleanup {
    pub at: Timestamp,
    pub cause: CleanupCause,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub removed_bytes: u64,
}

/// What started a cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CleanupCause {
    Rules,
    LowDisk,
    User,
}

/// One of the caller's conversations a device's report names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageConversation {
    pub name: String,
    pub id: ConversationId,
    pub title: String,
    pub archived: bool,
    /// Whether an agent of it works now, whose files only Clean Up removes.
    pub running: bool,
}

/// `POST /devices/{id}/storage`: what to remove before the device measures
/// again; answered with its new report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum StorageAction {
    /// Nothing: it measures only.
    Measure,
    /// Every conversation's temporary files not used in the last day.
    CleanUpTemporary,
    /// Every browser profile no browser runs on, and every recording not
    /// being written.
    ClearCache,
    /// Every downloaded component no running program holds.
    DeleteComponents,
    /// One conversation's files: all of them when `empty`, else those it did
    /// not use in the last day.
    #[serde(rename_all = "camelCase")]
    Conversation {
        #[garde(skip)]
        conversation: ConversationId,
        #[garde(skip)]
        empty: bool,
    },
}
