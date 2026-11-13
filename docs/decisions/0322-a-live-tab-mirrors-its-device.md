---
status: proposed
---

# A live tab mirrors its device

Supersedes:

- 0069 (A page never waits for a Host): the tabs closed on purpose in the
  browser's report (`closed`), and a tab lost with the browser that stays in
  the panel, which no longer hold.
- 0193 (A conversation's browser keeps its sign-ins): *Tabs are not
  restored*, and a handoff that ends the tabs, which no longer hold when a
  resident ends and the next one takes the conversation over.
- 0166 (An archived conversation's panel still opens tabs): the browser's
  tab that says the conversation is archived. Archiving releases the
  browser, and its tabs leave the panel.

## Problem

Alex asks the agent to check a page, and it opens `https://example.com` in
the conversation's browser. Later the runner updates and the browser ends
with it, or an hour passes and Jush releases the idle conversation. The
page's tab stays in the panel, marked lost. When Alex clicks it, Jush starts
Chrome again, shows *Starting browser…* and loads the page anew. A tab that
looks open is not, and showing it starts work Alex never asked for.

The tab has two owners. The browser on the device reports its tabs, and the
panel keeps a work tab for each, which the page and the plugin both write:

- The plugin adds a work tab for each reported tab, marks one `closed` when
  the browser lost it, and removes it when the report says it was closed on
  purpose.
- The page writes a tab's address and title, makes a tab without a browser
  tab for *Open in Browser Window*, and asks the plugin to open one (`bind`)
  when such a tab shows.
- Closing a work tab asks the plugin to close the browser's tab.

Keeping the two in step takes a `closed` flag, a `failure`, the last 256 tabs
closed on purpose so a closed tab is told from a lost one, and the numbers of
the lists taken before the plugin opened a tab. A simulator's tab has the
same two owners and a gap besides: closed while the device is offline, its
close reaches no device, so the work tab goes and the simulator stays booted
with no tab.

## Decision

A browser page's or a simulator's tab is in the panel exactly while it is
open on the device:

- It leaves the panel when it ends there: the user or the agent closes it,
  its page closes itself, the conversation is released (archived, deleted,
  or idle for an hour, as now), or the browser crashes. Closing the last tab
  ends the browser, as now.
- Showing a tab never starts a browser or boots a simulator.
- While the device cannot be reached (offline, asleep, a stopped Cloud, the
  backend restarting), its tabs stay as it last reported them and say it
  cannot be reached. Once it is back, the panel follows what it reports.
- Closing a tab removes it at once, the device offline included; the device
  closes the page or the simulator when it can.
- The browser window's + and a link's *Open in Browser Window* ask the
  device for a tab. Until the tab comes, the window shows it as it will look:
  the address bar on what it opens, and under it what Jush does, as
  *Starting the browser…*. The tab then comes selected. With the device
  offline they say so in a toast and open nothing.
- When the browser's resident ends and the browser did not end by itself,
  as when the runner updates, restarts or stops, or the Cloud stops, the
  next resident opens the conversation's tabs again, under the same ids and
  at their addresses, as Chrome reopens its tabs when it relaunches to
  update. The panel keeps the tabs throughout. Each page loads again, and
  what was typed into a page and not sent is lost. Simulators already come
  back this way (0031).

### One direction

```text
 the user, the agent               the device
 -------------------               ----------
 close, +, a link's   --- call -->  browser or mobile package
 Open, jush browser                     |
                                        | the conversation's
                                        | tabs, numbered
                                        v
 panel  <--  the plugin writes  <--  the backend keeps the
             the work's tabs         last list
```

1. **The device's list is the one source of which live tabs exist.** The
   browser package reports a conversation's tabs whole, numbered, when they
   change, as now, without the tabs closed on purpose. The mobile package
   says its devices changed, and the plugin lists them, as now.
2. **Only a live kind's plugin writes its tabs, and only from the device's
   list.** A live kind's work tab is the panel's copy of a listed tab: the
   plugin writes it whole, and the page never creates one nor writes its
   data. The page moves, selects and closes them. On each list of the
   conversation's primary device, the plugin:
   - adds a work tab for each listed tab that has none, placed as now;
   - closes on the device each listed tab whose work tab's id is retired:
     the user closed it, and the device did not hear;
   - removes each work tab whose tab the list does not name;
   - writes the others' data from the list: the browser's address, title,
     opener and showings.

   The mirroring is one change of the work, made by the backend from the
   whole list (`WorkDocument::mirror`), which both plugins ask for through
   one function of `plugin-interface`. A tab past the work's most tabs or
   data is left out, or keeps its data, while every other tab changes; a
   later list places it once there is room.

   ```rust
   impl PluginPort {
       /// Makes the work's tabs of `kind` the tabs a device has open,
       /// `listed`, in the list's order, as one change: each listed tab is
       /// created as given or has its data set to the given data, as far as
       /// it fits, and each tab of the kind the list does not name is
       /// removed. Answers the listed ids whose tabs the user closed, which
       /// the plugin closes on the device.
       pub async fn mirror_tabs(
           &self,
           kind: impl Into<String>,
           listed: Vec<CreateWorkTab>,
       ) -> Result<Vec<String>, PortFailure>;
   }
   ```

3. **A close retires the tab's id at once**, as now, and the plugin asks the
   device to close it. A device that cannot be reached gets it from its next
   list, by step 2. That list comes when the device next reports a change of
   the conversation's tabs or devices, or when its resident restarts while
   the conversation shows a tab of the kind (`package_lost`).
4. **The page opens a live tab through its plugin**, as a simulator's Open
   does. The browser's page method `open { url }`, with no address for the
   +, runs `tab.new` as a `user` caller and answers once the work holds the
   tab, from the list that names it. The browser package counts a tab the
   user asked for shown once, so it comes into the panel selected. A kind
   with `open` has the window's + and its intents ask its plugin instead of
   creating the tab; the window waits for the tab and shows the kind's
   `opening` meanwhile, as the simulators' window does while one boots.
   Every opening ends:
   - with the tab, once the work holds it;
   - with a toast, when the device refuses it, or the work has no room for
     it and the plugin closes it again;
   - without a word, when the tab closed before the work held it, as a page
     that closes itself does: a list numbered as high as the answer's, or
     higher, does not name it.

   ```ts
   interface TabKind<Data, Opens = Data> {
     /** The data of a new tab, or with `open`, what it asks the device for. */
     start?(): Opens
     /**
      * Has the kind's plugin make the tab `opens` asks for, in place of the
      * work creating it. The tab comes with the work once the device has
      * it; the plugin reports a refusal.
      */
     open?(opens: Opens, tab: KindTab): void
   }
   ```

5. **A resident hands its tabs on.** The browser resident keeps a record of
   each conversation browser's tabs in the conversation's profile,
   `tabs.json`, written again as they change: each tab's id, address,
   creator and showings, in order, with the browser's language and color
   scheme.
   - The record goes when the browser ends by itself: its last tab closed,
     its conversation released, Chrome crashed, ready or not. It stays when
     the resident ends or hands its profile to another resident, and while it
     waits, that resident reports nothing of the conversation's tabs, so the
     panel keeps them until the next resident lists them. The runner removes
     it at each release, before the residents release, at once or as it
     settles the releases at its hello, since a release reaches only a
     resident that runs.
   - A temporary command's tab, such as `content.fetch`'s, is not recorded:
     it closes as its command ends, and the next resident runs no command of
     the one before.
   - Turning the browser plugin off stops its residents, as a runner stop
     does, and removes its tabs from the panel. The records stay: once the
     plugin is on again, the next resident opens those tabs, and the plugin
     closes them by step 2.
   - A conversation reaches the next resident by its first call. After a
     runner restart that is the plugin's look on `package_lost`
     (`tab.list`), which wakes no stopped Cloud. The resident starts Chrome
     on the profile at once and opens the recorded tabs under their ids,
     each loading its address, before it takes any command for the
     conversation. Until its first list comes, the backend keeps the last
     one, and the shown tab says the browser starts.

### What goes

- The browser plugin's `bind` method (`BindTab`, `TabBound`), `Work::bind`,
  the `opened` lists, and the work tab's `closed` and `failure`.
- The page's `bind` call, its watch that reopens a lost tab, its *Couldn't
  open this page* with Retry, and its writes of a browser tab's address and
  title.
- `TabsChange::Tabs::closed`, `TabListResult::closed`, and the ring of the
  256 tabs closed on purpose.

### What earlier data and releases meet

- A migration removes each stored `browser` work tab that has `closed` or no
  `tab`, which no device lists, and the `closed` and `failure` fields of the
  rest.
- A runner of an earlier release updates itself as it connects. Its
  residents kept no record, so the browser tabs open before the first update
  to this release leave the panel once the new resident lists none.

### Performance

Estimated, not measured:

- A record is about 200 bytes per tab, written each time the list changes,
  a few times per navigation.
- At its next hello, a restarted runner starts one Chrome for each
  conversation whose browser ran before, as many as ran then, each with its
  pages loading at once.

## Spike

- **Does showing a lost tab start the browser?** In the development setup an
  agent opened `https://example.com`, and its browser resident was ended with
  `SIGTERM`. The tab stayed in the panel. Showing it started a new resident
  and a new Chrome at the click; the tab said *Starting browser…* and the
  page loaded again within 4 seconds.
- **Does a resident's Chrome outlive it?** On macOS, yes: the Chrome of the
  ended resident ran until the next resident's sweep removed it. The
  take-back starts its Chrome after the sweep, as every browser start does
  now.

## Prior art

- Redux keeps one store as the single source of truth, read-only to views,
  which change it only by dispatching actions
  ([Three Principles](https://redux.js.org/understanding/thinking-in-redux/three-principles)).
  Here the device is the store, the page's close and Open are the actions,
  and the panel is a view.
- A VS Code terminal's tab closes when its process exits. Persistent
  sessions reconnect a terminal after the window reloads, and revive its
  process after VS Code restarts
  ([Terminal Advanced](https://code.visualstudio.com/docs/terminal/advanced)).
- Chromium's session service writes the open tabs as they change and opens
  them again when the browser relaunches
  ([`session_service.h`](https://source.chromium.org/chromium/chromium/src/+/main:chrome/browser/sessions/session_service.h)).
  Chrome offers to restore after a crash; Jush does not reopen a browser
  that crashed, so a page that crashes Chrome is not opened into the next
  crash.
- Jush's simulators already keep a record of their devices and take them
  back when the next resident starts (0031).

## Alternatives

- **Keep a lost tab and open it again when shown**, as today: a tab that
  looks open is not, and showing it starts a browser.
- **Keep the browser and simulators through the hour-long idle release:**
  the user's pages would stay, but every conversation with a tab would hold
  a Chrome of a few hundred megabytes, and a simulator 1 to 3 GB, until
  someone closed its tabs.
- **Show a tab the page opens before the device has it**, held by the page:
  the tab would have a second owner while it opens. The window shows what
  opens over its tabs instead, and the tab comes within one round trip.
- **List a tab the user asked for as opening on the device:** the protocol
  would carry a state the window's own `opening` already shows.
- **Remove a closed tab only once the device closed it:** a tab the user
  closed would stay while the device is offline.
- **Reopen the tabs after Chrome crashes:** a page that crashed Chrome would
  crash it again at each reopening.

## Open questions

None.
