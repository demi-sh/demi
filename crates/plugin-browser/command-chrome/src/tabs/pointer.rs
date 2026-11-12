//! Where Chrome has a tab's mouse, and the hover that follows it.
//!
//! Every mouse event the tab is given, the agent's and a viewer's alike,
//! passes through `live::inputs::mouse`, which tells this module where it
//! went.
//!
//! Chrome changes the scale it renders a page at with no command of ours:
//! when a capture of the tab starts, for a recording or the live view, and
//! when the screen's ratio changes. The page then resizes to the same size,
//! and Chrome hovers another place than the mouse's, at ratio 2 the mouse's
//! coordinates halved, until the mouse moves again; a mouse event sent while
//! it resizes may land elsewhere too. So once the page painted after a
//! resize, the mouse is given again where it was, and the page hovers what is
//! under it, as a mouse that has not moved would leave it.
//!
//! ```text
//! Page.frameResized --> paint --> MouseMoved at the mouse's place
//!                                  (none with a button held or a dialog
//!                                   open, which holds every input)
//! ```

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chromiumoxide::{
    Page,
    cdp::browser_protocol::{
        input::{DispatchMouseEventParams, DispatchMouseEventType, MouseButton},
        page::EventFrameResized,
    },
};
use futures_util::StreamExt as _;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::driver::operation::Result;
use crate::tabs::dialog::Opening;
use crate::traced::SpawnTraced as _;

/// How long a resized page may take to paint before its mouse is given again
/// anyway: a background tab may never finish a redraw nobody asked to see.
const SETTLE: Duration = Duration::from_secs(5);

/// Where Chrome was last given the tab's mouse; cloning shares it.
#[derive(Clone, Default)]
pub struct Pointer(Arc<Mutex<Place>>);

#[derive(Clone, Copy, Default)]
struct Place {
    /// In CSS pixels; none before the tab's first mouse event.
    at: Option<(f64, f64)>,
    /// The buttons pressed and not yet released, by CDP's bits.
    held: i64,
}

impl Pointer {
    /// Notes `event` as Chrome is given it. A wheel turns where it is sent
    /// and moves no mouse.
    pub(crate) fn given(&self, event: &DispatchMouseEventParams) {
        let mut place = self.0.lock().expect("the tab's pointer is intact");
        match event.r#type {
            DispatchMouseEventType::MouseWheel => return,
            DispatchMouseEventType::MousePressed => place.held |= bit(event.button.as_ref()),
            DispatchMouseEventType::MouseReleased => place.held &= !bit(event.button.as_ref()),
            _ => {}
        }
        place.at = Some((event.x, event.y));
    }

    fn place(&self) -> Place {
        *self.0.lock().expect("the tab's pointer is intact")
    }
}

fn bit(button: Option<&MouseButton>) -> i64 {
    match button {
        Some(MouseButton::Left) => 1,
        Some(MouseButton::Right) => 2,
        Some(MouseButton::Middle) => 4,
        Some(MouseButton::Back) => 8,
        Some(MouseButton::Forward) => 16,
        Some(MouseButton::None) | None => 0,
    }
}

/// Follows `page`'s resizes until the tab ends, giving its mouse again after
/// each.
pub(crate) async fn observe(
    page: &Page,
    ended: CancellationToken,
    tasks: &TaskTracker,
    dialog: watch::Receiver<Opening>,
) -> Result<Pointer> {
    let pointer = Pointer::default();
    let mut resized = page.event_listener::<EventFrameResized>().await?;
    let page = page.clone();
    let following = pointer.clone();
    tasks.spawn_traced(async move {
        loop {
            tokio::select! {
                _ = ended.cancelled() => return,
                next = resized.next() => if next.is_none() { return },
            }
            tokio::select! {
                _ = ended.cancelled() => return,
                _settled = tokio::time::timeout(SETTLE, crate::tabs::viewport::paint(&page)) => {}
            }
            give_again(&page, &following, dialog.clone(), &ended).await;
        }
    });
    Ok(pointer)
}

async fn give_again(page: &Page, pointer: &Pointer, mut dialog: watch::Receiver<Opening>, ended: &CancellationToken) {
    let place = pointer.place();
    let Some((x, y)) = place.at.filter(|_| place.held == 0) else {
        // A held button would be dragged by a move.
        return;
    };
    if dialog.borrow().is_some() {
        return;
    }
    let Ok(moved) = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseMoved)
        .x(x)
        .y(y)
        .build()
    else {
        return;
    };
    tokio::select! {
        _ = ended.cancelled() => {}
        // A move the page answers with a dialog waits for its answer.
        _ = dialog.wait_for(|dialog| dialog.is_some()) => {}
        moved = page.execute(moved) => if let Err(error) = moved {
            tracing::warn!(event = "pointer.give_again_failed", %error, "the tab's mouse could not be given again after its page resized");
        }
    }
}
