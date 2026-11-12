//! A program's process, `demi-core`: it runs the cores its programs open,
//! and serves each program the cores' routes.
//!
//! A program reaches it in one of two ways, which serve the same routes:
//!
//! - It starts the process and speaks HTTP/2 over the process's standard
//!   input and output ([`run`]). The process ends with that connection,
//!   and every core closes.
//! - The process runs on its own, `demi-core serve`, until it is stopped
//!   ([`serve`]). Programs connect to it with its token, several at once,
//!   and a connection's end closes nothing; runners dial it as they dial a
//!   server.
//!
//! ```text
//! program --- HTTP/2 ---> PUT /plugins/{id} {manifest}  declares a plugin
//!                         DELETE /plugins/{id}          removes it
//!                         POST /cores {dir}             opens a core: 201
//!                                                       {id}, or 200 for
//!                                                       one open already
//!                         DELETE /cores/{id}            closes it
//!                         /cores/{id}/api/...           the core's routes,
//!                                                       as a page has them
//!                                                       under /api
//!                         /hooks/{id}                   a schedule's URL
//! ```
//!
//! The process is an isolated instance with one set of services for its
//! cores, as the server has for its users'. A program is the caller of each
//! core's routes, and a starter: the core's notifications go on the newest
//! starter lane a program opened on its page connection to the core.

mod cores;
mod devices;
mod listener;
mod packages;
mod plugins;
mod program;
mod runners;
mod stream;

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::{FromRequestParts, OriginalUri, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, connect, delete, get, post, put};
use axum::{Extension, Json, Router};
use demi_core::builtins::{self, RelayFamily};
use demi_core::conversation::claude_cli::MachineSlot;
use demi_core::pool::{ShardPlacement, ShardPool};
use demi_core::services::{CoreServices, CoreServicesError, CoreSettings};
use demi_core::tuning::{ConversationTuning, LifecycleTuning, MediaTuning, RunnerTuning};
use demi_core_blobs::blobs::BlobStores;
use demi_core_devices::claims::{PendingClaims, RecordedTokens};
use demi_core_devices::packages::ReleaseError;
use demi_core_devices::public_url::PublicUrl;
use demi_core_providers::vault::logins::LoginTiming;
use demi_plugin_interface::Manifest;
use demi_provider_claude_code::releases::DEFAULT_RELEASES_URL;
use demi_provider_common::media_catalog;
use demi_provider_common::models_dev::ModelsDevClient;
use demi_shared_types::{Clock, SystemClock};
use demi_web_api_protocol::connection::CONNECT_ROUTE;
use demi_web_api_protocol::cores::{OpenCore, OpenedCore};
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::ids::UserId;
use demi_web_api_protocol::settings::InstanceMode;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use tower_http::trace::TraceLayer;
use url::Url;

pub use self::listener::serve;

use self::cores::{OpenCores, ProcessOpener, not_open};
use self::packages::{ReleaseFiles, ServedPackages};
use self::plugins::ProgramPlugin;
use self::program::ProgramCaller;
use crate::api::Api;
use crate::body::JsonBody;
use crate::caller::CallerOf;
use crate::connection::{Closing, PageTuning};
use crate::error::ApiError;

/// Why the process did not run.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error(transparent)]
    Services(#[from] CoreServicesError),
    #[error("the cores' threads did not start: {0}")]
    Threads(std::io::Error),
    #[error(transparent)]
    Release(#[from] ReleaseError),
}

/// The process's cores and services, which its routes reach.
struct Process {
    cores: Arc<OpenCores>,
    api: Api,
    release: Arc<ReleaseFiles>,
    /// The core's routes, for a program's calls of a serving core.
    core_routes: Router,
    /// The core's routes anyone calls, a program or not: a plugin's sign-in
    /// callback, which the vendor sends the browser back to, and the files a
    /// page's frames load.
    open_routes: Router,
    /// How long a request of a program's plugin waits for a lane of it.
    returning: Duration,
    closing: CancellationToken,
    /// The cores' threads, until the process ends.
    pool: Mutex<Option<ShardPool>>,
}

/// A request a program made, over the connection it gave the process or
/// with the process's token.
#[derive(Clone)]
struct Authorized;

impl<S: Send + Sync> FromRequestParts<S> for Authorized {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        parts.extensions.get::<Authorized>().cloned().ok_or_else(unauthorized)
    }
}

fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        ErrorCode::Unauthenticated,
        "The call carries no valid token for this process",
    )
}

/// `PUT /plugins/{id}`: a plugin of a program's, its manifest.
#[derive(Debug, serde::Deserialize, garde::Validate)]
#[serde(transparent)]
struct DeclaredPlugin(#[garde(skip)] Manifest);

/// Runs the process over `io`, the program's connection, until it ends,
/// then closes every core. `public_url` is where the program's server
/// forwards the process's routes; without one, the cores offer neither a
/// plugin's OAuth nor a schedule's URL. `release` holds the roots of the
/// release whose command packages the cores' runners run; without a root,
/// they run none.
pub async fn run<IO>(io: IO, public_url: Option<Url>, release: Vec<PathBuf>) -> Result<(), ProcessError>
where
    IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let process = Process::start(public_url, release, false).await?;
    // Whatever comes over the connection is the program's.
    let app = routes(process.clone()).layer(Extension(Authorized));
    tracing::info!(event = "process.serving", "the process serves its program");
    let served = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
        .enable_connect_protocol()
        .serve_connection(TokioIo::new(io), TowerToHyperService::new(app))
        .await;
    if let Err(error) = served {
        tracing::warn!(event = "process.connection_failed", %error, "the program's connection failed");
    }
    process.close().await;
    tracing::info!(event = "process.ended", "the process ended with its program's connection");
    Ok(())
}

impl Process {
    /// Starts the process's services, with the release's plugins, which
    /// each program's declarations add to; `listens` for one on its own,
    /// which runners dial.
    async fn start(public_url: Option<Url>, release: Vec<PathBuf>, listens: bool) -> Result<Arc<Self>, ProcessError> {
        let release = Arc::new(ReleaseFiles::read(release).await?);
        let cores = Arc::new(OpenCores::new(public_url.clone()));
        let public = PublicUrl::default();
        if let Some(url) = &public_url {
            // A URL given is the one taken; the address goes unused.
            public.listening(Some(url), SocketAddr::from(([127, 0, 0, 1], 0)));
        }
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let claude_machine = MachineSlot::default();
        let runners = RunnerTuning::default();
        let returning = runners.returning;
        let claims = Arc::new(PendingClaims::new(RecordedTokens, runners.claims_per_minute));
        // The process keeps no directory of its own, and several may run on
        // one machine: each takes a directory of its own for its plugins.
        let plugins = builtins::plugins(&std::env::temp_dir().join(format!("demi-core-{}", std::process::id())));
        let settings = CoreSettings {
            families: builtins::families(claude_machine.clone())
                .with(demi_provider_relay::borrow::FAMILY, RelayFamily(public.clone())),
            public_url: public,
            clock: clock.clone(),
            plugins,
            packages: Arc::new(ServedPackages(release.clone())),
            models_dev_url: ModelsDevClient::DEFAULT_URL
                .parse()
                .expect("the models.dev address parses"),
            model_shutdowns_url: demi_provider_registry::shutdowns::DEFAULT_URL
                .parse()
                .expect("LiteLLM's address parses"),
            media_catalog_url: media_catalog::branch_url(option_env!("DEMI_BUILD_BRANCH")),
            media_catalog: media_catalog::Catalog::carried(),
            claude_releases: DEFAULT_RELEASES_URL
                .parse()
                .expect("the Claude Code distribution's address parses"),
            claude_machine,
            logins: LoginTiming::default(),
            runners,
            conversations: ConversationTuning::default(),
            media: MediaTuning::default(),
            lifecycle: LifecycleTuning::default(),
            stun: Vec::new(),
        };
        let services = CoreServices::start(
            settings,
            cores.clone(),
            InstanceMode::Isolated,
            false,
            BlobStores::own(clock),
        )
        .await?;
        let services = Arc::new(services);
        let opener = ProcessOpener {
            cores: cores.clone(),
            claims: listens.then(|| claims.clone()),
        };
        let pool = match ShardPool::start(
            ShardPlacement::Threads(NonZeroUsize::MIN),
            services.clone(),
            Arc::new(opener),
        )
        .await
        {
            Ok(pool) => pool,
            Err(error) => {
                services.close_providers().await;
                return Err(ProcessError::Threads(error));
            }
        };
        let api = Api {
            services,
            shards: pool.shards(),
            html: Arc::default(),
            pages: PageTuning::default(),
            schedule_owners: cores.clone(),
            claims,
        };
        let closing = CancellationToken::new();
        let core_routes = core_routes(api.clone(), release.clone(), closing.clone());
        let open_routes = open_routes(api.clone());
        tracing::info!(event = "process.started", "the process's services started");
        Ok(Arc::new(Self {
            cores,
            api,
            release,
            core_routes,
            open_routes,
            returning,
            closing,
            pool: Mutex::new(Some(pool)),
        }))
    }

