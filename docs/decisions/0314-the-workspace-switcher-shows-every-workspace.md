---
status: proposed, superseded in part by 0317
---

# The workspace switcher shows every workspace and what each one's conversations do

Supersedes: 0142 (An account holds workspaces), the part where the
switcher is always one row whose menu lists the workspaces.

## Problem

Alex keeps two workspaces, Personal and Work, and moves between them many
times a day. Each move takes two clicks, one to open the switcher's menu
and one to choose, and the sidebar never shows the other workspace's name
until the menu opens. With five workspaces the yellow dot on the row says
that one of them waits for Alex, not which one, and nothing says that a
conversation in Work still runs or has finished.

A folded project or group has the same gap: its dot says that a
conversation it hides has news, red, yellow or green, but not that one
still runs.

## Decision

The switcher keeps its place under the app's name and its height, and
shows by the number of workspaces:

```text
1 workspace        ┌──────────────────────────────┐
                   │ [▣] Personal               ⇅ │   the row, as before
                   └──────────────────────────────┘
2 workspaces       ┌──────────────────────────────┐
                   │▐[▣] Personal ▌  [▣]• Work    │   tile and name each
                   └──────────────────────────────┘
3 and more         ┌──────────────────────────────┐
                   │▐ [▣] ▌ [▣]•  [▣]•  [▣]   [▣] │   tiles alone
                   └──────────────────────────────┘
more than fit      ┌──────────────────────────────•
                   │ [▣]  [▣]  [▣]  [▣]  [▣]  [▣░ │   half of the next;
                   └──────────────────────────────┘   the end's dot
```

* **One workspace** stays the row whose menu makes another, since there is
  nothing to switch to.
* **Two and more** are segments of one track, as a segmented control's,
  equally wide; the shown workspace's segment wears the control's thumb.
  A click on another segment switches to it at once.
  * With two, each segment has its tile and name.
  * With three and more, each has its tile alone.
  * Each segment's tooltip says its workspace's name and the keys that
    switch to it.
* **When the tiles do not fit** at their least width, the track scrolls
  sideways without a scroll bar, as a browser's tabs do. The tiles widen so
  that the track shows whole tiles and half of the next one, and the end
  with more past it fades, so a cut tile says that more lie there. The
  shown workspace's segment scrolls into view whenever it changes.
* **Each other workspace's tile carries a dot** on its corner for its
  conversations, by the rule below; the shown workspace's carries none,
  since its rows show. A tile that an end of the track cuts or hides lends
  its dot to that end's top corner, which shows the most urgent of them,
  so a workspace scrolled away is still seen.
* **One rule for a dot that stands for conversations whose rows do not
  show**: another workspace's tile, a folded project's row and a folded
  group's label alike.
  1. The most urgent news, as now: red for a failure not read, then
     yellow for a request, then green for an answer not read.
  2. With no news, the accent while one runs: breathing while a model
     works, steady while only a command is awaited (0313).
  3. Otherwise none.

  When Alex reads the answer that made a folded project green, its dot
  turns back to the breathing accent if another conversation in it still
  runs.
* **The keyboard**: the track is one tab stop, on the shown segment.
  Left and Right, Home and End move the focus along the segments, and
  Return or Space switches to the focused one, as tabs that wait to be
  chosen do, and as Return opens the conversation the arrow keys reach in
  the sidebar's list. A switch shows a conversation, whose composer takes
  the focus, so a switch on every arrow key would end the walk after one
  step. ⌃1 to ⌃9 still switch.
* **A right-click on the segments** opens the switcher's menu: every
  workspace with its keys and its tile's dot, and a check on the shown
  one, then New Workspace and Workspace Settings…. The row's menu
  with one workspace is the same menu.
* The phone's sidebar draws the same, at its taller row height.

### Design

The rule is one function beside the row's own (`conversation-dot.ts`),
which replaces `mostUrgentNews`:

```ts
/** The conversation whose dot stands for `conversations`: news, then a running one. */
export function groupDot<C extends { id: string } & DotSource>(
  conversations: readonly C[],
  openId: string | null,
): C | null
```

The folded label and project row take its answer as they took the news
before. The page's workspace store finds each workspace's dot among
its conversations that are not archived, and `WorkspaceSwitcher` takes
them in place of the list of workspaces that wait:

```ts
dots: Readonly<Record<string, DotSource>>
```

`ConversationDot` draws the dot on an icon's corner too, so a tile, a menu
row and a label draw one dot. The number of workspaces picks the
switcher's layout. One function returns the menu's rows as context menu
groups, which the row's dropdown and the segments' right-click both draw
with `MenuEntries`. A context menu row gains a check for a menu of choices
drawn from data, and an ID for rows whose labels may repeat, as two
workspaces' names may:

```ts
export interface ContextMenuEntry {
  // ...
  /** What tells the row from the others when its label may repeat. */
  id?: string
  /** The row is one of a set, checked while it is the one in effect. */
  checked?: boolean
}
```

Nothing is stored and no protocol changes.

## Spike

None.

## Prior art

* **Zen Browser**, open source, draws each workspace as an icon at the
  bottom of its sidebar, and a click on one switches to it
  ([docs](https://docs.zen-browser.app/user-manual/workspaces),
  [code](https://github.com/zen-browser/desktop)).
* **macOS's segmented control** switches among a few views in one click,
  the chosen one drawn on a raised thumb.
* **Arc** draws each Space as an icon in a row at the bottom of its
  sidebar, and a click on one switches to it
  ([help](https://resources.arc.net/hc/en-us/articles/19228064149143-Spaces-Distinct-Browsing-Areas)).
* **Slack** and **Discord** show every workspace or server as a tile in a
  rail, with the tile of one that has news marked, and a click switches.

Demi departs from Slack's and Discord's rail by keeping the switcher in
the sidebar's width instead of adding a column; from Zen and Arc by
keeping it at the top, beside what it switches, and by naming the
workspaces while there are only two.

## Alternatives

* **Keep the menu alone**: two clicks for the most common move, and the
  dot cannot say which workspace waits.
* **Only news on a tile**, as a folded label had: a workspace whose
  conversations all run looks settled.
* **A dot for a running conversation over news**: a request would hide
  behind any running conversation of the same workspace.
* **A vertical rail of tiles**, as Slack's: a new column that the sidebar's
  width pays for, for a list that is often two or three.
* **Shrink the tiles to fit every workspace**: past six or seven the tiles
  get too small to hit.
* **Arrow keys that switch at once**, as a radio group's do: the
  composer of the conversation the switch shows takes the focus after the
  first step, and keeping it on the track would take an exception in the
  composer's rule for taking the focus.
* **A radio group's role** for the segments, as `Segmented.vue` has: a
  screen reader tells a radio group's arrow keys to choose at once, which
  these do not. The segments are tabs that wait to be chosen, with no tab
  panel of their own, since what they switch is the whole sidebar.
* **Names with three and more**: three names in a 256 px sidebar truncate
  to a few letters each.

## Open questions

None.
