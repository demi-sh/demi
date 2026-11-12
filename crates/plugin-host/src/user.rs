//! One user's plugins on the user's core.
//!
//! The module holds:
//!
//! - Which plugins the user has on, which decides the toolset of the user's
//!   conversations and which page states, page calls and user streams exist,
//!   and which of their features, which narrows the toolset
//!   (`crate::features`).
//! - An instance of each plugin on, made at its first request after the core
//!   started or the plugin was turned on, and dropped, with every task it
//!   started, when it is turned off. A conversation's deletion or fork reaches
//!   a plugin that keeps something of a conversation on or off, through an
//!   instance made for it alone.
//! - The context sources among them.
//! - The device directories of the plugins on.
//! - The answers to their port operations.
//!
//! The plugin host answers values, directories, changes, secrets, outbound
//! requests and sign-ins itself. What reaches the user's blobs, a
//! conversation's devices or the user's provider entries goes to the product,
//! which owns the blob namespace, the conversation's device access and the
//! providers. The product also owns the sealing of a secret, the client an
//! outbound request is sent with, the public URL and the time.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::store::StoreError;
use crate::store::{MirrorOutcome, WorkOutcome};
use crate::store::{PluginChanges, PluginStore, ValueWrite, Written};
use demi_command_declarations::NativeOperation;
use demi_device_interface::{CommandSet, GroupBuilder, JobCaller, PortError, RpcPort};
use demi_plugin_interface::{
    AskedVideo, AudioAsk, AudioVoices, Feature, ImageAsk, MadeAudio, MadeImage, MediaListing, MediaRefusal, VideoAsk,
    VideoStep, CallKind, DirectoryPath, DeviceDirectory, Plugin, PluginError, PluginId, PluginPort,
    PluginTransport, PortAnswer, PortFailure, PortMessage, PortRefusal, Reply, Request, Scope, StoredReport,
    StoredValue, UserWorkspaces, DeviceName, GitTree,
};
use demi_device_interface::{DeviceFile, DeviceRead};
use demi_shared_types::{B64Bytes, BlobRef, MediaKind, NodeId, Timestamp, TurnId};
use demi_web_api_protocol::conversations::PluginRevision;
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::ids::{ConversationId, DeviceId, UserId};
use demi_web_api_protocol::work::{CreateWorkTab, HeldState, WorkChange, WorkEffect, Work};
use demi_web_api_protocol::plugins::{PluginEntry, PluginStateAnswer};
use demi_web_api_protocol::workflows::WatchSubject;
use futures_util::StreamExt as _;
use futures_util::future::LocalBoxFuture;
use futures_util::stream::LocalBoxStream;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::commands::compose;
use crate::registry::{Plugins, Registry};
use crate::oauth::{Callback, CallbackError, SignIns};
use crate::web;

/// A video's job as the product follows it: each change of its state, then
/// its video or the failure that ended it. Dropped, only the waiting ends:
/// the job is the product's to follow to its end, and only a stop of the
/// call cancels it.
pub type VideoJob = LocalBoxStream<'static, Result<VideoStep, PortFailure>>;

/// The call a media request belongs to, by which the product keeps the job
/// a vendor took for it.
#[derive(Clone)]
pub struct MediaCall {
    /// The `rpc` call's id, which the runner keeps when it sends the call
    /// again after a restart; a request of no call has one of its own.
    pub id: String,
    /// The request's place among the call's media requests, the first 0.
    pub ordinal: u32,
    /// Whether the runner sent the call again after a restart: the request
    /// follows the job the first one asked for rather than asking again.
    pub resumed: bool,
    /// The call's port, for a command's call.
    pub rpc: Option<RpcPort>,
}

impl MediaCall {
    /// Says the call waits for a job the product keeps, which a call sent
    /// again after a restart finds ([`RpcPort::resumable`]).
    pub async fn resumable(&self) {
        if let Some(rpc) = &self.rpc
            && let Err(error) = rpc.resumable().await
        {
            tracing::warn!(event = "rpc_call.resumable_unsent", %error, "a call was not marked resumable");
        }
    }

    /// Whether the call, stopped, comes back ([`RpcPort::comes_back`]).
    pub fn comes_back(&self) -> bool {
        self.rpc.as_ref().is_some_and(RpcPort::comes_back)
    }
}

/// What the product does for the plugins' port: the operations that reach
/// a conversation's devices through its device access, and the user's blobs.
/// The user's core implements it.
pub trait ProductPort {
    /// Runs `operation` once on the conversation's primary device for the user,
    /// waking it or not as `kind` says, and answers its JSON result.
    fn package_call<'a>(
        &'a self,
        conversation: &'a ConversationId,
        operation: &'a NativeOperation,
        args: Map<String, Value>,
        input: String,
        kind: CallKind,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<Value, PortFailure>>;

    /// Reads `reads` where `on` says in the form that never wakes the device;
    /// [`PortRefusal::NotRunning`] when it is not running, or, on the
    /// primary device of a device, when it is no longer that device's.
    fn read_device_files<'a>(
        &'a self,
        conversation: &'a ConversationId,
        on: &'a ReadOn,
        reads: Vec<DeviceRead>,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<Vec<DeviceFile>, PortFailure>>;

    /// The models of `kind` of the user's provider entries, each with the
    /// entries that offer it.
    fn media_models(&self, kind: MediaKind) -> LocalBoxFuture<'_, Result<MediaListing, PortFailure>>;

    /// The user's paired devices, each with whether its runner is connected.
    fn devices(&self) -> LocalBoxFuture<'_, Result<Vec<DeviceName>, PortFailure>>;

    /// The commit `url`'s default branch points to, as the user's paired
    /// device `device` reads it with its own `git`.
    fn device_git_head<'a>(&'a self, device: &'a DeviceId, url: &'a str) -> LocalBoxFuture<'a, Result<String, PortFailure>>;

    /// The newest commit of `url`'s default branch as `device` clones it
    /// with its own `git` ([`PortMessage::DeviceGitFetch`]).
    fn device_git_fetch<'a>(
        &'a self,
        device: &'a DeviceId,
        url: &'a str,
        marker: &'a str,
    ) -> LocalBoxFuture<'a, Result<GitTree, PortFailure>>;

    /// Makes the image `ask` names for the conversation, through the entry
    /// and account the providers choose; its usage ledger row names the
    /// agent and the turn of `caller`, the job that called the command, if
    /// any. A vendor's job is kept as [`ProductPort::make_video`] keeps it.
    fn make_image<'a>(
        &'a self,
        conversation: &'a ConversationId,
        caller: Option<&'a JobCaller>,
        call: &'a MediaCall,
        ask: ImageAsk,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<MadeImage, PortFailure>>;

    /// Asks for the video `ask` names for the conversation, through the
    /// entry and account the providers choose, and answers the job the
    /// vendor took with its steps; its usage ledger row names the agent and
    /// the turn of `caller`. A resumed `call` follows the job its first
    /// asked for. `cancel` stops the call and cancels the job where the
    /// vendor can, unless the product is closing.
    fn make_video<'a>(
        &'a self,
        conversation: &'a ConversationId,
        caller: Option<&'a JobCaller>,
        call: &'a MediaCall,
        ask: VideoAsk,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<(AskedVideo, VideoJob), PortFailure>>;

    /// Makes the audio `ask` names for the conversation, through the entry
    /// the providers choose; its usage ledger row names the agent and the
    /// turn of `caller`. A vendor's job is kept as
    /// [`ProductPort::make_video`] keeps it.
    fn make_audio<'a>(
        &'a self,
        conversation: &'a ConversationId,
        caller: Option<&'a JobCaller>,
        call: &'a MediaCall,
        ask: AudioAsk,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<MadeAudio, PortFailure>>;

    /// The voices of speech model `model`, as the entry a call of it takes
    /// lists them, narrowed by `search`.
    fn audio_voices<'a>(
        &'a self,
        conversation: &'a ConversationId,
        model: String,
        search: Option<String>,
        cancel: &'a CancellationToken,
    ) -> LocalBoxFuture<'a, Result<AudioVoices, PortFailure>>;

    /// Stores `bytes` in the user's blob namespace.
    fn put_blob(&self, bytes: B64Bytes) -> LocalBoxFuture<'_, Result<BlobRef, PortFailure>>;

    fn get_blob(&self, blob: BlobRef) -> LocalBoxFuture<'_, Result<Option<B64Bytes>, PortFailure>>;

    /// `plaintext` sealed as `user`'s secret `name` of `plugin`, by the one
    /// owner of sealing (`crates/backend/src/config/secret.rs`).
    fn seal(&self, user: &UserId, plugin: &str, name: &str, plaintext: &[u8]) -> Vec<u8>;

    /// What `sealed` holds as that secret; none when it does not open.
    fn open(&self, user: &UserId, plugin: &str, name: &str, sealed: &[u8]) -> Option<Vec<u8>>;

    /// The client every outbound request of the plugins goes through: the core's, which sends only
    /// `https` and follows no redirect away from it.
    fn web_client(&self) -> reqwest::Client;

    /// Where the core's routes are reached from outside, under `/api`,
    /// which an OAuth redirect URI starts with; none while it is not known.
    fn public_url(&self) -> Option<Url>;

    /// The wall clock.
    fn now(&self) -> Timestamp;

    /// Runs `work` as a task of the user's core, which the core's close
    /// waits for: work that runs to its end though whoever started it went
    /// away, such as a token renewal that asked its vendor. A core already
    /// gone drops `work` unstarted.
    fn spawn(&self, work: LocalBoxFuture<'static, ()>);
}