    /// Closes every core, then the services; the runners waiting to be
    /// paired go.
    async fn close(&self) {
        self.closing.cancel();
        self.api.claims.close();
        self.cores.close_all(&self.api.services, &self.api.shards).await;
        let pool = self.pool.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(pool) = pool {
            pool.close().await;
        }
        self.api.services.close_providers().await;
    }

    /// Has every core take the programs' plugins as they are now, which a
    /// core that took them already does at once.
    async fn adopt_plugins(&self) {
        for id in self.cores.all_held() {
            // A core that closes meanwhile takes nothing more.
            let _ = self
                .api
                .shards
                .of(&id)
                .call(|core, _| async move { core.adopt_plugins().await })
                .await;
        }
    }
}

/// The core's routes, as a program calls them of a serving core.
fn core_routes(api: Api, release: Arc<ReleaseFiles>, closing: CancellationToken) -> Router {
    crate::routes()
        .route("/state", get(crate::state::read))
        // The reads' caching (`revalidate.rs`): the routes' answers only.
        .layer(middleware::from_fn(crate::revalidate::reads))
        .route(
            CONNECT_ROUTE,
            any(crate::connection::connect).layer(Extension(Closing(closing))),
        )
        .merge(crate::public_routes())
        .route("/devices", post(devices::create))
        .route("/devices/{id}/runner", connect(devices::channel))
        .fallback(no_route)
        .with_state(api)
        .layer(Extension(release))
        // Each stream is its request's connection.
        .layer(middleware::from_fn(stream::stream_peer))
}

/// The core's routes anyone calls.
fn open_routes(api: Api) -> Router {
    Router::new()
        .route("/plugins/{plugin}/oauth/callback", get(crate::oauth::callback))
        .merge(crate::public_routes())
        .fallback(|| async { unauthorized() })
        .with_state(api)
        .layer(middleware::from_fn(stream::stream_peer))
}

/// The process's routes.
fn routes(process: Arc<Process>) -> Router {
    Router::new()
        .route("/plugins/{id}", put(declare).delete(remove))
        .route("/cores", post(open))
        .route("/cores/{id}", delete(close))
        // The limit on unknown addresses takes the caller's address.
        .route("/hooks/{id}", any(hook).layer(middleware::from_fn(stream::stream_peer)))
        // A route with parameters would give them to the core's routes too.
        .fallback(core_call)
        .with_state(process)
        .layer(axum::extract::DefaultBodyLimit::max(crate::body::JSON_BODY_LIMIT))
        .layer(middleware::from_fn(crate::request_id::watch_abandon))
        // The request's line is `request_id::answered`'s alone.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| crate::request_id::span(request))
                .on_response(crate::request_id::answered)
                .on_failure(()),
        )
        // Outermost, so every answer carries the request's ID.
        .layer(middleware::from_fn(crate::request_id::assign))
}

