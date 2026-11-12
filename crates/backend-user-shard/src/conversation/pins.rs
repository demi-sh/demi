//! Pinned documents: the product's `demi pin` group, the context source that
//! gives the main agent their text, and the user's unpin.
//!
//! The main agent pins a text file of a device. Before each request the
//! context source compares the pins with what the transcript since the last
//! compaction shows the agent, its blocks, and gives what the agent lacks:
//!
//! ```text
//!   pin added ──► next request: its text
//!   compaction ─► next request: every pinned text the kept part of the
//!                 transcript does not show, as it is then
//!   user unpins ► next request: that the user unpinned it
//! ```
//!
//! A block tags each document's text with its pin's id and each unpin it tells
//! with `(pin N)`, which is how a later request finds what the agent has seen.
//! The transcript holds each block, so a later request replays the same bytes.
//! A document is read again from its device when it is given; one its device
//! cannot read comes as last read, which the block says. The agent's own unpin
//! needs no word, and an unpin by the user does not wake the agent or stop its
//! turn.
//!
//! Only the main agent pins: the composer shows its pins, and the user
//! controls only what the user sees. The documents hold at most
//! [`PINNED_TOKENS_MAX`] together.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};

use demi_agent_tools::{ContextAnswer, ContextSource, NodeContext};
use demi_backend_database::StorageError;
use demi_backend_database::pins::{PinRecord, Pinned, Unpinner};
use demi_backend_device_access::device_commands::conversation_of;
use demi_backend_device_access::{conversation_of as conversation_of_root, root_of};
use demi_backend_page_sync::Part;
use demi_device_interface::{Call, DeviceFile, DeviceRead, GroupBuilder, LeafBuilder, RpcError, RpcPort, TypedRpc};
use demi_shared_types::output::Table;
use demi_shared_types::{PINNED_DOCUMENTS_SOURCE, TurnId, text_tokens};
use demi_web_api_protocol::ids::{ConversationId, DeviceId};
use demi_web_api_protocol::pins::PinnedDocument;
use futures_util::future::LocalBoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::organize::{failed, refused, verb};
use crate::shard::Shard;

/// The most tokens the pinned documents hold together, by Demi's estimate.
///
/// Each comes back after every compaction, and a compaction has to leave the
/// model most of its window.
pub(crate) const PINNED_TOKENS_MAX: u64 = 25_000;

/// The most bytes a pin reads of a file: one over what the limit allows,
/// which tells a file over it.
const READ_BYTES: u64 = PINNED_TOKENS_MAX * 4 + 1;

/// The first line of every block of the source.
const MARKER: &str = "[Pinned documents]";

/// What opens a document's text in a block, before its pin's id.
const OPENING: &str = "<pinned id=\"";

/// What closes a document's text in a block.
const CLOSING: &str = "</pinned>";

/// What opens the line that tells an unpin by the user.
const UNPINNED: &str = "The user unpinned ";

/// What comes before the id of the pin an unpin line tells.
const UNPIN_TAG: &str = "(pin ";

/// What comes before the documents' texts in a block.
const KEEP_FOLLOWING: &str = "Pinned documents, as they are now. Keep following them.";

const SUMMARY: &str = "Documents pinned to this conversation. A pinned document's text comes to you before your next request and again, as it is then, after each compaction, so what it says holds for the whole conversation. The user sees each over the composer and can unpin it. Only the main agent pins.";

/// The group's entry in the model's capability index.
const ENTRY: &str = "Pins a document whose rules hold for the whole conversation, so it comes back after each compaction.";

const ADD_SUMMARY: &str = "Pin a text file of the device the command runs on. Pinned documents hold at most 25,000 tokens together.";

const REMOVE_SUMMARY: &str = "Unpin a pinned document.";

const LIST_SUMMARY: &str = "List the pinned documents: path, device and tokens.";

/// The input of `add` and `remove`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    /// The file's path, relative to the command's directory or absolute
    #[schemars(length(min = 1))]
    path: String,
}

/// The input of `list`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