/// Where a request's reads of files go (`crates/plugin-interface/src/lib.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOn {
    /// The conversation's primary device.
    Primary,
    /// The primary device, while it is this device's, as a project search
    /// asked of it.
    PrimaryOf(DeviceId),
    /// A command's job: the device it runs on, the conversation's primary
    /// device or an attached one, with a relative path read against the
    /// job's directory.
    Job { device: DeviceId, directory: String },
}

/// A page's call of a plugin method.
#[derive(Debug, Clone)]
pub struct PageCall {
    pub plugin: String,
    pub method: String,
    /// The JSON body, which the method's schema checks.
    pub params: Value,
    /// The conversation of the route, for a method of the conversation
    /// scope.
    pub conversation: Option<ConversationId>,
}

/// Why a page call did not answer.
#[derive(Debug, thiserror::Error)]
pub enum PageCallError {
    #[error("No plugin \"{0}\"")]
    UnknownPlugin(String),
    #[error("The plugin \"{0}\" is off")]
    Disabled(String),
    #[error("The plugin \"{plugin}\" has no method \"{method}\" here")]
    UnknownMethod { plugin: String, method: String },
    #[error("{0}")]
    InvalidParams(String),
    #[error(transparent)]
    Plugin(#[from] PluginError),
}

/// Why a change of a work was refused.
#[derive(Debug, thiserror::Error)]
pub enum WorkError {
    #[error("No plugin the user has on declares the tab kind \"{0}\"")]
    UnknownKind(String),
    #[error("The conversation holds as many tabs, or as much of their data, as it can")]
    Full,
    #[error("The conversation's tabs would be over their size")]
    TooLarge,
    #[error("No conversation of that id")]
    Missing,
    #[error(transparent)]
    Storage(#[from] StoreError),
}

impl WorkError {
    /// The refusal as the port gives it to a plugin.
    fn refusal(self) -> PortFailure {
        let code = match &self {
            Self::UnknownKind(_) => ErrorCode::UnknownTabKind,
            Self::Full => ErrorCode::WorkFull,
            Self::TooLarge => ErrorCode::TooLarge,
            Self::Missing => ErrorCode::ConversationNotFound,
            Self::Storage(_) => {
                let Self::Storage(error) = self else {
                    unreachable!("matched as a storage failure")
                };
                return storage(error);
            }
        };
        PortFailure::Refused(PortRefusal::Work {
            code,
            message: self.to_string(),
        })
    }
}

/// Why a plugin could not be turned on or off.
#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    #[error("No plugin \"{0}\"")]
    UnknownPlugin(String),
    #[error("The plugin \"{plugin}\" has no feature \"{feature}\"")]
    UnknownFeature { plugin: String, feature: String },
    #[error(transparent)]
    Storage(#[from] StoreError),
}

/// What follows from a plugin turned on or off that the product does.
#[derive(Debug)]
pub struct Switched {
    /// The packages whose residents every connected device stops, which ends
    /// what they hold for every conversation: those of a plugin turned off
    /// that no plugin still on binds.
    pub released: Vec<String>,
}

/// What a core does once its user's plugins adopted the program's newest
/// ([`UserPlugins::adopt`]).
pub struct Adopted {
    /// The plugins declared, declared again or removed since the last
    /// adoption.
    pub changed: Vec<PluginId>,
    /// The packages the removed plugins bound that no plugin on binds, whose
    /// residents stop.
    pub released: Vec<String>,
}

/// The commands and revision of the plugins a user has on, which a
/// conversation's tree opens with.
pub struct PluginToolset {
    pub commands: CommandSet,
    /// The ids of the plugins on that declare commands, in registration
    /// order: two sets of the same commands have the same revision.
    pub revision: String,
}

/// A plugin's instance while the user has it on.
#[derive(Clone)]
struct Live {
    instance: Rc<dyn Plugin>,
    /// Cancelled when the user turns the plugin off, which ends its requests
    /// under way and its open user streams.
    ended: CancellationToken,
}

/// What every request of the user's plugins shares.
pub(crate) struct Shared {
    /// The program's plugins, whose newest registry each read takes.
    plugins: Arc<Plugins>,
    /// The registry the core adopted last, which the next adoption tells
    /// what changed against.
    adopted: RefCell<Arc<Registry>>,
    pub(crate) user: UserId,
    /// The instance of each plugin the user has on, by its index, once a
    /// request made it.
    lives: RefCell<HashMap<usize, Live>>,
    pub(crate) product: Rc<dyn ProductPort>,
    pub(crate) store: Rc<dyn PluginStore>,
    /// The user's OAuth sign-ins under way, and the turns of their renewals.
    pub(crate) sign_ins: SignIns,
    changes: Rc<dyn PluginChanges>,
    /// The user's choice of each plugin, by its id, once read: every switch
    /// goes through this core, so the copy stays current. A plugin the user
    /// never chose for is on.
    switches: RefCell<Option<BTreeMap<String, bool>>>,
    /// The user's choice of each feature, by the plugin's id and the
    /// feature's, once read, as `switches` is.
    feature_switches: RefCell<Option<BTreeMap<(String, String), bool>>>,
    /// Held by a switch of a plugin or of a feature from its read of the
    /// choices until it applied its own, so two switches at once never
    /// apply one over the other.
    switching: tokio::sync::Mutex<()>,
    /// Cancelled when the user next turns a plugin on or off, which makes
    /// what a direct channel's introduction told of the user's streams out
    /// of date (`crates/runner-direct/src/lib.rs`).
    switched: RefCell<CancellationToken>,
    /// Each plugin's device directories for the user, by its id, once read:
    /// every change goes through this core.
    directories: RefCell<Option<BTreeMap<String, Vec<DeviceDirectory>>>>,
    /// How many times each plugin's conversation state changed since the
    /// core started, by the plugin's index and the conversation; a pair
    /// without an entry has changed none.
    revisions: RefCell<HashMap<(usize, ConversationId), u64>>,
    /// The conversations that run, as the shard last said.
    running: RefCell<HashSet<ConversationId>>,
    /// The conversations that run, as the plugins that follow it were
    /// last told: `running` a moment ago, until the telling caught up.
    told_running: RefCell<HashSet<ConversationId>>,
}

impl Shared {
    /// The program's plugins now.
    pub(crate) fn registry(&self) -> Arc<Registry> {
        self.plugins.current()
    }

    /// Sends `request` to plugin `plugin`, with a port for the request's
    /// conversation and, for a command, the call's rpc port.
    pub(crate) async fn request(
        self: &Rc<Self>,
        plugin: usize,
        request: Request,
        conversation: Option<ConversationId>,
        rpc: Option<RpcPort>,
        cancel: CancellationToken,
    ) -> Result<Reply, PluginError> {
        self.request_on(plugin, request, conversation, None, rpc, cancel).await
    }

    /// [`Shared::request`], whose reads of the conversation's files are
    /// bound to the primary device of `device` when one is named.
    pub(crate) async fn request_on(
        self: &Rc<Self>,
        plugin: usize,
        request: Request,
        conversation: Option<ConversationId>,
        device: Option<DeviceId>,
        rpc: Option<RpcPort>,
        cancel: CancellationToken,
    ) -> Result<Reply, PluginError> {
        let (caller, read_on, call) = match (&request, device) {
            (Request::Command { invocation, .. }, _) => {
                let caller = invocation.caller.clone();
                // A device name that is no device's reads nowhere; the read
                // answers that the device is not running.
                let read_on = match DeviceId::try_from(invocation.device.as_str()) {
                    Ok(device) => ReadOn::Job { device, directory: invocation.cwd.clone() },
                    Err(_) => ReadOn::Primary,
                };
                let call = (!invocation.call_id.is_empty()).then(|| (invocation.call_id.clone(), invocation.resumed));
                (caller, read_on, call)
            }
            (_, Some(device)) => (None, ReadOn::PrimaryOf(device), None),
            (_, None) => (None, ReadOn::Primary, None),
        };
        let call = call.unwrap_or_else(|| (format!("{:032x}", rand::random::<u128>()), false));
        let transport = RequestPort {
            shared: self.clone(),
            plugin,
            conversation,
            rpc,
            cancel: cancel.clone(),
            read_on,
            caller,
            call,
            media_asked: std::cell::Cell::new(0),
            video: RefCell::new(None),
        };
        let registry = self.registry();
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        // A conversation's deletion or fork reaches a plugin that is off too,
        // through an instance made for it alone, so what it keeps of the
        // conversation follows the conversation whatever the switch says; so
        // does a read of its reply blocks' facts, as a block keeps its room
        // while its plugin is off.
        let lifecycle = matches!(
            request,
            Request::ConversationDeleted { .. } | Request::ConversationForked { .. } | Request::ReplyBlocks { .. }
        );
        let live = match self.live(&registry, plugin, &enabled) {
            Some(live) => live,
            // A removed plugin keeps nothing of a conversation any more.
            None if lifecycle && !registry.plugins[plugin].removed => Live {
                instance: registry.plugins[plugin].factory.instance(&self.user),
                ended: CancellationToken::new(),
            },
            None => {
                return Err(PluginError::Ended {
                    message: format!("The plugin \"{}\" is off", self.plugin_id(plugin)),
                });
            }
        };
        let port = PluginPort::new(Rc::new(transport), cancel);
        // A request the switch cut off ends here, which drops what it was
        // doing; the instance goes once the last request holding it ended.
        tokio::select! {
            reply = live.instance.call(request, port) => reply,
            () = live.ended.cancelled() => Err(PluginError::Ended {
                message: format!("The plugin \"{}\" was turned off", self.plugin_id(plugin)),
            }),
        }
    }

