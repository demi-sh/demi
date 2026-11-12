//! What the backend's conversations are assembled from.
//!
//! - The product's instructions, or the identity a conversation names.
//! - The toolset of the plugins the user has on, less the program's plugins
//!   the conversation does not name, with the product's `demi device` and
//!   `demi attachment` groups.
//! - The user's subagent settings, which each spawn reads.
//! - The device each node reaches, and the conversation's current primary device.
//! - The execution context source, which tells a node that the conversation's
//!   execution context changed.
//! - The plugins that are context sources.

use std::rc::{Rc, Weak};

use demi_agent_server::{ProductPrompt, PromptSource};
use demi_agent_tools::{
    ContextAnswer, ContextSource, ConversationProfiles, DeviceResolver, NewProfileDraft, NodeContext, Profile, WorkflowListing,
    ProfileEdit, ProfileModel, SubagentSettings, SubagentSource, Toolset, ToolsetSource,
    runs_on as job_model,
};
use demi_conversation_lane_protocol::{CacheChart, JobModel};
use demi_shared_types::{ModelSelection, NodeId, PictureFailure, WorkflowChoice};

use super::runs_on::runs_on;
use demi_core_device_access::attachment_commands::attachment_group;
use demi_core_device_access::device_commands::device_group;

use super::organize::conversation_group;
use crate::provider_commands::providers_group;
use demi_core_device_access::{DeviceCore, conversation_of};
use demi_plugin_host::ContextAsk;
use demi_runner_client::RemoteDevice;
use demi_agent_store::Placement;
use demi_web_api_protocol::ids::DeviceId;
use demi_device_interface::{DeviceError, DeviceErrorKind};
use demi_plugin_interface::{EXECUTION_SOURCE, PluginId};
use demi_shared_types::TurnId;
use demi_core_store::StorageError;
use demi_provider_registry::entry::ProviderEntry;
use demi_web_api_protocol::subagents::SubagentProfile;
use futures_util::future::LocalBoxFuture;
use tokio_util::sync::CancellationToken;

use crate::Core;
use crate::schedules::commands::schedule_group;
use crate::credential_commands::credentials_group;
use crate::notify_commands::notify_group;

/// The product's instructions, the identity that opens every node's system
/// prompt unless its conversation names another.
const INSTRUCTIONS: &str = "You are Demi, an agent that works for the user on their computers: a Demi Cloud machine or the devices they paired. You write and run software, research, operate websites, and work with files and data, across turns.";

/// The harness guide every node's system prompt carries after its identity,
/// a profile's node too, around the words of the web app's Files view, which
/// a conversation with an identity of its own goes without.
const GUIDE_START: &str = "Guidelines:\n- Commands run on the conversation's primary device, in its working directory; a context block names both. The directory stays between turns";
const GUIDE_FILES: &str = ", and Files shows it";
const GUIDE_END: &str = "; to work in another directory, move there with `demi device move` first.\n- To wait for a command or a subagent, end your turn: its end or its message wakes you, and you continue then. Never wait with sleep or a loop.\n- The user reads only your replies, rendered as Markdown. Be concise.\n- Deliver a file as an attachment: `demi attachment upload`, then `<attachment id=\"a3\" />` on its own line; a medium a tool result showed you has its `attachment_id` already.\n- Link what the user can open: a device file `[runner.md](docs/runner.md#L438)`, a subagent `[File icons](agent:4)`, a commit `[Fix the cart total](commit:3f2a9c1)`, or `commit:src/v8@86229da` in another repository, a conversation `[Login test](conversation:<id>)`.";

/// The product's layers of a conversation's system prompts: Demi's
/// identity and the whole guide, or the identity it names and the guide
/// without the Files view.
pub(crate) struct CorePrompts {
    /// Weak: the core owns the agent server that holds the source.
    pub(crate) core: Weak<Core>,
}

