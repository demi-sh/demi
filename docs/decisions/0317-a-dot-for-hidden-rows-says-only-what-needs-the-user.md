---
status: proposed
---

# A dot for hidden rows says only what needs the user

Supersedes:

- 0314 (the workspace switcher shows every workspace), the second step of
  its rule for a dot that stands for conversations whose rows do not show,
  the accent while one runs, and the dot that turns back to that accent once
  the news is read.
- 0313 (a dot breathes while a model works), a folded group's label among
  the places that draw a running thing.

## Problem

Alex works in the Personal workspace while two conversations of the Work
workspace build and test. The Work tile carries a breathing accent dot. Alex
reads it as "something there wants me", switches, and finds both still
running, with nothing to read or answer. A folded project and a folded group
do the same. A dot on something Alex cannot see should mean that going there
is worth it.

## Decision

A dot that stands for conversations whose rows do not show, on another
workspace's tile, a folded project's row and a folded group's label, shows
only news that needs the user:

1. Red for a failure not read, then yellow for a request, then green for an
   answer not read, the most urgent first, as now.
2. Otherwise none, however many of them run.

A row's own dot is unchanged: a conversation that runs still shows the
accent, breathing while its model works (0313), where its row shows.

The dot for hidden rows is the dot one of those rows wears, so it says what
the row says. A conversation whose failure is still unread, and that a
schedule starts again, wears the accent while it runs, as its row and the
tab's title count do; it lends no dot until that turn ends, and lends its
news again then.

### Build

`groupDot` in `conversation-dot.ts` drops its running rank, so every place
that reads it, the sidebar's folded projects and groups, the workspace
switcher's tiles and the ends that lend a cut tile's dot, follows at once.
The gallery's sidebar and switcher specimens show a folded project and a
tile whose conversations only run, with no dot.

## Prior art

Slack marks another workspace only for unread messages and mentions, never
for activity; Mail and Messages badge a folder or a chat only for what is
unread.

## Alternatives

- **Keep the accent, steady, never breathing**: still a dot that leads to
  nothing to do.
- **Count news a running conversation still holds**: the hidden dot would
  say red while the row says it runs, and the tab's title would count it
  otherwise.

## Open questions

None.