    /// The instance of plugin `plugin`, made now when it has none; none
    /// while `enabled`, the user's switches in `registry`, has it off.
    fn live(&self, registry: &Registry, plugin: usize, enabled: &[bool]) -> Option<Live> {
        if !enabled[plugin] {
            return None;
        }
        let mut lives = self.lives.borrow_mut();
        let live = lives.entry(plugin).or_insert_with(|| Live {
            instance: registry.plugins[plugin].factory.instance(&self.user),
            ended: CancellationToken::new(),
        });
        Some(live.clone())
    }

    /// The user's choices of the plugins, by their ids.
    async fn switches(&self) -> Result<BTreeMap<String, bool>, StoreError> {
        if let Some(switches) = &*self.switches.borrow() {
            return Ok(switches.clone());
        }
        let switches = self.store.switches().await?;
        self.switches.replace(Some(switches.clone()));
        Ok(switches)
    }

    /// Whether the user has each plugin of `registry` on, by its index: a
    /// removed one is off.
    async fn enabled(&self, registry: &Registry) -> Result<Vec<bool>, StoreError> {
        let switches = self.switches().await?;
        Ok(registry
            .plugins
            .iter()
            .map(|registered| {
                !registered.removed && switches.get(registered.id().as_str()).copied().unwrap_or(true)
            })
            .collect())
    }

    /// Whether the user has each feature of each plugin of `registry` on,
    /// by the plugin's index and the feature's.
    async fn features(&self, registry: &Registry) -> Result<Vec<Vec<bool>>, StoreError> {
        let cached = self.feature_switches.borrow().clone();
        let switches = match cached {
            Some(switches) => switches,
            None => {
                let switches = self.store.feature_switches().await?;
                self.feature_switches.replace(Some(switches.clone()));
                switches
            }
        };
        Ok(registry
            .plugins
            .iter()
            .map(|registered| {
                let plugin = registered.id().as_str();
                registered
                    .factory
                    .manifest()
                    .features
                    .iter()
                    .map(|feature| {
                        switches
                            .get(&(plugin.to_owned(), feature.id.clone()))
                            .copied()
                            .unwrap_or(feature.on_by_default)
                    })
                    .collect()
            })
            .collect())
    }

    pub(crate) fn plugin_id(&self, plugin: usize) -> String {
        self.registry().plugins[plugin].id().as_str().to_owned()
    }

    /// The device the conversation works on, from the records: none for a
    /// Cloud conversation whose Cloud was never made. It asks no device.
    async fn primary_device(&self, conversation: &ConversationId) -> Result<Option<DeviceId>, PortFailure> {
        self.store.primary_device(conversation.clone()).await.map_err(storage)
    }

    fn revision(&self, plugin: usize, conversation: &ConversationId) -> u64 {
        let key = (plugin, conversation.clone());
        self.revisions.borrow().get(&key).copied().unwrap_or(0)
    }

    /// Marks plugin `plugin`'s page state of `scope` changed: the user's
    /// goes to every page of the user; a conversation's raises its revision,
    /// which the conversation's metadata carries.
    pub(crate) fn changed(&self, plugin: usize, conversation: Option<&ConversationId>) {
        match conversation {
            None => self.changes.plugin_changed(self.plugin_id(plugin)),
            Some(conversation) => {
                let key = (plugin, conversation.clone());
                *self.revisions.borrow_mut().entry(key).or_default() += 1;
                self.changes.metadata_changed(conversation.clone());
            }
        }
    }

    /// Applies `change` to the conversation's work and marks its entry in the
    /// list, which carries the work; answers the work's revision and what
    /// the change did.
    async fn apply_work(
        &self,
        conversation: &ConversationId,
        changes: Vec<WorkChange>,
    ) -> Result<(u64, Vec<WorkEffect>), WorkError> {
        match self
            .store
            .change_work(conversation.clone(), changes)
            .await?
        {
            WorkOutcome::Changed { revision, effects } => {
                self.changes.conversation_changed(conversation.clone());
                Ok((revision, effects))
            }
            WorkOutcome::Unchanged { revision } => Ok((revision, Vec::new())),
            WorkOutcome::Full => Err(WorkError::Full),
            WorkOutcome::TooLarge => Err(WorkError::TooLarge),
            WorkOutcome::Missing => Err(WorkError::Missing),
        }
    }

    /// Makes the tabs of `kind` the tabs `listed` a device has open
    /// (`WorkDocument::mirror`), and answers the listed ids whose tabs the
    /// user closed. A change of what the plugin owns tells no plugin.
    async fn mirror_work(
        &self,
        conversation: &ConversationId,
        kind: String,
        listed: Vec<CreateWorkTab>,
    ) -> Result<Vec<String>, WorkError> {
        match self
            .store
            .mirror_work(conversation.clone(), kind, listed)
            .await?
        {
            MirrorOutcome::Mirrored { changed, closed } => {
                if changed {
                    self.changes.conversation_changed(conversation.clone());
                }
                Ok(closed)
            }
            MirrorOutcome::Missing => Err(WorkError::Missing),
        }
    }

    /// Tells the plugin that owns the kind of the tab the user removed, if the
    /// user has it on, after the change is answered: the page never waits for
    /// the plugin's work, which may wake a device.
    ///
    /// A tab the user created is none of the plugin's business: the user
    /// creates tabs of the kinds no plugin owns, such as a file's, and the
    /// plugin its own.
    fn tell_owner(
        self: &Rc<Self>,
        conversation: ConversationId,
        effect: WorkEffect,
        enabled: &[bool],
    ) {
        let WorkEffect::Removed(tab) = effect else {
            return;
        };
        let Some(owner) = self
            .registry()
            .kind_owner(&tab.kind)
            .filter(|owner| enabled.get(*owner).copied().unwrap_or(false))
        else {
            return;
        };
        let request = Request::TabClosed {
            user: self.user.clone(),
            conversation: conversation.clone(),
            tab,
        };
        self.spawn_request(owner, request, conversation);
    }

    /// Sends `request` about `conversation` to plugin `plugin` without
    /// waiting for it; what it could not do is logged, since nobody waits
    /// for its answer.
    fn spawn_request(
        self: &Rc<Self>,
        plugin: usize,
        request: Request,
        conversation: ConversationId,
    ) {
        let shared = self.clone();
        tokio::task::spawn_local(async move {
            let asked = shared
                .request(
                    plugin,
                    request,
                    Some(conversation.clone()),
                    None,
                    CancellationToken::new(),
                )
                .await;
            if let Err(error) = asked {
                tracing::warn!(
                    event = "plugin_change.failed",
                    plugin = shared.plugin_id(plugin),
                    %conversation,
                    %error,
                    "a plugin could not do its part of a change"
                );
            }
        });
    }

    /// Each plugin's device directories, by its id.
    async fn directories(&self) -> Result<BTreeMap<String, Vec<DeviceDirectory>>, StoreError> {
        if let Some(directories) = &*self.directories.borrow() {
            return Ok(directories.clone());
        }
        let directories = self.store.directories().await?;
        self.directories.replace(Some(directories.clone()));
        Ok(directories)
    }