/// The `pin` group, whose handlers act in `shard`.
pub(crate) fn pin_group(shard: Weak<Shard>) -> GroupBuilder {
    GroupBuilder::new("pin", SUMMARY)
        .index_entry(ENTRY)
        .leaf(
            LeafBuilder::rpc("add", ADD_SUMMARY)
                .input::<PathArgs>()
                .positionals(["path"])
                .bind(TypedRpc::new(verb(shard.clone(), add))),
        )
        .leaf(
            LeafBuilder::rpc("remove", REMOVE_SUMMARY)
                .input::<PathArgs>()
                .positionals(["path"])
                .bind(TypedRpc::new(verb(shard.clone(), remove))),
        )
        .leaf(
            LeafBuilder::rpc("list", LIST_SUMMARY)
                .input::<NoArgs>()
                .bind(TypedRpc::new(verb(shard, list))),
        )
}

/// The conversation and the device of a call the main agent made, and the
/// absolute path its `path` names there.
async fn place(shard: &Shard, call: &Call<PathArgs>) -> Result<(ConversationId, DeviceId, String), RpcError> {
    let invocation = &call.invocation;
    let conversation = conversation_of(invocation)?;
    if invocation.caller.as_ref().is_none_or(|caller| caller.node != root_of(&conversation)) {
        return Err(refused("only the main agent pins documents, which the user sees over the composer"));
    }
    let device = DeviceId::try_from(invocation.device.as_str()).map_err(failed)?;
    let paths = shard.device_paths(&device).await.map_err(failed)?;
    let path = paths.resolve(&invocation.cwd, call.args.path.trim());
    Ok((conversation, device, path))
}

async fn add(shard: Rc<Shard>, call: Call<PathArgs>, port: RpcPort) -> Result<u8, RpcError> {
    let (conversation, device, path) = place(&shard, &call).await?;
    let text = match shard.read_document(&conversation, &device, &path).await {
        Ok(Document { text, whole: true }) => text,
        Ok(Document { whole: false, .. }) => {
            return Err(refused(format!(
                "{path} is more than {PINNED_TOKENS_MAX} tokens, all that pinned documents hold"
            )));
        }
        Err(Unread::Refused(why)) => return Err(refused(why)),
        Err(Unread::Failed(why)) => return Err(failed(why)),
    };
    let tokens = text_tokens(&text);
    let control = &shard.services().control;
    match control
        .pin(conversation.clone(), device, path.clone(), text, PINNED_TOKENS_MAX)
        .await
        .map_err(failed)?
    {
        Pinned::Pinned(_) => {}
        Pinned::Already => {
            port.stdout(format!("{path} is pinned already.\n")).await?;
            return Ok(0);
        }
        Pinned::Over { pinned } => {
            return Err(refused(format!(
                "{path} is {tokens} tokens, and {pinned} are pinned already: pinned documents hold at most {PINNED_TOKENS_MAX} together"
            )));
        }
        Pinned::Gone => return Err(failed("the conversation is gone")),
    }
    shard.mark(Part::Metadata(conversation));
    port.stdout(format!("Pinned {path}, {tokens} tokens.\n")).await?;
    Ok(0)
}

async fn remove(shard: Rc<Shard>, call: Call<PathArgs>, port: RpcPort) -> Result<u8, RpcError> {
    let (conversation, device, path) = place(&shard, &call).await?;
    let control = &shard.services().control;
    let pinned = control
        .pins(conversation.clone())
        .await
        .map_err(failed)?
        .into_iter()
        .find(|pin| pin.unpinned_by.is_none() && pin.device_id == device && pin.path == path);
    let Some(pin) = pinned else {
        return Err(refused(format!("{path} is not pinned")));
    };
    control.unpin(conversation.clone(), pin.id, Unpinner::Agent).await.map_err(failed)?;
    shard.mark(Part::Metadata(conversation));
    port.stdout(format!("Unpinned {path}.\n")).await?;
    Ok(0)
}

async fn list(shard: Rc<Shard>, call: Call<NoArgs>, port: RpcPort) -> Result<u8, RpcError> {
    let conversation = conversation_of(&call.invocation)?;
    let pins = shard.services().control.pins(conversation).await.map_err(failed)?;
    let mut table = Table::new(["path", "device", "tokens"]);
    for pin in pins.iter().filter(|pin| pin.unpinned_by.is_none()) {
        table.row([pin.path.clone(), shard.device_name(&pin.device_id).await, pin.tokens.to_string()]);
    }
    port.stdout(table.render()).await?;
    Ok(0)
}

