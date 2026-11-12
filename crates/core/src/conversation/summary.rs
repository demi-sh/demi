//! Conversations as the web app lists and shows them: the sidebar entry of
//! each, and the metadata of one a page shows.
//!
//! An entry comes with its status, from the live tree or else from its last
//! checkpoint, and its output revision. Metadata comes with the directory its
//! work runs in, its root's latest ended turn, from that same checkpoint, the
//! cost of its rows of no agent, and each list its view shows whole under its
//! revision: its messages saved for later as the composer lists them, the
//! states plugins hold for its work's tabs, its attached devices, its
//! directory's workflows and its permission requests. Each is read with an
//! indexed query that touches no transcript row. The work's tabs are the
//! entry's, so the page draws the work as soon as the conversation is listed.
//!
//! The persisted facts come from a read-only connection, so listing hundreds of
//! conversations takes no writer from a running one.

use demi_core_store::StorageError;
use demi_core_store::columns::decode;
use demi_core_store::conversation_index::ConversationRecord;
use demi_core_store::{tree::{self, EndedTurn, SummaryFacts, Terminal}};
use demi_shared_types::{ModelSelection, SessionPhase};
use demi_web_api_protocol::conversations::{
    ConversationMetadata, ConversationStatus, ConversationSummary, LastTurn, ModelSettings,
    TurnOutcome, UnreadEnd,
};
use demi_web_api_protocol::attached_devices::{AttachedDevice, ConversationDevices};
use demi_web_api_protocol::ids::{ConversationId, ProviderId};
use demi_web_api_protocol::work::Work;
use futures_util::future::try_join_all;
use tokio_util::sync::CancellationToken;

use crate::Core;
use demi_core_device_access::root_of;

impl Core {
    /// The entries of the user's conversations that are archived, the last
    /// archived first, or that are not, in sidebar order.
    pub async fn conversation_summaries(
        &self,
        archived: bool,
    ) -> Result<Vec<ConversationSummary>, StorageError> {
        let store = self.store();
        let (records, mut works) = futures_util::future::try_join(
            store.conversations(archived),
            store.works(archived),
        )
        .await?;
        try_join_all(records.into_iter().map(|record| {
            let work = readable(&record.id, works.remove(&record.id).unwrap_or_else(|| Ok(Work::empty())));
            self.summarize(record, work)
        }))
        .await
    }

    /// `record` as the sidebar lists it.
    ///
    /// The live tree is asked before the database is read: a tree does nothing by
    /// itself only once the save that ends its action has committed, so the
    /// facts read afterwards are at least that save's, and a status is never an idle
    /// tree's view of an older checkpoint.
    pub async fn conversation_summary(
        &self,
        record: ConversationRecord,
    ) -> Result<ConversationSummary, StorageError> {
        let work = readable(&record.id, self.store().work(record.id.clone()).await);
        self.summarize(record, work).await
    }

    async fn summarize(&self, record: ConversationRecord, work: Work) -> Result<ConversationSummary, StorageError> {
        let live = self
            .agent()
            .tree(&root_of(&record.id))
            .map(|tree| (LiveRun::of(tree.is_running(), tree.agents_work()), tree.root().session().phase()));
        let facts = self.summary_facts(&record.id).await;
        let resolved = self.device_core().resolve_target(&record).await?;
        let archived = record.archived();
        let resuming = self.restart_resumes().contains(&record.id);
        Ok(ConversationSummary {
            unread: record.unread_end().map(|end| if end.failed { UnreadEnd::Failed } else { UnreadEnd::Answered }),
            status: status(live, resuming, &facts),
            target: record.target.shown(&resolved),
            worktree: record.target.worktree().map(|worktree| worktree.name.clone()),
            id: record.id,
            workspace_id: record.workspace,
            title: record.title,
            archived,
            archived_at: record.archived_at,
            pinned: record.pinned,
            read_revision: record.read_revision,
            created_at: record.created_at,
            updated_at: record.updated_at,
            revision: facts.revision,
            permission_requests: record.permission_requests,
            unread_permission_requests: record.unread_permission_requests,
            scheduled: record.scheduled,
            work,
        })
    }