    /// Replaces plugin `plugin`'s device directories and answers each one's
    /// path on every device.
    async fn set_directories(
        &self,
        plugin: usize,
        directories: Vec<DeviceDirectory>,
    ) -> Result<Vec<DirectoryPath>, PortFailure> {
        DeviceDirectory::check_set(&directories).map_err(PortError::Failed)?;
        let mut current = self.directories().await.map_err(storage)?;
        let registry = self.registry();
        let id = registry.plugins[plugin].id();
        self.store
            .set_directories(id.as_str().to_owned(), directories.clone())
            .await
            .map_err(storage)?;
        let paths = directories
            .iter()
            .map(|directory| DirectoryPath {
                name: directory.name.clone(),
                path: directory.path(id),
            })
            .collect();
        current.insert(id.as_str().to_owned(), directories);
        self.directories.replace(Some(current));
        Ok(paths)
    }
}

/// One user's plugins. Cloning it is cheap.
#[derive(Clone)]
pub struct UserPlugins(Rc<Shared>);

/// What a package lost on a device (`crates/command-protocol/src/messages.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lost<'a> {
    /// The state of one package, whose resident ended unsaid.
    Package(&'a str),
    /// The state of every package, which a runner that started anew or a
    /// Cloud that stopped took with it.
    Every,
}

impl UserPlugins {
    /// An instance of every plugin of `plugins` for `user`, whose port
    /// reaches `product`, the user's values in `store` and the user's
    /// pages through `changes`.
    pub fn new(
        plugins: Arc<Plugins>,
        user: UserId,
        product: Rc<dyn ProductPort>,
        store: Rc<dyn PluginStore>,
        changes: Rc<dyn PluginChanges>,
    ) -> Self {
        let adopted = RefCell::new(plugins.current());
        Self(Rc::new(Shared {
            plugins,
            adopted,
            user,
            lives: RefCell::default(),
            product,
            store,
            sign_ins: SignIns::default(),
            changes,
            switches: RefCell::new(None),
            feature_switches: RefCell::new(None),
            switching: tokio::sync::Mutex::new(()),
            switched: RefCell::default(),
            directories: RefCell::new(None),
            revisions: RefCell::new(HashMap::new()),
            running: RefCell::default(),
            told_running: RefCell::default(),
        }))
    }

    /// Whether `conversation` runs now, which the shard says at each change
    /// of its tree's status: each plugin that follows it and that the user
    /// has on is told when it changed (`Request::ConversationRunning`).
    ///
    /// What is told is read after the plugins are, so a start and a stop
    /// close together never reach a plugin in the wrong order: the telling
    /// that comes last finds nothing left to tell.
    pub fn conversation_running(&self, conversation: &ConversationId, running: bool) {
        let changed = if running {
            self.0.running.borrow_mut().insert(conversation.clone())
        } else {
            self.0.running.borrow_mut().remove(conversation)
        };
        if !changed {
            return;
        }
        let this = Self(self.0.clone());
        let conversation = conversation.clone();
        tokio::task::spawn_local(async move {
            let plugins = this
                .receivers_of(|registered| registered.factory.manifest().running)
                .await;
            let running = this.0.running.borrow().contains(&conversation);
            let told = if running {
                this.0.told_running.borrow_mut().insert(conversation.clone())
            } else {
                this.0.told_running.borrow_mut().remove(&conversation)
            };
            if !told {
                return;
            }
            for plugin in plugins {
                let request = Request::ConversationRunning {
                    user: this.0.user.clone(),
                    conversation: conversation.clone(),
                    running,
                };
                this.0.spawn_request(plugin, request, conversation.clone());
            }
        });
    }

    /// Whether the user has each plugin of `registry` on, by its index.
    async fn enabled(&self, registry: &Registry) -> Result<Vec<bool>, StoreError> {
        self.0.enabled(registry).await
    }

    /// The program's plugins with whether the user has each on, for the
    /// settings page.
    pub async fn entries(&self) -> Result<Vec<PluginEntry>, StoreError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        let features = self.0.features(&registry).await?;
        Ok(registry
            .present()
            .map(|(index, registered)| {
                let manifest = registered.factory.manifest();
                PluginEntry {
                    id: manifest.id.to_string(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    enabled: enabled[index],
                    features: manifest
                        .features
                        .iter()
                        .zip(&features[index])
                        .map(|(feature, on)| (feature.id.clone(), *on))
                        .collect(),
                    packages: registered.packages.iter().cloned().collect(),
                }
            })
            .collect())
    }

    /// Takes the program's plugins as they are now, after a declaration
    /// or a removal of one of its plugins: a removed plugin loses its
    /// instance, its tabs and its packages, as one the user turns off does,
    /// and the plugin list is marked as changed. Answers what the product
    /// does next, or none when nothing changed since the last adoption.
    ///
    /// A plugin declared again keeps its instance, and the requests it has
    /// under way go on.
    pub async fn adopt(&self) -> Result<Option<Adopted>, StoreError> {
        let _switching = self.0.switching.lock().await;
        let registry = self.0.registry();
        let before = self.0.adopted.borrow().clone();
        if Arc::ptr_eq(&before, &registry) {
            return Ok(None);
        }
        let mut changed = Vec::new();
        let mut removed = Vec::new();
        for (index, registered) in registry.plugins.iter().enumerate() {
            let held = before.plugins.get(index);
            let same = held.is_some_and(|held| held.removed == registered.removed && held.declaration == registered.declaration);
            if same {
                continue;
            }
            changed.push(registered.id().clone());
            if registered.removed {
                removed.push(index);
            }
        }
        let enabled = self.enabled(&registry).await?;
        let mut packages = Vec::new();
        for &index in &removed {
            if let Some(live) = self.0.lives.borrow_mut().remove(&index) {
                live.ended.cancel();
            }
            self.close_tabs(&registry, index).await?;
            packages.extend(registry.plugins[index].packages.iter());
        }
        let released = Self::unbound(&registry, &enabled, packages.into_iter());
        // Only now: an adoption that failed is made again by the next one.
        self.0.adopted.replace(registry);
        let switched = std::mem::take(&mut *self.0.switched.borrow_mut());
        switched.cancel();
        self.0.changes.plugins_changed();
        Ok(Some(Adopted { changed, released }))
    }

    /// Turns `plugin` on or off for the user, and marks the plugin list and
    /// the plugin's state as changed; any change ends what
    /// [`Self::next_switch`] gave. Answers what the product does next, or none
    /// when the choice did not change.
    ///
    /// A plugin turned off loses its instance, which ends its requests under
    /// way, the tasks it started and its open user streams, and the tabs of
    /// its kinds in every conversation's work, an archived one's included.
    /// Its durable data stays.
    pub async fn switch(&self, plugin: &str, enabled: bool) -> Result<Option<Switched>, SwitchError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(plugin) else {
            return Err(SwitchError::UnknownPlugin(plugin.to_owned()));
        };
        let _switching = self.0.switching.lock().await;
        let mut current = self.enabled(&registry).await?;
        if current[index] == enabled {
            return Ok(None);
        }
        self.0
            .store
            .set_switch(plugin.to_owned(), enabled)
            .await?;
        current[index] = enabled;
        let mut switches = self.0.switches().await?;
        switches.insert(plugin.to_owned(), enabled);
        self.0.switches.replace(Some(switches));
        let mut released = Vec::new();
        if !enabled {
            if let Some(live) = self.0.lives.borrow_mut().remove(&index) {
                live.ended.cancel();
            }
            self.close_tabs(&registry, index).await?;
            released = Self::unbound(&registry, &current, registered.packages.iter());
        }
        let switched = std::mem::take(&mut *self.0.switched.borrow_mut());
        switched.cancel();
        self.0.changes.plugins_changed();
        self.0
            .changes.plugin_changed(registered.id().as_str().to_owned());
        Ok(Some(Switched { released }))
    }

    /// Turns `plugin`'s `feature` on or off for the user, and marks the
    /// plugin list as changed; any change ends what [`Self::next_switch`]
    /// gave. Answers whether the choice changed. A feature turned off only
    /// takes its leaves out of the command set: the plugin's instance, its
    /// tabs and its packages stay.
    pub async fn switch_feature(&self, plugin: &str, feature: &str, enabled: bool) -> Result<bool, SwitchError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(plugin) else {
            return Err(SwitchError::UnknownPlugin(plugin.to_owned()));
        };
        let Some(position) = registered
            .factory
            .manifest()
            .features
            .iter()
            .position(|declared| declared.id == feature)
        else {
            return Err(SwitchError::UnknownFeature {
                plugin: plugin.to_owned(),
                feature: feature.to_owned(),
            });
        };
        let _switching = self.0.switching.lock().await;
        let current = self.0.features(&registry).await?;
        if current[index][position] == enabled {
            return Ok(false);
        }
        self.0
            .store
            .set_feature_switch(plugin.to_owned(), feature.to_owned(), enabled)
            .await?;
        if let Some(switches) = &mut *self.0.feature_switches.borrow_mut() {
            switches.insert((plugin.to_owned(), feature.to_owned()), enabled);
        }
        let switched = std::mem::take(&mut *self.0.switched.borrow_mut());
        switched.cancel();
        self.0.changes.plugins_changed();
        Ok(true)
    }

    /// The packages whose residents a device is to hold none of: those of every
    /// plugin the user has off that no plugin on binds. The runner stops them
    /// when it connects, as turning their plugin off stops them on a connected
    /// one. A list of what is off, not of what is on, since a device also runs
    /// packages no plugin binds.
    pub async fn stopped_packages(&self) -> Result<Vec<String>, StoreError> {
        let registry = self.0.registry();
        let current = self.enabled(&registry).await?;
        let off = registry
            .plugins
            .iter()
            .enumerate()
            .filter(|(index, _)| !current[*index])
            .flat_map(|(_, registered)| registered.packages.iter());
        Ok(Self::unbound(&registry, &current, off))
    }

