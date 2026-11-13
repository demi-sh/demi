//! The documents a conversation's main agent pinned, which come back to it
//! after each compaction; the page shows each as a chip over the composer,
//! and the user unpins one (`DELETE /conversations/:id/pins/:pin`).

use jush_shared_types::MAX_SAFE_INTEGER;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::DeviceId;

/// A pinned document: a file of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PinnedDocument {
    /// What unpins it.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub id: u64,
    pub device_id: DeviceId,
    /// What the user calls the device.
    pub device: String,
    /// Its absolute path on the device.
    pub path: String,
}