impl PromptSource for CorePrompts {
    fn prompt<'a>(&'a self, root: &'a NodeId) -> LocalBoxFuture<'a, Result<ProductPrompt, String>> {
        Box::pin(async move {
            let core = self
                .core
                .upgrade()
                .ok_or_else(|| "the backend is shutting down".to_owned())?;
            let record = core
                .store()
                .conversation(conversation_of(root))
                .await
                .map_err(|error| error.to_string())?;
            Ok(match record.and_then(|record| record.identity) {
                Some(identity) => ProductPrompt {
                    identity: Rc::from(identity),
                    guide: Rc::from(format!("{GUIDE_START}{GUIDE_END}")),
                },
                None => ProductPrompt {
                    identity: Rc::from(INSTRUCTIONS),
                    guide: Rc::from(format!("{GUIDE_START}{GUIDE_FILES}{GUIDE_END}")),
                },
            })
        })
    }
}

impl Core {
    /// The program's plugins the conversation of `root` names, none for all
    /// of them, as is the user's with no root.
    async fn named_plugins(&self, root: Option<&NodeId>) -> Result<Option<Vec<String>>, StorageError> {
        let Some(root) = root else {
            return Ok(None);
        };
        Ok(self
            .store()
            .conversation(conversation_of(root))
            .await?
            .and_then(|record| record.plugins))
    }
}

/// What a conversation's tree opens with: the commands of the plugins the
/// user has on, with the product's `demi device`, `demi attachment`, `demi
/// conversation`, `demi credentials`, `demi providers` and `demi notify`
/// groups.
pub(crate) struct CoreToolsets {
    /// Weak: the core owns the agent server that holds the source.
    pub(crate) core: Weak<Core>,
}

impl ToolsetSource for CoreToolsets {
    fn revision<'a>(&'a self, root: Option<&'a NodeId>) -> LocalBoxFuture<'a, Result<Rc<str>, String>> {
        Box::pin(async move {
            let core = self
                .core
                .upgrade()
                .ok_or_else(|| "the backend is shutting down".to_owned())?;
            let named = core.named_plugins(root).await.map_err(|error| error.to_string())?;
            let revision = core
                .plugins()
                .revision(named.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            Ok(Rc::from(revision))
        })
    }

    fn current<'a>(&'a self, root: Option<&'a NodeId>) -> LocalBoxFuture<'a, Result<Toolset, String>> {
        Box::pin(async move {
            let core = self
                .core
                .upgrade()
                .ok_or_else(|| "the backend is shutting down".to_owned())?;
            let named = core.named_plugins(root).await.map_err(|error| error.to_string())?;
            let devices: Weak<dyn DeviceCore> = self.core.clone();
            let toolset = core
                .plugins()
                .toolset(vec![
                    device_group(devices.clone()),
                    attachment_group(devices),
                    conversation_group(self.core.clone()),
                    credentials_group(self.core.clone()),
                    schedule_group(self.core.clone()),
                    providers_group(self.core.clone()),
                    notify_group(self.core.clone()),
                ], named.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            Ok(Toolset {
                commands: Rc::new(toolset.commands),
                revision: Rc::from(toolset.revision),
            })
        })
    }
}

/// The user's subagent settings as the control store holds them now, and
/// the changes `demi agent profiles` makes to them through the settings
/// page's path.
pub(crate) struct CoreSubagents {
    /// Weak: the core owns the agent server that holds the source.
    pub(crate) core: Weak<Core>,
}

impl CoreSubagents {
    fn core(&self) -> Result<Rc<Core>, String> {
        self.core
            .upgrade()
            .ok_or_else(|| "the backend is shutting down".to_owned())
    }
}

impl SubagentSource for CoreSubagents {
    fn current(&self) -> LocalBoxFuture<'_, Result<SubagentSettings, String>> {
        Box::pin(async move {
            let core = self.core()?;
            let settings = core
                .store().subagent_settings()
                .await
                .map_err(|error| error.to_string())?;
            let entries = entries(&core).await.map_err(|error| error.to_string())?;
            let profiles = settings
                .profiles
                .into_iter()
                .map(|profile| profile_of(profile, &entries))
                .collect();
            Ok(SubagentSettings {
                enabled: settings.enabled,
                max_live: settings.max_live.bound(),
                max_depth: settings.max_depth.bound(),
                profiles,
            })
        })
    }

    fn runs_on<'a>(&'a self, selections: &'a [ModelSelection]) -> LocalBoxFuture<'a, Vec<JobModel>> {
        Box::pin(async move {
            let Ok(core) = self.core() else {
                // The backend is shutting down: the frames name what the
                // selections hold.
                return selections
                    .iter()
                    .map(|selection| job_model(selection, None, None))
                    .collect();
            };
            runs_on(core.services(), core.user(), selections).await
        })
    }

    fn create_profile(
        &self,
        profile: NewProfileDraft,
    ) -> LocalBoxFuture<'_, Result<Profile, String>> {
        Box::pin(async move {
            let core = self.core()?;
            let created = core.create_drafted_profile(profile).await?;
            Ok(answered(&core, created).await)
        })
    }

