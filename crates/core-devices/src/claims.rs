//! Runners waiting to be paired (`crates/core-api/src/runner_socket.rs`), as
//! a server and a program's process pair them.
//!
//! An unpaired runner has no user yet, so it waits at the edge, under the code
//! it printed, until a signed-in user or a program claims that code for a
//! core. The claim creates the device and hands the waiting runner its token,
//! and the runner's socket moves into the core. Each principal may try ten
//! codes a minute.
//!
//! Where the token is kept, which finds the device when its runner connects
//! again, is the pairing's owner's ([`DeviceTokens`]): a server's control
//! database, or the device's record in a process's core.
//!
//! An agent pairs a computer it reaches without the user typing the code (`demi
//! device pair`): the code is proof enough, since it reaches only the runner's
//! output and its log, which only an account that runs commands on that
//! computer reads, so an agent holding it already controls the computer.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use demi_core_store::StorageError;
use demi_core_store::devices::{DeviceRecord, new_device_id};
use demi_core_store::store::CoreStore;
use demi_core_store::tokens::TokenHash;
use demi_runner_protocol::values::DeviceToken;
use demi_runner_protocol::wire::RunnerInfo;
use demi_web_api_protocol::devices::DeviceDto;
use demi_web_api_protocol::ids::{DeviceId, UserId};
use futures_util::future::BoxFuture;
use tokio::sync::oneshot;

use demi_shared_gates::RateWindow;

use crate::codes::{ClaimCode, new_device_token};

/// Where a paired device's token is kept, which finds the device when its
/// runner connects again.
pub trait DeviceTokens: Send + Sync {
    /// Makes `runner`'s device `id` in `owner`'s core, `store`, which the
    /// token whose hash is `token` finds from then on.
    fn create<'a>(
        &'a self,
        store: &'a CoreStore,
        owner: &'a UserId,
        id: DeviceId,
        runner: &'a RunnerInfo,
        token: TokenHash,
    ) -> BoxFuture<'a, Result<DeviceRecord, StorageError>>;

    /// Unmakes the device `id` that [`Self::create`] made, whose runner went
    /// away before it held its token.
    fn delete<'a>(&'a self, store: &'a CoreStore, id: DeviceId) -> BoxFuture<'a, Result<(), StorageError>>;
}

/// The tokens a program's process keeps: each in its device's record, which
/// the process finds among its open cores.
pub struct RecordedTokens;

impl DeviceTokens for RecordedTokens {
    fn create<'a>(
        &'a self,
        store: &'a CoreStore,
        _: &'a UserId,
        id: DeviceId,
        runner: &'a RunnerInfo,
        token: TokenHash,
    ) -> BoxFuture<'a, Result<DeviceRecord, StorageError>> {
        Box::pin(store.create_device(id, runner.name.clone(), runner.platform, Some(token)))
    }

    fn delete<'a>(&'a self, store: &'a CoreStore, id: DeviceId) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move { store.delete_device(id).await.map(|_| ()) })
    }
}

/// The window claim attempts are counted in. Attempts are limited because a
/// pairing code is short, and unlimited tries would let a user guess one.
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);

/// The runners waiting to be paired, and each principal's recent claims.
pub struct PendingClaims {
    tokens: Box<dyn DeviceTokens>,
    /// A `std` mutex: the edge's threads share the waiting runners, and no
    /// section awaits.
    state: Mutex<State>,
    /// Each user's claims within the last minute.
    attempts: RateWindow<UserId>,
}

#[derive(Default)]
struct State {
    waiting: HashMap<ClaimCode, Waiting>,
    closed: bool,
}

struct Waiting {
    runner: RunnerInfo,
    grant: oneshot::Sender<ClaimGrant>,
}

/// A waiting runner a claim took: it is no longer anyone else's to claim.
pub struct PendingRunner {
    pub runner: RunnerInfo,
    grant: oneshot::Sender<ClaimGrant>,
}

