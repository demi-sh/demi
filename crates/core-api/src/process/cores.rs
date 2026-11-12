//! The cores a process holds, each in the directory a program named: the
//! opens and closes its programs ask, and the lookups the process's services
//! and pool make of them.
//!
//! A program that opens a core the process holds already, as one does that
//! connects again, is answered its id once an open or a close of it under
//! way ended. A core's slot goes through these phases:
//!
//! ```text
//! Opening -> TakingUp -> Serving -> Closing -> (gone)
//!   |          |
//!   +----------+--> (gone), when the open fails
//! ```
//!
//! Its store is open from `TakingUp` on, which the vault and the pool read
//! as the core takes up what it left; only a `Serving` core takes the
//! program's calls.

use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::http::StatusCode;
use demi_core::Core;
use demi_core::extensions::Extensions;
use demi_core::pool::{Opener, Shards};
use demi_core::services::{CoreServices, MigrationBlobs};
use demi_core::start_passes::take_up;
use demi_core::starter::Starter;
use demi_core_blobs::store::Storage;
use demi_core_devices::claims::PendingClaims;
use demi_core_providers::vault::entries::VaultStores;
use demi_core_store::StorageError;
use demi_core_store::seal::CoreSecret;
use demi_core_store::devices::DeviceRecord;
use demi_core_store::store::{CORE_DATABASE, CoreStore};
use demi_core_store::tokens::TokenHash;
use demi_shared_artifacts::{Mode, Permissions, Publication};
use demi_shared_types::ScheduleId;
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::ids::UserId;
use demi_web_api_protocol::settings::InstanceMode;
use futures_util::future::BoxFuture;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::program::ProgramStarter;
use crate::error::ApiError;
use crate::schedules::ScheduleOwners;
use crate::plugin_lanes::PluginLanes;
use crate::starter_lane::StarterLane;

/// Held while a process has the core open, so another process's open of
/// the directory is refused.
const LOCK: &str = "core.lock";
/// The core's principal id.
const PRINCIPAL: &str = "principal";
/// The secret the core's keys derive from.
const SECRET: &str = "core-secret";

/// The cores the process holds, by id.
pub(crate) struct OpenCores {
    slots: Mutex<HashMap<UserId, Slot>>,
    /// Held by an open from its look at the slots until it has a slot of
    /// its own, so two opens of one directory never both make a core.
    opening: tokio::sync::Mutex<()>,
    /// Notified as a slot serves or goes, which an open waiting for one
    /// under way hears, and the process's stop.
    changed: Notify,
    /// Set as the process stops, which opens no core any more.
    stopping: AtomicBool,
    /// Where the process is reached from outside; a core's routes are
    /// under `/cores/{id}` of it.
    public_url: Option<Url>,
}

enum Slot {
    /// Its directory is being opened.
    Opening { directory: PathBuf },
    /// Its store is open.
    Held { core: Held, phase: Phase },
}

