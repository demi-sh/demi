---
status: proposed, superseded in part by 0317
---

# A running dot breathes while a model works, and all breathe as one

## Problem

Alex's agent starts a ten-minute build in the background and ends its turn
to wait for it. For those ten minutes the conversation's sidebar dot
breathes exactly as it does while the model writes, so Alex cannot tell a
conversation that is thinking from one that only waits for a command.
Beside it, the dots of three other running conversations each breathe at
their own phase, since each started its animation when it appeared, and
the sidebar flickers out of step.

## Decision

* **A dot breathes only while a model works**: an agent's turn is under
  way (a request, its output, a compaction it asked for, a retry's wait),
  or input waits to start one. A conversation that runs only because a
  command it awaits still runs keeps its accent dot steady. When the
  command ends and its report wakes the agent, the dot breathes again.
  A subagent counts the same way: one that only waits for its own
  command does not make the dot breathe.
* The same rule holds wherever a running thing is drawn: the sidebar row,
  a folded group's label, the dot on a tab of the work panel, an agent's
  mark and the Agents window's status bar, the Agents chip, and the chip of
  running commands, which stays steady since a command is not a model.
* **Every breathing dot keeps one beat**, the page's: each starts its
  animation at the phase the page's clock is in, so all brighten and dim
  together, wherever and whenever they appeared.
* A service still counts as nothing running, as before.

### Design

The backend's conversation status gains one value, between Running and
the settled ones:

```rust
pub enum ConversationStatus {
    Running,
    /// The tree runs only because a command it awaits still runs: no
    /// agent's session works and no input waits.
    AwaitingCommands,
    // ...
}
```

`Tree::agents_work()` says whether a model works: the root's session
is not settled (but for an idle compaction), input waits, a command's
report waits to be taken in, a child starts or delivers its completion, or
a live child's session is not settled. The summary says Running when it
holds and `AwaitingCommands` when only `is_running()` does.

The page's product status gains `awaiting_commands` beside `active`, from
the summary or, for a conversation it holds, from its own records: the
root's phase, each running subagent's session phase and the running
commands. Both count as running wherever running matters (a running
conversation cannot be archived); only `active` breathes.

Each time a `StatusDot`'s breath starts, as it appears, starts to breathe or
shows again after `display: none`, its CSS animation's start time is set to
the page clock's origin, so every breathing dot shares one phase.

## Prior art

None of the AI apps Jush compares with leaves a command running between an
agent's turns (0206), so none has a state of waiting for one to show.

## Alternatives

* **A separate flag beside `active`**: two flags for one set of exclusive
  states, which this project's rules refuse.
* **Synchronize with a shared JavaScript timer**: every dot would
  re-render each frame; aligning the CSS animation's start costs nothing
  after it starts.
* **A negative `animation-delay` set as the dot starts to breathe**: a dot
  hidden with `display: none` and shown again, as in the phone's sidebar
  drawer, restarts its animation with the old delay and falls out of step.

## Open questions

None.
