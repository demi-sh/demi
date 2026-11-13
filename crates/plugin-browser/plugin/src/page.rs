//! The conversation browser's tab list and tab methods: each runs the operation
//! the agent's `jush browser` command runs, as a package call for the
//! conversation's user, and holds no browser logic.
//!
//! The tab list is the plugin's conversation state: what the browser package
//! last reported, which the backend keeps, so reading it asks no device and
//! works while the device is away. Closing, moving and stopping a tab operate the browser without
//! waking it; opening a tab is work the user starts. Opening and closing
//! browser tabs for work tabs is [`crate::tabs`]'s, through
//! the same operations.
//!
//! - A user waits in a window, not in a script, so for a `user` caller
//!   `tab.new`, `open`, `back`, `forward` and `reload` answer once their work
//!   started, and the navigations name the number of the last tab list before
//!   the request. The page ends its loading only on a list numbered after it.
//! - `stop` runs `browser.stop`, which only a `user` caller reaches: the agent
//!   waits for its commands rather than stopping a load, so `jush browser` has
//!   no `stop`.
//! - A method that does not wake a stopped Cloud is admitted as a user stream
//!   is and ends as one does (`conversation_busy` when streams end).
//! - The reasons the methods, stream and device access can give are one closed
//!   set ([`ControlReason`]) that the page must cover whole, so a reason without
//!   a sentence is a build error, never a fallback.

use jush_command_declarations::NativeOperation;
use jush_command_package_browser_protocol::PACKAGE;
use jush_command_package_browser_protocol::browser::{
    BackInput, BrowserCreatedBy, BrowserErrorCode, BrowserOperation, BrowserTab,
    FailureDocument, ForwardInput, NavigationResult, OpenInput, PREFIX, ReloadInput, StopInput,
    TabCloseInput, TabId, TabListInput, TabMoved, TabNewInput, TabNewResult, TabPruneInput,
    TabPruneResult,
};
use jush_plugin_interface::{
    CallKind, Method, Page, PluginError, PluginPort, PortFailure, PortRefusal, Scope, State,
};
use jush_shared_types::MAX_SAFE_INTEGER;
use jush_web_api_protocol::ids::{ConversationId, DeviceId};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tabs::KIND;
use crate::reason::ControlReason;

/// The conversation state: the conversation browser's tabs, none while it
/// does not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BrowserTabs {
    /// The number of the device's tab list they are; 0 before it reported.
    #[schemars(range(max = MAX_SAFE_INTEGER))]
    pub list: u64,
    pub tabs: Vec<BrowserTab>,
}

/// What the backend keeps of a report: its tabs, whose list number the
/// report keeps beside them.
#[derive(Serialize, Deserialize)]
struct KeptTabs {
    tabs: Vec<BrowserTab>,
}

/// `open { url }`: a new tab of the conversation's browser, loading `url`,
/// or blank without one: the window's + and a link's Open in Browser
/// Window. It comes into the work once the device lists it, selected.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenTab {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub url: Option<String>,
}

/// What `open` answers, once the work holds it: the browser's new tab.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabOpened {
    pub tab: String,
    /// The number of a tab list that names the tab: one numbered as high or
    /// higher that does not name it comes after the tab closed.
    #[schemars(range(max = MAX_SAFE_INTEGER))]
    pub list: u64,
}

/// `navigate { tab, url }`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigateTab {
    pub tab: String,
    #[schemars(length(min = 1))]
    pub url: String,
}

/// Where `history` moves a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryAction {
    Back,
    Forward,
    Reload,
}

/// `history { tab, action }`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabHistory {
    pub tab: String,
    pub action: HistoryAction,
}

/// `stop { tab }`: the user's Stop.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopTab {
    pub tab: String,
}

/// The operation `operation` names, in the browser's package.
fn operation(name: &str) -> NativeOperation {
    NativeOperation {
        package: PACKAGE.into(),
        operation: format!("{PREFIX}{name}"),
    }
}

/// The page: its tab list, its methods, each declared with the operations
/// it calls, and its tab kind, whose tabs the plugin brings up to date
/// whenever the browser package says the conversation's tabs changed.
pub(crate) fn page() -> Page {
    Page::new("@jush-ai/plugin-browser")
        .conversation_state(State::new::<BrowserTabs>().calls(operation("tab.list")).held())
        .tab_kind(KIND)
        .method(
            Method::new::<OpenTab, TabOpened>("open", Scope::Conversation)
                .calls(operation("tab.new")),
        )
        .method(
            Method::new::<NavigateTab, TabMoved>("navigate", Scope::Conversation)
                .calls(operation("open")),
        )
        .method(
            Method::new::<TabHistory, TabMoved>("history", Scope::Conversation)
                .calls(operation("back"))
                .calls(operation("forward"))
                .calls(operation("reload")),
        )
        .method(
            Method::new::<StopTab, TabMoved>("stop", Scope::Conversation)
                .calls(operation("stop")),
        )
        .reasons::<ControlReason>()
}