impl Slot {
    fn directory(&self) -> &Path {
        match self {
            Self::Opening { directory } => directory,
            Self::Held { core, .. } => &core.directory,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// It takes up what its last run left.
    TakingUp,
    /// It takes the program's calls.
    Serving,
    /// It closes.
    Closing,
}

/// What the process holds of an open core.
struct Held {
    directory: PathBuf,
    store: CoreStore,
    /// Locked while the core is open; dropping it unlocks the directory.
    _lock: File,
    /// Cancelled as the core closes, which ends the program's page
    /// connections to it.
    closed: CancellationToken,
    /// What the core asks of its programs.
    starter_lane: Arc<StarterLane>,
    /// The lanes of the programs' plugins.
    plugin_lanes: Arc<PluginLanes>,
}

/// What a call of a serving core's routes is given of it.
#[derive(Clone)]
pub(crate) struct Serving {
    pub(crate) closed: CancellationToken,
    pub(crate) starter_lane: Arc<StarterLane>,
    pub(crate) plugin_lanes: Arc<PluginLanes>,
}

impl OpenCores {
    pub(crate) fn new(public_url: Option<Url>) -> Self {
        Self {
            slots: Mutex::default(),
            opening: tokio::sync::Mutex::default(),
            changed: Notify::new(),
            stopping: AtomicBool::new(false),
            public_url,
        }
    }

    fn slots(&self) -> MutexGuard<'_, HashMap<UserId, Slot>> {
        // No section panics while it holds the lock.
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store of the core `id`, from its take-up to its close.
    fn store(&self, id: &UserId) -> Option<CoreStore> {
        match self.slots().get(id) {
            Some(Slot::Held { core, .. }) => Some(core.store.clone()),
            _ => None,
        }
    }

    /// The core `id` while it takes the program's calls.
    pub(crate) fn serving(&self, id: &UserId) -> Option<Serving> {
        match self.slots().get(id) {
            Some(Slot::Held {
                core,
                phase: Phase::Serving,
            }) => Some(Serving {
                closed: core.closed.clone(),
                starter_lane: core.starter_lane.clone(),
                plugin_lanes: core.plugin_lanes.clone(),
            }),
            _ => None,
        }
    }

    /// The plugin lanes of the core `id`, from its take-up to its close.
    pub(crate) fn plugin_lanes(&self, id: &UserId) -> Option<Arc<PluginLanes>> {
        match self.slots().get(id) {
            Some(Slot::Held { core, .. }) => Some(core.plugin_lanes.clone()),
            _ => None,
        }
    }

    /// The cores whose store is open.
    pub(crate) fn all_held(&self) -> Vec<UserId> {
        self.slots()
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Held { .. }))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The serving core that holds the device whose runner's token is
    /// `token`, and the device.
    pub(crate) async fn device_by_token(&self, token: TokenHash) -> Result<Option<(UserId, DeviceRecord)>, StorageError> {
        for core in self.all_serving() {
            let Some(store) = self.store(&core) else {
                continue;
            };
            if let Some(device) = store.device_by_token(token.clone()).await? {
                return Ok(Some((core, device)));
            }
        }
        Ok(None)
    }

    /// The cores that take the programs' calls.
    pub(crate) fn all_serving(&self) -> Vec<UserId> {
        self.slots()
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Held { phase: Phase::Serving, .. }))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Opens the core in `directory`, creating it when the directory is
    /// absent or empty, and answers its id once it serves, with whether this
    /// open opened it: a core the process holds already is answered once an
    /// open or a close of it under way ended.
    pub(crate) async fn open(
        &self,
        directory: &Path,
        services: &CoreServices,
        shards: &Shards,
    ) -> Result<(UserId, bool), ApiError> {
        let directory = std::path::absolute(directory).map_err(|error| ApiError::invalid_body(error.to_string()))?;
        tokio::fs::create_dir_all(&directory).await.map_err(internal)?;
        // One directory under two names is one core.
        let directory = tokio::fs::canonicalize(&directory).await.map_err(internal)?;
        let opening = loop {
            let opening = self.opening.lock().await;
            if self.stopping.load(Ordering::Acquire) {
                return Err(ApiError::backend_closing());
            }
            let changed = self.changed.notified();
            let held = self.slots().iter().find(|(_, slot)| slot.directory() == directory).map(|(id, slot)| {
                let serving = matches!(slot, Slot::Held { phase: Phase::Serving, .. });
                (id.clone(), serving)
            });
            match held {
                Some((id, true)) => return Ok((id, false)),
                Some((_, false)) => {
                    drop(opening);
                    changed.await;
                }
                None => break opening,
            }
        };
        let principal = directory.join(PRINCIPAL);
        if !tokio::fs::try_exists(&principal).await.map_err(internal)? {
            refuse_unless_empty(&directory).await?;
        }
        let lock = lock(&directory).await?;
        // A process that created the core between the look and the lock
        // wrote its principal.
        let id = match tokio::fs::read_to_string(&principal).await {
            Ok(text) => UserId::try_from(text.trim().to_owned()).map_err(|_| not_a_core())?,
            Err(error) if error.kind() == ErrorKind::NotFound => create(&principal).await?,
            Err(error) => return Err(internal(error)),
        };
        {
            let mut slots = self.slots();
            // A copy of a directory the process holds names the same core.
            if slots.contains_key(&id) {
                return Err(already_open());
            }
            slots.insert(id.clone(), Slot::Opening { directory: directory.clone() });
        }
        drop(opening);
        let opened = self.take_up(&id, &directory, lock, services, shards).await;
        match opened {
            Ok(()) => {
                self.changed.notify_waiters();
                Ok((id, true))
            }
            Err(error) => {
                self.discard(&id, services, shards).await;
                Err(error)
            }
        }
    }

