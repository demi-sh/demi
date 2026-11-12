---
status: accepted
---

# A system button stays once its conversation has had what it shows

Supersedes: 0308 (A conversation's work opens in windows on a taskbar), the
sentence "A system button with nothing to show is not there."

## Problem

Alex asks the agent to run a long build in the background. *Running 1*
appears on the taskbar. Alex opens its window to glance at the output, then
clicks the conversation, which minimizes the window. The build ends. The
button now reads *Running 0*, and it stays that way, in this conversation,
for good: a system button shows while its count is more than none or its
window is open, and a minimized window counts as open. In a conversation
where Alex never opened the window, the same build's button goes the
moment the build ends, and the taskbar shifts.

So the same history leaves a button in one conversation and none in
another, a *0* says nothing, and after a reload nothing can bring an ended
command back: the page knows a command only from its live frames, and
forgets the ended ones. Agents does not have that last gap, since every
subagent is stored and listed with the conversation, ended or not, and its
window lists the ended ones under Completed. Commands have no such list,
although the backend keeps each ended command's whole output and the call
that started it.

## Decision

### What the user sees

Agents, Running, Services and Schedules follow one rule:

- **A button shows once the conversation has had what it shows, and
  stays.** Agents: a subagent on the current path. Running: a command left
  running in the background. Services: a service. Schedules: a schedule.
  The first one brings the button in with the motion it has today (it
  grows from nothing). A conversation that never had one shows no button.
- **Its number counts what is active, and shows only above zero.** Agents,
  Running and Services count what runs; Schedules counts the schedules
  that are not paused, none in an archived conversation, whose schedules
  wake nothing. At zero, the button shows its icon alone. Its width
  changes with the motion it has today, so the buttons beside it slide.
- **Nothing running, the window still has something to show.** Agents,
  Running and Services each list their ended ones under Completed in the
  window's title bar, newest first, as Agents does today. Picking one shows
  it in the window's one ended tab, as a finished subagent shows today.

```text
before  | +  Agents 0  Running 0 |      (or no button, by chance)
after   | +  [Agents]  [Running 2] |    [Agents]: icon alone, nothing runs
```

Schedules' button goes when the last schedule is deleted, since a deleted
schedule is not kept and its menu would be empty. The others never go:
their ended ones are kept with the conversation.

### What the backend keeps and sends

A command is *left running* when its `shell` call returned while it still
ran: a background command, a service, or a command that outlasted its
call's window. The call's stored shell view already says it, with the
status `running` it had when the call completed; a Stop stores `aborted`.
Its input says whether it started a service. Nothing new is decided for
these facts, only made readable without decoding every block:

- **`blocks.left_running`**, a text column, `command` or `service`, null
  for every other row, written from the block like `command`. A migration
  fills it from the stored shell calls, reading each record as the
  release that wrote it did. An index of the rows that have it, which
  holds every column the list reads, lets the list touch no transcript
  row, as the rest of the metadata does not.
- **The conversation's ended commands** come with its metadata, as its
  schedules do, so the page has them as it opens the conversation and
  whenever the metadata is sent again: `ConversationMetadata.ended_commands`,
  each the command left running by a call on the current path of the root
  or of a child on that path, the same path Agents shows its children
  from, that ended. Its title, start and end are its `command_outputs`
  row's; the start, which that row did not keep before, falls back to its
  call's.
- **`command_outputs.started_at`**, a new column: when a command ends, the
  same transaction copies its start from its `running_commands` row, so
  its time in the list is the time it ran, not its call's.

```rust
pub struct EndedCommand {
    pub command_id: CommandId,
    /// The agent whose call started it.
    pub node: NodeId,
    pub title: String,
    pub service: bool,
    pub started_at: Option<Timestamp>,
    pub ended_at: Timestamp,
    pub end: CommandEnd,
}
```

- **`GET /conversations/:id/commands/:commandId/output`** answers an ended
  command's output as the terminal shows it, `EndedOutput`: the script its
  call ran and the last 32,768 characters of its output, as many as its
  call's view holds, or why it was not stored.

Running commands still come by their live frames. A command that ends
while the page follows it is in the page's dock with its end, as today,
when its call's view says it was left running; a later metadata lists it
with the rest.

### The page

- The dock's commands are those left running: the live ones whose call no
  longer runs and that run or were left running, then the metadata's
  ended ones the page does not follow. A command that ended within its
  call is no longer the dock's.
- `OwnWindow` gets `used`, derived from what the window holds: a child for
  Agents, a command of its kind for Running and Services. The system area
  shows a window whose `used` is true; the count is passed only above zero,
  on the taskbar and in a phone's window list. `SchedulesButton` shows
  while there is a schedule and counts those that wake the conversation.
- Running and Services get a Completed menu in their title bar,
  `CommandHistoryMenu`, the counterpart of `SubagentHistoryMenu`. A picked
  command's tab reads its output by the new route the first time it shows,
  says *Loading output…* meanwhile, and offers Retry when the read fails.
- The gallery's Terminals specimen lists ended commands under Completed,
  reads their output, and shows the buttons with and without a number.

A conversation forked from this one copies the blocks it copies today,
with their new column.

## Spike

None ran. Read in the code:

- `shell_view` (`crates/agent-tools/src/result.rs`) stores the command's
  status when the call completes. Only a call's file lists are written to
  it afterwards (`crates/agent-store/src/changes.rs`), so a call that left
  its command running keeps `running`.
- `command_outputs` keeps every ended command's title, end and output blob.

## Prior art

- macOS and iOS show an app's badge only above zero, and the Dock keeps an
  app that is kept there whether or not it runs.
- [VS Code](https://github.com/microsoft/vscode) keeps a task's terminal in
  the Terminal panel after the task exits, with its output, until the user
  closes it.
- Demi's own Agents window lists finished subagents under Completed. This
  record gives commands the same.

## Alternatives

- **Keep the button in the conversation's window layout once it has shown.**
  Only the page would change. It would store, beside the subagents and
  schedules the backend already lists, a second record of their having
  been; and for commands it would record that a page saw one, not that one
  ran. Its window would still have nothing to show once nothing runs.
- **Keep the button's room with no number at zero.** The user prefers the
  buttons to slide with motion.
- **Hide a button whose window is minimized and that counts zero.** The
  button would still come and go with what runs, and the taskbar shift.
