//! The process on its own, `demi-core serve`: it listens on a TCP address
//! for its programs and its runners, until it is stopped, which closes
//! every core. Each connection speaks HTTP/1.1 or HTTP/2 in cleartext, as a
//! server's does behind a proxy that ends TLS.
//!
//! | Who       | Routes                              | Credential           |
//! | --------- | ----------------------------------- | -------------------- |
//! | A program | `/plugins`, `/cores`,               | the process's token  |
//! |           | `/cores/{id}/api/...`               |                      |
//! | A runner  | `/api/runner`, its pipes and the    | its device's token,  |
//! |           | command packages' artifacts         | or none while it     |
//! |           | (`runners.rs`)                      | waits to be paired   |
//! | Anyone    | `/hooks/{id}`, a plugin's sign-in   | none                 |
//! |           | callback, the files a page's frames |                      |
//! |           | load                                |                      |
//!
//! A program presents the token as `Authorization: Bearer`. Its connection
//! is one of many: its end closes nothing, and the cores, their
//! conversations and their turns go on while it restarts.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Extension;
use demi_runner_protocol::wire::Watched;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use subtle::ConstantTimeEq as _;
use tokio::net::TcpListener;
use url::Url;

use super::devices::ConnectionWrites;
use super::stream::Remote;
use super::{Authorized, Process, ProcessError};

/// The token a program presents.
struct Token(String);

/// Serves the process's routes and its runners' on `listener` until `stop`
/// resolves, then closes every core. A program presents `token`;
/// `public_url` is where the process is reached from outside, without which
/// the cores offer neither a plugin's OAuth nor a schedule's URL; `release`
/// holds the roots of the release whose command packages the cores' runners
/// run.
pub async fn serve(
    listener: TcpListener,
    token: String,
    public_url: Option<Url>,
    release: Vec<PathBuf>,
    stop: impl Future<Output = ()>,
) -> Result<(), ProcessError> {
    let process = Process::start(public_url, release, true).await?;
    let app = super::routes(process.clone())
        .merge(super::runners::routes(process.clone()))
        .layer(middleware::from_fn_with_state(Arc::new(Token(token)), authorize));
    tracing::info!(
        event = "process.listening",
        address = %listener.local_addr().map_or_else(|error| error.to_string(), |address| address.to_string()),
        "the process listens for its programs and runners"
    );
    let mut stop = std::pin::pin!(stop);
    loop {
        let accepted = tokio::select! {
            () = &mut stop => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, remote) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                // As one connection that failed to arrive, such as one with
                // too many files open; the next may.
                tracing::warn!(event = "process.accept_failed", %error, "a connection could not be accepted");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };
        // Small frames leave at once: a runner's and a lane's messages are
        // small, and their latency matters.
        let _ = stream.set_nodelay(true);
        // A runner's link counts it lost once the connection's writes stall.
        let (io, writes) = Watched::new(stream);
        let app = app
            .clone()
            .layer(Extension(ConnectionWrites(writes)))
            .layer(Extension(Remote(remote.ip())));
        tokio::spawn(async move {
            let mut builder = auto::Builder::new(TokioExecutor::new());
            builder.http2().enable_connect_protocol();
            let served = builder
                .serve_connection_with_upgrades(TokioIo::new(io), TowerToHyperService::new(app))
                .await;
            // A peer that went away ends its own connection; its requests'
            // lines say the rest.
            if let Err(error) = served {
                tracing::debug!(event = "process.connection_ended", %error, "a connection ended");
            }
        });
    }
    drop(listener);
    tracing::info!(event = "process.stopping", "the process stops: its cores close");
    process.close().await;
    Ok(())
}

/// Marks a request that presents the process's token as a program's.
async fn authorize(State(token): State<Arc<Token>>, mut request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    // Compared in a time that tells nothing of where they differ.
    if presented.is_some_and(|presented| bool::from(presented.as_bytes().ct_eq(token.0.as_bytes()))) {
        request.extensions_mut().insert(Authorized);
    }
    next.run(request).await
}