    /// Opens the store of the core `id` in `directory` and takes up what
    /// its last run left, as the server's start does for each of its cores.
    async fn take_up(
        &self,
        id: &UserId,
        directory: &Path,
        lock: File,
        services: &CoreServices,
        shards: &Shards,
    ) -> Result<(), ApiError> {
        let secret = CoreSecret::load_or_create(&directory.join(SECRET))
            .await
            .map_err(internal)?;
        let objects = demi_core_blobs::store::open(directory, &Storage::Local)
            .await
            .map_err(internal)?;
        services.blobs.place(id, objects.store);
        let store = CoreStore::open(
            directory,
            id.clone(),
            secret.vault_key(),
            services.clock.clone(),
            MigrationBlobs::new(services.blobs.for_user(id), tokio::runtime::Handle::current()),
        )
        .await?;
        self.slots().insert(
            id.clone(),
            Slot::Held {
                core: Held {
                    directory: directory.to_owned(),
                    store: store.clone(),
                    _lock: lock,
                    closed: CancellationToken::new(),
                    starter_lane: Arc::default(),
                    plugin_lanes: Arc::default(),
                },
                phase: Phase::TakingUp,
            },
        );
        services.vault.hold_core(id).await?;
        services.returning.take(std::slice::from_ref(&store)).await?;
        take_up(&[(id.clone(), store)], shards).await?;
        if let Some(Slot::Held { phase, .. }) = self.slots().get_mut(id) {
            *phase = Phase::Serving;
        }
        tracing::info!(event = "core.opened", core = %id, "a core opened");
        Ok(())
    }

    /// Closes the serving core `id`: ends the programs' connections to it,
    /// closes it as the server's stop closes each core, then its store, and
    /// unlocks its directory.
    pub(crate) async fn close(&self, id: &UserId, services: &CoreServices, shards: &Shards) -> Result<(), ApiError> {
        {
            let mut slots = self.slots();
            let Some(Slot::Held { core, phase }) = slots.get_mut(id) else {
                return Err(not_open());
            };
            if *phase != Phase::Serving {
                return Err(not_open());
            }
            *phase = Phase::Closing;
            core.closed.cancel();
        }
        self.discard(id, services, shards).await;
        tracing::info!(event = "core.closed", core = %id, "a core closed");
        Ok(())
    }

    /// Closes every core, as the process ends: an open under way serves
    /// first, and a close under way ends, and no core opens afterwards.
    pub(crate) async fn close_all(&self, services: &CoreServices, shards: &Shards) {
        {
            // An open past this look sees the stop.
            let _opening = self.opening.lock().await;
            self.stopping.store(true, Ordering::Release);
        }
        loop {
            let changed = self.changed.notified();
            for id in self.all_serving() {
                // A core another close took meanwhile is that close's.
                let _ = self.close(&id, services, shards).await;
            }
            if self.slots().is_empty() {
                return;
            }
            changed.await;
        }
    }

    /// Closes what the process holds of the core `id`, whatever its phase,
    /// and forgets it.
    async fn discard(&self, id: &UserId, services: &CoreServices, shards: &Shards) {
        let store = self.store(id);
        if store.is_some() {
            // A core the open never started has nothing to close.
            let _ = shards.of(id).close().await;
            services.vault.release_core(id);
        }
        if let Some(store) = store {
            for error in store.close().await {
                tracing::error!(
                    event = "core.store_close_failed",
                    core = %id,
                    error = &error as &dyn std::error::Error,
                    "a core's store did not close"
                );
            }
        }
        // Dropping the slot unlocks the directory.
        self.slots().remove(id);
        self.changed.notify_waiters();
    }
}

/// Refuses a directory that holds anything: a server's core, or what is no
/// core at all.
async fn refuse_unless_empty(directory: &Path) -> Result<(), ApiError> {
    let mut entries = tokio::fs::read_dir(directory).await.map_err(internal)?;
    while let Some(entry) = entries.next_entry().await.map_err(internal)? {
        // A process that failed to create the core may leave its lock.
        if entry.file_name() != LOCK {
            return Err(if entry.file_name() == CORE_DATABASE {
                ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::NotACoreDirectory,
                    "The directory holds a core no program created",
                )
            } else {
                not_a_core()
            });
        }
    }
    Ok(())
}