/// Why a claim paired no device.
#[derive(Debug, thiserror::Error)]
pub enum ClaimRefusal {
    /// The user tried as many codes as a minute allows.
    #[error("too many claim attempts")]
    TooManyAttempts,
    /// No runner waits under the code: it expired, was claimed, or was
    /// never issued.
    #[error("unknown or expired pairing code")]
    UnknownCode,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// What a claim hands its runner: the user who claimed it, the device made
/// for it, the token only the runner receives, and where the device's answer
/// goes once the runner's socket is bound in the user's core.
pub struct ClaimGrant {
    pub owner: UserId,
    pub device: DeviceRecord,
    pub token: DeviceToken,
    pub bound: oneshot::Sender<DeviceDto>,
}

impl PendingClaims {
    pub fn new(tokens: impl DeviceTokens + 'static, attempts_per_minute: usize) -> Self {
        Self {
            tokens: Box::new(tokens),
            state: Mutex::default(),
            attempts: RateWindow::new(attempts_per_minute, ATTEMPT_WINDOW),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // No section panics while it holds the lock, so a poisoned one still
        // holds whole records.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pairs the runner waiting under `code`, spelled as a person types it,
    /// with `user`: the device is made, its token goes to that runner alone,
    /// and the answer is the device once the runner's socket is bound in the
    /// user's core. Add Device's `POST /devices/claim` and `demi device pair`
    /// both claim this way.
    pub async fn claim(
        &self,
        store: &CoreStore,
        user: &UserId,
        code: &str,
    ) -> Result<DeviceDto, ClaimRefusal> {
        if !self.attempts.attempt(user) {
            return Err(ClaimRefusal::TooManyAttempts);
        }
        let pending = ClaimCode::parse(code)
            .and_then(|code| self.take(&code))
            .ok_or(ClaimRefusal::UnknownCode)?;
        let token = new_device_token();
        let id = new_device_id();
        let device = self
            .tokens
            .create(store, user, id.clone(), &pending.runner, TokenHash::of(token.expose()))
            .await?;
        match pending.grant(user.clone(), device, token).await {
            Ok(device) => Ok(device),
            Err(_) => {
                // The runner went away before it held its token: the device
                // it would have been goes too.
                self.tokens.delete(store, id).await?;
                Err(ClaimRefusal::UnknownCode)
            }
        }
    }

    /// [`Self::claim`] for an agent's `demi device pair`, whose refusal tells
    /// the agent what to do next.
    pub async fn pair(&self, store: &CoreStore, user: &UserId, code: &str) -> Result<DeviceDto, String> {
        match self.claim(store, user, code).await {
            Ok(device) => Ok(device),
            Err(ClaimRefusal::UnknownCode) => Err(format!(
                "{code}: no computer waits under this pairing code; it expired or was never issued. The installer shows a new code when one expires: read the newest one it shows and pass that."
            )),
            Err(ClaimRefusal::TooManyAttempts) => Err(
                "Too many pairing codes were tried in the last minute; wait a minute, then pass the newest code the installer shows.".into(),
            ),
            Err(ClaimRefusal::Storage(error)) => Err(error.to_string()),
        }
    }

    /// The runner waiting under `code`, which only this claim holds from now
    /// on.
    fn take(&self, code: &ClaimCode) -> Option<PendingRunner> {
        let waiting = self.lock().waiting.remove(code)?;
        Some(PendingRunner {
            runner: waiting.runner,
            grant: waiting.grant,
        })
    }

    /// Puts `runner` up for claiming under `code`; none once the server or
    /// the process stops.
    pub fn wait(
        &self,
        code: ClaimCode,
        runner: RunnerInfo,
    ) -> Option<oneshot::Receiver<ClaimGrant>> {
        let mut state = self.lock();
        if state.closed {
            return None;
        }
        let (grant, granted) = oneshot::channel();
        state.waiting.insert(code, Waiting { runner, grant });
        Some(granted)
    }

    /// Takes a code back: it expired, or its runner went away.
    pub fn withdraw(&self, code: &ClaimCode) {
        self.lock().waiting.remove(code);
    }

    /// Lets every waiting runner go, whose sockets then close, and puts up
    /// no more.
    pub fn close(&self) {
        let waiting = {
            let mut state = self.lock();
            state.closed = true;
            std::mem::take(&mut state.waiting)
        };
        drop(waiting);
    }
}

impl PendingRunner {
    /// Hands the runner its device and token. The answer is the device as it
    /// is once the runner is bound; none when the runner went away before.
    pub fn grant(self, owner: UserId, device: DeviceRecord, token: DeviceToken) -> oneshot::Receiver<DeviceDto> {
        let (bound, answer) = oneshot::channel();
        // A runner that went away dropped its end, and `answer` then resolves
        // without a device.
        let _ = self.grant.send(ClaimGrant {
            owner,
            device,
            token,
            bound,
        });
        answer
    }
}