/// A file's text as read for a pin.
struct Document {
    text: String,
    /// False when the file holds more than a pin reads, and `text` is its
    /// first part.
    whole: bool,
}

/// Why a document was not read.
enum Unread {
    /// What the path names is no text file.
    Refused(String),
    /// The device did not answer.
    Failed(String),
}

impl Shard {
    /// The text of the file at `path` on `device`, at most what a pin reads.
    async fn read_document(&self, conversation: &ConversationId, device: &DeviceId, path: &str) -> Result<Document, Unread> {
        let reads = [DeviceRead::new(path.to_owned(), READ_BYTES)];
        // The path is absolute, so the directory it is read against is none
        // other than its own.
        let files = self
            .device_shard()
            .read_job_files(conversation, device, path, &reads, &CancellationToken::new())
            .await
            .map_err(|error| Unread::Failed(format!("{path} was not read: {error}")))?;
        match files.into_iter().next() {
            Some(DeviceFile::File { bytes, size }) => {
                let mut bytes = bytes.into_bytes().to_vec();
                let whole = size < READ_BYTES;
                if !whole {
                    // The read may end inside a character.
                    let end = match std::str::from_utf8(&bytes) {
                        Ok(_) => bytes.len(),
                        Err(error) if error.error_len().is_none() => error.valid_up_to(),
                        Err(_) => return Err(Unread::Refused(format!("{path} is not text"))),
                    };
                    bytes.truncate(end);
                }
                let text = String::from_utf8(bytes).map_err(|_| Unread::Refused(format!("{path} is not text")))?;
                Ok(Document { text, whole })
            }
            Some(DeviceFile::Missing) | None => Err(Unread::Refused(format!("there is no file at {path}"))),
            Some(DeviceFile::Directory { .. } | DeviceFile::Other) => {
                Err(Unread::Refused(format!("{path} is not a file")))
            }
            Some(DeviceFile::Unreadable { message }) => Err(Unread::Refused(format!("{path} cannot be read: {message}"))),
        }
    }

    /// The pinned documents of `conversation`, as its metadata lists them.
    pub(crate) async fn conversation_pins(&self, conversation: &ConversationId) -> Result<Vec<PinnedDocument>, StorageError> {
        let mut pinned = Vec::new();
        for pin in self.services().control.pins(conversation.clone()).await? {
            if pin.unpinned_by.is_none() {
                pinned.push(PinnedDocument {
                    id: pin.id,
                    device: self.device_name(&pin.device_id).await,
                    device_id: pin.device_id,
                    path: pin.path,
                });
            }
        }
        Ok(pinned)
    }

