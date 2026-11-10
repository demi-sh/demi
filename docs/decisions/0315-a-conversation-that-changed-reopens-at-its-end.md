---
status: proposed
---

# A conversation that changed since the reader left reopens at its end

## Problem

Alex reads a running conversation near its end, and a wheel turns the view
up a notch, so the list stops following the end. Alex switches to another
conversation. The turn goes on for ten minutes and ends. When Alex opens the
conversation again to see its outcome, the list opens at the row Alex left,
screens above the answer, and Alex has to find the way down.

The list reopens at the row the reader left whenever the reader had left
the end, however much the conversation changed meanwhile.

## Decision

Where the list opens depends on whether the transcript changed while the
reader was away:

* **Changed**, as a turn that ran on or a message that arrived: the list
  opens at its end, with what arrived in view, as when the reader left it
  following the end. The session's own upkeep, such as a compaction it ran
  while idle or a resume after a restart, is no change to the reader.
* **Unchanged**: the list opens at the row the reader left, as now, so a
  reader who scrolled up to read something and looked away finds it again.

The arrow to the end and every other way of moving stay as they are.

### Design

The saved place keeps where the transcript ended when the reader left:

```ts
export interface PersistedScrollState {
  /** The row the reader left in view; null when the reader followed the end. */
  anchor: ScrollAnchor | null
  /** Where the transcript ended when the reader left (`transcriptEnd`). */
  end?: string
  heightCache: Map<string, number>
}
```

`transcriptEnd(history)` in `web-ui`'s `agent/history.ts` names the
transcript's last block that is news to a reader by its place, its id and
the length of its text, which grows while it streams, such as `41:b7:812`.
A compaction, a resume, the context and response blocks that no row shows
are not news. It is undefined unless the latest window is held.

`useBlockVirtualizer` takes it as an option (`end: () => string | undefined`)
that the conversation's list passes while it shows the latest window. As
the list places itself, a different end opens it at its end. An end unknown
on either side, as for a window of older history the reader went back to,
keeps the saved row, since nothing says the end moved.

The comparison reads the content, not the transcript's version, because the
version's epoch is new whenever the backend builds the transcript again, as
after a restart or an upgrade, though not a word changed.

The list compares once, as it places itself. A conversation opened from its
copy on disk shows that copy until it catches up with its session; what the
catch-up brings arrives below a list placed at the saved row and does not
move it, as any row that arrives while the reader reads does. A reader who
came back to a conversation the page kept connected, which is the case
above, always compares with what the session holds.

The saved place lives where it lives now, with the conversation's draft in
the page's IndexedDB, whose schema gains the optional `end`; a draft saved
before it has none and keeps its saved row. No protocol changes.

## Spike

None.

## Prior art

* **Element Web**, the Matrix client, keeps each room's place in
  [`RoomScrollStateStore`](https://github.com/element-hq/element-web/blob/develop/apps/web/src/stores/RoomScrollStateStore.ts):
  a `focussedEvent` and a `pixelOffset` when the reader scrolled away from
  the live timeline, none while following it. A room the reader left
  scrolled up reopens at that event however many events arrived since. Demi
  keeps the same two cases and adds a third: a place the reader left, in a
  conversation that changed since, gives way to the end.
* **Slack** and **Discord** open a channel with unread messages at the
  first unread one, and a channel with none where the reader left it.
* **Telegram** opens a chat with unread messages at the first of them.
* **ChatGPT** and **Claude** open a conversation at its end.

Demi departs from Slack and Telegram by opening at the end rather than at
the first new row: a turn's news is its outcome, which stands at the end,
and the steps before it are folded into its run.

## Alternatives

* **Keep reopening at the saved row**: the case above.
* **Always open at the end**, as ChatGPT does: a reader who scrolled up to
  read and looked away loses the place even though nothing changed.
* **Open at the first row that arrived**, as Slack does: for an agent's turn
  that row is an early step, and the outcome is still screens below.

## Open questions

None.
