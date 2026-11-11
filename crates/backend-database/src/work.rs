//! The conversations' work: each one's tabs, the ids it ever had and the revision
//! that counts its changes.
//!
//! A request of changes reads the row and writes it in one transaction, so one
//! conversation's requests apply one at a time, each whole or not at all. The
//! backend never interprets a tab.

use std::collections::HashMap;

use demi_plugin_host::store::{MirrorOutcome, WorkOutcome};
use demi_web_api_protocol::ids::{ConversationId, UserId};
use demi_web_api_protocol::work::{CreateWorkTab, WorkChange, WorkDocument, Refused, Work};
use rusqlite::{Connection, OptionalExtension, params};

use super::StorageError;
use super::columns::{decode, json, to_json};
use super::control::ControlService;

impl ControlService {
    /// `owner`'s conversations that are not archived and whose work has
    /// a tab: those a lost package's plugin may have something to correct
    /// in (`crates/command-protocol/src/messages.rs`).
    pub async fn conversations_with_tabs(
        &self,
        owner: UserId,
    ) -> Result<Vec<ConversationId>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare(
                "SELECT conversations.id FROM conversations
                 JOIN conversation_work ON conversation_work.conversation_id = conversations.id
                 WHERE conversations.user_id = ?1 AND conversations.archived_at IS NULL
                   AND json_array_length(conversation_work.document, '$.tabs') > 0",
            )?;
            let ids = statement
                .query_map([owner.as_str()], |row| row.get::<_, String>(0))?
                .map(|id| {
                    let id = id?;
                    decode("conversations", "id", ConversationId::try_from(id.as_str()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
        .await
    }

    /// The conversation's work; the empty one when it never changed. A
    /// stored document that no longer matches the work's shape is corrupt,
    /// not repaired.
    pub async fn work(&self, conversation: ConversationId) -> Result<Work, StorageError> {
        self.call(move |connection, _| {
            let (revision, document) = stored(connection, &conversation)?;
            Ok(Work {
                revision,
                tabs: document.tabs,
                windows: document.windows,
            })
        })
        .await
    }

    /// The works of `owner`'s conversations that are archived, or that are
    /// not, by conversation; one that never changed has no entry. A stored
    /// document that no longer matches the work's shape is that
    /// conversation's own failure, not the read's, so the list the works go
    /// in does not fail for it.
    pub async fn works(
        &self,
        owner: UserId,
        archived: bool,
    ) -> Result<HashMap<ConversationId, Result<Work, StorageError>>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare(
                "SELECT conversations.id, conversation_work.revision, conversation_work.document
                 FROM conversation_work
                 JOIN conversations ON conversations.id = conversation_work.conversation_id
                 WHERE conversations.user_id = ?1 AND (conversations.archived_at IS NOT NULL) = ?2",
            )?;
            let mut rows = statement.query(params![owner.as_str(), archived])?;
            let mut works = HashMap::new();
            while let Some(row) = rows.next()? {
                let id = decode("conversations", "id", ConversationId::try_from(row.get::<_, String>(0)?))?;
                let revision = row.get::<_, i64>(1)?;
                let document = row.get::<_, String>(2)?;
                let work = decode("conversation_work", "revision", u64::try_from(revision)).and_then(|revision| {
                    let document: WorkDocument = json("conversation_work", "document", &document)?;
                    Ok(Work { revision, tabs: document.tabs, windows: document.windows })
                });
                works.insert(id, work);
            }
            Ok(works)
        })
        .await
    }

    /// Makes the tabs of `kind` in `conversation`'s work the tabs `listed`,
    /// as one change (`WorkDocument::mirror`), and answers the listed ids
    /// whose tabs the user closed.
    pub async fn mirror_work(
        &self,
        conversation: ConversationId,
        kind: String,
        listed: Vec<CreateWorkTab>,
    ) -> Result<MirrorOutcome, StorageError> {
        self.call(move |connection, now| {
            let transaction = connection.transaction()?;
            let exists: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM conversations WHERE id = ?1",
                    [conversation.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Ok(MirrorOutcome::Missing);
            }
            let (revision, mut document) = stored(&transaction, &conversation)?;
            let mirrored = document.mirror(&kind, listed);
            let changed = !mirrored.effects.is_empty();
            if changed {
                store(&transaction, &conversation, revision + 1, &document, now.as_millisecond())?;
                transaction.commit()?;
            }
            Ok(MirrorOutcome::Mirrored { changed, closed: mirrored.closed })
        })
        .await
    }