    fn edit_profile<'a>(
        &'a self,
        name: &'a str,
        edit: ProfileEdit,
    ) -> LocalBoxFuture<'a, Result<Profile, String>> {
        Box::pin(async move {
            let core = self.core()?;
            let changed = core.edit_named_profile(name, edit).await?;
            Ok(answered(&core, changed).await)
        })
    }

    fn delete_profile<'a>(&'a self, name: &'a str) -> LocalBoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.core()?.delete_named_profile(name).await })
    }

    fn conversation_profiles<'a>(
        &'a self,
        root: &'a NodeId,
    ) -> LocalBoxFuture<'a, Result<ConversationProfiles, String>> {
        Box::pin(async move {
            self.core()?
                .conversation_profiles(&conversation_of(root))
                .await
        })
    }

    fn workflows<'a>(&'a self, root: &'a NodeId) -> LocalBoxFuture<'a, Result<WorkflowListing, String>> {
        Box::pin(async move { self.core()?.workflow_listing(&conversation_of(root)).await })
    }

    fn choose_workflow<'a>(
        &'a self,
        root: &'a NodeId,
        choice: WorkflowChoice,
    ) -> LocalBoxFuture<'a, Result<(WorkflowChoice, Option<String>), String>> {
        Box::pin(async move {
            self.core()?
                .choose_workflow(&conversation_of(root), choice)
                .await
        })
    }

    fn read_workflow_file<'a>(
        &'a self,
        root: &'a NodeId,
        directory: &'a str,
        path: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.core()?
                .workflow_file_text(&conversation_of(root), directory, path)
                .await
        })
    }
}

/// The provider entries in the user's scope.
pub(crate) async fn entries(core: &Core) -> Result<Vec<ProviderEntry>, StorageError> {
    core.services().vault.entries_for(core.user()).await
}

/// The profile a change wrote, as the agent reads it.
async fn answered(core: &Core, profile: SubagentProfile) -> Profile {
    // The write is done: entries that cannot be read only leave the entry's
    // name out of the answer.
    let entries = entries(core).await.unwrap_or_default();
    profile_of(profile, &entries)
}

/// A stored profile as the agent reads it, with its entry's name among
/// `entries`.
fn profile_of(profile: SubagentProfile, entries: &[ProviderEntry]) -> Profile {
    Profile {
        name: profile.name,
        description: profile.description,
        model: profile.model.map(|model| ProfileModel {
            provider_name: entries
                .iter()
                .find(|entry| entry.id == model.provider_id)
                .map(|entry| entry.label.clone()),
            provider_id: model.provider_id.as_str().to_owned(),
            model_id: model.model_id,
            thinking_effort: model.thinking_effort,
            service_tier_id: model.service_tier_id,
        }),
        context_limit: profile.context_limit,
        instructions: profile.instructions,
        can_spawn: profile.can_spawn,
        enabled: profile.enabled,
    }
}

