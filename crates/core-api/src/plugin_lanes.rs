//! The lanes of a program's plugins in one core: a program opens one for
//! each plugin it serves, `{ kind: "plugin", plugin: "<id>" }`, on its
//! connection to the core, and the core's requests of the plugin travel on
//! it in the messages of `crates/plugin-interface/src/wire.rs`, from the
//! plugin's instance on the core's thread (`crates/core-api/src/process/plugins.rs`).
//!
//! A message that holds bytes is a binary lane message: the JSON, each value
//! of bytes in it the index of a buffer, then the buffers
//! (`crates/shared-types/src/bytes.rs`):
//!
//! ```text
//! u32 JSON length | JSON | u32 length | bytes | u32 length | bytes ...
//! ```
//!
//! Several lanes of a plugin may be open, as several programs serve it or
//! one that restarts opens its lanes before its old connection is gone. A
//! request goes on the newest, which the newest program opened.
//!
//! A request outlives its lane: once a lane of its plugin is open again, it
//! goes there under its number, marked `resent`, and waits as long as its
//! agent does. A context request does not wait: one whose lane closes ends,
//! unless another lane of its plugin is open. A request whose reply came
//! is over, whatever its lane does next.
//!
//! ```text
//! request 7 --- { request: 7, call } -------------> lane 1, which closes
//! request 7 --- { request: 7, call, resent: true } -> lane 2, opened later
//!           <-- its port messages and its reply ---- lane 2
//! ```
//!
//! A port message is answered on the lane it came on. A read of input
//! under way when its lane closed answers the request's next read, on
//! whatever lane it comes, and so does input read for a lane that never
//! received it: the request's input arrives once, in its order. A request
//! its agent stops while no lane of its plugin is open ends at once, as
//! nobody hears the stop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::ws::close_code;
use demi_core::{Lane, LaneClose, LaneEnd, LaneMessage, LaneReader};
use demi_device_interface::{PortRequest, PortResponse};
use demi_plugin_interface::wire::{HostMessage, PluginMessage};
use demi_plugin_interface::{PluginError, PluginId, PortAnswer, PortMessage, Reply, Request};
use demi_shared_types::B64Bytes;
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

/// What a lane hands a request.
pub(crate) enum Event {
    /// A message of its port, from the lane `lane`, whose answer goes back
    /// on it.
    Port { lane: u64, port: u64, message: PortMessage },
    Reply(Result<Reply, PluginError>),
}

/// The plugin lanes of one core: the open lanes of each plugin, and the
/// requests in flight.
#[derive(Default)]
pub struct PluginLanes {
    state: Mutex<State>,
    /// Notified as a lane opens, which a request that waits for one hears.
    opened: Notify,
}

#[derive(Default)]
struct State {
    /// The open lanes of each plugin, oldest first.
    lanes: HashMap<PluginId, Vec<OpenLane>>,
    /// The requests in flight, by number.
    flights: HashMap<u64, Flight>,
    /// How many lanes were opened for the core.
    opens: u64,
}

/// An open lane, as the core's requests reach it.
struct OpenLane {
    number: u64,
    out: mpsc::UnboundedSender<HostMessage>,
    /// Cancelled as its plugin is removed, which closes the lane.
    removed: CancellationToken,
}

/// A request in flight.
struct Flight {
    plugin: PluginId,
    /// What goes again to the next lane while it waits for its reply; none
    /// once it was answered.
    call: Option<Request>,
    /// The lane it went to last; none while no lane of its plugin is open
    /// since that one closed.
    lane: Option<u64>,
    /// It waits for the next lane once its lane closes; a context request
    /// does not.
    waits: bool,
    events: mpsc::UnboundedSender<Event>,
    /// Input read for it that no lane received.
    unread: Option<Vec<u8>>,
    /// The lane and port message of the read of input under way.
    read: Option<(u64, u64)>,
    /// The read of a later lane that the read under way answers, as its
    /// own lane closed.
    joined: Option<(u64, u64)>,
    /// Its agent stopped it.
    stopped: bool,
}

