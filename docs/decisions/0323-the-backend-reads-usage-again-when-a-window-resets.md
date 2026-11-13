---
status: accepted
---

# The backend reads an account's usage again when one of its windows resets

## Problem

At 9 PM, Alex's Claude Code account in Settings › Providers shows *5-hour
limit · Resets now · 2% used*, and at 11 PM it still does. A second account,
full at 6 PM, keeps *Limit reached* for hours after its window reset. Both
stay until the vendor reports the account again.

The backend reads an account's usage when the user presses Refresh usage,
when Smart chooses an account whose snapshot is more than five minutes old,
and from each response the account gets. An account nobody uses gets none of
them. Routing already counts a window whose reset passed as reset, so the
page says the account is used up while requests use it.

## Decision

Shortly after a window's reset passes, the backend reads the account's usage
from the vendor again, with the probe Refresh usage runs, and stores and
announces the answer as it does any other snapshot. The page then shows the
vendor's numbers and the next reset, or no reset when the vendor names none.
The page still asks nothing: refreshing at a reset is the owner's schedule,
as 0030 (Opening a page shows what is known and asks nothing) calls for.

* **When.** 30 seconds after the reset, as CodexBar waits, so a vendor whose
  clock runs a little behind the server's has rolled the window over.
* **Which resets.** A reset is waited for when it changes what the window
  shows: some of the window was used, and its value was read before the
  reset. A window with nothing used reads the same after its reset, and a
  vendor that keeps rolling an unused window forward would otherwise have an
  idle account read at every reset, for good.
* **One timer per account.** It waits for the earliest of those resets. The
  backend sets it whenever it writes the account's snapshot, and when it
  first reads the snapshot from the account's record. After a restart, the
  first reading of an account sets it, and a reset that passed while the
  backend was down is read at once.
* **Nothing read twice.** As the timer ends, it looks at the snapshot again:
  a response or a probe since, such as Refresh usage pressed after the
  reset, has read past it, and nothing more is read. The account also keeps
  when it was last read for a reset, and a reset at or before that time is
  never read for again, whether the probe failed or the vendor still reports
  the window as before; otherwise such a vendor would be probed in a loop.
  The backend forgets this as it stops, so a failed read is tried once more
  after a restart.
* **A failed probe** is logged, and the pages are told, as they are told today
  when a full window resets. The snapshot stays until the account is next
  used, Smart reads it, or the user presses Refresh usage.
* A signed-out account is not read, and its timer ends, as removing an
  account or its entry ends it.
* The timer replaces the announcement at a full window's reset, which told
  the pages and read nothing. The announcement when a vendor's refusal mark
  ends stays.

On the page, a window whose reset passed and that was not read again yet,
for those 30 seconds or after a failed probe, shows *0% used* and no reset,
as routing counts it. *Resets now* is gone.

Each probe reads a usage endpoint and spends no inference. Claude Code's
runs its CLI on the account's machine, and wakes the machine when it sleeps.
An account is read at most once per reset of a window it used: with Claude
Code's 5-hour and weekly windows, about five times a day while it is used,
and once more after its last use, when those windows reset.

The design to build from:

```rust
// crates/provider-registry/src/store.rs
pub trait RegistryStore {
    /// The entry `id`; none once it is deleted.
    fn entry(&self, id: ProviderId) -> BoxFuture<'_, Result<Option<ProviderEntry>, StoreError>>;
}

// crates/provider-registry/src/registry.rs
impl ProviderRegistry {
    /// The registry, which `AccountQuotas`' timers probe accounts through.
    pub fn new(/* as today */) -> Arc<Self>;

    /// Reads `account`'s usage from its vendor; false when it could not.
    pub(crate) async fn probe_account(&self, entry: &ProviderId, account: &CredentialId) -> bool;
}

// crates/provider-registry/src/quotas.rs
struct ResetWatch {
    /// When the account was last read for a reset.
    read: Option<Timestamp>,
    /// The reset the account's timer waits for, with the timer's number.
    timer: Option<(Timestamp, u64)>,
}
```

Nothing stored changes, and an older page gets the same announcements.

## Spike

None ran before the build. A vendor's window cannot be made to reset on
demand, so the acceptance ran a development backend against a stand-in for
Codex's usage endpoint:

* A window full until a minute later was read again 30.0 seconds after its
  reset, and the page showed the stand-in's next window.
* A reset that passed while the backend was stopped was read once, a second
  after the backend's first reading of the account.
* A stand-in that kept reporting the passed reset was read once, not again.
* A window with nothing used, whose reset the stand-in kept a minute ahead,
  was read every 90 seconds until only windows with some use were waited
  for; then it was read no more.
* A window read after its reset was read again after a restart until the
  rule looked at when its value was requested; then it was not.
* Refresh usage pressed after a reset, before the timer ended, left the
  timer nothing to read.

Two points stay unknown, as no real reset was watched: whether every vendor
has rolled the window over 30 seconds after its reset, and what each answers
for a window that has not started again.

## Prior art

CodexBar, a macOS menu bar app for Codex and Claude usage, added the same in
0.38.0 ([#1847](https://github.com/steipete/CodexBar/pull/1847)). It sets one
refresh at the next known reset plus 30 seconds, and remembers the resets it
tried, so a provider that keeps a stale snapshot is not asked again and waits
for the regular polling. CodexBar also polls every 1 to 15 minutes. Jush does
not: the backend hears each response's usage, and Smart reads old snapshots
before it chooses.

## Alternatives

* The page asks when a window it shows resets, as if Refresh usage were
  pressed. Every open tab would probe, the snapshot routing reads would stay
  old, and 0030 rules out asking on sight.
* Polling every few minutes: it probes accounts whose usage did not change.
* Showing the window as reset without reading it again: the next reset stays
  unknown until the account is used.

## Open questions

None.