/// Where a core's agents run: a subagent's own device, and each conversation's
/// current primary device for its root.
pub(crate) struct CoreDevices {
    /// Weak: the core owns the agent server that holds the resolver.
    pub(crate) core: Weak<Core>,
}

impl DeviceResolver for CoreDevices {
    type Device = RemoteDevice;

    async fn device(&self, context: NodeContext<'_>) -> Result<Rc<RemoteDevice>, DeviceError> {
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| DeviceError::offline("the backend is shutting down"))?;
        let conversation = conversation_of(context.root);
        match agent_device(context.placement) {
            Some(device) => core.device_core().agent_device(&conversation, &device).await,
            None => core.device_core().conversation_device(&conversation).await,
        }
    }

    async fn adopted_device(&self, context: NodeContext<'_>) -> Result<Rc<RemoteDevice>, DeviceError> {
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| DeviceError::offline("the backend is shutting down"))?;
        let conversation = conversation_of(context.root);
        match agent_device(context.placement) {
            Some(device) => core.device_core().adopting_agent_device(&conversation, &device).await,
            None => core.device_core().adopting_device(&conversation).await,
        }
    }

    async fn device_name(&self, device: &str) -> Option<String> {
        let core = self.core.upgrade()?;
        let device = DeviceId::try_from(device.to_owned()).ok()?;
        core.store().device(device).await.ok().flatten().map(|record| record.name)
    }

    /// The conversation's target, read without reaching the device.
    async fn placement(&self, context: NodeContext<'_>) -> Result<Placement, DeviceError> {
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| DeviceError::offline("the backend is shutting down"))?;
        core
            .placement(&conversation_of(context.root))
            .await
            .map_err(|error| DeviceError::new(DeviceErrorKind::Failed { code: None }, error.to_string()))
    }
}

/// The device of a subagent's own device, by its `placement`; none for the
/// root, and for a subagent on the user's Cloud not made yet, which works
/// where the conversation does until it is made.
fn agent_device(placement: Option<&Placement>) -> Option<DeviceId> {
    let device = placement?.device.clone()?;
    DeviceId::try_from(device).ok()
}

/// The context source that announces a change of the conversation's
/// execution context: a target switch, attached devices, a Cloud reset.
pub(crate) struct ExecutionContext {
    pub(crate) core: Weak<Core>,
}

impl ContextSource for ExecutionContext {
    fn name(&self) -> &str {
        EXECUTION_SOURCE
    }

    fn context<'a>(
        &'a self,
        node: NodeContext<'a>,
        _turn: &'a TurnId,
        seen: &'a [&'a str],
    ) -> LocalBoxFuture<'a, Result<Option<ContextAnswer>, String>> {
        Box::pin(async move {
            let Some(core) = self.core.upgrade() else {
                return Ok(None);
            };
            core
                .execution_context(&conversation_of(node.root), seen)
                .await
                .map(|text| text.map(ContextAnswer::from))
                .map_err(|error| format!("the execution context cannot be read: {error}"))
        })
    }
}

/// A plugin that is a context source, asked while its user has it on
/// (`crates/plugin-interface/src/manifest.rs`).
pub(crate) struct PluginContext {
    pub(crate) core: Weak<Core>,
    pub(crate) plugin: PluginId,
}

impl ContextSource for PluginContext {
    fn name(&self) -> &str {
        self.plugin.as_str()
    }