/// How a read of input goes.
pub(crate) enum Reading {
    /// It takes input read before that no lane received.
    Unread(B64Bytes),
    /// A read under way, whose lane closed, answers it.
    Joined,
    /// It reads the request's input.
    Relayed,
}

/// A request sent: its number, and what its lanes hand it.
pub(crate) struct Sent {
    pub(crate) number: u64,
    pub(crate) events: mpsc::UnboundedReceiver<Event>,
}

/// The number of a new request: the clock's milliseconds times 1000 or more,
/// so no request of any run of the process takes another's number, which a
/// program makes its work idempotent by, and below 2^53, which a JSON reader
/// keeps whole.
fn next_number() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let floor = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX / 1000) * 1000);
    let next = |last: u64| (last + 1).max(floor);
    match LAST.try_update(Ordering::Relaxed, Ordering::Relaxed, |last| Some(next(last))) {
        Ok(last) | Err(last) => next(last),
    }
}

impl PluginLanes {
    fn state(&self) -> MutexGuard<'_, State> {
        // No section panics while it holds the lock.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends `call` to `plugin` on its newest lane; while none is open, it
    /// waits for one for `wait`, and fails at once without it.
    pub(crate) async fn send(&self, plugin: &PluginId, call: Request, wait: Option<Duration>) -> Result<Sent, PluginError> {
        let deadline = wait.map(|wait| tokio::time::Instant::now() + wait);
        loop {
            let opened = self.opened.notified();
            {
                let mut state = self.state();
                let newest = state.lanes.get(plugin).and_then(|lanes| lanes.last()).map(|lane| (lane.number, lane.out.clone()));
                if let Some((lane, out)) = newest {
                    let number = next_number();
                    let (events, received) = mpsc::unbounded_channel();
                    // The lane is listed until its end, which hears of every
                    // message still queued for it.
                    let _ = out.send(HostMessage::Call { request: number, call: call.clone(), resent: false });
                    state.flights.insert(
                        number,
                        Flight {
                            plugin: plugin.clone(),
                            call: Some(call),
                            lane: Some(lane),
                            waits: wait.is_some(),
                            events,
                            unread: None,
                            read: None,
                            joined: None,
                            stopped: false,
                        },
                    );
                    return Ok(Sent { number, events: received });
                }
            }
            let Some(deadline) = deadline else {
                return Err(unreachable(plugin));
            };
            if tokio::time::timeout_at(deadline, opened).await.is_err() {
                return Err(unreachable(plugin));
            }
        }
    }

    /// Answers the port message `port` of request `request`, which came on
    /// lane `lane`, on that lane; the answer of a read of input whose lane
    /// closed goes to the read that joined it. Input that no lane receives
    /// is kept for the request's next read.
    pub(crate) fn answer(&self, lane: u64, request: u64, port: u64, answer: Result<PortAnswer, String>) {
        let mut state = self.state();
        let Some(flight) = state.flights.get_mut(&request) else {
            return;
        };
        let (mut lane, mut port) = (lane, port);
        if flight.read == Some((lane, port)) {
            flight.read = None;
            if let Some(joined) = flight.joined.take() {
                (lane, port) = joined;
            }
        }
        let plugin = flight.plugin.clone();
        let message = match answer {
            Ok(answer) => HostMessage::Answer { request, port, answer },
            Err(error) => HostMessage::Unanswered { request, port, error },
        };
        match state.out(&plugin, lane) {
            Some(out) => {
                let _ = out.send(message);
            }
            None => salvage(&mut state, message),
        }
    }

