//! The runners that dial a process on its own, as they dial a server
//! (`demi-runner run --backend <the process's address>`): their socket, their
//! pipes and the command packages' artifacts.
//!
//! - A runner's token finds its device among the open cores, whose records
//!   keep each device's token (`crates/core-devices/src/claims.rs`).
//! - A runner without a token waits under the code it prints until a
//!   program claims it for a core, `POST /cores/{id}/api/devices/claim`.
//!
//! The process refuses no token: it cannot tell a removed device from one
//! whose core is not open yet. A socket whose token no open core holds is
//! closed, and its runner dials again, as after a dropped connection. A
//! device removed while its runner is connected is revoked, as on a server.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Extension, Router};
use axum_extra::TypedHeader;
use axum_extra::headers::Authorization;
use axum_extra::headers::authorization::Bearer;
use demi_core_devices::devices::RunnerSocket;
use demi_core_devices::packages::ARTIFACT_ROUTE;
use demi_core_store::tokens::TokenHash;
use demi_runner_protocol::wire::LINK_FRAME_BYTES;
use tracing::Instrument as _;

use super::Process;
use super::devices::ConnectionWrites;
use crate::runner_socket::Presented;
use crate::runners::{RunnerDevice, RunnerOf};

/// The routes a runner reaches the process on.
pub(super) fn routes(process: Arc<Process>) -> Router {
    let pipes = crate::runners::routes()
        .with_state(process.api.clone())
        .layer(middleware::from_fn_with_state(process.clone(), token_device));
    Router::new()
        .route("/api/runner", any(socket))
        .route(&format!("{ARTIFACT_ROUTE}/{{sha256}}"), get(super::packages::artifact))
        .with_state(process.clone())
        .merge(pipes)
        .layer(Extension(process.release.clone()))
        .layer(middleware::from_fn(super::stream::stream_peer))
        .layer(middleware::from_fn(crate::request_id::assign))
}

/// `WS /api/runner`: the runner's link, which a core takes once its hello
/// came.
async fn socket(
    State(process): State<Arc<Process>>,
    Extension(ConnectionWrites(writes)): Extension<ConnectionWrites>,
    upgrade: WebSocketUpgrade,
) -> Response {
    // The runner's socket logs under the request ID of its upgrade.
    let span = tracing::Span::current();
    upgrade
        .max_message_size(LINK_FRAME_BYTES)
        .max_frame_size(LINK_FRAME_BYTES)
        .on_upgrade(move |socket| accept(process, RunnerSocket::new(socket, writes)).instrument(span))
}

/// Serves a runner's socket until a core takes it or it closes: its token
/// finds its device among the open cores.
async fn accept(process: Arc<Process>, socket: RunnerSocket) {
    let api = &process.api;
    let find = |token| async {
        Ok(match process.cores.device_by_token(token).await? {
            Some((owner, device)) => Presented::Device(owner, device),
            None => {
                tracing::info!(event = "runner_connection.closed", reason = "unknown_token", "closing a runner connection whose token no open core holds");
                Presented::Away
            }
        })
    };
    crate::runner_socket::accept(socket, &api.services.runners, &api.claims, &api.shards, find).await;
}

/// Finds the device whose token a request of a runner's pipes bears among
/// the open cores; 401 without one of them.
async fn token_device(
    State(process): State<Arc<Process>>,
    authorization: Option<TypedHeader<Authorization<Bearer>>>,
    mut request: Request,
    next: Next,
) -> Response {
    let unauthorized = || (StatusCode::UNAUTHORIZED, "device token required").into_response();
    let Some(TypedHeader(Authorization(bearer))) = authorization else {
        return unauthorized();
    };
    match process.cores.device_by_token(TokenHash::of(bearer.token())).await {
        Ok(Some((owner, device))) => {
            request.extensions_mut().insert(RunnerOf(RunnerDevice { owner, id: device.id }));
            next.run(request).await
        }
        Ok(None) => unauthorized(),
        Err(error) => {
            tracing::error!(
                event = "runner_device.read_failed",
                route = "pipe",
                error = &error as &dyn std::error::Error,
                "a pipe's device could not be read"
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
