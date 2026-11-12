---
status: proposed
---

# Conversations that name one browser profile share its sign-ins

Supersedes: 0193 (A conversation's browser keeps its sign-ins), the part
that gives every conversation a profile of its own, for a conversation that
names one.

## Problem

A program runs many conversations on one device, each for its own team, and
each files tickets in the same web tracker. The tracker signs in with single
sign-on and a code from a phone. Alex signs in through the browser of one
conversation. In the next conversation the tracker opens signed out: each
conversation's browser has a profile of its own, in its scratch directory
(0193), so Alex signs in again, once per conversation, and again for a
conversation that replaces an archived one.

## Decision

A conversation can name a browser profile. Its browser runs on that
profile, and every conversation on the same device that names it uses the
same browser: a sign-in, a cookie or a site's storage that one of them
makes, the others see, as two windows of one Chrome do.

`CreateConversation` and `PATCH /conversations/{id}` take `browserProfile`,
such as `"support"`, and null for the conversation's own profile, as today.
The conversation's metadata names it. A Fork, and a conversation an agent
starts from it, name the same profile, as they keep its model. The web
app's conversations name none and are unchanged. The SDK's `Conversation`
gains `update(patch)`, the route's PATCH.

### The profile is a directory

A named profile is a directory in the home of the device's user:

```text
~/.demi/browser-profiles/support/     Chrome's user data of the profile
  profile.lock                        held while a Chrome runs on it
  downloads/  uploads/                emptied when that Chrome stops
```

- It is made when a browser first starts on it, and outlives every
  conversation.
- Demi keeps no record of it: no table, no list. A program lists, copies or
  removes profiles as any directory on the device. A profile removed while
  its browser runs is signed out once that browser stops: Chrome keeps what
  it holds and writes the directory again.
- The name is a directory's name that every system a device runs reads as
  itself alone: 1 to 64 lowercase ASCII letters, digits, `-` and `_`, and
  none of the names Windows keeps for its devices, such as `con` and `nul`.
  macOS and Windows tell no case apart, so `Support` and `support` would be
  one directory under two names, and two Chromes would take it from each
  other.
- A conversation that moves to another device uses that device's profile of
  the same name.

### What each conversation keeps of its own

Several conversations share the profile's Chrome, and each keeps:

- **Its tabs.** `demi browser tab list` lists the conversation's tabs only,
  numbered from its own sequence, and a command names only those. A popup
  belongs to the conversation of the tab that opened it.
- **Its tab record**, from which a new resident opens its tabs again
  (0322), in the conversation's scratch directory, where a conversation's
  own profile keeps it today.
- **Its live view, view links and recordings**, which follow its tabs.
- **Its viewports.**

The downloads and uploads, and the language, time zone and color scheme
its pages see, are the Chrome's, as today.

### Lifetime

- The profile's Chrome starts with the first tab any of its conversations
  opens, and stops when the last tab of all of them closes.
- A conversation's release closes its own tabs and leaves the others'; so
  does the close of an agent's tabs after the conversation stops (0318).
- A crash of Chrome ends the tabs of every conversation on it, as it ends
  one conversation's today.
- A resident that ends, or hands the profile to the next resident, leaves
  each conversation's tabs in its record, and the next resident opens them
  again, on the profile, signed in.
- After a change of `browserProfile`, the conversation's next browser
  command closes its tabs on the profile before, as a release does, and
  works on the one it names now. A job takes the conversation's choice at
  its start, so a script an earlier job left running names the profile
  before: the conversation counts its changes, and a command of a job that
  saw fewer changes than the browser's works on the browser as it is.
- A conversation's tab record names the profile its tabs ran on and that
  count. A resident that reopens them does so on that profile, also when a
  live view reaches it first; a command that names a later profile closes
  them instead.
- A Chrome whose cleanup failed leaves the profile to no other Chrome in
  that resident: every conversation that names it is refused, as one
  conversation's browser is refused today.

### Build

- `CommandContext` (`crates/command-protocol/src/invocation.rs`) gains
  `browser_profile: Option<BrowserProfile>`, the name and the count of
  changes, which the core fills from the conversation's record in
  `core.sqlite`, two columns of its `conversations`. A server's
  conversations, which move into each user's core, name none. A stream that
  reaches the runner directly carries none, and takes the browser as the
  conversation's commands left it.
- The browser resident keeps one Chrome per profile directory, and each
  conversation's browser is an environment on it
  (`crates/plugin-browser/command-chrome/src/tabs/environment.rs`):
  - **The profile's Chrome** owns what there is one of per Chrome: its
    process tree, the profile and its lock, the CDP connection and its
    event pump, the capture extension's connection, the downloads and
    uploads, and the screen. It starts with the first environment and
    retires with the last.
  - **A conversation's environment** owns what there is one of per
    conversation: the tab registry and its numbers, the tab record, the live
    hub, the recordings, the viewport of a tab the user opens, and the
    change counter. Its registry follows the Chrome's target events and
    admits a target its conversation's command created, or one whose opener
    is one of its tabs.
  - **The screen is the Chrome's.** Chrome has one screen for all its
    windows (`Emulation.updateScreen`). The profile's Chrome keeps it at the
    largest size and the highest ratio any of its conversations' viewers
    asked for, so one conversation's viewer never shrinks another's.
- A conversation that names no profile is a profile's Chrome with one
  conversation's tabs, so both cases are one code path.

## Spike

Chrome for Testing, headless, on one profile, through CDP:

- **A popup names its opener.** Pages opened by `window.open(url)`, by
  `window.open(url, '_blank', 'noopener')` and by a link with
  `target=_blank rel=noopener` each arrived in `Target.targetCreated` with
  `openerId` naming the tab that opened them.
- **A tab's viewport is its own; the screen is shared.**
  `Emulation.setDeviceMetricsOverride` on one tab left another's size as it
  was; `Emulation.updateScreen` through one tab changed the other's
  `devicePixelRatio` and `screen.width` too.
- **A Chrome with no page lives on.** Started with `--no-startup-window`,
  as Demi starts it, Chrome ran on with no page after its one page closed,
  and opened a new one.
- **A profile removed under a running Chrome.** Chrome went on, made the
  directory again and wrote to it, and closed cleanly.

## Prior art

- Chrome runs one process per user data directory, so two Chromes cannot
  share one
  ([user data directory](https://chromium.googlesource.com/chromium/src/+/main/docs/user_data_dir.md)).
- Browser Use: parallel agents that share sign-ins share one browser
  session, each in its own tabs, since two sessions cannot run on one
  `user_data_dir`.
- Playwright's `launchPersistentContext` takes a `userDataDir` the caller
  chooses, and every page of the context shares its storage.

## Alternatives

- **One profile per device for every conversation.** It changes the web
  app's conversations, which keep a profile each, and leaves a program no
  way to keep two accounts' sign-ins apart on one device.
- **Profiles as records Demi keeps.** A profile is Chrome's directory; a
  record beside it would be a second owner of the same fact.
- **A Chrome per conversation, with cookies copied between them.** It misses
  site storage, IndexedDB and service workers, and two Chromes would write
  one site's cookies over each other.

## Open questions

None.
