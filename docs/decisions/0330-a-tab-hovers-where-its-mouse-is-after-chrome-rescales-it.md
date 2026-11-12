---
status: accepted
---

# A tab hovers where its mouse is after Chrome rescales it

## Problem

Alex's agent checks that a video player shows its controls on hover. The
tab's viewport is 1440 × 900 at pixel ratio 2. The agent moves the mouse off
the player, runs `demi browser record t1 start`, then moves onto the player:
the controls stay hidden. Only a second move shows them, so the recording
shows the controls missing, and the agent concludes that the hover is broken.

Chrome causes it. When a capture of the tab starts, for a recording or for
the live view, or when the screen's ratio changes, Chrome renders the page at
another scale. The page gets a `resize` event at the same size, and Chrome
then hovers the place at the mouse's coordinates halved, at ratio 2, instead
of the mouse's own place. A move sent while that happens lands at the wrong
place too. The page stays wrong until the mouse moves again. Nobody sees the
real mouse: the tab's mouse exists only in the events Demi sends Chrome.

## Decision

The tab knows where its mouse is, and gives it to Chrome again once the page
has settled after each resize. The page then hovers what is under the mouse,
as a mouse that has not moved would leave it.

```text
Page.frameResized --> paint --> MouseMoved at the mouse's place
                                 (none with a button held or a dialog
                                  open, which holds every input)
```

- `tabs/pointer.rs` owns the tab's `Pointer`: the place Chrome was last
  given the mouse, in CSS pixels, and the buttons held there. Every mouse
  event the tab is given passes through `live::inputs::mouse`, the agent's
  and a viewer's alike, which updates it. A wheel event leaves it, since a
  wheel moves no mouse.
- The same module follows `Page.frameResized` for the tab's life. After a
  resize it waits until the page has painted (`tabs::viewport::paint`, the
  barrier the viewport already uses), then sends one `MouseMoved` at the
  place. Without a place, with a button held or with a dialog open it sends
  nothing: a move would drag, and a dialog holds every input.
- The extra move is no input of the agent's or the user's, so recordings do
  not draw it. The page does see one more `pointermove` at the same place.

Nothing is stored and nothing changes in any protocol.

## Spike

Each check ran 2 to 5 times in the development Cloud on a grid page that
reports the hovered cell:

| Case | Before | After |
|---|---|---|
| Ratio 2, mouse inside, recording starts | Hovers the halved place | Correct |
| Ratio 2, move inside right after `record start` | Halved place until a second move | Correct |
| Ratios 1, 1.5 and 3, both cases above | Correct, or correct after the capture fails at ratio 3 | Correct |
| Ratio 2, the live view opens the tab | Hovers another place | Correct |
| Ratios 2 and 3, a page that animates all the time | Not run | Correct |
| Wheel, then a recording starts | Not run | Correct |

Two other timings were tried first and failed:

- Sending the move at the resize event itself left the hover at doubled
  coordinates on a ratio-3 tab.
- Sending it at the recording's first picture after a resize fixed only
  recordings, while the live view's capture resizes the page too.

The paint barrier did not cause another resize: a recording at ratio 2 still
showed one `resize` event.

## Prior art

Playwright's and Puppeteer's `Mouse` classes also remember the mouse's last
place, but only to start a move's steps from it. Neither gives it to Chrome
again after a resize, and neither captures tabs the way Demi does.

## Alternatives

- **Raise the screen's ratio to the tab's when a viewport is set**, so that a
  capture needs no rescaling. The screen's ratio follows the viewers today and
  only rises. This does not cover the live view raising it, which rescales
  every tab, and it changes how every window paints.
- **Wait in `record start` until the capture runs.** This fixes only the
  first move after a recording's start, not a hover that was already on the
  page, and not the live view.

## Open questions

None.