    fn context<'a>(
        &'a self,
        node: NodeContext<'a>,
        turn: &'a TurnId,
        seen: &'a [&'a str],
    ) -> LocalBoxFuture<'a, Result<Option<ContextAnswer>, String>> {
        Box::pin(async move {
            let Some(core) = self.core.upgrade() else {
                return Ok(None);
            };
            // A program's plugin the conversation does not name is neither
            // searched nor asked, as it gives the conversation no commands.
            let named = core
                .named_plugins(Some(node.root))
                .await
                .map_err(|error| format!("the plugin {} was not asked: {error}", self.plugin))?;
            let conversation = conversation_of(node.root);
            let paths = core
                .primary_device_paths(&conversation)
                .await
                .map_err(|error| format!("the plugin {} was not asked: {error}", self.plugin))?;
            let cwd = core
                .node_placement(node)
                .await
                .map_err(|error| format!("the plugin {} was not asked: {error}", self.plugin))?
                .directory;
            // A plugin the user has off keeps nothing, and is not asked.
            let searches = core
                .plugins()
                .searches_projects(&self.plugin, named.as_deref())
                .await
                .map_err(|error| format!("the plugin {} was not asked: {error}", self.plugin))?;
            let project = if searches {
                core.kept_project_search(&self.plugin, node.root).await
            } else {
                None
            };
            let asked = ContextAsk {
                conversation,
                node: node.node.clone(),
                cwd,
                paths,
                turn: turn.clone(),
                seen: seen.iter().map(|text| (*text).to_owned()).collect(),
                project,
            };
            // The request ends with the provider request it serves, which
            // drops this future.
            core
                .plugins()
                .context(&self.plugin, named.as_deref(), asked, CancellationToken::new())
                .await
                .map(|text| text.map(ContextAnswer::from))
                .map_err(|error| format!("the plugin {} gave no context: {error}", self.plugin))
        })
    }
}

/// What the backend keeps of a conversation's agents that the tree shows:
/// each agent's cost and, read apart, its cache chart, from the usage ledger
/// (`crates/core-store/src/usage.rs`), and each side question's card.
pub(crate) struct CoreAgents {
    /// Weak: the core owns the agent server that holds the source.
    pub(crate) core: Weak<Core>,
}

impl demi_agent_server::AgentsSource for CoreAgents {
    fn resuming(&self, root: &NodeId) -> bool {
        self.core
            .upgrade()
            .is_some_and(|core| core.restart_resumes().contains(&demi_core_device_access::conversation_of(root)))
    }

    fn kept<'a>(&'a self, root: &'a NodeId) -> LocalBoxFuture<'a, demi_agent_server::KeptAgents> {
        Box::pin(async move {
            let mut kept = demi_agent_server::KeptAgents::default();
            let Some(core) = self.core.upgrade() else {
                return kept;
            };
            let conversation = demi_core_device_access::conversation_of(root);
            match core.store().conversation_costs(conversation.clone()).await {
                Ok(costs) => {
                    kept.costs = costs
                        .into_iter()
                        .filter_map(|kept| Some((kept.agent?, kept.cost)))
                        .collect();
                }
                // The entries show no cost until the agent's next row.
                Err(error) => tracing::warn!(
                    event = "conversation_state.read_failed",
                    %conversation,
                    what = "kept_costs",
                    error = &error as &dyn std::error::Error,
                    "a conversation's kept costs were not read"
                ),
            }
            match core
                .store()
                .databases()
                .read(&conversation, demi_core_store::side_questions::cards)
                .await
            {
                Ok(cards) => {
                    kept.cards = cards
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(id, title, folded)| (id, demi_agent_server::SideCard { title, folded }))
                        .collect();
                }
                // The cards show untitled and open until they change.
                Err(error) => tracing::warn!(
                    event = "conversation_state.read_failed",
                    %conversation,
                    what = "side_cards",
                    error = &error as &dyn std::error::Error,
                    "a conversation's side question cards were not read"
                ),
            }
            kept
        })
    }

    fn charts<'a>(
        &'a self,
        root: &'a NodeId,
    ) -> LocalBoxFuture<'a, std::collections::HashMap<u64, CacheChart>> {
        Box::pin(async move {
            let Some(core) = self.core.upgrade() else {
                return std::collections::HashMap::new();
            };
            let conversation = demi_core_device_access::conversation_of(root);
            match core.store().conversation_charts(conversation.clone()).await {
                Ok(charts) => charts.into_iter().collect(),
                // The cards show no rate and no chart until the agent's next
                // turn call.
                Err(error) => {
                    tracing::warn!(
                        event = "conversation_state.read_failed",
                        %conversation,
                        what = "cache_charts",
                        error = &error as &dyn std::error::Error,
                        "a conversation's cache charts were not read"
                    );
                    std::collections::HashMap::new()
                }
            }
        })
    }
}

