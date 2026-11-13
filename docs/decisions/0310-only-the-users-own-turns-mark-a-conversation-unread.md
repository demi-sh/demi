---
status: proposed
---

# Only what the user asked for marks a conversation unread

Supersedes: 0137 (Only what the user asked for notifies when it ends), its
sentence that the sidebar's dots and the tab are unchanged.

## Problem

Alex's conversation *Coordinate the release* sends a task to the
conversation *Fix the flaky test* with `jush conversation send`. Alex never
wrote in *Fix the flaky test*, but each time its agent finishes the task
and reports back, its sidebar dot turns green, its title turns bold and the
browser tab counts it as done. With a few conversations talking to each
other, the sidebar fills with green dots for work no person asked for, and
the dot of an answer Alex is waiting for drowns among them.

The push notifications already tell these turns apart (0137): a turn that
another conversation or a schedule started notifies nothing. The dots do
not, because a conversation is unread whenever any output is newer than
what the user read.

## Decision

A conversation is marked unread only when a turn the user asked for ends,
by the same rule that decides its notification (0137): the user's own
message or steer started or joined it, or it continues work the user
asked for, such as a turn a command's report woke after the user's
request.

* A turn that another conversation's message or a schedule's message
  started marks nothing when it ends, green or red: no dot, no bold title,
  no count in the tab, the badge or the workspace switcher. Its agent
  tells what is worth telling with `jush notify send`, as for
  notifications.
* An answer the user has not read yet stays unread while other turns
  follow it, and its dot keeps its own color: the user's turn that
  answered stays green when another conversation's turn after it fails.
* The blue dot of a running conversation and the yellow mark of a
  permission request are unchanged.
* What the conversation shows is still read as 0136 says.

```text
  turn started by        ends with    dot today   dot after
  ---------------------  -----------  ----------  ---------
  the user               answer       green       green
  the user               error        red         red
  another conversation   answer       green       none
  another conversation   error        red         none
  a schedule             answer       green       none
```

### Design

The shard already takes the end of a turn the user asked for at each rest
of the tree (`take_turn_end_asked_by_user`, in `follow_turn_end`). At that
point it now also records it on the conversation's row in the control
database, before it notifies:

```sql
ALTER TABLE conversations ADD COLUMN asked_end_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversations ADD COLUMN asked_end_failed INTEGER NOT NULL DEFAULT 0
  CHECK (asked_end_failed IN (0, 1));
```

`asked_end_revision` is the conversation's output revision at that rest,
and `asked_end_failed` whether the turn ended with an error. The entry the
sidebar reads says what is unread instead of whether anything is:

```rust
/// The end of a turn the user asked for that the user has not read.
pub enum UnreadEnd { Answered, Failed }

pub struct ConversationSummary {
    // was `unread: bool`
    pub unread: Option<UnreadEnd>,
}
```

It is `Some` when `asked_end_revision > read_revision`. The output
revision still advances with all output, and reading still moves
`read_revision` to the revision the page shows, so reading clears the mark
as before. The dot is the accent while the conversation runs, else green
or red by `unread`, else none; it no longer reads the last turn's status.

A row that exists before the upgrade takes its `read_revision` as its
`asked_end_revision`, since the control database cannot tell which of its
output the user asked for: every green and red dot clears once at the
upgrade, and marks from then on follow this rule.

## Prior art

None of the AI apps Jush compares with lets one conversation start turns in
another, so none has this case. The rule follows 0137, so a dot and a
notification never disagree.

## Alternatives

* **Keep `unread` a boolean and the dot's color from the last turn's
  status.** Smaller, but an unread answer of the user's shows red when
  another conversation's later turn fails, and green over the user's own
  failure.
* **Leave red dots for every failure.** A failure the user did not cause
  and has nothing to do about would still take the user's attention; the
  agent that started the turn hears of the failure and can tell the user.

## Open questions

None.
