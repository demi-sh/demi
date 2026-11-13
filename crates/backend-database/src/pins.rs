//! The documents a conversation's main agent pinned
//! (`crates/backend-user-shard/src/conversation/pins.rs`).
//!
//! A file is pinned once at a time, and the pinned documents of a
//! conversation hold at most a number of tokens together, which a pin checks
//! in the transaction that writes it. A pin's id counts within its
//! conversation and is never taken again; a Fork copies the rows with their
//! ids, which the transcript it copies names. A row stays once unpinned, so
//! the agent can be told of an unpin by the user, which `told_at` records.

use jush_shared_types::Timestamp;
use jush_web_api_protocol::ids::{ConversationId, DeviceId};
use rusqlite::{Connection, OptionalExtension as _, Row, params};
use serde::{Deserialize, Serialize};

use super::StorageError;
use super::columns::{decode, instant};
use super::control::ControlService;

const TABLE: &str = "conversation_pins";

/// Jush's estimate of the text column's tokens in SQL, as `text_tokens` makes
/// it: its UTF-8 bytes divided by 4, rounded up.
const TOKENS: &str = "(length(CAST(text AS BLOB)) + 3) / 4";

/// Who unpinned a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unpinner {
    Agent,
    User,
}

impl Unpinner {
    fn column(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::User => "user",
        }
    }
}

/// A document the main agent pinned, without its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRecord {
    pub id: u64,
    pub device_id: DeviceId,
    /// Its absolute path on that device.
    pub path: String,
    pub pinned_at: Timestamp,
    /// The tokens of its text as last read.
    pub tokens: u64,
    /// Who unpinned it, once it is unpinned.
    pub unpinned_by: Option<Unpinner>,
    /// Whether the agent was told that the user unpinned it.
    pub told: bool,
}

/// A row of a Fork's source, which its destination is published with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PinCopy {
    pub id: u64,
    pub device_id: DeviceId,
    pub path: String,
    pub pinned_at: Timestamp,
    pub text: String,
    pub read_at: Timestamp,
    #[serde(deserialize_with = "Option::deserialize")]
    pub unpinned_at: Option<Timestamp>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub unpinned_by: Option<Unpinner>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub told_at: Option<Timestamp>,
}

/// What a pin did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pinned {
    /// The document is pinned, as the row of this id.
    Pinned(u64),
    /// The file is pinned already.
    Already,
    /// Its text would take the pinned documents past their limit, which hold
    /// this many tokens already.
    Over { pinned: u64 },
    /// The conversation is gone.
    Gone,
}

fn unpinner(row: &Row<'_>) -> Result<Option<Unpinner>, StorageError> {
    match row.get::<_, Option<String>>("unpinned_by")?.as_deref() {
        None => Ok(None),
        Some("agent") => Ok(Some(Unpinner::Agent)),
        Some("user") => Ok(Some(Unpinner::User)),
        Some(other) => decode(TABLE, "unpinned_by", Err(format!("not an unpinner: {other}"))),
    }
}

fn optional_instant(row: &Row<'_>, column: &'static str) -> Result<Option<Timestamp>, StorageError> {
    match row.get::<_, Option<i64>>(column)? {
        None => Ok(None),
        Some(_) => instant(row, TABLE, column).map(Some),
    }
}

fn id(row: &Row<'_>) -> Result<u64, StorageError> {
    decode(TABLE, "id", u64::try_from(row.get::<_, i64>("id")?))
}

fn device(row: &Row<'_>) -> Result<DeviceId, StorageError> {
    decode(TABLE, "device_id", DeviceId::try_from(row.get::<_, String>("device_id")?))
}

fn record(row: &Row<'_>) -> Result<PinRecord, StorageError> {
    Ok(PinRecord {
        id: id(row)?,
        device_id: device(row)?,
        path: row.get("path")?,
        pinned_at: instant(row, TABLE, "pinned_at")?,
        tokens: decode(TABLE, "text", u64::try_from(row.get::<_, i64>("tokens")?))?,
        unpinned_by: unpinner(row)?,
        told: row.get::<_, Option<i64>>("told_at")?.is_some(),
    })
}

