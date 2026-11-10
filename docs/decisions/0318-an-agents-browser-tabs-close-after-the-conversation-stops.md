---
status: proposed
---

# An agent's browser tabs close after the conversation stops

## Problem

Over a day, the agent of one conversation checks a local page after each
edit, signs in to a dashboard to read a number, and has a subagent compare
three docs pages. Each of these opens tabs, and nothing closes them: the
conversation's Browser window ends the day with twenty tabs Alex never
opened, and the device's browser runs every one of their pages. Alex's own
tabs drown among them, and closing them by hand is Alex's chore.

## Decision

**A tab an agent opened closes 30 minutes after the conversation stops
running, unless the user made it theirs.** A tab the user opened stays until
someone closes it, as now.

- **The agent's**: a tab `demi browser tab new` made, and a page's popup
  from one of the agent's tabs that is not the user's.
- **The user's**: a tab the user opened, one an agent showed to the user
  with `demi browser show` (a sign-in, a choice, a result to look at), and
  one the user acted in: a click, a key, a wheel turn, a paste or a dialog's
  answer in its live view, or an address, Back, Forward, Reload or Stop in
  its window. Once a tab is the user's, it stays the user's.
- **When**: when the conversation stops running, its agents' turns and the
  commands they await all ended, the agents' tabs that are not the user's
  close 30 minutes later. The conversation running again before then, at the
  user's message, a schedule's or another conversation's, cancels the close,
  which its next stop sets again. A subagent that ends before its parent
  leaves its tabs until the conversation stops: Demi follows when the
  conversation runs, not each agent's turn.
- **While the user watches**: a tab whose live view the user watches when
  its time comes closes once nobody watches it, so a page never vanishes
  under the user's eyes; so does the tab its page came from, which a popup
  such as a sign-in may still talk to.
- A temporary command's tab, such as `content.fetch`'s, closes at its end, as
  now.

Thirty minutes is long enough for the user to read what the turn reported
and look at a page it names, and for the agent to go on after a short reply
without opening its pages again. A sign-in it needs again comes back without
asking, as a conversation's browser keeps its sign-ins (0193).

### Build

- `plugin-interface`: a plugin declares that it follows whether each
  conversation runs (`Manifest::running`), and receives
  `Request::ConversationRunning { user, conversation, running }` each time a
  conversation starts or stops running. The backend tells the plugins the user
  has on, at each change of whether the conversation runs, in order.
- The browser plugin follows it. A stop starts a wait of 30 minutes for the
  conversation, which a start cancels. At its end, when the tabs the browser
  last reported include one an agent or a page opened that was never shown,
  the plugin calls the device's new operation `tab.prune`, on the
  conversation's primary device; a conversation without one, as most have
  none, asks its device nothing. The call is no activity: it neither wakes a
  stopped Cloud, whose browser is gone anyway, nor keeps a running one up.
  The wait is the backend's, as the device's browser knows nothing of
  agents' activity and the backend decides its release: a restart of the
  backend forgets the waits, and the next stop sets them again.
- `tab.prune` closes every tab of the agents' that is not the user's and
  that no viewer watches, nor a watched tab came from; it answers the tabs it
  closed and those it left because a viewer watches them. While one is left,
  the plugin asks again a minute later, as long as the conversation does not
  run. A tab that fails to close is left for the next stop, and the others
  still close. It is no `demi browser` leaf, and the browser refuses it from
  an agent.
- The device's tab keeps whether the user acted in it; `shows` already
  counts the shows. A popup whose opener closed has no known owner and stays.

## Prior art

Playwright's MCP server and browser-use close the browser context the agent's
session made when the session ends; Chrome's tab groups for an agent, as in
Claude for Chrome, leave the tabs to the user. Demi keeps the tabs a short
while past the turn, as the user may look at them, and hands over the ones
the user took.

## Alternatives

- **Close at the turn's end**: the page the reply names is gone before the
  user opens it.
- **Ask the agent to close its tabs**: an agent that forgets leaves them, as
  today.
- **Close a tab no one used for 30 minutes**, with no turn: a long turn that
  comes back to a page after a build loses it in the middle of its work.
- **Follow each agent's turn**: Demi follows whether the conversation runs;
  the tree's agents would need a turn's end of their own told to plugins, for
  a subagent's tabs to go a little sooner.
- **Hold the wait on the device**: the device's browser has no notion of
  agent activity, and keeping it so keeps the backend the one that decides
  when a browser's tabs or the browser itself go.

## Open questions

None.