    /// Of `packages`, those that no plugin turned on in `current` binds. A
    /// package two plugins bind stays while either is on.
    fn unbound<'a>(
        registry: &Registry,
        current: &[bool],
        packages: impl Iterator<Item = &'a String>,
    ) -> Vec<String> {
        let mut unbound: Vec<String> = packages
            .filter(|package| {
                !registry
                    .plugins
                    .iter()
                    .enumerate()
                    .any(|(other, registered)| current[other] && registered.packages.contains(*package))
            })
            .cloned()
            .collect();
        unbound.sort();
        unbound.dedup();
        unbound
    }

    /// Removes the tabs of plugin `plugin`'s kinds from every work of the
    /// user's conversations, archived ones included, and marks the entry in the
    /// list of each that changed.
    async fn close_tabs(&self, registry: &Registry, plugin: usize) -> Result<(), StoreError> {
        let Some(page) = &registry.plugins[plugin].page else {
            return Ok(());
        };
        let closed = self
            .0
            .store
            .close_tab_kinds(page.tab_kinds.clone())
            .await?;
        for conversation in closed {
            self.0.changes.conversation_changed(conversation);
        }
        Ok(())
    }

    /// The toolset of the plugins the user has on: their commands, with the
    /// product's `product` groups under `demi`.
    pub async fn toolset(&self, product: Vec<GroupBuilder>) -> Result<PluginToolset, StoreError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        let features = self.0.features(&registry).await?;
        let on = |index: usize, feature: &Feature| {
            let declared = &registry.plugins[index].factory.manifest().features;
            declared
                .iter()
                .position(|candidate| candidate.id == feature.id)
                .is_some_and(|position| features[index][position])
        };
        let commands = compose(&registry, Some(&self.0), product, |index| enabled[index], on)
            .expect("the plugins' commands were checked as they were registered");
        Ok(PluginToolset {
            commands,
            revision: Self::revision_of(&registry, &enabled, &features),
        })
    }

    /// The revision of the toolset the user's plugins give now.
    pub async fn revision(&self) -> Result<String, StoreError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        let features = self.0.features(&registry).await?;
        Ok(Self::revision_of(&registry, &enabled, &features))
    }

    /// The plugins on that have commands, each with the number of its
    /// program's declaration of it and the features it has off, such as
    /// `web!search` or `chat@3`.
    fn revision_of(registry: &Registry, enabled: &[bool], features: &[Vec<bool>]) -> String {
        let parts: Vec<String> = registry
            .plugins
            .iter()
            .zip(enabled)
            .zip(features)
            .filter(|((registered, enabled), _)| **enabled && !registered.commands.is_empty())
            .map(|((registered, _), on)| {
                let mut part = registered.id().as_str().to_owned();
                if let Some(declaration) = registered.declaration {
                    part.push('@');
                    part.push_str(&declaration.to_string());
                }
                for (feature, on) in registered.factory.manifest().features.iter().zip(on) {
                    if !on {
                        part.push('!');
                        part.push_str(&feature.id);
                    }
                }
                part
            })
            .collect();
        parts.join(",")
    }

    /// Asks the context source `plugin` for a node's new context block;
    /// nothing while the user has it off.
    pub async fn context(
        &self,
        plugin: &PluginId,
        asked: ContextAsk,
        cancel: CancellationToken,
    ) -> Result<Option<String>, PluginError> {
        let registry = self.0.registry();
        let Some((index, _)) = registry.plugin(plugin.as_str()) else {
            return Ok(None);
        };
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        if !enabled[index] {
            return Ok(None);
        }
        let request = Request::Context {
            user: self.0.user.clone(),
            conversation: asked.conversation.clone(),
            node: asked.node,
            cwd: asked.cwd,
            paths: asked.paths,
            turn: asked.turn,
            seen: asked.seen,
            project: asked.project,
        };
        let reply = self
            .0
            .request(index, request, Some(asked.conversation), None, cancel)
            .await?;
        match reply {
            Reply::Context { text } => Ok(text),
            reply => Err(PluginError::failed(format!(
                "the plugin answered a context request with {reply:?}"
            ))),
        }
    }

    /// Whether `plugin` declares a project search and the user has it on.
    pub async fn searches_projects(&self, plugin: &PluginId) -> Result<bool, StoreError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(plugin.as_str()) else {
            return Ok(false);
        };
        Ok(registered.factory.manifest().project_search && self.enabled(&registry).await?[index])
    }

    /// Asks `plugin`, which declares a project search, to search
    /// `directory`, the conversation's project directory on the primary device
    /// of `device`, which its reads are bound to: what it found and what
    /// tells of a change to it; none while the user has it off.
    pub async fn project_search(
        &self,
        plugin: &PluginId,
        conversation: &ConversationId,
        device: &DeviceId,
        directory: String,
        paths: demi_device_paths::DevicePaths,
        cancel: CancellationToken,
    ) -> Result<Option<(Value, Vec<WatchSubject>)>, PluginError> {
        let registry = self.0.registry();
        let Some((index, _)) = registry.plugin(plugin.as_str()) else {
            return Ok(None);
        };
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        if !enabled[index] {
            return Ok(None);
        }
        let request = Request::ProjectSearch {
            user: self.0.user.clone(),
            conversation: conversation.clone(),
            directory,
            paths,
        };
        let reply = self
            .0
            .request_on(index, request, Some(conversation.clone()), Some(device.clone()), None, cancel)
            .await?;
        match reply {
            Reply::Project { found, watch } => Ok(Some((found, watch))),
            reply => Err(PluginError::failed(format!(
                "the plugin answered a project search with {reply:?}"
            ))),
        }
    }

    /// Every plugin's device directories for the user, which device access
    /// installs before a job: a plugin the user has off has none, so a device
    /// loses its directories at its next installation.
    pub async fn directories(&self) -> Result<Vec<(PluginId, Vec<DeviceDirectory>)>, StoreError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        let mut directories = self.0.directories().await?;
        Ok(registry
            .plugins
            .iter()
            .zip(enabled)
            .map(|(registered, enabled)| {
                let set = match directories.remove(registered.id().as_str()) {
                    Some(set) if enabled => set,
                    _ => Vec::new(),
                };
                (registered.id().clone(), set)
            })
            .collect())
    }

    /// What is cancelled when the user next turns a plugin on or off. Taken
    /// before the user's streams are read, it tells that what was read is
    /// out of date, such as a direct channel's introduction.
    pub fn next_switch(&self) -> CancellationToken {
        self.0.switched.borrow().clone()
    }

    /// What ends the user stream `name` once its plugin is turned off; none
    /// while the user has its plugin off, when the stream does not exist.
    pub async fn stream_end(&self, name: &str) -> Result<Option<CancellationToken>, StoreError> {
        let registry = self.0.registry();
        let Some(index) = registry.stream_owner(name) else {
            return Ok(None);
        };
        let enabled = self.enabled(&registry).await?;
        Ok(self.0.live(&registry, index, &enabled).map(|live| live.ended))
    }

    /// The user state of each plugin the user has on that declares one, by
    /// id, for the product state.
    pub async fn page_states(&self) -> Result<BTreeMap<String, Value>, PluginError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        let mut states = BTreeMap::new();
        for (index, registered) in registry.present() {
            if enabled[index] && registered.state(Scope::User).is_some() {
                // One plugin after another: the product state's read waits for them all.
                let started = std::time::Instant::now();
                let state = self.state_of(index, None, CancellationToken::new()).await?;
                tracing::info!(
                    event = "plugin_page_state.read",
                    plugin = registered.id().as_str(),
                    ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    "a plugin's page state was read"
                );
                states.insert(registered.id().as_str().to_owned(), state);
            }
        }
        Ok(states)
    }

    /// The user state of `plugin`; none for a plugin the user has off or
    /// whose page declares none.
    pub async fn page_state(&self, plugin: &str) -> Result<Option<Value>, PluginError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(plugin) else {
            return Ok(None);
        };
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        if !enabled[index] || registered.state(Scope::User).is_none() {
            return Ok(None);
        }
        self.state_of(index, None, CancellationToken::new())
            .await
            .map(Some)
    }

    /// The conversation state of `plugin` for `conversation`, with the
    /// revision it was read at: read after the revision, so a change made
    /// while it is read raises the revision past it.
    pub async fn conversation_state(
        &self,
        plugin: &str,
        conversation: ConversationId,
        cancel: CancellationToken,
    ) -> Result<PluginStateAnswer, PageCallError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(plugin) else {
            return Err(PageCallError::UnknownPlugin(plugin.to_owned()));
        };
        if registered.state(Scope::Conversation).is_none() {
            return Err(PageCallError::UnknownPlugin(plugin.to_owned()));
        }
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        if !enabled[index] {
            return Err(PageCallError::Disabled(plugin.to_owned()));
        }
        let revision = self.0.revision(index, &conversation);
        let state = self.state_of(index, Some(conversation), cancel).await?;
        Ok(PluginStateAnswer { revision, state })
    }

    /// The revision of each plugin's conversation state for `conversation`,
    /// for the conversation's summary: every plugin that declares one, so
    /// turning a plugin on changes no summary.
    pub fn plugin_revisions(&self, conversation: &ConversationId) -> Vec<PluginRevision> {
        self.0
            .registry()
            .present()
            .filter(|(_, registered)| registered.state(Scope::Conversation).is_some())
            .map(|(index, registered)| PluginRevision {
                plugin: registered.id().as_str().to_owned(),
                revision: self.0.revision(index, conversation),
            })
            .collect()
    }

    /// The command package `package` said `message` about `conversation`
    /// from a resident on one of its devices: each plugin that binds it and
    /// that the user has on receives it, in the order the messages came
    /// (`crates/plugin-interface/src/request.rs`).
    pub async fn package_message(
        &self,
        package: &str,
        conversation: &ConversationId,
        device: &DeviceId,
        message: serde_json::Value,
    ) {
        for plugin in self.receivers(package).await {
            let request = Request::PackageMessage {
                user: self.0.user.clone(),
                conversation: conversation.clone(),
                device: device.clone(),
                message: message.clone(),
            };
            self.0.spawn_request(plugin, request, conversation.clone());
        }
    }

    /// What `lost` names was kept on a device of each of `conversations` and
    /// ended without its package saying so.
    ///
    /// Each plugin that binds such a package, that the user has on and that has
    /// work tabs in one of the conversations, receives `package_lost` for it
    /// (`crates/command-protocol/src/messages.rs`). A plugin that shows nothing
    /// of a conversation has nothing to correct.
    pub async fn package_lost(&self, device: &DeviceId, lost: Lost<'_>, conversations: &[ConversationId]) {
        let plugins = match lost {
            Lost::Package(package) => self.receivers(package).await,
            Lost::Every => self.receivers_of(|registered| !registered.packages.is_empty()).await,
        };
        if plugins.is_empty() {
            return;
        }
        let registry = self.0.registry();
        for conversation in conversations {
            let work = match self.0.store.work(conversation.clone()).await {
                Ok(work) => work,
                Err(error) => {
                    // The plugin then shows the lost state until its
                    // package next says something about the conversation.
                    tracing::warn!(event = "package_tabs.read_failed", %conversation, error = &error as &dyn std::error::Error, "a lost package's work could not be read");
                    continue;
                }
            };
            for &plugin in &plugins {
                let registered = &registry.plugins[plugin];
                if !work.tabs.iter().any(|tab| registered.owns_kind(&tab.kind)) {
                    continue;
                }
                let request = Request::PackageLost {
                    user: self.0.user.clone(),
                    conversation: conversation.clone(),
                    device: device.clone(),
                };
                self.0.spawn_request(plugin, request, conversation.clone());
            }
        }
    }

    /// The user deleted `conversation`: each plugin that keeps something of
    /// a conversation, on or off, is told (`Request::ConversationDeleted`),
    /// and the answer waits for every one; the first that failed is the
    /// answer's failure, which the deletion keeps pending for.
    pub async fn conversation_deleted(&self, conversation: &ConversationId) -> Result<(), PluginError> {
        let requests = self.keepers().into_iter().map(|plugin| {
            let request = Request::ConversationDeleted {
                user: self.0.user.clone(),
                conversation: conversation.clone(),
            };
            self.0.request(plugin, request, Some(conversation.clone()), None, CancellationToken::new())
        });
        for reply in futures_util::future::join_all(requests).await {
            reply?;
        }
        Ok(())
    }

    /// The user forked `from` into `conversation`: each plugin that keeps
    /// something of a conversation, on or off, is told
    /// (`Request::ConversationForked`), and the answer waits for every one.
    pub async fn conversation_forked(&self, from: &ConversationId, conversation: &ConversationId) -> Result<(), PluginError> {
        let requests = self.keepers().into_iter().map(|plugin| {
            let request = Request::ConversationForked {
                user: self.0.user.clone(),
                conversation: conversation.clone(),
                from: from.clone(),
            };
            self.0.request(plugin, request, Some(conversation.clone()), None, CancellationToken::new())
        });
        for reply in futures_util::future::join_all(requests).await {
            reply?;
        }
        Ok(())
    }

    /// The facts of the reply blocks `ids` of `tag`, by id, null for one the
    /// user does not have, as the plugin whose page draws the tag answers
    /// them, on or off (`Request::ReplyBlocks`); none for a tag no plugin
    /// draws.
    pub async fn reply_block_facts(
        &self,
        tag: &str,
        ids: Vec<String>,
    ) -> Result<Option<Map<String, Value>>, PluginError> {
        let Some((plugin, _)) = self.reply_block(tag) else {
            return Ok(None);
        };
        let request = Request::ReplyBlocks { user: self.0.user.clone(), tag: tag.to_owned(), ids: ids.clone() };
        match self.0.request(plugin, request, None, None, CancellationToken::new()).await? {
            Reply::ReplyBlocks { mut facts } => {
                for id in ids {
                    facts.entry(id).or_insert(Value::Null);
                }
                Ok(Some(facts))
            }
            reply => Err(PluginError::failed(format!("the plugin answered its reply blocks with {reply:?}"))),
        }
    }

    /// Whether the plugin whose page draws reply blocks of `tag` is on; false
    /// for a tag no plugin draws.
    pub async fn reply_block_on(&self, tag: &str) -> Result<bool, PluginError> {
        let Some((plugin, _)) = self.reply_block(tag) else {
            return Ok(false);
        };
        let enabled = self.enabled(&self.0.registry()).await.map_err(PluginError::failed)?;
        Ok(enabled[plugin])
    }

    /// The HTML document of `plugin`'s reply block `id` of `tag`, while the
    /// plugin is on (`Request::ReplyBlockDocument`); none for a block the
    /// user does not have.
    pub async fn reply_block_document(
        &self,
        plugin: &str,
        tag: &str,
        id: String,
        cancel: CancellationToken,
    ) -> Result<Option<String>, PageCallError> {
        let Some((index, true)) = self.reply_block(tag) else {
            return Ok(None);
        };
        if self.0.plugin_id(index) != plugin {
            return Ok(None);
        }
        let enabled = self.enabled(&self.0.registry()).await.map_err(PluginError::failed)?;
        if !enabled[index] {
            return Err(PageCallError::Disabled(plugin.to_owned()));
        }
        let request = Request::ReplyBlockDocument { user: self.0.user.clone(), tag: tag.to_owned(), id };
        match self.0.request(index, request, None, None, cancel).await? {
            Reply::ReplyBlockDocument { document } => Ok(document),
            reply => Err(PluginError::failed(format!("the plugin answered a block's document with {reply:?}")).into()),
        }
    }

    /// The plugin whose page draws reply blocks of `tag`, by its index, and
    /// whether they have a document.
    fn reply_block(&self, tag: &str) -> Option<(usize, bool)> {
        self.0.registry().present().find_map(|(index, registered)| {
            let block = registered.page.as_ref()?.blocks.iter().find(|block| block.tag == tag)?;
            Some((index, block.document))
        })
    }

    /// The plugins that keep something of a conversation, by their indices.
    fn keepers(&self) -> Vec<usize> {
        self.0
            .registry()
            .present()
            .filter(|(_, registered)| registered.factory.manifest().conversations)
            .map(|(index, _)| index)
            .collect()
    }

    /// The plugins that bind `package` and that the user has on.
    async fn receivers(&self, package: &str) -> Vec<usize> {
        self.receivers_of(|registered| registered.packages.contains(package)).await
    }

    /// The plugins `bound` takes that the user has on, by their indices.
    async fn receivers_of(&self, bound: impl Fn(&crate::registry::Registered) -> bool) -> Vec<usize> {
        let registry = self.0.registry();
        let enabled = match self.enabled(&registry).await {
            Ok(enabled) => enabled,
            Err(error) => {
                tracing::warn!(event = "package_message.receivers_failed", %error, "the plugins a package message is for could not be read");
                return Vec::new();
            }
        };
        registry
            .present()
            .filter(|(index, registered)| enabled[*index] && bound(registered))
            .map(|(index, _)| index)
            .collect()
    }

    /// The conversation's work.
    pub async fn work(&self, conversation: ConversationId) -> Result<Work, StoreError> {
        self.0.store.work(conversation).await
    }

    /// The held conversation states of the plugins the user has on whose
    /// conversation state is held, each as its plugin holds it now: a plugin
    /// that has none, or fails, adds nothing, and the page reads that state
    /// itself. The page has the work's tabs from the conversation's entry
    /// already.
    pub async fn tab_states(
        &self,
        conversation: ConversationId,
        cancel: CancellationToken,
    ) -> Result<Vec<HeldState>, StoreError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        let mut states = Vec::new();
        for (index, registered) in registry.present() {
            let held = registered.state(Scope::Conversation).is_some_and(|state| state.held);
            if !enabled[index] || !held {
                continue;
            }
            // Read after the revision, as `conversation_state` does.
            let revision = self.0.revision(index, &conversation);
            let request = Request::PageState {
                user: self.0.user.clone(),
                conversation: Some(conversation.clone()),
                held: true,
            };
            let reply = self
                .0
                .request(index, request, Some(conversation.clone()), None, cancel.clone())
                .await;
            if let Ok(Reply::State { state }) = reply {
                states.push(HeldState {
                    plugin: registered.id().as_str().to_owned(),
                    revision,
                    state,
                });
            }
        }
        Ok(states)
    }

    /// A page's changes of the conversation's work, all or none: each tab
    /// they create is of a kind a plugin the user has on declares.
    ///
    /// Answers the work's revision once the changes are in it and whether they
    /// changed it, and tells the kind's plugin of each tab the user created or
    /// removed.
    pub async fn change_work(
        &self,
        conversation: ConversationId,
        changes: Vec<WorkChange>,
    ) -> Result<(u64, bool), WorkError> {
        let registry = self.0.registry();
        let enabled = self.enabled(&registry).await?;
        for change in &changes {
            if let WorkChange::Create(create) = change {
                let owned = registry
                    .kind_owner(&create.kind)
                    .is_some_and(|owner| enabled[owner]);
                if !owned {
                    return Err(WorkError::UnknownKind(create.kind.clone()));
                }
            }
        }
        let (revision, effects) = self.0.apply_work(&conversation, changes).await?;
        let changed = !effects.is_empty();
        for effect in effects {
            self.0.tell_owner(conversation.clone(), effect, &enabled);
        }
        Ok((revision, changed))
    }

    /// The vendor sent the browser back to `plugin`'s OAuth callback: a sign-in this user started,
    /// of a plugin the user has on, is completed; answers the vendor's name.
    pub async fn oauth_callback(
        &self,
        plugin: &str,
        callback: Callback,
        cancel: CancellationToken,
    ) -> Result<String, CallbackError> {
        let unknown = || CallbackError::UnknownPlugin(plugin.to_owned());
        let registry = self.0.registry();
        let Some((index, _)) = registry.plugin(plugin) else {
            return Err(unknown());
        };
        if !self.enabled(&registry).await?[index] {
            return Err(unknown());
        }
        self.0.oauth_callback(index, callback, &cancel).await
    }

    async fn state_of(
        &self,
        plugin: usize,
        conversation: Option<ConversationId>,
        cancel: CancellationToken,
    ) -> Result<Value, PluginError> {
        let request = Request::PageState {
            user: self.0.user.clone(),
            conversation: conversation.clone(),
            held: false,
        };
        let reply = self
            .0
            .request(plugin, request, conversation, None, cancel)
            .await?;
        match reply {
            Reply::State { state } => Ok(state),
            reply => Err(PluginError::failed(format!(
                "the plugin answered its page state with {reply:?}"
            ))),
        }
    }

    /// Calls a page method: the plugin has it for the call's scope, and
    /// its parameters fit its schema.
    pub async fn page_call(
        &self,
        call: PageCall,
        cancel: CancellationToken,
    ) -> Result<Value, PageCallError> {
        let registry = self.0.registry();
        let Some((index, registered)) = registry.plugin(&call.plugin) else {
            return Err(PageCallError::UnknownPlugin(call.plugin));
        };
        let enabled = self.enabled(&registry).await.map_err(PluginError::failed)?;
        if !enabled[index] {
            return Err(PageCallError::Disabled(call.plugin));
        }
        let scope = match call.conversation {
            Some(_) => Scope::Conversation,
            None => Scope::User,
        };
        let method = registered.page.as_ref().and_then(|page| {
            page.methods
                .iter()
                .find(|method| method.name == call.method && method.scope == scope)
        });
        let Some(method) = method else {
            return Err(PageCallError::UnknownMethod {
                plugin: call.plugin,
                method: call.method,
            });
        };
        method
            .params
            .check(&call.params)
            .map_err(PageCallError::InvalidParams)?;
        let Value::Object(params) = call.params else {
            return Err(PageCallError::InvalidParams(
                "the parameters are not an object".into(),
            ));
        };
        let request = Request::PageCall {
            user: self.0.user.clone(),
            method: call.method,
            params,
            conversation: call.conversation.clone(),
        };
        let reply = self
            .0
            .request(index, request, call.conversation, None, cancel)
            .await?;
        match reply {
            Reply::Result { result } => Ok(result),
            reply => Err(PluginError::failed(format!(
                "the plugin answered a page call with {reply:?}"
            ))
            .into()),
        }
    }
}

