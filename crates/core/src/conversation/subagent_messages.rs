//! A message of the user's to one subagent of a conversation, which a
//! program sends again when its answer was lost: the subagent takes it once
//! (`AgentServer::message_subagent`).

use std::rc::Rc;

use demi_agent_server::SubagentMessageError;
use demi_core_device_access::root_of;
use demi_core_permissions::Stops;
use demi_shared_types::{BlockId, NodeId};
use demi_web_api_protocol::conversations::{SubagentMessage, SubagentMessageAnswer};
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::ids::ConversationId;

use crate::Core;
use crate::conversation::TreeRefusal;

/// Why a message to a subagent was not taken.
#[derive(Debug, thiserror::Error)]
pub enum SubagentMessageRefusal {
    #[error("the conversation has no such subagent")]
    NoSubagent,
    #[error("the main agent takes messages as the conversation's")]
    MainAgent,
    /// The subagent refused it, as one that is stopping does.
    #[error("{0}")]
    Refused(String),
    /// The conversation refused to open, as it refuses a page's open.
    #[error("{1}")]
    Conversation(ErrorCode, String),
    #[error("{0}")]
    Failed(String),
    #[error("the backend is shutting down")]
    Closing,
}

impl From<SubagentMessageError> for SubagentMessageRefusal {
    fn from(error: SubagentMessageError) -> Self {
        match error {
            SubagentMessageError::NoSubagent => Self::NoSubagent,
            SubagentMessageError::MainAgent => Self::MainAgent,
            SubagentMessageError::Refused(reason) => Self::Refused(reason),
            SubagentMessageError::Failed(reason) => Self::Failed(reason),
        }
    }
}

impl From<TreeRefusal> for SubagentMessageRefusal {
    fn from(refusal: TreeRefusal) -> Self {
        match refusal {
            TreeRefusal::Refused(code, message) => Self::Conversation(code, message),
            TreeRefusal::Failed(reason) => Self::Failed(reason),
        }
    }
}

impl Core {
    /// Hands the subagent `node` of the conversation `id` the message, under
    /// its id or a new one, opening the conversation's tree when it is not
    /// open; answers the id.
    ///
    /// The message denies the permission requests the subagent's calls wait
    /// on, as a message to the main agent denies its own.
    pub async fn message_subagent(
        self: &Rc<Self>,
        id: &ConversationId,
        node: &NodeId,
        message: SubagentMessage,
    ) -> Result<SubagentMessageAnswer, SubagentMessageRefusal> {
        {
            // Counted before the open, so the close waits for it before the
            // agent shuts down.
            let _opening = self.tree_openers().token();
            if self.is_closing() {
                return Err(SubagentMessageRefusal::Closing);
            }
            self.restore_tree(id).await?;
        }
        let message_id = message.id.unwrap_or_else(|| {
            BlockId::try_from(uuid::Uuid::new_v4().simple().to_string()).expect("a uuid is not empty")
        });
        let held = demi_core_permissions::held_requests(
            self.permission_core(),
            id,
            &Stops::Agents(std::slice::from_ref(node)),
        );
        let delivered = self
            .agent()
            .message_subagent(&root_of(id), node, message_id.clone(), message.text)
            .await;
        // A close that began after the open disposes the tree under the
        // delivery, which then says why only in the agents' words.
        if delivered.is_err() && self.is_closing() {
            return Err(SubagentMessageRefusal::Closing);
        }
        delivered?;
        self.deny(id, held).await;
        Ok(SubagentMessageAnswer { id: message_id })
    }
}