/// The conversation state: the browser's tabs as its package last reported
/// them, which the backend keeps; none while it never reported or its browser
/// does not run. It asks no device.
pub(crate) async fn tabs(port: &PluginPort) -> Result<Value, PluginError> {
    to_value(reported(port).await?.unwrap_or(BrowserTabs {
        list: 0,
        tabs: Vec::new(),
    }))
}

/// The conversation state as the package last reported it, and none while
/// it never did.
pub(crate) async fn held_tabs(port: &PluginPort) -> Result<Option<Value>, PluginError> {
    reported(port).await?.map(to_value).transpose()
}

/// The tabs the primary device last reported; none before it did.
pub(crate) async fn reported(port: &PluginPort) -> Result<Option<BrowserTabs>, PluginError> {
    let Some(report) = port.report().await? else {
        return Ok(None);
    };
    let kept: KeptTabs = serde_json::from_value(report.value).map_err(|error| {
        PluginError::failed(format!("the browser's kept tabs do not decode: {error}"))
    })?;
    Ok(Some(BrowserTabs {
        list: report.revision,
        tabs: kept.tabs,
    }))
}

/// Keeps `tabs` as what the package on `device` reported, at `revision`;
/// answers whether `device` is the conversation's primary device, whose tabs
/// its work shows.
pub(crate) async fn keep(
    port: &PluginPort,
    device: &DeviceId,
    revision: u64,
    tabs: Vec<BrowserTab>,
) -> Result<bool, PluginError> {
    let value = serde_json::to_value(KeptTabs { tabs }).map_err(|error| {
        PluginError::failed(format!("the browser's tabs do not encode: {error}"))
    })?;
    Ok(port.keep_report(device.as_str(), revision, value).await?)
}

/// A new tab, starting the environment when needed: work the user starts,
/// which wakes a stopped Cloud. It starts loading `url` at once, when given.
pub(crate) async fn new_tab(
    port: &PluginPort,
    url: Option<String>,
) -> Result<TabNewResult, PluginError> {
    let input = TabNewInput { timeout: None, url };
    run::<TabNewResult, _>(port, BrowserOperation::TabNew, input, CallKind::Starts)
        .await
        .map_err(refused)
}

/// Lists the browser's tabs on the device, as a look that wakes no stopped
/// Cloud: it brings the conversation to the device's resident, which opens
/// again the tabs an earlier resident left and reports them. The answer
/// itself is not kept: the report that follows it is.
pub(crate) async fn look(port: &PluginPort) -> Result<(), PortFailure> {
    let input = TabListInput {
        offset: None,
        limit: Some(1),
        timeout: None,
    };
    run::<Value, _>(port, BrowserOperation::TabList, input, CallKind::Looks)
        .await
        .map(|_| ())
}

/// Closes the browser's tab `tab`. A tab the browser does not have is closed
/// already; one a device cannot close now, a stopped Cloud's included, fails,
/// and its next browser lists it again.
pub(crate) async fn close(port: &PluginPort, tab: &str) -> Result<(), PluginError> {
    let Ok(tab) = TabId::try_from(tab.to_owned()) else {
        return Ok(());
    };
    let input = TabCloseInput { tab, timeout: None };
    match run::<Value, _>(port, BrowserOperation::TabClose, input, CallKind::Operates).await {
        Ok(_) => Ok(()),
        Err(failure) if tab_missing(&failure) => Ok(()),
        Err(failure) => Err(refused(failure)),
    }
}

/// Whether the conversation's browser, as it last reported, has a tab an
/// agent or a page opened that the agent never showed: one `prune` may
/// close. Without one there is nothing to ask the device.
pub(crate) async fn prunes_any(port: &PluginPort) -> Result<bool, PluginError> {
    Ok(reported(port).await?.is_some_and(|reported| {
        reported.tabs.iter().any(|tab| {
            tab.shows == 0
                && matches!(tab.created_by, BrowserCreatedBy::Agent { .. } | BrowserCreatedBy::Page { .. })
        })
    }))
}

/// Closes the agents' tabs the user did not make theirs, and answers the
/// ones it left because a viewer watches them. The call is no activity, so
/// it never keeps a Cloud up, and a stopped Cloud is refused, not woken.
pub(crate) async fn prune(port: &PluginPort) -> Result<TabPruneResult, PortFailure> {
    let input = TabPruneInput { timeout: None };
    run(port, BrowserOperation::TabPrune, input, CallKind::Looks).await
}

/// Starts loading `url` in the browser's tab `tab`.
pub(crate) async fn navigate(
    port: &PluginPort,
    tab: &str,
    url: String,
) -> Result<TabMoved, PluginError> {
    let input = OpenInput {
        tab: tab_id(tab.to_owned())?,
        url,
        load: None,
        show: None,
        timeout: None,
    };
    moved(run(port, BrowserOperation::Open, input, CallKind::Operates).await)
}