    /// Starts the read of input of request `request`, its port message
    /// `port` on lane `lane`.
    pub(crate) fn read(&self, request: u64, lane: u64, port: u64) -> Reading {
        let mut state = self.state();
        let Some(flight) = state.flights.get(&request) else {
            return Reading::Relayed;
        };
        let plugin = flight.plugin.clone();
        let under_way = flight.read.map(|(read_lane, _)| state.out(&plugin, read_lane).is_some());
        let Some(flight) = state.flights.get_mut(&request) else {
            return Reading::Relayed;
        };
        if let Some(bytes) = flight.unread.take() {
            return Reading::Unread(B64Bytes::from(bytes));
        }
        match under_way {
            // Its lane closed: what it reads is this read's.
            Some(false) => {
                flight.joined = Some((lane, port));
                Reading::Joined
            }
            // A program that reads twice at once reads in turn.
            Some(true) => Reading::Relayed,
            None => {
                flight.read = Some((lane, port));
                Reading::Relayed
            }
        }
    }

    /// Request `request` was stopped: its lane hears it, and the request
    /// waits for the program's reply. Answers false, and forgets the
    /// request, while no lane of its plugin is open.
    pub(crate) fn stop(&self, request: u64) -> bool {
        let mut state = self.state();
        let Some(flight) = state.flights.get_mut(&request) else {
            return false;
        };
        flight.stopped = true;
        let lane = flight.lane;
        let plugin = flight.plugin.clone();
        match lane.and_then(|lane| state.out(&plugin, lane)) {
            Some(out) => {
                let _ = out.send(HostMessage::Cancel { request, cancel: true });
                true
            }
            None => {
                state.flights.remove(&request);
                false
            }
        }
    }

    /// Request `request` is over; `cancel` tells its lane, when it ended
    /// before its reply.
    pub(crate) fn forget(&self, request: u64, cancel: bool) {
        let mut state = self.state();
        let Some(flight) = state.flights.remove(&request) else {
            return;
        };
        if cancel && let Some(out) = flight.lane.and_then(|lane| state.out(&flight.plugin, lane)) {
            let _ = out.send(HostMessage::Cancel { request, cancel: true });
        }
    }

    /// Opens a lane of `plugin`, which the program declared: it takes the
    /// requests of the plugin that wait for a lane, and every new one until
    /// a newer lane opens.
    pub(crate) fn claim(self: &Arc<Self>, plugin: PluginId) -> Claim {
        let (out, calls) = mpsc::unbounded_channel();
        let removed = CancellationToken::new();
        let number = {
            let mut state = self.state();
            state.opens += 1;
            let number = state.opens;
            state.lanes.entry(plugin.clone()).or_default().push(OpenLane {
                number,
                out,
                removed: removed.clone(),
            });
            state.resend(&plugin);
            number
        };
        self.opened.notify_waiters();
        Claim {
            lanes: self.clone(),
            plugin,
            number,
            calls,
            removed,
            unsent: Vec::new(),
        }
    }

    /// Closes the lanes of `plugin`, which no program declares any longer:
    /// a program that declares it again opens its lanes again.
    pub(crate) fn remove(&self, plugin: &PluginId) {
        for open in self.state().lanes.get(plugin).into_iter().flatten() {
            open.removed.cancel();
        }
    }

    /// Hands `event` to request `request`, when it went to lane `lane` last.
    /// A reply ends what lanes do for it: no lane receives it again.
    fn hand(&self, lane: u64, request: u64, event: Event) -> bool {
        let mut state = self.state();
        let Some(flight) = state.flights.get_mut(&request) else {
            return false;
        };
        if flight.lane != Some(lane) {
            return false;
        }
        if matches!(event, Event::Reply(_)) {
            flight.call = None;
        }
        flight.events.send(event).is_ok()
    }

    /// Lane `number` of `plugin` ended, with `unsent` the messages it did
    /// not deliver: its requests go to the newest lane of the plugin, if
    /// one is open, or wait for the next; one that was stopped or answered
    /// ends, and so does a context request that finds no lane.
    fn ended(&self, plugin: &PluginId, number: u64, unsent: Vec<HostMessage>) {
        let mut state = self.state();
        if let Some(lanes) = state.lanes.get_mut(plugin) {
            lanes.retain(|open| open.number != number);
            if lanes.is_empty() {
                state.lanes.remove(plugin);
            }
        }
        for message in unsent {
            salvage(&mut state, message);
        }
        state.flights.retain(|_, flight| {
            if flight.lane != Some(number) {
                return true;
            }
            flight.lane = None;
            // Its request hears of the end as its events' sender drops.
            flight.call.is_some() && !flight.stopped
        });
        state.resend(plugin);
        state.flights.retain(|_, flight| flight.lane.is_some() || flight.waits);
    }
}

