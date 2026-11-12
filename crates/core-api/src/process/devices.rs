//! A program's devices: it makes one, then carries its runner's bytes to the
//! process over a channel of the program's own, such as `docker exec` of a
//! `demi-runner run --stdio`.
//!
//! ```text
//! runner's stdio <-- bytes --> program <-- CONNECT .../devices/{id}/runner ->
//!                                          :protocol h2c, HTTP/2 inside:
//!                                          the runner's socket and pipes
//! ```
//!
//! The channel names the device, so the runner presents no token; the
//! runner's routes are the ones the server serves a runner whose token names
//! its device. The channel ends with its core, as the program closes it.
//!
//! A channel on which no runner opens its socket within the hello deadline
//! is closed, and the socket's hello then has the deadline a server's runner
//! has: a command that is no runner, which the program started by mistake,
//! ends its channel rather than holding it open.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Extension, Json, Router};
use demi_core_devices::devices::RunnerSocket;
use demi_core_devices::packages::ARTIFACT_ROUTE;
use demi_runner_protocol::wire::{LINK_FRAME_BYTES, Watched, WriteProgress};
use demi_web_api_protocol::devices::{DeviceAnswer, DeviceKind};
use hyper::ext::Protocol;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::service::TowerToHyperService;
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use super::packages::ReleaseFiles;
use super::stream::stream_peer;
use crate::api::Api;
use crate::caller::{CallerOf, Principal};
use crate::error::ApiError;
use crate::runners::{RunnerDevice, RunnerOf};

/// `POST /devices`: makes a device, 201 with it.
pub(crate) async fn create(
    State(state): State<Api>,
    Principal(user): Principal,
) -> Result<(StatusCode, Json<DeviceAnswer>), ApiError> {
    let device = state
        .shards
        .of(&user)
        .call(|core, _| async move { core.create_device().await })
        .await??;
    Ok((StatusCode::CREATED, Json(DeviceAnswer { device })))
}

/// `CONNECT /devices/{id}/runner`, with `:protocol` `h2c`: the device's
/// channel, on which the process serves its runner's routes once it
/// answers 200.
pub(crate) async fn channel(
    State(state): State<Api>,
    Principal(owner): Principal,
    CallerOf(caller): CallerOf,
    Path(device): Path<String>,
    Extension(release): Extension<Arc<ReleaseFiles>>,
    mut request: Request,
) -> Result<Response, ApiError> {
    let protocol = request.extensions().get::<Protocol>().map(Protocol::as_str);
    if protocol != Some(CHANNEL_PROTOCOL) {
        return Ok(super::no_route(request).await);
    }
    let record = crate::devices::owned_device(&state, &owner, &device, Some(DeviceKind::User)).await?;
    let upgrade = hyper::upgrade::on(&mut request);
    let device = RunnerDevice { owner, id: record.id };
    let span = tracing::info_span!("runner_channel", device_id = %device.id);
    let core_closed = caller.gone();
    let deadline = state.services.runners.hello_deadline;
    let arrived = CancellationToken::new();
    tokio::spawn(
        async move {
            let io = match upgrade.await {
                Ok(io) => io,
                Err(error) => {
                    tracing::info!(event = "runner_channel.not_opened", %error, "a runner's channel did not open");
                    return;
                }
            };
            tracing::info!(event = "runner_channel.opened", "a runner's channel opened");
            // The channel is the runner's connection, as its TCP connection
            // is the server's: its link counts it lost once its writes stall.
            let (io, writes) = Watched::new(TokioIo::new(io));
            let routes = runner_routes(state, device, release, ConnectionWrites(writes), RunnerArrived(arrived.clone()));
            let serving = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .enable_connect_protocol()
                .serve_connection(TokioIo::new(io), TowerToHyperService::new(routes));
            let served = tokio::select! {
                served = serving => served,
                // Dropping the connection ends the channel, and with it the
                // runner, as the program's own end of it does.
                _ = core_closed => {
                    tracing::info!(event = "runner_channel.closed", "a runner's channel ended with its core");
                    return;
                }
                () = no_runner(deadline, &arrived) => {
                    tracing::info!(
                        event = "runner_channel.closed",
                        reason = "no_runner",
                        deadline_ms = u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                        "closing a runner's channel on which no runner came"
                    );
                    return;
                }
            };
            match served {
                Ok(()) => tracing::info!(event = "runner_channel.closed", "a runner's channel closed"),
                Err(error) => tracing::info!(event = "runner_channel.closed", %error, "a runner's channel broke"),
            }
        }
        .instrument(span),
    );
    Ok(StatusCode::OK.into_response())
}

/// The channel's `:protocol`: HTTP/2 without TLS (RFC 9113 § 3.1), which
/// the channel carries.
const CHANNEL_PROTOCOL: &str = "h2c";

/// Whether the writes of a runner's connection move: a device's channel, or
/// the TCP connection a runner dialed the process on.
#[derive(Clone)]
pub(super) struct ConnectionWrites(pub(super) Arc<WriteProgress>);

/// Cancelled once a runner opened its socket on the channel.
#[derive(Clone)]
struct RunnerArrived(CancellationToken);

/// Resolves once `deadline` passed and no runner opened its socket on the
/// channel; never once one did.
async fn no_runner(deadline: std::time::Duration, arrived: &CancellationToken) {
    tokio::select! {
        () = tokio::time::sleep(deadline) => {}
        () = arrived.cancelled() => std::future::pending().await,
    }
}

/// The routes a device's channel serves its runner, as the device `device`.
fn runner_routes(
    api: Api,
    device: RunnerDevice,
    release: Arc<ReleaseFiles>,
    writes: ConnectionWrites,
    arrived: RunnerArrived,
) -> Router {
    crate::runners::routes()
        .route("/api/runner", any(socket))
        .route(&format!("{ARTIFACT_ROUTE}/{{sha256}}"), get(super::packages::artifact))
        .fallback(super::no_route)
        .with_state(api)
        .layer(Extension(RunnerOf(device)))
        .layer(Extension(release))
        .layer(Extension(writes))
        .layer(Extension(arrived))
        .layer(middleware::from_fn(stream_peer))
        .layer(middleware::from_fn(crate::request_id::assign))
}

/// `WS /api/runner`: the runner's link, which its core takes once its hello
/// came.
async fn socket(
    State(state): State<Api>,
    RunnerOf(device): RunnerOf,
    Extension(ConnectionWrites(writes)): Extension<ConnectionWrites>,
    Extension(RunnerArrived(arrived)): Extension<RunnerArrived>,
    upgrade: WebSocketUpgrade,
) -> Response {
    arrived.cancel();
    let deadline = state.services.runners.hello_deadline;
    let shards = state.shards.clone();
    let span = tracing::Span::current();
    upgrade
        .max_message_size(LINK_FRAME_BYTES)
        .max_frame_size(LINK_FRAME_BYTES)
        .on_upgrade(move |socket| {
            let socket = RunnerSocket::new(socket, writes);
            crate::runner_socket::accept_device(deadline, shards, device.owner, device.id, socket).instrument(span)
        })
}
