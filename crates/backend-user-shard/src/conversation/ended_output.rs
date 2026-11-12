//! An ended command's output as a page's terminal shows it: the end of the
//! output the conversation keeps, which a page reads when it first shows the
//! command, and the script its call ran.

use demi_agent_store::{StoreError, StoredOutput, media::BlobStore};
use demi_backend_database::tree;
use demi_device_interface::{Seen, Streams};
use demi_shared_types::CommandId;
use demi_agent_tools::VIEW_CHARS;
use demi_web_api_protocol::conversations::EndedOutput;
use demi_web_api_protocol::ids::ConversationId;
use serde_json::Value;

use crate::shard::Shard;

impl Shard {
    /// The output of `command` of `conversation`, as its terminal shows it;
    /// none when the conversation keeps no output of it, as of one that runs.
    pub async fn ended_output(
        &self,
        conversation: &ConversationId,
        command: &CommandId,
    ) -> Result<Option<EndedOutput>, StoreError> {
        let services = self.services();
        let db = services.conversations.db(conversation);
        let blobs = services.blobs.for_user(self.user());
        let Some(stored) = tree::stored_command(&db, &blobs as &dyn BlobStore, command).await? else {
            return Ok(None);
        };
        let wanted = command.clone();
        let call = db
            .read(move |connection| tree::command_call(connection, &wanted))
            .await
            .map_err(|error| StoreError::Failed(error.to_string()))?
            .flatten();
        let script = call
            .and_then(|(_, call)| serde_json::from_str::<Value>(&call.input).ok())
            .and_then(|input| input["script"].as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(Some(match stored.output {
            StoredOutput::Stored { output } => {
                let text: String = output
                    .text(Streams::Both, output.binary_stdout_length(), Seen::default())
                    .chunks(0)
                    .into_iter()
                    .map(|chunk| chunk.text)
                    .collect();
                let chars = text.chars().count();
                let skip = chars.saturating_sub(VIEW_CHARS);
                let start = text.char_indices().nth(skip).map_or(text.len(), |(at, _)| at);
                EndedOutput {
                    script,
                    output: text[start..].to_owned(),
                    chars: chars as u64,
                    not_stored: None,
                }
            }
            StoredOutput::NotStored(reason) => EndedOutput {
                script,
                output: String::new(),
                chars: 0,
                not_stored: Some(reason),
            },
        }))
    }
}