impl State {
    /// The sender of lane `lane` of `plugin`, while it is open.
    fn out(&self, plugin: &PluginId, lane: u64) -> Option<mpsc::UnboundedSender<HostMessage>> {
        self.lanes.get(plugin)?.iter().find(|open| open.number == lane).map(|open| open.out.clone())
    }

    /// Sends each request of `plugin` that waits for a lane to its newest,
    /// if one is open, again under its number.
    fn resend(&mut self, plugin: &PluginId) {
        let Some(newest) = self.lanes.get(plugin).and_then(|lanes| lanes.last()) else {
            return;
        };
        let (lane, out) = (newest.number, newest.out.clone());
        for (&request, flight) in &mut self.flights {
            if flight.plugin != *plugin || flight.lane.is_some() || flight.stopped {
                continue;
            }
            let Some(call) = &flight.call else {
                continue;
            };
            let _ = out.send(HostMessage::Call { request, call: call.clone(), resent: true });
            flight.lane = Some(lane);
        }
    }
}

/// Keeps what `message`, which no lane delivered, read for its request:
/// input the request's next read takes.
fn salvage(state: &mut State, message: HostMessage) {
    let HostMessage::Answer {
        request,
        answer: PortAnswer::Rpc {
            response: PortResponse::Input { bytes: Some(bytes) },
        },
        ..
    } = message
    else {
        return;
    };
    if let Some(flight) = state.flights.get_mut(&request) {
        flight.unread.get_or_insert_default().extend_from_slice(bytes.as_bytes());
    }
}

/// Whether `message` reads the request's standard input.
pub(crate) fn reads_input(message: &PortMessage) -> bool {
    matches!(message, PortMessage::Rpc { request: PortRequest::ReadStdin {} })
}

/// The answer to a read of input that takes `bytes`.
pub(crate) fn input(bytes: B64Bytes) -> PortAnswer {
    PortAnswer::Rpc {
        response: PortResponse::Input { bytes: Some(bytes) },
    }
}

fn unreachable(plugin: &PluginId) -> PluginError {
    PluginError::failed(format!(
        "the program of the plugin \"{plugin}\" is unreachable: no lane of the plugin is open"
    ))
}

/// An open lane of a plugin, from its claim until it ends.
pub(crate) struct Claim {
    lanes: Arc<PluginLanes>,
    plugin: PluginId,
    number: u64,
    calls: mpsc::UnboundedReceiver<HostMessage>,
    removed: CancellationToken,
    /// What the lane took and did not deliver.
    unsent: Vec<HostMessage>,
}

