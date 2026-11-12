//! A runner's socket as every way in to a core takes it
//! (`crates/runner-protocol/src/wire/mod.rs`): its first message is its
//! hello, which must come within the hello deadline, and a socket whose
//! device is known moves into the core that holds it.
//!
//! - A runner that dials a server, or a process on its own, presents the
//!   token of its device, which the way in finds, or none, and waits under
//!   the code it prints until it is claimed ([`accept`]).
//! - A runner on a channel a program gives is the device the channel names
//!   ([`accept_device`]).

use std::rc::Rc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use demi_core::pool::Shards;
use demi_core::tuning::RunnerTuning;
use demi_core::{Core, KeptJobs};
use demi_core_devices::claims::{ClaimGrant, PendingClaims};
use demi_core_devices::codes::ClaimCode;
use demi_core_devices::devices::{RunnerSocket, send};
use demi_core_store::StorageError;
use demi_core_store::devices::DeviceRecord;
use demi_core_store::tokens::TokenHash;
use demi_runner_protocol::values::DeviceToken;
use demi_runner_protocol::wire::{self, HelloErrorCode, Inbound, Outbound, RunnerInfo};
use demi_web_api_protocol::ids::{DeviceId, UserId};

/// What a runner's hello says.
struct Hello {
    protocol: u32,
    token: Option<DeviceToken>,
    runner: RunnerInfo,
    kept: KeptJobs,
}

/// What a runner's token finds, as the way in that took its socket looks.
pub enum Presented {
    /// The device it names, in the core of its owner.
    Device(UserId, DeviceRecord),
    /// No device: the runner is told so, and stops.
    Unknown,
    /// No device that takes its runner now, which the lookup logged: the
    /// socket closes without a word, and the runner dials again, as after a
    /// dropped connection.
    Away,
}

/// Serves the socket of a runner that dialed until a core takes it or it
/// closes: a token finds its device by `find`; a runner without one waits
/// under the code it prints until it is claimed.
pub async fn accept<Found>(
    socket: RunnerSocket,
    tuning: &RunnerTuning,
    claims: &PendingClaims,
    shards: &Shards,
    find: impl FnOnce(TokenHash) -> Found,
) where
    Found: Future<Output = Result<Presented, StorageError>>,
{
    let Some((mut socket, hello)) = greet(socket, tuning.hello_deadline).await else {
        return;
    };
    let Hello { token, runner, kept, .. } = hello;
    let Some(token) = token else {
        // A managed device is born with its token; one without it is
        // misbooted, never a device waiting to be paired.
        if runner.managed == Some(true) {
            let reason = "a managed device presents its device token; it is never paired";
            refuse(socket, &runner, HelloErrorCode::UnknownDevice, reason).await;
        } else {
            await_claim(claims, tuning.claim_lifetime, shards, socket, runner, kept.instance).await;
        }
        return;
    };
    let found = tokio::select! {
        found = find(TokenHash::of(token.expose())) => found,
        // The runner went away during the lookup: its socket never becomes
        // online. A repeated hello says nothing new.
        () = closed(&mut socket) => return,
    };
    let (owner, device) = match found {
        Ok(Presented::Device(owner, device)) => (owner, device),
        Ok(Presented::Unknown) => {
            refuse(socket, &runner, HelloErrorCode::UnknownDevice, "unknown device").await;
            return;
        }
        Ok(Presented::Away) => return,
        Err(error) => {
            tracing::error!(
                event = "runner_device.read_failed",
                error = &error as &dyn std::error::Error,
                "a runner's device could not be read"
            );
            refuse(socket, &runner, HelloErrorCode::Internal, "the device could not be read").await;
            return;
        }
    };
    let adopted = shards
        .of(&owner)
        .adopt(move |core| async move { core.adopt_runner(device, runner, kept, socket).await })
        .await;
    if adopted.is_err() {
        closing_refused("runner");
    }
}

/// The hello of the socket, which must come within `deadline`, in the
/// protocol this end speaks; none once the socket closed or was refused.
async fn greet(mut socket: RunnerSocket, deadline: Duration) -> Option<(RunnerSocket, Hello)> {
    let hello = match tokio::time::timeout(deadline, hello(&mut socket)).await {
        Ok(Some(hello)) => hello,
        Ok(None) => return None,
        Err(_) => {
            tracing::info!(event = "runner_connection.closed", reason = "no_hello", deadline_ms = u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX), "closing a runner connection that sent no hello");
            return None;
        }
    };
    if hello.protocol != wire::VERSION {
        let reason = format!("unsupported protocol {}; this backend speaks {}", hello.protocol, wire::VERSION);
        refuse(socket, &hello.runner, HelloErrorCode::UnsupportedProtocol, &reason).await;
        return None;
    }
    Some((socket, hello))
}

/// Logs that a core that closes took no runner: dropping the socket closes
/// it without a word, and the runner dials again.
fn closing_refused(kind: &'static str) {
    tracing::info!(event = "socket.refused", kind, reason = "shutting_down", "a runner arrived while its core closes");
}

