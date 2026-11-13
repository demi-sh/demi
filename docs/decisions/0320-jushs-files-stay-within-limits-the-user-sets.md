---
status: accepted
---

# Jush's files stay within limits the user sets, and Settings shows them

## Problem

Alex works with Jush for a month on a laptop. Each conversation's scratch
directory keeps everything its agents put there: a subagent copies a
repository into it and builds it, which leaves a 30 GB `target`; an
acceptance check exports 600,000 frames; a recording's export writes PNG
frames beside the recording. Nothing removes any of it while the
conversation exists, archived or not, so the scratch root holds 230 GB, and
the laptop's disk is 86% full before Alex learns where the space went.
Nothing in Jush shows how much it takes, on a device or on the server, and
nothing lets Alex clean it up or set a limit.

## Decision

Settings gets a **Storage** page in Devices & Data, after Devices. It shows
the server first, then one set of cleanup rules for all of Alex's devices,
then one row per device; a device's row opens its detail.

```text
Storage
+----------------------------------------------------------------------+
| Server  example-host                         42 GB of 96 GB, 1 h ago |
| [#####################                                           ]   |
| * Conversations 3.2 GB  * Attachments 15 GB  * Clouds 29 GB  * Logs  |
+----------------------------------------------------------------------+
| * Conversations  deleted only with their conversation         3.2 GB |
| * Attachments    uploaded files and generated media            15 GB |
| * Clouds         each Cloud's disk                             29 GB |
| * Logs           [14 days v] [At most 2 GB v]                 0.4 GB |
| * Search index   built again when needed                      0.4 GB |
+----------------------------------------------------------------------+
Devices
Every device cleans up by these rules.
+----------------------------------------------------------------------+
| Temporary files      [After 30 days unused v]   [At most 20 GB v]    |
| Cache                [After 30 days unused v]   [At most 10 GB v]    |
+----------------------------------------------------------------------+
+----------------------------------------------------------------------+
| alex-mac      macOS, This Mac, 86% full       [##   ]  Jush  37 GB > |
| Cloud         Linux, Cloud                    [#    ]  Jush 2.1 GB > |
| alex-desktop  Windows 11, offline 2 days      [#    ]  Jush 4.8 GB > |
+----------------------------------------------------------------------+
```

A device's detail shows what Jush takes on it, by kind and by conversation,
and cleans it up by hand; it sets no rule of its own:

```text
Storage > alex-mac
+----------------------------------------------------------------------+
| 1.5 TB of 1.8 TB used                                        Measure |
| Measured 3 min ago, cleaned up 42 GB 5 h ago                         |
| [##################################################            ]     |
| * Temporary 26 GB  * Cache 10 GB  * Components 0.7 GB  * Other apps  |
+----------------------------------------------------------------------+
| * Temporary files  what agents keep              26 GB [Clean Up...] |
| * Cache            browser sign-ins, recordings  10 GB [Clear...   ] |
| * Components       downloaded again when needed 0.7 GB [Delete...  ] |
+----------------------------------------------------------------------+
By Conversation
+----------------------------------------------------------------------+
| Fix the login form  used 10 days ago             12.4 GB [Empty... ] |
| Weekly report       running, used today           3.1 GB [Clean Up ] |
+----------------------------------------------------------------------+
```

### What counts as what

On a device, everything Jush keeps is one of three kinds:

| Kind | What it is | Where |
| --- | --- | --- |
| Temporary files | What agents put in `$JUSH_SCRATCH` | A conversation's scratch directory, but its `.browser` and `.recordings` |
| Cache | A conversation's browser profile, its sign-ins and cache, and its recordings with their exports | `.browser` and `.recordings` in the scratch directory |
| Components | What Jush downloads to run: Chrome, the Claude Code CLI, codecs, simulator runtimes | The runner's artifact cache and the mobile runtimes |

A recording's export moves from `scratch/recording-<id>` into the
recording's own directory, so an export is cache and goes with its
recording. Its frames stay PNG: agents read their pixels, which JPEG's
artifacts would mislead.

On the server, the kinds are the conversations' databases, the attachments
and media, the Clouds' disks, the log, and the search index. Each is shown
with what already removes it. Only the log has rules, which replace
`JUSH_LOG_KEEP_DAYS` and `JUSH_LOG_MAX_SIZE`. A Cloud already keeps only its
current and previous generations, so the Clouds have no rule.

### The rules

The two device kinds that grow with work each have two rules, one set for all
of a user's devices, each rule applied on each device on its own:

| Kind | Unused for | At most, per device |
| --- | --- | --- |
| Temporary files | 30 days | 20 GB |
| Cache | 30 days | 10 GB |

- **The choices**: Unused for 7, 14, 30, 60 or 90 days, or Never; At most
  5, 10, 20, 50 or 100 GB, or No Limit.
- **What is removed whole**: a top-level entry of a conversation's scratch
  directory, and of each subagent's `agent-<n>` folder; a conversation's
  browser profile; one recording with its export. A rule never picks a
  part of one, so what an entry holds goes together.
- **When an entry was last used**: the newest of the modification and status
  change times (creation times on Windows) of anything in it, since `tar`,
  `cp -a` and `wget` give a file written today a modification time of years
  ago, and `mv` keeps it. Access times are not read, since macOS and most
  Linux mounts do not keep them up to date.
- **Links and other file systems**: the walk never follows a link and stays
  on the scratch root's file system. A link goes as a link; an entry that
  holds a mount, such as a disk image an agent attached, is neither counted
  past it nor removed.
- **Unused for**: an entry not used for that long goes.
- **At most**: when a kind takes more than its limit on a device, its
  entries go from the one used longest ago until the kind is back under the
  limit.
