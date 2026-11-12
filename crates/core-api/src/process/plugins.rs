//! A program's plugins: the program declares each with `PUT /plugins/{id}`,
//! at any time, and each core's instance of one sends its requests to the
//! program on a lane of the plugin (`crates/core-api/src/plugin_lanes.rs`).
//!
//! ```text
//! core's thread                       plugin lanes             program
//! LanePlugin::call --- send --------> { request, call } ------->
//!   port.relay   <---- Event::Port -- { request, port, message } <--
//!   -------------------- answer ----> { request, port, answer } ->
//!   reply        <---- Event::Reply - { request, reply } <---------
//! ```
//!
//! A request runs on its core's thread, where its port is; the lanes only
//! carry messages. While no lane of the plugin is open, a request waits for
//! one as long as a device whose connection dropped counts as online, and
//! then fails; a context request does not wait, and the agent asks its
//! model without the plugin's block. A page call's task keeps its port
//! after its reply, until the instance ends or its lane does.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use demi_plugin_interface::{
    Manifest, Plugin, PluginError, PluginFactory, PluginId, PluginPort, PortMessage, Reply, Request, UserId,
};
use futures_util::future::LocalBoxFuture;
use tokio_util::sync::{CancellationToken, DropGuard};

use super::cores::OpenCores;
use crate::plugin_lanes::{Event, PluginLanes, Reading, Sent, input, reads_input};

/// A program's plugin, as the process registers it.
pub(super) struct ProgramPlugin {
    manifest: Manifest,
    cores: Arc<OpenCores>,
    /// How long a request waits for a lane of the plugin to open.
    returning: Duration,
}

impl ProgramPlugin {
    pub(super) fn new(manifest: Manifest, cores: Arc<OpenCores>, returning: Duration) -> Self {
        Self {
            manifest,
            cores,
            returning,
        }
    }
}

impl PluginFactory for ProgramPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn instance(&self, user: &UserId) -> Rc<dyn Plugin> {
        let ended = CancellationToken::new();
        Rc::new(LanePlugin {
            core: user.clone(),
            plugin: self.manifest.id.clone(),
            cores: self.cores.clone(),
            returning: self.returning,
            ended: ended.clone(),
            _ends: ended.drop_guard(),
        })
    }
}

/// A program plugin's instance in one core: each request goes to the
/// program on the plugin's newest lane of the core.
struct LanePlugin {
    core: UserId,
    plugin: PluginId,
    cores: Arc<OpenCores>,
    returning: Duration,
    /// Cancelled as the instance ends, which ends the tasks it started.
    ended: CancellationToken,
    _ends: DropGuard,
}

impl Plugin for LanePlugin {
    fn call(&self, request: Request, port: PluginPort) -> LocalBoxFuture<'_, Result<Reply, PluginError>> {
        Box::pin(async move {
            let lanes = self.cores.plugin_lanes(&self.core).ok_or_else(|| PluginError::Ended {
                message: "the core is closing".into(),
            })?;
            let wait = (!matches!(request, Request::Context { .. })).then_some(self.returning);
            let keeps_port = matches!(request, Request::PageCall { .. });
            let Sent { number, mut events } = lanes.send(&self.plugin, request, wait).await?;
            let mut call = Called {
                lanes: lanes.clone(),
                request: number,
                over: false,
            };
            let mut stopped = false;
            let reply = loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Some(Event::Port { lane, port: id, message }) => {
                            answer(&lanes, &port, number, lane, id, message, &self.ended);
                        }
                        Some(Event::Reply(reply)) => break reply,
                        None => break Err(lane_ended()),
                    },
                    // The program hears that the request is stopped, and
                    // replies when its work has ended; with no lane open,
                    // nobody hears it, and the request ends now.
                    () = port.cancelled(), if !stopped => {
                        stopped = true;
                        if !lanes.stop(number) {
                            break Err(PluginError::Ended {
                                message: "the request was stopped while its program was unreachable".into(),
                            });
                        }
                    }
                }
            };
            call.over = true;
            if !(keeps_port && reply.is_ok()) {
                lanes.forget(number, false);
                return reply;
            }
            // A page call's task keeps its port, until the instance ends or
            // the lane does.
            let ended = self.ended.clone();
            tokio::task::spawn_local(async move {
                loop {
                    tokio::select! {
                        event = events.recv() => match event {
                            Some(Event::Port { lane, port: id, message }) => answer(&lanes, &port, number, lane, id, message, &ended),
                            _ => break,
                        },
                        () = ended.cancelled() => break,
                    }
                }
                lanes.forget(number, false);
            });
            reply
        })
    }
}

/// Passes the port message `id` of request `request`, which came on lane
/// `lane`, to `port`, and its answer back on that lane, in a task that
/// `ended` ends. A read of input takes no read of its own when input that
/// no lane received, or a read under way whose lane closed, answers it.
fn answer(
    lanes: &Arc<PluginLanes>,
    port: &PluginPort,
    request: u64,
    lane: u64,
    id: u64,
    message: PortMessage,
    ended: &CancellationToken,
) {
    if reads_input(&message) {
        match lanes.read(request, lane, id) {
            Reading::Unread(bytes) => {
                lanes.answer(lane, request, id, Ok(input(bytes)));
                return;
            }
            Reading::Joined => return,
            Reading::Relayed => {}
        }
    }
    let (lanes, port, ended) = (lanes.clone(), port.clone(), ended.clone());
    tokio::task::spawn_local(async move {
        let answer = tokio::select! {
            answer = port.relay(message) => answer.map_err(|error| error.to_string()),
            () = ended.cancelled() => return,
        };
        lanes.answer(lane, request, id, answer);
    });
}

/// A request in flight: dropping it before its reply, as when the plugin
/// host stops waiting, tells the program and forgets it.
struct Called {
    lanes: Arc<PluginLanes>,
    request: u64,
    over: bool,
}

impl Drop for Called {
    fn drop(&mut self) {
        if !self.over {
            self.lanes.forget(self.request, true);
        }
    }
}

fn lane_ended() -> PluginError {
    PluginError::Ended {
        message: "the program's plugin lane ended".into(),
    }
}