/// Writes `pins`, a Fork source's rows, as `destination`'s.
pub(crate) fn copy_pins(connection: &Connection, destination: &ConversationId, pins: &[PinCopy]) -> Result<(), StorageError> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO conversation_pins
           (conversation_id, id, device_id, path, pinned_at, text, read_at, unpinned_at, unpinned_by, told_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for pin in pins {
        statement.execute(params![
            destination.as_str(),
            decode(TABLE, "id", i64::try_from(pin.id))?,
            pin.device_id.as_str(),
            pin.path,
            pin.pinned_at.as_millisecond(),
            pin.text,
            pin.read_at.as_millisecond(),
            pin.unpinned_at.map(|at| at.as_millisecond()),
            pin.unpinned_by.map(Unpinner::column),
            pin.told_at.map(|at| at.as_millisecond()),
        ])?;
    }
    Ok(())
}

impl ControlService {
    /// Every row of `conversation`'s pins, the oldest first.
    pub async fn pins(&self, conversation: ConversationId) -> Result<Vec<PinRecord>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare_cached(&format!(
                "SELECT id, device_id, path, pinned_at, {TOKENS} AS tokens, unpinned_by, told_at
                 FROM conversation_pins WHERE conversation_id = ?1 ORDER BY id"
            ))?;
            let mut rows = statement.query(params![conversation.as_str()])?;
            let mut pins = Vec::new();
            while let Some(row) = rows.next()? {
                pins.push(record(row)?);
            }
            Ok(pins)
        })
        .await
    }

    /// Every row of `conversation`'s pins, as a Fork copies them.
    pub async fn pin_copies(&self, conversation: ConversationId) -> Result<Vec<PinCopy>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare_cached(
                "SELECT id, device_id, path, pinned_at, text, read_at, unpinned_at, unpinned_by, told_at
                 FROM conversation_pins WHERE conversation_id = ?1 ORDER BY id",
            )?;
            let mut rows = statement.query(params![conversation.as_str()])?;
            let mut pins = Vec::new();
            while let Some(row) = rows.next()? {
                pins.push(PinCopy {
                    id: id(row)?,
                    device_id: device(row)?,
                    path: row.get("path")?,
                    pinned_at: instant(row, TABLE, "pinned_at")?,
                    text: row.get("text")?,
                    read_at: instant(row, TABLE, "read_at")?,
                    unpinned_at: optional_instant(row, "unpinned_at")?,
                    unpinned_by: unpinner(row)?,
                    told_at: optional_instant(row, "told_at")?,
                });
            }
            Ok(pins)
        })
        .await
    }

    /// The text of the pin `id` of `conversation` as last read.
    pub async fn pin_text(&self, conversation: ConversationId, id: u64) -> Result<Option<String>, StorageError> {
        let Ok(id) = i64::try_from(id) else {
            return Ok(None);
        };
        self.call(move |connection, _| {
            Ok(connection
                .prepare_cached("SELECT text FROM conversation_pins WHERE conversation_id = ?1 AND id = ?2")?
                .query_row(params![conversation.as_str(), id], |row| row.get(0))
                .optional()?)
        })
        .await
    }

    /// The tokens of `conversation`'s pinned documents, by their texts as last
    /// read.
    pub async fn pinned_tokens(&self, conversation: ConversationId) -> Result<u64, StorageError> {
        self.call(move |connection, _| {
            let tokens: i64 = connection
                .prepare_cached(&format!(
                    "SELECT COALESCE(SUM({TOKENS}), 0) FROM conversation_pins
                     WHERE conversation_id = ?1 AND unpinned_at IS NULL"
                ))?
                .query_row(params![conversation.as_str()], |row| row.get(0))?;
            decode(TABLE, "text", u64::try_from(tokens))
        })
        .await
    }

    /// Pins the document at `path` on `device`, whose text is `text`, while
    /// the pinned documents then hold at most `limit` tokens.
    pub async fn pin(
        &self,
        conversation: ConversationId,
        device: DeviceId,
        path: String,
        text: String,
        limit: u64,
    ) -> Result<Pinned, StorageError> {
        self.call(move |connection, now| {
            let transaction = connection.transaction()?;
            let already: bool = transaction
                .prepare_cached(
                    "SELECT EXISTS (SELECT 1 FROM conversation_pins
                     WHERE conversation_id = ?1 AND device_id = ?2 AND path = ?3 AND unpinned_at IS NULL)",
                )?
                .query_row(params![conversation.as_str(), device.as_str(), path], |row| row.get(0))?;
            if already {
                return Ok(Pinned::Already);
            }
            let pinned: i64 = transaction
                .prepare_cached(&format!(
                    "SELECT COALESCE(SUM({TOKENS}), 0) FROM conversation_pins
                     WHERE conversation_id = ?1 AND unpinned_at IS NULL"
                ))?
                .query_row(params![conversation.as_str()], |row| row.get(0))?;
            let pinned = decode(TABLE, "text", u64::try_from(pinned))?;
            if pinned + jush_shared_types::text_tokens(&text) > limit {
                return Ok(Pinned::Over { pinned });
            }
            let next: i64 = transaction
                .prepare_cached("SELECT COALESCE(MAX(id), 0) + 1 FROM conversation_pins WHERE conversation_id = ?1")?
                .query_row(params![conversation.as_str()], |row| row.get(0))?;
            let inserted = transaction
                .prepare_cached(
                    "INSERT INTO conversation_pins (conversation_id, id, device_id, path, pinned_at, text, read_at)
                     SELECT id, ?2, ?3, ?4, ?5, ?6, ?5 FROM conversations WHERE id = ?1",
                )?
                .execute(params![conversation.as_str(), next, device.as_str(), path, now.as_millisecond(), text])?;
            let outcome = match inserted {
                0 => Pinned::Gone,
                _ => Pinned::Pinned(decode(TABLE, "id", u64::try_from(next))?),
            };
            transaction.commit()?;
            Ok(outcome)
        })
        .await
    }

    /// Unpins the pinned document `id` of `conversation`, by `by`; false when
    /// it is not pinned.
    pub async fn unpin(&self, conversation: ConversationId, id: u64, by: Unpinner) -> Result<bool, StorageError> {
        let Ok(id) = i64::try_from(id) else {
            return Ok(false);
        };
        self.call(move |connection, now| {
            let changed = connection
                .prepare_cached(
                    "UPDATE conversation_pins SET unpinned_at = ?3, unpinned_by = ?4
                     WHERE conversation_id = ?1 AND id = ?2 AND unpinned_at IS NULL",
                )?
                .execute(params![conversation.as_str(), id, now.as_millisecond(), by.column()])?;
            Ok(changed > 0)
        })
        .await
    }

    /// Records that the agent was told the user unpinned the documents `ids`.
    pub async fn unpins_told(&self, conversation: ConversationId, ids: Vec<u64>) -> Result<(), StorageError> {
        self.call(move |connection, now| {
            let transaction = connection.transaction()?;
            for id in ids {
                let Ok(id) = i64::try_from(id) else {
                    continue;
                };
                transaction
                    .prepare_cached(
                        "UPDATE conversation_pins SET told_at = ?3
                         WHERE conversation_id = ?1 AND id = ?2 AND unpinned_by = 'user' AND told_at IS NULL",
                    )?
                    .execute(params![conversation.as_str(), id, now.as_millisecond()])?;
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }

    /// Keeps `text` as the pinned document `id`'s text as last read.
    pub async fn pin_read(&self, conversation: ConversationId, id: u64, text: String) -> Result<(), StorageError> {
        let Ok(id) = i64::try_from(id) else {
            return Ok(());
        };
        self.call(move |connection, now| {
            connection
                .prepare_cached(
                    "UPDATE conversation_pins SET text = ?3, read_at = ?4 WHERE conversation_id = ?1 AND id = ?2",
                )?
                .execute(params![conversation.as_str(), id, text, now.as_millisecond()])?;
            Ok(())
        })
        .await
    }
}