/// What a node gives a context source before a provider request.
pub struct ContextAsk {
    pub conversation: ConversationId,
    pub node: NodeId,
    /// The node's working directory.
    pub cwd: String,
    /// How the node's device writes its paths.
    pub paths: demi_device_paths::DevicePaths,
    /// The node's current input turn.
    pub turn: TurnId,
    /// The text of the source's own blocks the model receives, oldest
    /// first.
    pub seen: Vec<String>,
    /// What the plugin's project search last found, for a plugin that
    /// declares one.
    pub project: Option<Value>,
}

/// One request's port: the call's rpc port for a command, the user's
/// values and pages, and the product's operations for the request's
/// conversation.
struct RequestPort {
    shared: Rc<Shared>,
    plugin: usize,
    conversation: Option<ConversationId>,
    rpc: Option<RpcPort>,
    cancel: CancellationToken,
    /// Where its reads of files go.
    read_on: ReadOn,
    /// The job that called the command, for a command an agent's job
    /// called.
    caller: Option<JobCaller>,
    /// The id of the request's call, and whether the runner sent it again.
    call: (String, bool),
    /// How many media requests it made.
    media_asked: std::cell::Cell<u32>,
    /// The request's video's job, from its ask to its video; dropped with
    /// the request.
    video: RefCell<Option<VideoJob>>,
}