/// The first message, which must be a hello; none when the socket closed or
/// sent something else, which breaks the protocol.
async fn hello(socket: &mut WebSocket) -> Option<Hello> {
    loop {
        let frame = match socket.recv().await? {
            Ok(Message::Binary(frame)) => frame,
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(Message::Text(_)) => {
                tracing::info!(event = "runner_connection.closed", reason = "text_frame", "closing a runner connection that sent a text frame");
                return None;
            }
            Ok(Message::Close(_)) | Err(_) => return None,
        };
        // The hello belongs to the connection: no number covers it.
        let own = match wire::Session::new().read(&frame) {
            Ok(wire::Read::Own(own)) => own,
            _ => {
                tracing::info!(event = "runner_connection.closed", reason = "first_not_hello", "closing a runner connection whose first message is not a hello");
                return None;
            }
        };
        return match wire::decode::<Outbound>(own) {
            Ok(Outbound::Hello {
                protocol,
                device_token,
                runner,
                instance,
                jobs,
                resume,
            }) => Some(Hello {
                protocol,
                token: device_token,
                runner,
                kept: KeptJobs {
                    instance,
                    jobs,
                    resume,
                },
            }),
            Ok(_) => {
                tracing::info!(event = "runner_connection.closed", reason = "first_not_hello", "closing a runner connection whose first message is not a hello");
                None
            }
            Err(error) => {
                tracing::info!(event = "runner_connection.closed", reason = "malformed_frame", %error, "closing a runner connection over a malformed frame");
                None
            }
        };
    }
}

/// Resolves once the socket closed; the messages before that are dropped,
/// since a runner sends nothing but its hello before it is answered.
async fn closed(socket: &mut WebSocket) {
    while let Some(Ok(message)) = socket.recv().await {
        if matches!(message, Message::Close(_)) {
            return;
        }
    }
}

/// Answers the hello with a refusal and closes the socket.
async fn refuse(mut socket: RunnerSocket, runner: &RunnerInfo, code: HelloErrorCode, reason: &str) {
    tracing::warn!(
        event = "runner_hello.refused",
        code = %code,
        reason,
        runner_name = %runner.name,
        platform = %runner.platform,
        "a runner's hello was refused"
    );
    let refusal = Inbound::HelloError {
        code,
        reason: reason.into(),
    };
    // A runner that went away hears nothing, and there is nothing to close.
    if send(&mut socket, &refusal).await.is_ok() {
        let _ = socket.send(Message::Close(None)).await;
    }
}


/// Serves the socket of the runner a channel carries for the device `device`
/// of `owner`'s core until the core takes it: the runner's hello must come
/// within `deadline`, and presents no token, since the channel names the
/// device.
pub async fn accept_device(
    deadline: std::time::Duration,
    shards: Shards,
    owner: UserId,
    device: DeviceId,
    socket: RunnerSocket,
) {
    let Some((socket, hello)) = greet(socket, deadline).await else {
        return;
    };
    let Hello { runner, kept, .. } = hello;
    let adopted = shards
        .of(&owner)
        .adopt(move |core: Rc<Core>| async move {
            match core.store().device(device).await {
                Ok(Some(record)) => core.adopt_runner(record, runner, kept, socket).await,
                Ok(None) => refuse(socket, &runner, HelloErrorCode::UnknownDevice, "unknown device").await,
                Err(error) => {
                    tracing::error!(
                        event = "runner_device.read_failed",
                        error = &error as &dyn std::error::Error,
                        "a runner's device could not be read"
                    );
                    refuse(socket, &runner, HelloErrorCode::Internal, "the device could not be read").await;
                }
            }
        })
        .await;
    if adopted.is_err() {
        closing_refused("runner");
    }
}

/// Waits for a claim of the runner, printing a new code every `lifetime`,
/// and hands the socket to the core the claim was for once claimed. Frames
/// the runner sends meanwhile are ignored; unclaimed runners are not pinged.
async fn await_claim(
    claims: &PendingClaims,
    lifetime: Duration,
    shards: &Shards,
    mut socket: RunnerSocket,
    runner: RunnerInfo,
    instance: u64,
) {
    loop {
        let code = ClaimCode::generate();
        let Some(granted) = claims.wait(code, runner.clone()) else {
            return;
        };
        let pending = Inbound::ClaimPending {
            claim_token: code.printed(),
        };
        if send(&mut socket, &pending).await.is_err() {
            claims.withdraw(&code);
            return;
        }
        let expired = tokio::time::sleep(lifetime);
        tokio::pin!(expired, granted);
        let grant = loop {
            tokio::select! {
                () = &mut expired => break None,
                grant = &mut granted => match grant {
                    Ok(grant) => break Some(grant),
                    // The server or the process is shutting down and lets
                    // its waiting runners go.
                    Err(_) => return,
                },
                message = socket.recv() => match message {
                    Some(Ok(Message::Close(_)) | Err(_)) | None => {
                        claims.withdraw(&code);
                        return;
                    }
                    Some(Ok(_)) => {}
                },
            }
        };
        match grant {
            // The code expired: it is dead, and a new one goes out.
            None => claims.withdraw(&code),
            Some(grant) => {
                hand_over(shards, socket, runner, instance, grant).await;
                return;
            }
        }
    }
}

/// Moves the claimed runner's socket into the core it was claimed for,
/// which gives the runner its token. A runner lost on the way leaves the grant
/// unanswered, and the claim then deletes the device it made.
async fn hand_over(
    shards: &Shards,
    socket: RunnerSocket,
    runner: RunnerInfo,
    instance: u64,
    grant: ClaimGrant,
) {
    let ClaimGrant {
        owner,
        device,
        token,
        bound,
    } = grant;
    let adopted = shards
        .of(&owner)
        .adopt(move |core| async move {
            core.adopt_claimed(device, runner, instance, token, socket, bound).await
        })
        .await;
    if adopted.is_err() {
        closing_refused("runner_claimed");
    }
}
