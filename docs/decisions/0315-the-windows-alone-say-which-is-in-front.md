---
status: accepted
---

# The windows alone say which is in front, and the address follows them

Supersedes: 0311 (a conversation's work is one record, named work), where it
makes the address the source of what the work shows, and its rule for a closed
tab, which now holds within the tab's window.

## Problem

Alex sends a screenshot in a conversation whose windows are all closed, clicks
it, and the Media window opens on it. Alex closes Media, by its tab's close
button or the window's, and the Changes window opens in its place.

Two parts of the page each hold which window is in front:

- **The stage**, the windows' layout (0308): which windows stand, their order,
  which one is in front (`focused`), and which are minimized. Removing or
  minimizing the window in front already hands the front to the newest window
  still shown, or to none.
- **The work's selection** (`web/conversation/work.ts`): `open`, whether the
  address names a tab, and the newest entry of `recent`, the tabs in the order
  they were shown, across every window. `shownSelection` falls back from a
  closed tab to the one shown before it in any window, then to the first
  pinned kind, Changes.

Two watchers copy each to the other (`SessionWindows.vue`). Closing Media
removes its window from the stage and its tab from `recent`; `open` stays
true, so the selection falls back to Changes, the address names `tab=change`,
and the watcher from the address to the stage opens the Changes window.

The fallback is the work panel's (0014), which had one strip of tabs where
Changes was always there. With a window per kind, falling back to another
kind's tab opens another window. 0311 already said a closed tab gives way to
the one its window showed before, but the code applies it across windows.

## Decision

**The stage alone says which windows stand and which is in front; each window
shows the newest of its own tabs; the address is written from the stage and
read only as a request to it.**

### One owner

- **In front:** the stage's `focused`, in the layout the conversation keeps
  (0311). Nothing else holds it: the work's `open` goes, and so do
  `shownSelection` and its fallback to the first pinned kind.
- **Each window's tab:** the newest of the window's own tabs in `recent`, else
  its first, as `workWindows` already reads it. `recent` stays as 0311 keeps
  it: one order for every device, written with the layout.
- **The address:** the shown tab of the work's window highest in the stacking
  order that is not minimized, with its kind's place, such as
  `?tab=change&view=diff`; nothing while no window of the work is shown. It is
  a function of the stage and `recent`, written whenever they change it.

### What changes the stage

Each of the following is a request to the stage, which then writes the
address. A window coming to the front, or none being in front, is a step of
the web browser's history, as today; a close writes the address in place.

| What happens | What the stage does |
| --- | --- |
| A tab is clicked, a pill or a link opens an intent, or a tab's kind asks to show it (`shows`) | The tab goes to the end of `recent`; its window opens where it stood, if closed, and comes to the front |
| A tab closes | Its window shows its newest remaining tab; a window with none closes |
| A window closes or is minimized | The newest window still shown comes to the front, or none |
| The address changes by a navigation the work did not write, such as Back, Forward or a link | A named tab is opened as a click opens it; an entry the work wrote that names none minimizes the work's shown windows; an address nobody marked that names none, such as the sidebar's, leaves the stage as the layout says, as 0311 keeps |

Closing never opens or raises a window that was closed or minimized, as no
desktop system does.

A phone keeps its stage in memory and writes no layout, as 0308 says; its
Back to Conversation still goes back to the entry before the window came.

### Interfaces

- `SessionWork` (`web-ui/windows/session-work.ts`) loses `open`, `select(id)`
  and `close()`: the stage and `recent` say what they said. It gains:
  - `inFront(tab, cause)`: the windows say which tab is in front, or none,
    and why. `came` is their first word since the conversation came on
    screen, `closed` follows a close, and `moved` is anything else. The host
    writes the address from it: in place for `came` and `closed`, a step of
    the history for `moved`. Only the windows know a close: a pinned kind's
    tab, such as Changes, never leaves the work when its window closes.
  - `request`: the tab, or none, that something outside the windows asks to
    have in front, which the windows take once (`took`). A request made by
    the address (`fromAddress`) holds back what the windows say until they
    take it, and is dropped once the page knows that no window can show its
    tab. Any other request waits for its tab without holding back the
    address.
  - `pick(tab)`: the user picked a tab in its window, the newest it showed.
- `WorkState` (`web/conversation/work.ts`) loses `open` and the selection read
  from the address; `history` is `recent` with what the page has not written
  yet over it, without the address's tab.
- `work-tabs.ts` loses `shownSelection`; `workAddress` takes the shown tab
  instead of deriving one.
- `TabKind.shows`, `work.select(conversation, kind, id)`, intents,
  `canGo` and `go` keep their meaning: each brings a tab's window to the
  front, now as a request to the stage.

### Older data

None changes: the layout already keeps `focused` and `recent`. An address with
`tab=` opens that tab's window as before.

## Spike

The reproduction above, on the running product of `main`: an image sent,
clicked, and its window closed by either button opened Changes, with the
address naming `tab=change&view=diff`. Nothing else was uncertain.

## Prior art

- macOS and Windows: closing or minimizing the key window makes the next window
  of the app in the window list key, and never opens one that was closed
  (AppKit's `NSWindow` window list; Windows' Z-order activation).
- VS Code: closing a group's last editor closes the group and activates the
  most recently active group still open (`mostRecentActiveGroups` in
  `src/vs/workbench/browser/parts/editor/editorPart.ts`); closing an editor
  shows the most recent editor of the same group
  (`workbench.editor.focusRecentEditorAfterClose`), never another group's.
- Chrome: closing a tab activates a tab of the same window.

Demi departs from none of them.

## Alternatives

- **Fall back only within the closed tab's window, and keep both owners.** It
  ends this case, but the stage and the selection still correct each other
  through watchers, so the next path that changes one without the other
  opens or hides a window again.
- **Make the address the one owner, and derive the stage's front from it.** An
  address names one tab: it cannot say which windows stand, their order or
  which are minimized, and it is each browser tab's, where the layout is the
  conversation's on every device (0311).

## Open questions

None.