impl RequestPort {
    /// The call of the request's next media request.
    fn media_call(&self) -> MediaCall {
        let ordinal = self.media_asked.get();
        self.media_asked.set(ordinal + 1);
        MediaCall { id: self.call.0.clone(), ordinal, resumed: self.call.1, rpc: self.rpc.clone() }
    }

    async fn answer(&self, message: PortMessage) -> Result<PortAnswer, PortFailure> {
        let shared = &self.shared;
        let product = &*shared.product;
        let answer = match message {
            PortMessage::Rpc { request } => {
                let Some(rpc) = &self.rpc else {
                    return Err(PortError::Unexpected {
                        asked: "rpc",
                        answered: "no rpc port: the request is not a command",
                    }
                    .into());
                };
                PortAnswer::Rpc {
                    response: rpc.forward(request).await?,
                }
            }
            PortMessage::ReadValue { key } => {
                let value = shared
                    .store
                    .value(shared.plugin_id(self.plugin), key)
                    .await
                    .map_err(storage)?;
                PortAnswer::Value {
                    value: value.map(stored),
                }
            }
            PortMessage::ListValues => {
                let values = shared
                    .store
                    .values(shared.plugin_id(self.plugin))
                    .await
                    .map_err(storage)?;
                PortAnswer::Values {
                    values: values
                        .into_iter()
                        .map(|(key, value)| (key, stored(value)))
                        .collect(),
                }
            }
            PortMessage::WriteValue {
                key,
                value,
                revision,
                blobs,
            } => {
                let write = ValueWrite {
                    plugin: shared.plugin_id(self.plugin),
                    key,
                    document: value,
                    revision,
                    blobs,
                };
                let written = shared
                    .store
                    .write_value(write)
                    .await
                    .map_err(storage)?;
                match written {
                    Written::Revision(revision) => PortAnswer::Written { revision },
                    Written::Conflict => return Err(PortFailure::Refused(PortRefusal::Conflict)),
                }
            }
            PortMessage::RemoveValue { key, revision } => {
                let removed = shared
                    .store
                    .remove_value(
                        shared.plugin_id(self.plugin),
                        key,
                        revision,
                    )
                    .await
                    .map_err(storage)?;
                match removed {
                    Written::Revision(_) => PortAnswer::Done,
                    Written::Conflict => return Err(PortFailure::Refused(PortRefusal::Conflict)),
                }
            }
            PortMessage::PutBlob { bytes } => PortAnswer::Blob {
                blob: product.put_blob(bytes).await?,
            },
            PortMessage::GetBlob { blob } => PortAnswer::Bytes {
                bytes: product.get_blob(blob).await?,
            },
            PortMessage::SetDirectories { directories } => PortAnswer::Directories {
                paths: shared.set_directories(self.plugin, directories).await?,
            },
            PortMessage::ReadDeviceFiles { reads } => PortAnswer::DeviceFiles {
                files: product
                    .read_device_files(self.conversation()?, &self.read_on, reads, &self.cancel)
                    .await?,
            },
            PortMessage::Changed { scope } => {
                let conversation = match scope {
                    Scope::User => None,
                    Scope::Conversation => Some(self.conversation()?),
                };
                shared.changed(self.plugin, conversation);
                PortAnswer::Done
            }
            PortMessage::PackageCall {
                operation,
                args,
                input,
                kind,
            } => {
                let registry = shared.registry();
                let registered = &registry.plugins[self.plugin];
                if !registered.packages.contains(&operation.package) {
                    return Err(PortError::Unexpected {
                        asked: "package_call",
                        answered: "a package the plugin's commands do not bind",
                    }
                    .into());
                }
                let conversation = self.conversation()?;
                let result = product
                    .package_call(conversation, &operation, args, input, kind, &self.cancel)
                    .await?;
                PortAnswer::Called { result }
            }
            PortMessage::ReadReport => {
                let conversation = self.conversation()?;
                let report = match shared.primary_device(conversation).await? {
                    Some(device) => shared
                        .store
                        .report(shared.plugin_id(self.plugin), conversation.clone(), device)
                        .await
                        .map_err(storage)?,
                    None => None,
                };
                PortAnswer::Report {
                    report: report.map(|report| StoredReport {
                        revision: report.revision,
                        value: report.value,
                    }),
                }
            }
            PortMessage::WriteReport { device, revision, value } => {
                let device = DeviceId::try_from(device.as_str()).map_err(|error| {
                    PortFailure::Refused(PortRefusal::Operation {
                        stderr: format!("the report names no device: {error}"),
                    })
                })?;
                let conversation = self.conversation()?;
                shared
                    .store
                    .keep_report(
                        shared.plugin_id(self.plugin),
                        conversation.clone(),
                        device.clone(),
                        revision,
                        value,
                    )
                    .await
                    .map_err(storage)?;
                let primary = shared.primary_device(conversation).await?.as_ref() == Some(&device);
                PortAnswer::Kept { primary }
            }
            PortMessage::Devices => PortAnswer::Devices {
                devices: product.devices().await?,
            },
            PortMessage::DeviceGitHead { device, url } => PortAnswer::GitHead {
                commit: product.device_git_head(&device, &url).await?,
            },
            PortMessage::DeviceGitFetch { device, url, marker } => PortAnswer::GitTree {
                tree: product.device_git_fetch(&device, &url, &marker).await?,
            },
            PortMessage::Workspaces => {
                let workspaces = shared.store.workspaces().await.map_err(storage)?;
                let current = match &self.conversation {
                    Some(conversation) => shared
                        .store
                        .conversation_workspace(conversation.clone())
                        .await
                        .map_err(storage)?,
                    None => None,
                };
                PortAnswer::Workspaces {
                    workspaces: UserWorkspaces { workspaces, current },
                }
            }
            PortMessage::WorkTabs => {
                let registry = shared.registry();
                let registered = &registry.plugins[self.plugin];
                let mut work = shared
                    .store
                    .work(self.conversation()?.clone())
                    .await
                    .map_err(storage)?;
                work.tabs.retain(|tab| registered.owns_kind(&tab.kind));
                PortAnswer::Work { work }
            }
            PortMessage::CreateWorkTab { tab } => {
                if !shared.registry().plugins[self.plugin].owns_kind(&tab.kind) {
                    return Err(WorkError::UnknownKind(tab.kind).refusal());
                }
                self.work_change(WorkChange::Create(tab)).await?
            }
            PortMessage::UpdateWorkTab { id, data } => {
                self.own_tab(&id).await?;
                self.work_change(WorkChange::Update { id, data }).await?
            }
            PortMessage::RemoveWorkTab { id } => {
                self.own_tab(&id).await?;
                self.work_change(WorkChange::Remove { id }).await?
            }
            PortMessage::MirrorWorkTabs { kind, tabs } => {
                let registry = shared.registry();
                let registered = &registry.plugins[self.plugin];
                if let Some(other) = std::iter::once(&kind)
                    .chain(tabs.iter().map(|tab| &tab.kind))
                    .find(|kind| !registered.owns_kind(kind))
                {
                    return Err(WorkError::UnknownKind(other.clone()).refusal());
                }
                let closed = shared
                    .mirror_work(self.conversation()?, kind, tabs)
                    .await
                    .map_err(|error| error.refusal())?;
                PortAnswer::Mirrored { closed }
            }
            PortMessage::ReadSecret { name } => PortAnswer::Secret {
                secret: shared.read_secret(self.plugin, name.as_str()).await?,
            },
            PortMessage::WriteSecret {
                name,
                value,
                revision,
            } => PortAnswer::Written {
                revision: shared
                    .write_secret(self.plugin, name.as_str(), &value, revision)
                    .await?,
            },
            PortMessage::RemoveSecret { name, revision } => {
                shared
                    .remove_secret(self.plugin, name.as_str(), revision)
                    .await?;
                PortAnswer::Done
            }
            PortMessage::SendRequest { request } => {
                let registry = shared.registry();
                let id = registry.plugins[self.plugin].id();
                PortAnswer::Response {
                    response: web::send(&product.web_client(), id, request, &self.cancel).await?,
                }
            }
            PortMessage::OauthStart => PortAnswer::AuthorizationUrl {
                url: shared.oauth_start(self.plugin, &self.cancel).await?,
            },
            PortMessage::OauthToken { refused } => PortAnswer::AccessToken {
                token: shared
                    .oauth_token(self.plugin, refused.as_deref(), &self.cancel)
                    .await?,
            },
            PortMessage::OauthSignOut => {
                shared.oauth_sign_out(self.plugin, &self.cancel).await?;
                PortAnswer::Done
            }
            PortMessage::MediaModels { kind } => PortAnswer::MediaModels {
                listing: product.media_models(kind).await?,
            },
            PortMessage::MakeImage { ask } => {
                let call = self.media_call();
                PortAnswer::Image {
                    image: product
                        .make_image(self.conversation()?, self.caller.as_ref(), &call, ask, &self.cancel)
                        .await?,
                }
            }
            PortMessage::MakeVideo { ask } => {
                if self.video.borrow().is_some() {
                    return Err(Self::job_failure("the call has a video's job already"));
                }
                let call = self.media_call();
                let (asked, job) = product
                    .make_video(self.conversation()?, self.caller.as_ref(), &call, ask, &self.cancel)
                    .await?;
                self.video.replace(Some(job));
                PortAnswer::VideoAsked { asked }
            }
            PortMessage::MakeAudio { ask } => {
                let call = self.media_call();
                PortAnswer::Audio {
                    audio: product
                        .make_audio(self.conversation()?, self.caller.as_ref(), &call, ask, &self.cancel)
                        .await?,
                }
            }
            PortMessage::AudioVoices { model, search } => PortAnswer::AudioVoices {
                voices: product.audio_voices(self.conversation()?, model, search, &self.cancel).await?,
            },
            PortMessage::VideoProgress => {
                // Taken out for the wait, so no borrow is held across it.
                let Some(mut job) = self.video.take() else {
                    return Err(Self::job_failure("the call has no video's job"));
                };
                let step = tokio::select! {
                    step = job.next() => step,
                    () = self.cancel.cancelled() => return Err(PortError::Ended("the call was stopped".into()).into()),
                };
                match step {
                    Some(Ok(step)) => {
                        // The job ends with its video; until then it waits
                        // for the next ask.
                        if !matches!(step, VideoStep::Made { .. }) {
                            self.video.replace(Some(job));
                        }
                        PortAnswer::VideoProgress { step }
                    }
                    Some(Err(failure)) => return Err(failure),
                    None => {
                        return Err(Self::job_failure("the video's job ended"));
                    }
                }
            }
        };
        Ok(answer)
    }