    /// The block the main agent of `conversation` reads before its next
    /// request, whose transcript since the last compaction holds the source's
    /// texts `seen`; none when they show the agent all there is to say.
    async fn pins_context(&self, conversation: &ConversationId, seen: &[&str]) -> Result<Option<String>, StorageError> {
        let control = &self.services().control;
        let pins = control.pins(conversation.clone()).await?;
        let seen = Seen::of(seen);
        let mut lines = Vec::new();
        let mut told = Vec::new();
        for pin in &pins {
            // An unpin told before the last compaction is told again only
            // when the kept transcript still shows the document's text.
            if pin.unpinned_by == Some(Unpinner::User)
                && !seen.told.contains(&pin.id)
                && (!pin.told || seen.shown.contains(&pin.id))
            {
                lines.push(format!(
                    "{UNPINNED}{} on {} {UNPIN_TAG}{}): stop following it, and do not pin it again unless the user asks.",
                    pin.path,
                    self.device_name(&pin.device_id).await,
                    pin.id
                ));
                told.push(pin.id);
            }
        }
        let (shown, given): (Vec<&PinRecord>, Vec<&PinRecord>) = pins
            .iter()
            .filter(|pin| pin.unpinned_by.is_none())
            .partition(|pin| seen.shown.contains(&pin.id));
        if !given.is_empty() {
            lines.push(KEEP_FOLLOWING.to_owned());
            let mut room = PINNED_TOKENS_MAX.saturating_sub(shown.iter().map(|pin| pin.tokens).sum());
            for pin in given {
                lines.push(self.pinned_text(conversation, pin, &mut room).await?);
            }
        }
        if !told.is_empty() {
            control.unpins_told(conversation.clone(), told).await?;
        }
        if lines.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!("{MARKER}\n{}", lines.join("\n\n"))))
    }

    /// `pin`'s document as the block gives it: read again from its device,
    /// or as last read with a line that says so, cut where `room` ends.
    async fn pinned_text(&self, conversation: &ConversationId, pin: &PinRecord, room: &mut u64) -> Result<String, StorageError> {
        let control = &self.services().control;
        let device = self.device_name(&pin.device_id).await;
        let (mut text, note) = match self.read_document(conversation, &pin.device_id, &pin.path).await {
            Ok(Document { text, .. }) => {
                control.pin_read(conversation.clone(), pin.id, text.clone()).await?;
                (text, None)
            }
            Err(Unread::Refused(why) | Unread::Failed(why)) => {
                let text = control.pin_text(conversation.clone(), pin.id).await?.unwrap_or_default();
                (text, Some(format!("{why}; this is its text as last read.")))
            }
        };
        let tokens = text_tokens(&text);
        let mut cut = None;
        if tokens > *room {
            let mut end = usize::try_from(*room * 4).unwrap_or(usize::MAX).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            cut = Some(format!(
                "[Cut here: pinned documents hold at most {PINNED_TOKENS_MAX} tokens together. Read the rest from the file.]"
            ));
        }
        *room = room.saturating_sub(tokens);
        let mut block = String::new();
        if let Some(note) = note {
            let _ = writeln!(block, "{note}");
        }
        let _ = writeln!(block, "{OPENING}{}\" path=\"{}\" device=\"{device}\">", pin.id, pin.path);
        block.push_str(&text);
        if !text.ends_with('\n') {
            block.push('\n');
        }
        if let Some(cut) = cut {
            let _ = writeln!(block, "{cut}");
        }
        block.push_str(CLOSING);
        Ok(block)
    }
}

/// What the source's blocks since the last compaction showed the agent: the
/// pins whose text they gave, and the unpins they told.
#[derive(Default)]
struct Seen {
    shown: HashSet<u64>,
    told: HashSet<u64>,
}

impl Seen {
    fn of(texts: &[&str]) -> Self {
        let mut seen = Self::default();
        for text in texts {
            let Some(body) = text.strip_prefix(MARKER) else {
                continue;
            };
            // A document's own lines are its text, never the block's tags.
            let mut inside = false;
            for line in body.lines() {
                if inside {
                    inside = line != CLOSING;
                } else if let Some(rest) = line.strip_prefix(OPENING) {
                    inside = true;
                    if let Some(id) = id_before(rest, '"') {
                        seen.shown.insert(id);
                    }
                } else if line.starts_with(UNPINNED)
                    && let Some(at) = line.rfind(UNPIN_TAG)
                    && let Some(id) = id_before(&line[at + UNPIN_TAG.len()..], ')')
                {
                    seen.told.insert(id);
                }
            }
        }
        seen
    }
}

/// The pin id `text` starts with, which `end` follows.
fn id_before(text: &str, end: char) -> Option<u64> {
    text[..text.find(end)?].parse().ok()
}

/// The pins context source: what the main agent learns of its pinned
/// documents; nothing for a subagent.
pub(crate) struct Pins {
    pub(crate) shard: Weak<Shard>,
}

impl ContextSource for Pins {
    fn name(&self) -> &str {
        PINNED_DOCUMENTS_SOURCE
    }

    fn context<'a>(
        &'a self,
        node: NodeContext<'a>,
        _: &'a TurnId,
        seen: &'a [&'a str],
    ) -> LocalBoxFuture<'a, Result<Option<ContextAnswer>, String>> {
        Box::pin(async move {
            if node.node != node.root {
                return Ok(None);
            }
            let Some(shard) = self.shard.upgrade() else {
                return Ok(None);
            };
            shard
                .pins_context(&conversation_of_root(node.root), seen)
                .await
                .map(|text| text.map(ContextAnswer::from))
                .map_err(|error| format!("the pinned documents cannot be read: {error}"))
        })
    }
}