/// Runs `work`, a change a program asked of the process, to its end in a
/// task of its own, whether or not the program stays for the answer: a
/// program that goes away mid-way leaves no core half opened or half
/// closed, and no core behind the plugins the others took.
async fn to_the_end<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> T {
    match tokio::spawn(work).await {
        Ok(done) => done,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

/// `PUT /plugins/{id}`: declares the program's plugin `id`, in place of the
/// one declared before, 204; every open core takes it before the answer.
/// A manifest that breaks a rule, or takes the id of a plugin of the
/// release, is refused, 400.
async fn declare(
    State(process): State<Arc<Process>>,
    _: Authorized,
    Path(id): Path<String>,
    JsonBody(DeclaredPlugin(manifest)): JsonBody<DeclaredPlugin>,
) -> Result<StatusCode, ApiError> {
    if manifest.id.as_str() != id {
        return Err(ApiError::invalid_body(format!(
            "The manifest's id is \"{}\", not the plugin's in the path",
            manifest.id
        )));
    }
    let plugin = ProgramPlugin::new(manifest, process.cores.clone(), process.returning);
    process
        .api
        .services
        .plugins
        .declare(Arc::new(plugin))
        .map_err(|error| ApiError::invalid_body(error.to_string()))?;
    to_the_end(async move { process.adopt_plugins().await }).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /plugins/{id}`: removes the program's plugin `id`, 204, whose
/// commands leave every open core before the answer, and whose lanes
/// close; 404 for a plugin no program declared.
async fn remove(State(process): State<Arc<Process>>, _: Authorized, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let Some(plugin) = process.api.services.plugins.remove(&id) else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::UnknownPlugin,
            format!("No program declared the plugin \"{id}\""),
        ));
    };
    to_the_end(async move {
        process.adopt_plugins().await;
        for core in process.cores.all_held() {
            if let Some(lanes) = process.cores.plugin_lanes(&core) {
                lanes.remove(&plugin);
            }
        }
    })
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /cores`: opens the core in `dir`, 201 with its id; 200 with it for
/// a core the process has open already.
async fn open(
    State(process): State<Arc<Process>>,
    _: Authorized,
    JsonBody(open): JsonBody<OpenCore>,
) -> Result<(StatusCode, Json<OpenedCore>), ApiError> {
    let (id, opened) = to_the_end(async move {
        let api = &process.api;
        process.cores.open(&PathBuf::from(open.dir), &api.services, &api.shards).await
    })
    .await?;
    let status = if opened { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(OpenedCore { id })))
}

/// `DELETE /cores/{id}`: closes the core for every program, 204.
async fn close(State(process): State<Arc<Process>>, _: Authorized, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    let id = UserId::try_from(id).map_err(|_| not_open())?;
    to_the_end(async move {
        let api = &process.api;
        process.cores.close(&id, &api.services, &api.shards).await
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `/cores/{id}/api/...`: a call of the serving core `id`, which the core's
/// routes answer as the server's answer a page's under `/api`; without the
/// process's token, only those anyone calls.
async fn core_call(State(process): State<Arc<Process>>, mut request: Request) -> Result<Response, ApiError> {
    let Some((id, place)) = request
        .uri()
        .path()
        .strip_prefix("/cores/")
        .and_then(|rest| rest.split_once("/api/"))
    else {
        return Ok(no_route(request).await);
    };
    let id = UserId::try_from(id.to_owned()).map_err(|_| not_open())?;
    let core = process.cores.serving(&id).ok_or_else(not_open)?;
    let path = match request.uri().query() {
        Some(query) => format!("/{place}?{query}"),
        None => format!("/{place}"),
    };
    *request.uri_mut() = path.parse::<Uri>().map_err(|_| not_open())?;
    let routes = match request.extensions().get::<Authorized>() {
        Some(_) => &process.core_routes,
        None => &process.open_routes,
    };
    let caller = ProgramCaller::new(id, core, process.cores.clone());
    request.extensions_mut().insert(CallerOf(Arc::new(caller)));
    // The router answers every request.
    Ok(routes.clone().oneshot(request).await.unwrap_or_else(|never| match never {}))
}

/// `/hooks/{id}`: a call of a schedule's URL, which reaches the serving core
/// that holds the schedule.
async fn hook(State(process): State<Arc<Process>>, id: Path<String>, request: Request) -> Result<Response, ApiError> {
    crate::schedules::hook(State(process.api.clone()), id, request).await
}

pub(super) async fn no_route(request: Request) -> Response {
    // The core's routes see their own path; the program called the whole.
    let path = match request.extensions().get::<OriginalUri>() {
        Some(OriginalUri(uri)) => uri.path().to_owned(),
        None => request.uri().path().to_owned(),
    };
    ApiError::no_route(request.method(), &path).into_response()
}