/// Which paths a complete reply names are files on its conversation's primary
/// device: one look for them all, admitted as a user's look is, which never
/// wakes a stopped Cloud (`crates/core-device-access/src/stream.rs`).
pub(crate) struct CoreReplyFiles {
    /// Weak: the core owns the agent server that holds the source.
    pub(crate) core: Weak<Core>,
}

impl demi_agent_server::ReplyFiles for CoreReplyFiles {
    fn files<'a>(
        &'a self,
        root: &'a NodeId,
        cwd: &'a str,
        paths: Vec<String>,
    ) -> LocalBoxFuture<'a, Option<std::collections::HashMap<String, String>>> {
        Box::pin(async move {
            let core = self.core.upgrade()?;
            let conversation = conversation_of(root);
            let device_paths = match core.primary_device_paths(&conversation).await {
                Ok(device_paths) => device_paths,
                Err(error) => {
                    tracing::warn!(event = "conversation_state.read_failed", %conversation, what = "reply_paths", error = &error as &dyn std::error::Error, "a reply's code paths were not looked at");
                    return None;
                }
            };
            let resolved: Vec<(String, String)> = paths
                .into_iter()
                .map(|path| {
                    let on_device = device_paths.resolve(cwd, &path);
                    (path, on_device)
                })
                .collect();
            // Released with the core's tasks' cancellation, which a look
            // needs only to end its wait.
            let cancel = tokio_util::sync::CancellationToken::new();
            let looked = core
                .device_core()
                .look_at_device(&conversation, &cancel, async move |device| {
                    use demi_device_interface::DeviceFs as _;
                    let stats = futures_util::future::join_all(
                        resolved.iter().map(|(_, on_device)| device.device.stat(on_device.as_str())),
                    )
                    .await;
                    resolved
                        .into_iter()
                        .zip(stats)
                        .filter(|(_, stat)| {
                            matches!(stat, Ok(demi_device_interface::FileStat { kind: demi_device_interface::FileKind::File, .. }))
                        })
                        .map(|(paths, _)| paths)
                        .collect::<std::collections::HashMap<String, String>>()
                })
                .await;
            match looked {
                Ok(files) => Some(files),
                // A stopped Cloud or an offline device cannot say; the reply's
                // code stays code.
                Err(error) => {
                    tracing::warn!(event = "conversation_state.read_failed", %conversation, what = "reply_paths", %error, "a reply's code paths could not be looked at");
                    None
                }
            }
        })
    }

    fn copy_picture<'a>(
        &'a self,
        root: &'a NodeId,
        node: &'a NodeId,
        device: Option<&'a str>,
        cwd: &'a str,
        path: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, PictureFailure>> {
        Box::pin(async move {
            // The backend closing, a conversation with no device yet, a device
            // that is no device and a home not known are each a device the
            // picture cannot be copied from.
            let core = self.core.upgrade().ok_or(PictureFailure::DeviceUnavailable)?;
            let conversation = conversation_of(root);
            let device = device
                .and_then(|device| demi_web_api_protocol::ids::DeviceId::try_from(device).ok())
                .ok_or(PictureFailure::DeviceUnavailable)?;
            // A path from the device's home is read from the home its runner
            // reported.
            let paths = core
                .primary_device_paths(&conversation)
                .await
                .map_err(|_| PictureFailure::DeviceUnavailable)?;
            let path = match paths.home_relative(path) {
                Some(_) => {
                    let home = core.devices().home(&device).ok_or(PictureFailure::DeviceUnavailable)?;
                    paths.from_home(&home, path)
                }
                None => path.to_owned(),
            };
            demi_core_device_access::attachment_commands::copy_picture(
                core.device_core(),
                &conversation,
                &device,
                cwd,
                node.clone(),
                &path,
            )
            .await
        })
    }
}