    /// Why a step of a video's job was not answered.
fn job_failure(message: &str) -> PortFailure {
    PortFailure::Refused(PortRefusal::Media { refusal: MediaRefusal::Failed { message: message.into() } })
}

/// Applies a plugin's change of the request's conversation's work.
    async fn work_change(&self, change: WorkChange) -> Result<PortAnswer, PortFailure> {
        let conversation = self.conversation()?;
        let (revision, _) = self
            .shared
            .apply_work(conversation, vec![change])
            .await
            .map_err(|error| error.refusal())?;
        Ok(PortAnswer::WorkRevision { revision })
    }

    /// Refuses a change of a tab of another plugin's kind; a tab the work
    /// no longer has is the change's to answer.
    async fn own_tab(&self, id: &str) -> Result<(), PortFailure> {
        let work = self
            .shared
            .store
            .work(self.conversation()?.clone())
            .await
            .map_err(storage)?;
        let registry = self.shared.registry();
        let registered = &registry.plugins[self.plugin];
        match work.tabs.iter().find(|tab| tab.id == id) {
            Some(tab) if !registered.owns_kind(&tab.kind) => {
                Err(WorkError::UnknownKind(tab.kind.clone()).refusal())
            }
            _ => Ok(()),
        }
    }

    fn conversation(&self) -> Result<&ConversationId, PortFailure> {
        self.conversation
            .as_ref()
            .ok_or(PortFailure::Refused(PortRefusal::NoConversation))
    }
}

impl PluginTransport for RequestPort {
    fn request(&self, message: PortMessage) -> LocalBoxFuture<'_, Result<PortAnswer, PortError>> {
        Box::pin(async move {
            match self.answer(message).await {
                Ok(answer) => Ok(answer),
                Err(PortFailure::Refused(refusal)) => Ok(PortAnswer::Refused { refusal }),
                Err(PortFailure::Port(error)) => Err(error),
            }
        })
    }
}

fn stored(value: crate::store::PluginValue) -> StoredValue {
    StoredValue {
        value: value.document,
        revision: value.revision,
    }
}

/// A storage failure as the port reports it: the operation failed, which a
/// plugin cannot cause.
pub(crate) fn storage(error: StoreError) -> PortFailure {
    PortFailure::Port(PortError::Failed(error.to_string()))
}
