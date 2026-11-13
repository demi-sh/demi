//! A conversation's tab numbers.
//!
//! The resident draws them from the conversation's `tab` sequence in the backend
//! eight at a time (`crates/command-protocol/src/numbers.rs`), so most tabs
//! take theirs without waiting. A number it never uses is a gap.
//!
//! A tab's id outlasts each browser of the conversation, so does the count
//! that numbers the tab lists, which only grows within the conversation.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use jush_command_protocol::ResidentSequence;
use jush_command_sdk::Numbers;
use tokio::sync::watch;

use crate::driver::operation::{BrowserError, Result};

/// How many numbers one draw takes.
const DRAW: u32 = 8;
/// How long the backend has to answer a draw; past it, the step that needed
/// a number fails.
const DRAW_TIMEOUT: Duration = Duration::from_secs(15);

/// One conversation's tab numbers at hand, where more come from, and the
/// count of changes its tab lists are numbered by. Cloning shares them.
#[derive(Clone)]
pub struct TabNumbers(Arc<Inner>);

struct Inner {
    /// Where more come from; none for a resident that was given no numbers
    /// source.
    numbers: Option<Numbers>,
    conversation: String,
    spare: Mutex<VecDeque<u64>>,
    /// Counts the changes to what the conversation's browsers show, each
    /// browser's after the one before, so a tab list is numbered after every
    /// list an earlier browser numbered.
    changes: watch::Sender<u64>,
}

impl TabNumbers {
    /// The tab numbers of `conversation`, drawn from `numbers`.
    pub fn new(numbers: Numbers, conversation: String) -> Self {
        Self::from_source(Some(numbers), conversation)
    }

    /// The tab numbers of `conversation`, drawn from `numbers` once the
    /// resident has a numbers source.
    pub fn from_source(numbers: Option<Numbers>, conversation: String) -> Self {
        Self(Arc::new(Inner {
            numbers,
            conversation,
            spare: Mutex::new(VecDeque::new()),
            changes: watch::channel(0).0,
        }))
    }

    /// The next number, drawing more first when none is at hand. A draw that
    /// fails or times out fails only this call, with why; the next call
    /// draws again.
    pub async fn next(&self) -> Result<u64> {
        if let Some(number) = self.spare().pop_front() {
            return Ok(number);
        }
        let numbers = self.0.numbers.as_ref().ok_or_else(|| {
            BrowserError::Unavailable("the resident was given no tab numbers".into())
        })?;
        let draw = numbers.draw(&self.0.conversation, ResidentSequence::Tab, DRAW);
        let first = tokio::time::timeout(DRAW_TIMEOUT, draw)
            .await
            .map_err(|_| {
                BrowserError::Unavailable("the backend gave out no tab numbers in time".into())
            })?
            .map_err(|error| BrowserError::Unavailable(format!("no tab numbers: {error}")))?;
        let mut spare = self.spare();
        spare.extend(first..first + u64::from(DRAW));
        Ok(spare.pop_front().expect("a draw gives at least one number"))
    }

    /// The count of changes a new browser of the conversation numbers its tab
    /// lists by. Its start is a change of what the browser shows, so its
    /// first list is numbered above every list of the browsers before it.
    pub fn browser_changes(&self) -> watch::Sender<u64> {
        self.0.changes.send_modify(|revision| *revision += 1);
        self.0.changes.clone()
    }

    /// Counts the end of the conversation's browser as a change of what it
    /// shows: its tabs are gone with it.
    pub fn browser_ended(&self) {
        self.0.changes.send_modify(|revision| *revision += 1);
    }

    /// Follows the changes to what the conversation's browsers show, which
    /// the resident tells the plugin of.
    pub fn watch_changes(&self) -> watch::Receiver<u64> {
        self.0.changes.subscribe()
    }

    /// The number of the conversation's latest tab list, which a list read
    /// while no browser runs carries.
    pub fn list_number(&self) -> u64 {
        *self.0.changes.borrow()
    }

    fn spare(&self) -> MutexGuard<'_, VecDeque<u64>> {
        // No section panics while it holds the lock.
        self.0.spare.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