    /// What a page shows of `record` while it shows the conversation.
    pub async fn conversation_metadata(&self, record: ConversationRecord) -> Result<ConversationMetadata, StorageError> {
        let tree = self.agent().tree(&root_of(&record.id));
        let plugins_changed = match &tree {
            Some(tree) => {
                *tree.toolset() != *self.plugins().revision(record.plugins.as_deref()).await.map_err(StorageError::from)?
            }
            None => false,
        };
        let facts = self.summary_facts(&record.id).await;
        let resolved = self.device_core().resolve_target(&record).await?;
        let devices = self.store().attached_device_listing(record.id.clone())
            .await?
            .into_iter()
            .map(|(attached, attached_at)| AttachedDevice {
                device_id: attached.device,
                name: attached.name,
                cwd: attached.cwd,
                attached_at,
            })
            .collect();
        let tab_states = self
            .plugins()
            .tab_states(record.id.clone(), CancellationToken::new())
            .await
            .map_err(StorageError::from)?;
        let permissions = demi_core_permissions::read(self.permission_core(), &record.id).await?;
        let other_cost = self.store().conversation_costs(record.id.clone())
            .await?
            .into_iter()
            .find(|kept| kept.agent.is_none())
            .map(|kept| kept.cost)
            .unwrap_or_default();
        let first_call_at = self.store().conversation_first_call(record.id.clone()).await?;
        let schedules = self.conversation_schedules(&record.id).await?;
        let notifications_card = self.store().notifications_card(record.id.clone()).await?;
        Ok(ConversationMetadata {
            model: record.model.as_ref().map(settings).transpose()?,
            workflow: record.workflow,
            browser_profile: record.browser_profile,
            identity: record.identity,
            plugins: record.plugins,
            cwd: resolved.path().to_owned(),
            last_turn: facts.last_turn.as_ref().map(last_turn),
            turn_speed: facts.turn_speed,
            suggested_replies: self.store().suggested_replies(record.id.clone()).await?,
            plugins_changed,
            notifications_card,
            other_cost,
            first_call_at,
            tab_states,
            devices: ConversationDevices {
                revision: record.devices_revision,
                devices,
            },
            last_attachment: facts.last_attachment,
            plugin_revisions: self.plugins().plugin_revisions(&record.id),
            permissions,
            schedules,
            id: record.id,
        })
    }

    /// The facts of the conversation `id`'s checkpoint an entry and its
    /// metadata show.
    ///
    /// A conversation whose database cannot be opened, as one a build that
    /// does not know its schema made, is shown without its facts: it alone
    /// fails to open, and the user's other conversations and pages work on
    /// (`crates/backend-database/src/lib.rs`).
    pub(crate) async fn summary_facts(&self, id: &ConversationId) -> SummaryFacts {
        match self.store().databases().read(id, tree::summary).await {
            Ok(facts) => facts.unwrap_or(SummaryFacts::EMPTY),
            Err(error) => {
                tracing::error!(
                    event = "conversation_summary.read_failed",
                    conversation = %id,
                    error = &error as &dyn std::error::Error,
                    "a conversation's database could not be read for its summary"
                );
                SummaryFacts::EMPTY
            }
        }
    }
}

/// The work `read` gives, or an empty one when the conversation's stored
/// work cannot be read: that conversation then shows no tabs, and the
/// user's other conversations are listed as they are.
fn readable(id: &ConversationId, read: Result<Work, StorageError>) -> Work {
    read.unwrap_or_else(|error| {
        tracing::error!(
            event = "conversation_summary.read_failed",
            conversation = %id,
            what = "work",
            error = &error as &dyn std::error::Error,
            "a conversation's work could not be read for its entry"
        );
        Work::empty()
    })
}

/// The model settings a conversation's selection shows (`crates/shared-types/src/catalog.rs`).
fn settings(selection: &ModelSelection) -> Result<ModelSettings, StorageError> {
    Ok(ModelSettings {
        provider_id: decode(
            "conversations",
            "model",
            ProviderId::try_from(selection.provider_id.as_str()),
        )?,
        model_id: selection.model.id.clone(),
        thinking_effort: selection.thinking_effort().map(str::to_owned),
        service_tier_id: selection.service_tier_id.clone(),
    })
}

/// The root's latest ended turn as the metadata carries it.
fn last_turn(turn: &EndedTurn) -> LastTurn {
    LastTurn {
        id: turn.id.clone(),
        outcome: match turn.outcome {
            Terminal::Response => TurnOutcome::Finished,
            Terminal::Error => TurnOutcome::Failed,
            Terminal::Abort => TurnOutcome::Stopped,
        },
    }
}

/// Whether a live tree runs, and whether a model works in it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveRun {
    Working,
    AwaitingCommands,
    Resting,
}

impl LiveRun {
    fn of(running: bool, agents_work: bool) -> Self {
        match (running, agents_work) {
            (false, _) => Self::Resting,
            (true, true) => Self::Working,
            (true, false) => Self::AwaitingCommands,
        }
    }
}

/// A conversation's status: running or compacting while a model of its live
/// tree works, awaiting commands while the tree runs only for a command
/// that is not a service; resuming while the core resumes its turn a
/// restart interrupted; interrupted when its checkpoint was saved in a turn
/// and no tree is live; else what its latest terminal block says. `live` is
/// how the live tree runs, and its root's phase.
fn status(live: Option<(LiveRun, SessionPhase)>, resuming: bool, facts: &SummaryFacts) -> ConversationStatus {
    match live {
        Some((LiveRun::Working, SessionPhase::Compacting)) => return ConversationStatus::Compacting,
        Some((LiveRun::Working, _)) => return ConversationStatus::Running,
        Some((LiveRun::AwaitingCommands, _)) => return ConversationStatus::AwaitingCommands,
        _ if resuming => return ConversationStatus::Resuming,
        None if facts.phase != SessionPhase::Idle => return ConversationStatus::Interrupted,
        Some((LiveRun::Resting, _)) | None => {}
    }
    match facts.last {
        Some(Terminal::Response) => ConversationStatus::Completed,
        Some(Terminal::Error) => ConversationStatus::Error,
        Some(Terminal::Abort) => ConversationStatus::Stopped,
        None => ConversationStatus::Idle,
    }
}