- **Never removed by a rule**: an entry used in the last 24 hours, and a
  browser profile whose browser the runner is running. A kind can therefore
  stay over its limit while it is all in use, and the device's detail says
  so: *Over the limit: all in use in the last day.* The 24 hours keep a
  task that paused overnight whole.
- **Archiving** a conversation removes its whole scratch directory, as it
  removes its browser profile and recordings today. Unarchiving it finds the
  directory empty.
- **Components** have no rule: the artifact cache already removes a line's
  older versions once a newer one is installed.
- **Conversations** are never removed by a rule. Claude Code's
  `cleanupPeriodDays` removes transcripts after 30 days by default, and
  users who lost theirs reported it as a bug.

### When

The runner measures its kinds a minute after it receives the rules and then
every 6 hours, on a background thread at low priority, and applies the rules
after each measurement. It measures again for each button on a device's
page. The free space the runner reads every 5 minutes (0312) falling under
10% of the disk applies the rules at once. The backend measures the server
when Storage asks and its last measurement is older than 10 minutes.

- 6 hours: a measurement walks every file, about 2 minutes for a million
  files, so it is not worth doing more often for numbers that change slowly.
- 10 minutes: a page opened twice in a row does not walk the server twice.
- 10%: the share at which macOS and Windows warn that a disk is almost
  full.

Every cleanup, by a rule or by hand, is recorded on the device: when it ran,
what started it, and what it removed. Storage shows the last one.

### By hand

- **Clean Up…** on Temporary files removes every conversation's temporary
  files on the device but those used in the last day, after a confirmation.
- **Clear…** on Cache removes every browser profile and recording but those
  in use, after a confirmation. The conversations sign in again.
- **Delete…** on Components removes every component no running program
  holds; each is downloaded again when needed.
- **Empty…** on a conversation's row removes its whole scratch directory,
  after a confirmation; a running conversation's row offers **Clean Up**
  instead, which removes what the conversation has not used in the last day.

### Who sees what

- **The server**: an administrator sees the whole server and sets the log's
  rules. Any other user sees only their own conversations and attachments,
  with no rules and no capacity bar. Clouds share their blocks on the
  server's disk, so a Cloud's own share has no meaningful size.
- **The devices**: each user sees and sets the rules of their own devices.

### What the agent reads

The context's sentence on `$JUSH_SCRATCH` says the directory is small and
cleaned up:

> Files only you need (screenshots, probe scripts) go in $JUSH_SCRATCH,
> which shows as no change; what goes unused for a while, or past its size
> limit, is removed.

So an agent puts a repository it builds somewhere else, and knows a file
there may be gone weeks later.

### Build

- **Rules** live in the user's preferences (`user_preferences`), as
  `storage: { temporary: Rule, cache: Rule }`, `Rule { unusedDays:
  Option<u16>, maxBytes: Option<u64> }`, where none is Never or No Limit.
  The runner receives them in `HelloOk` and in a new `Inbound::StorageRules`
  whenever they change.
- **Measurement**: a new `Outbound::StorageMeasured { report }` carries
  `measured_at`, the disk's size and free space, each conversation's
  temporary and cache bytes and last use, the components' bytes (the
  installation directory but the scratch, and the artifact cache), and the
  last cleanup. Bytes are allocated blocks, as `du` counts them; symbolic
  links are not followed. The backend keeps the latest report per device in
  a new `devices.storage` column, so an offline device still shows its last
  one with its age.
- **By hand**: a new `Inbound::Storage { id, action }`, where the action is
  Measure, a kind, or a conversation and whether to empty it, answered with
  `Outbound::StorageDone { id, report }` or `StorageFailed { id, reason }`.
- **The server**: the backend measures its data directory with the same
  cadence, per user from the conversations' and attachments' owners and the
  Clouds' devices. `server_settings` gains `log_keep_days INTEGER NOT NULL
  DEFAULT 14` and `log_max_bytes INTEGER NOT NULL DEFAULT 2147483648`; the
  two variables and their flags are removed: a server whose configuration
  still sets one fails the configuration check the upgrade runs before it
  stops anything, which names the variable.

## Spike

None: each rule acts on directories the runner already owns, and each
measurement on numbers the system already gives.

## Prior art

- [BuildKit's garbage collection](https://docs.docker.com/build/cache/garbage-collection/):
  each policy has a `keepDuration` and a `keepBytes`, applied from the
  least recently used record. Jush's two rules are the same pair. BuildKit
  never removes a record in use, as Jush never removes an entry used in the
  last day.
- [Bazel's disk cache](https://bazel.build/remote/caching#disk-cache):
  `--experimental_disk_cache_gc_max_size` and `--experimental_disk_cache_gc_max_age`,
  the same pair again, by modification time.
- [Cargo's automatic gc](https://doc.rust-lang.org/cargo/reference/unstable.html#gc):
  downloads unused for 3 months go; Cargo records last use itself, which
  Jush derives from modification times.
- Chromium's disk cache: one size limit for the whole cache, evicted from
  the least recently used entry, not a limit per site.
- Windows Storage Sense and macOS Storage: a page of what takes the disk by
  kind, rules for temporary files by age, and a run when the disk is low.

## Alternatives

- **Rules per device.** A Cloud and a laptop could keep different amounts,
  but Alex sets the same thing on each device and forgets one. One set for
  all devices, applied on each, is what Storage Sense's policies do across
  a fleet.
- **Browser data and recordings as kinds of their own.** That follows the
  directories, not what Alex understands: both are cache to Alex.
- **A limit per conversation.** One busy conversation hits it while twenty
  idle ones keep theirs; a limit per device removes the idle ones first.
- **Keeping scratch until the conversation is deleted, as today.** That is
  the problem.

## Open questions

None.