    /// Applies `changes` to the conversation's work in order, all or none,
    /// as one change of its revision.
    pub async fn change_work(
        &self,
        conversation: ConversationId,
        changes: Vec<WorkChange>,
    ) -> Result<WorkOutcome, StorageError> {
        self.call(move |connection, now| {
            let transaction = connection.transaction()?;
            let exists: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM conversations WHERE id = ?1",
                    [conversation.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Ok(WorkOutcome::Missing);
            }
            let (revision, mut document) = stored(&transaction, &conversation)?;
            let effects = match document.apply_all(changes) {
                Ok(effects) if effects.is_empty() => {
                    return Ok(WorkOutcome::Unchanged { revision });
                }
                Ok(effects) => effects,
                Err(Refused::Full) => return Ok(WorkOutcome::Full),
                Err(Refused::TooLarge) => return Ok(WorkOutcome::TooLarge),
            };
            let revision = revision + 1;
            store(&transaction, &conversation, revision, &document, now.as_millisecond())?;
            transaction.commit()?;
            Ok(WorkOutcome::Changed { revision, effects })
        })
        .await
    }

    /// Removes the tabs of `kinds` from every work of `owner`'s
    /// conversations, archived ones included, in one transaction, and
    /// answers the conversations whose work changed. The plugin that owns
    /// the kinds is off, and a tab only it draws would show as one that
    /// cannot be shown once its conversation is unarchived.
    pub async fn close_tab_kinds(
        &self,
        owner: UserId,
        kinds: Vec<String>,
    ) -> Result<Vec<ConversationId>, StorageError> {
        self.call(move |connection, now| {
            let transaction = connection.transaction()?;
            let with_tabs = {
                let mut statement = transaction.prepare(
                    "SELECT conversations.id FROM conversations
                     JOIN conversation_work ON conversation_work.conversation_id = conversations.id
                     WHERE conversations.user_id = ?1
                       AND json_array_length(conversation_work.document, '$.tabs') > 0",
                )?;
                statement
                    .query_map([owner.as_str()], |row| row.get::<_, String>(0))?
                    .map(|id| {
                        let id = id?;
                        decode("conversations", "id", ConversationId::try_from(id.as_str()))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            let mut changed = Vec::new();
            for conversation in with_tabs {
                let (revision, mut document) = stored(&transaction, &conversation)?;
                let removals: Vec<WorkChange> = document
                    .tabs
                    .iter()
                    .filter(|tab| kinds.contains(&tab.kind))
                    .map(|tab| WorkChange::Remove { id: tab.id.clone() })
                    .collect();
                if removals.is_empty() {
                    continue;
                }
                document
                    .apply_all(removals)
                    .expect("removing tabs takes no room");
                store(&transaction, &conversation, revision + 1, &document, now.as_millisecond())?;
                changed.push(conversation);
            }
            transaction.commit()?;
            Ok(changed)
        })
        .await
    }
}

/// Writes the conversation's work at `revision`.
fn store(
    connection: &Connection,
    conversation: &ConversationId,
    revision: u64,
    document: &WorkDocument,
    now: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO conversation_work (conversation_id, revision, document, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (conversation_id) DO UPDATE SET revision = excluded.revision, document = excluded.document, updated_at = excluded.updated_at",
        params![
            conversation.as_str(),
            i64::try_from(revision).expect("a revision fits in 63 bits"),
            to_json(document),
            now
        ],
    )?;
    Ok(())
}

/// The conversation's revision and document; 0 and the empty one before
/// its first change.
fn stored(
    connection: &Connection,
    conversation: &ConversationId,
) -> Result<(u64, WorkDocument), StorageError> {
    let row: Option<(i64, String)> = connection
        .query_row(
            "SELECT revision, document FROM conversation_work WHERE conversation_id = ?1",
            [conversation.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((revision, document)) = row else {
        return Ok((0, WorkDocument::default()));
    };
    let revision = decode("conversation_work", "revision", u64::try_from(revision))?;
    let document = json("conversation_work", "document", &document)?;
    Ok((revision, document))
}