/// Answers the page call `method` with `params`, which its schema checked,
/// for the request's `conversation`, whose tabs `work` follows. A method
/// that opens, closes or moves a tab changes the list the device tells the
/// plugin of: the held list and the work follow that notice
/// (`Work::sync`), never the call.
pub(crate) async fn call(
    method: &str,
    params: Map<String, Value>,
    conversation: &ConversationId,
    port: &PluginPort,
    work: &crate::tabs::Work,
) -> Result<Value, PluginError> {
    match method {
        "open" => {
            let OpenTab { url } = decode(params)?;
            to_value(work.open(conversation, port, url).await?)
        }
        "navigate" => {
            let NavigateTab { tab, url } = decode(params)?;
            to_value(navigate(port, &tab, url).await?)
        }
        "history" => {
            let TabHistory { tab, action } = decode(params)?;
            let tab = tab_id(tab)?;
            let navigated = match action {
                HistoryAction::Back => {
                    let input = BackInput {
                        tab,
                        load: None,
                        timeout: None,
                    };
                    run(port, BrowserOperation::Back, input, CallKind::Operates).await
                }
                HistoryAction::Forward => {
                    let input = ForwardInput {
                        tab,
                        load: None,
                        timeout: None,
                    };
                    run(port, BrowserOperation::Forward, input, CallKind::Operates).await
                }
                HistoryAction::Reload => {
                    let input = ReloadInput {
                        tab,
                        load: None,
                        timeout: None,
                    };
                    run(port, BrowserOperation::Reload, input, CallKind::Operates).await
                }
            };
            to_value(moved(navigated)?)
        }
        "stop" => {
            let StopTab { tab } = decode(params)?;
            let input = StopInput {
                tab: tab_id(tab)?,
                timeout: None,
            };
            let stopped = run::<TabMoved, _>(port, BrowserOperation::Stop, input, CallKind::Operates)
                .await
                .map_err(refused)?;
            to_value(stopped)
        }
        method => Err(PluginError::failed(format!(
            "the browser has no method {method}"
        ))),
    }
}

/// Runs the browser's operation `variant` with `input` and decodes its
/// answer as `T`.
async fn run<T: DeserializeOwned, I: Serialize>(
    port: &PluginPort,
    variant: fn(I) -> BrowserOperation,
    input: I,
    kind: CallKind,
) -> Result<T, PortFailure> {
    let Ok(Value::Object(args)) = serde_json::to_value(&input) else {
        unreachable!("a browser operation's input serializes to an object")
    };
    let name = variant(input).name();
    let result = port.package_call(operation(name), args, kind).await?;
    serde_json::from_value(result).map_err(|error| {
        PortFailure::Refused(PortRefusal::Operation {
            stderr: format!("the browser answered what the plugin cannot read: {error}"),
        })
    })
}

/// What a user's navigation of one tab answers: the number of the last tab
/// list before it started, or its refusal; a stopped Cloud is refused as
/// `device_stopped`, never woken.
fn moved(navigated: Result<NavigationResult, PortFailure>) -> Result<TabMoved, PluginError> {
    let navigated = navigated.map_err(refused)?;
    let list = navigated.list.ok_or_else(|| {
        PluginError::failed("the browser answered a user's navigation without its tab list")
    })?;
    Ok(TabMoved { list })
}

/// The tab a call names; one that is no tab's id is a tab the browser does
/// not have.
fn tab_id(tab: String) -> Result<TabId, PluginError> {
    TabId::try_from(tab).map_err(|_| refusal(ControlReason::TabNotFound, "No such tab".to_owned()))
}

/// A refusal the page says in its words for `reason`.
pub(crate) fn refusal(reason: ControlReason, message: String) -> PluginError {
    PluginError::refused(reason.to_string(), message)
}

fn tab_missing(failure: &PortFailure) -> bool {
    browser_failure(failure)
        .is_some_and(|document| document.error.code == BrowserErrorCode::TabNotFound)
}

/// The browser's own failure, when the operation failed with one: what it
/// wrote to its standard error.
fn browser_failure(failure: &PortFailure) -> Option<FailureDocument> {
    let PortFailure::Refused(PortRefusal::Operation { stderr }) = failure else {
        return None;
    };
    serde_json::from_str(stderr.trim()).ok()
}

/// A failed call as the page sees it: the browser's refusal under its own
/// code; a browser whose resident did not start on the device as
/// `browser_unavailable`, with the runner's words; or the device access's
/// refusal, which passes on.
fn refused(failure: PortFailure) -> PluginError {
    if let Some(document) = browser_failure(&failure) {
        return refusal(document.error.code.into(), document.error.message);
    }
    match failure {
        PortFailure::Refused(PortRefusal::Resident { message }) => {
            refusal(ControlReason::BrowserUnavailable, message)
        }
        failure => failure.into(),
    }
}

fn decode<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, PluginError> {
    serde_json::from_value(Value::Object(params)).map_err(|error| PluginError::Usage {
        message: error.to_string(),
    })
}

fn to_value(value: impl Serialize) -> Result<Value, PluginError> {
    serde_json::to_value(value).map_err(PluginError::failed)
}