impl Claim {
    /// Carries the core's requests to the program and the program's
    /// messages back, until the program closes the lane, its connection
    /// ends, or the core closes.
    pub(crate) async fn serve(mut self, mut lane: Lane, mut reader: LaneReader) {
        let end = loop {
            tokio::select! {
                () = self.removed.cancelled() => {
                    break LaneEnd::Close(LaneClose { code: close_code::NORMAL, reason: "plugin_removed" });
                }
                message = self.calls.recv() => {
                    // The lanes hold the lane's sender while it is listed.
                    let Some(message) = message else {
                        break LaneEnd::Close(LaneClose { code: close_code::AWAY, reason: "core_closing" });
                    };
                    let request = message.request();
                    let encoded = match encode(&message) {
                        Ok(encoded) => encoded,
                        // A request the program cannot be sent fails as a
                        // plugin that failed.
                        Err(error) => {
                            tracing::error!(event = "plugin_lane.unencoded", plugin = %self.plugin, %error, "a message of a program's plugin did not encode");
                            let failed = PluginError::failed(format!("the request did not encode: {error}"));
                            self.lanes.hand(self.number, request, Event::Reply(Err(failed)));
                            continue;
                        }
                    };
                    let sent = match encoded {
                        LaneMessage::Text(text) => lane.send(text).await,
                        LaneMessage::Binary(bytes) => lane.send_bytes(bytes).await,
                    };
                    if sent.is_err() {
                        self.unsent.push(message);
                        break LaneEnd::Gone;
                    }
                }
                message = reader.next() => {
                    let Some(message) = message else {
                        break LaneEnd::Gone;
                    };
                    let message = match decode(message) {
                        Ok(message) => message,
                        Err(error) => {
                            tracing::info!(event = "plugin_lane.unread", plugin = %self.plugin, %error, "a program's plugin sent a message that does not read");
                            break LaneEnd::Close(LaneClose { code: close_code::POLICY, reason: "unexpected_message" });
                        }
                    };
                    match message {
                        PluginMessage::Reply { request, reply } => {
                            self.lanes.hand(self.number, request, Event::Reply(Ok(reply)));
                        }
                        PluginMessage::Error { request, error } => {
                            self.lanes.hand(self.number, request, Event::Reply(Err(error)));
                        }
                        PluginMessage::Port { request, port, message } => {
                            let event = Event::Port { lane: self.number, port, message };
                            if !self.lanes.hand(self.number, request, event) {
                                let over = HostMessage::Unanswered { request, port, error: "the request is over".into() };
                                let sent = match encode(&over) {
                                    Ok(LaneMessage::Text(text)) => lane.send(text).await,
                                    Ok(LaneMessage::Binary(bytes)) => lane.send_bytes(bytes).await,
                                    Err(_) => Ok(()),
                                };
                                if sent.is_err() {
                                    break LaneEnd::Gone;
                                }
                            }
                        }
                    }
                }
            }
        };
        drop(self);
        lane.end(reader, end);
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        // What was queued for the lane and never sent.
        self.calls.close();
        while let Ok(message) = self.calls.try_recv() {
            self.unsent.push(message);
        }
        self.lanes.ended(&self.plugin, self.number, std::mem::take(&mut self.unsent));
    }
}

trait Numbered {
    fn request(&self) -> u64;
}

impl Numbered for HostMessage {
    fn request(&self) -> u64 {
        match self {
            HostMessage::Call { request, .. }
            | HostMessage::Answer { request, .. }
            | HostMessage::Unanswered { request, .. }
            | HostMessage::Cancel { request, .. } => *request,
        }
    }
}

/// `message` as its lane message: text without bytes, binary with them.
fn encode(message: &HostMessage) -> serde_json::Result<LaneMessage> {
    let (json, buffers) = demi_shared_types::to_json_apart(message)?;
    if buffers.is_empty() {
        return Ok(LaneMessage::Text(json));
    }
    let length = 4 + json.len() + buffers.iter().map(|buffer| 4 + buffer.len()).sum::<usize>();
    let mut bytes = Vec::with_capacity(length);
    for part in std::iter::once(Bytes::from(json)).chain(buffers) {
        let size = u32::try_from(part.len()).map_err(serde::ser::Error::custom)?;
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(&part);
    }
    Ok(LaneMessage::Binary(bytes.into()))
}

/// The program's message of `message`.
fn decode(message: LaneMessage) -> Result<PluginMessage, String> {
    let (json, buffers) = match message {
        LaneMessage::Text(text) => (Bytes::from(text), Vec::new()),
        LaneMessage::Binary(mut bytes) => {
            let mut parts = Vec::new();
            while !bytes.is_empty() {
                if bytes.len() < 4 {
                    return Err("a part's length is cut short".into());
                }
                let size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
                let _ = bytes.split_to(4);
                if bytes.len() < size {
                    return Err("a part is cut short".into());
                }
                parts.push(bytes.split_to(size));
            }
            let mut parts = parts.into_iter();
            let json = parts.next().ok_or("a binary message holds no JSON")?;
            (json, parts.collect())
        }
    };
    demi_shared_types::from_json_apart(&json, buffers).map_err(|error| error.to_string())
}