/// Locks the directory for this process, refused while another holds it.
async fn lock(directory: &Path) -> Result<File, ApiError> {
    let path = directory.join(LOCK);
    let file = tokio::task::spawn_blocking(move || {
        let file = File::options().create(true).truncate(false).write(true).open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    file.ok_or_else(already_open)
}

/// Makes a new core's principal and keeps it in `path`.
async fn create(path: &Path) -> Result<UserId, ApiError> {
    let id = UserId::try_from(uuid::Uuid::new_v4().to_string()).expect("a UUID is not empty");
    let publication = Publication {
        mode: Mode::CreateNew,
        permissions: Permissions::Default,
        durable: true,
    };
    demi_shared_artifacts::publish_bytes(path, format!("{id}\n").as_bytes(), publication)
        .await
        .map_err(internal)?;
    Ok(id)
}

fn internal(error: impl std::error::Error + 'static) -> ApiError {
    ApiError::internal(&error)
}

fn not_a_core() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        ErrorCode::NotACoreDirectory,
        "The directory holds something other than a core",
    )
}

fn already_open() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        ErrorCode::CoreOpen,
        "Another process has this core open",
    )
}

pub(crate) fn not_open() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, "No core of that id is open")
}

/// The vault reads the cores' entries through the slots: a program's process
/// is an isolated instance, whose cores share nothing.
impl VaultStores for OpenCores {
    fn store_of<'a>(&'a self, user: &'a UserId) -> BoxFuture<'a, Result<CoreStore, StorageError>> {
        Box::pin(async move { self.store(user).ok_or(StorageError::Closed) })
    }

    fn users(&self) -> BoxFuture<'_, Result<Vec<UserId>, StorageError>> {
        Box::pin(async move {
            Ok(self
                .slots()
                .iter()
                .filter(|(_, slot)| matches!(slot, Slot::Held { .. }))
                .map(|(id, _)| id.clone())
                .collect())
        })
    }

    fn master(&self) -> BoxFuture<'_, Result<Option<UserId>, StorageError>> {
        Box::pin(async { Ok(None) })
    }

    /// The process routes no change of the mode.
    fn keep_mode(&self, _: InstanceMode) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async { Ok(()) })
    }
}

/// A schedule's URL reaches the serving core that holds the schedule.
impl ScheduleOwners for OpenCores {
    fn owner<'a>(&'a self, id: &'a ScheduleId) -> BoxFuture<'a, Result<Option<UserId>, StorageError>> {
        Box::pin(async move {
            for core in self.all_serving() {
                let Some(store) = self.store(&core) else {
                    continue;
                };
                if store.schedule(id.clone()).await?.is_some() {
                    return Ok(Some(core));
                }
            }
            Ok(None)
        })
    }
}

/// What the process gives its pool: the store of each core it holds, and
/// the programs' starter.
pub(crate) struct ProcessOpener {
    pub(crate) cores: Arc<OpenCores>,
    /// The runners waiting to be paired, which an agent's `demi device pair`
    /// claims; none for a process over stdio, which no runner dials.
    pub(crate) claims: Option<Arc<PendingClaims>>,
}

impl Opener for ProcessOpener {
    fn store<'a>(&'a self, user: &'a UserId) -> BoxFuture<'a, Result<Option<CoreStore>, StorageError>> {
        Box::pin(async move { Ok(self.cores.store(user)) })
    }

    fn starter(&self, user: &UserId, core: Weak<Core>) -> Rc<dyn Starter> {
        let starter_lane = match self.cores.slots().get(user) {
            Some(Slot::Held { core, .. }) => core.starter_lane.clone(),
            // The pool starts only a core whose store it found.
            _ => Arc::default(),
        };
        let public_url = self.cores.public_url.as_ref().and_then(|url| {
            format!("{}/cores/{user}", url.as_str().trim_end_matches('/')).parse().ok()
        });
        Rc::new(ProgramStarter {
            user: user.clone(),
            core,
            starter_lane,
            public_url,
            claims: self.claims.clone(),
        })
    }

    fn extensions(&self, _: &UserId) -> Extensions {
        Extensions::default()
    }
}
