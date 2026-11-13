---
status: proposed
---

# Demi becomes Jush, and every installation moves to the new name by itself

## Problem

The product is renamed from Demi to Jush. Alex runs a server at
`https://agents.example.com` with two paired devices, a Mac and a Windows
PC, and one Cloud. Each of them holds the old name in places a program
looks for by name:

- The server keeps everything under `/opt/demi`, runs the units
  `demi-backend.service` and `demi-machine-manager.service` as the user
  `demi`, reads `/opt/demi/config/demi.env`, whose settings are named
  `DEMI_*`, and mounts its Clouds' file system at `/opt/demi/data/cloud`.
- Each device runs `~/.demi/runner/<installation>/demi-runner`, started by
  the login item `sh.demi.runner.<id>`, and keeps its scratch, logs and
  plugins under `~/.demi`.
- A Cloud's home is `/home/demi`, and the backend stores that path for the
  Cloud and for every conversation that worked there.
- The browser holds the session cookie `demi_session` and Alex's drafts and
  preferences under `localStorage` keys `demi.*`.
- Alex's repositories keep their workflows and instructions in `.demi/`.
- Every agent is taught the command `demi` and the variable `DEMI_SCRATCH`.

A release that only replaced the text would leave each of these behind: the
installed server would not find the next release, the devices would start a
program that is gone, and Alex would lose drafts, Cloud paths and
workflows. Alex must not reinstall anything or pair a device again.

## Decision

Every name the product, its programs, its stored data and its documents use
becomes Jush's in one release, and each installation moves itself to the new
names as it updates, keeping its data.

### The names

| What | Before | After |
| --- | --- | --- |
| Product | Demi | Jush |
| Command and programs | `demi`, `demi-server`, `demi-runner`, … | `jush`, `jush-server`, `jush-runner`, … |
| Crates and modules | `demi-*`, `demi_*` | `jush-*`, `jush_*` |
| Packages | `@demicodes/*` | `@jush-ai/*` (private, not published) |
| Variables | `DEMI_*` | `JUSH_*` |
| Device directory | `~/.demi` | `~/.jush` |
| Server directory | `/opt/demi`, `/var/lib/demi`, `/run/demi-*` | `/opt/jush`, `/var/lib/jush`, `/run/jush-*` |
| Server user and group | `demi`, `demi-cloud` | `jush`, `jush-cloud` |
| Cloud home | `/home/demi` | `/home/jush` |
| Project directory | `.demi/` | `.jush/` |
| Plugin packages | `demi.browser`, … | `jush.browser`, … |
| Cookie, headers | `demi_session`, `X-Demi-*`, `demi-runner-*` | `jush_session`, `X-Jush-*`, `jush-runner-*` |
| Repository and image | `demi-sh/demi`, `ghcr.io/demi-sh/demi` | `jush-ai/jush`, `ghcr.io/jush-ai/jush` |
| Site and identifiers | `demi.sh`, `sh.demi.runner` | `jush.ai`, `ai.jush.runner` |

The otter stays. The wordmark reads "Jush", with a capital J: next to the
round otter the mixed case keeps the friendly rhythm the lowercase "demi"
had, where "JUSH" in capitals reads heavier and louder.

The rename covers the decision records too: their text and their file names
follow the new name, as a one-time exception to the rule that a merged record
never changes. The old name remains only in this record, in the migrations
below, which must know what they move, and in the values that keep it.

### The values that keep the earlier name

A value that what is already stored depends on, or that a program from
before reads, is a value, not a name, and keeps its bytes:

- The labels the instance secret derives keys under, and the labels each
  sealed row of the vault is bound to (`demi provider vault`,
  `demi credential`, …): a new label would open no stored secret and no
  push subscription.
- The marks Claude Code's provider keeps in a conversation's blocks,
  `demiBlob` and `demi_left_out`.
- The values of the update's steps that stop and start the backend,
  `stopDemi` and `startDemi`: the page of a release from before follows the
  update that brings the rename, and refuses a step it does not know.

Between servers that lend providers to each other, a borrower sends its
version and name under both the new and the earlier headers, and a lender
reads either; a share key from before, `demi-share-key-…`, is still read.
These go with the migrations.

### What an installation meets

Each step below runs once, when the first Jush release meets an
installation from before, and is removed with the other migrations when the
history starts again at the first formal release.

**The server.** The installed `demi-server` finds the next release in
`demi-sh/demi`, which GitHub redirects to `jush-ai/jush` after the
repository's transfer. It downloads the assets named
`demi-<version>-server-linux-<arch>.tar.zst` and
`demi-<version>-image-linux-<arch>.tar`, and hands the move over to that
release's `bin/demi-server`, passing its lock as `DEMI_SERVER_LOCK_FD`. So
until the history restarts, every release also publishes those two assets
under the earlier names, and its archive carries `bin/demi-server` as a
link to `jush-server`. That run, finding anything of the earlier names,
first moves the installation:

1. Stops the `demi-*` services, the manager saving every running Cloud, and
   stops there if one does not stop; then disables and removes their units,
   and removes every `/run/demi-*`.
2. Renames `/opt/demi` to `/opt/jush`, with the Clouds' store unmounted,
   points `current` at its new place, relabels the store, rewrites its
   `/etc/fstab` line and its comment, and mounts it again unless it is
   mounted.
3. Renames the user `demi` and the groups `demi` and `demi-cloud`, keeping
   their IDs, so every file keeps its owner.
4. Writes `/opt/jush/config/jush.env` from `demi.env`: each `DEMI_` setting
   under its `JUSH_` name, and each path of before at its new place. The
   earlier file stays as `jush.env.before-<version>`.
5. Replaces `/usr/local/bin/demi-server` with `jush-server`, and installs
   the release's units.

Each step looks at what is there before it acts, so a run that stops
anywhere is taken up by the next: `/opt/jush/releases/<version>/bin/jush-server
move` while `demi-server` no longer runs. The checks that come before every
move, such as the room for the databases' copies, come after this one has
stopped the services, since they read the new layout: a check that fails
there leaves the server stopped until its cause is fixed and the move runs
again.

It then goes on with the move as any move does. Neither a rollback nor a
failed start returns to a release from before the rename, which cannot run
in the new layout: the server stays and says why, and the snapshot the
upgrade took stays. The work ahead of an update that an earlier server
hands over does nothing, since the move gets its release ready itself.

A container deployment changes its image to `ghcr.io/jush-ai/jush` and its
volume's path to `/opt/jush` by hand, as it changes its image for any move.

**A paired device.** The backend still answers a runner that asks with
`demi-runner-*` headers and fetches `demi-runner`. The runner from before
downloads the new one over its own `<installation>/demi-runner` and starts
it through its launcher, which names the installation in `DEMI_HOME`. That
runner takes the earlier variables as the current ones and, before it opens
anything in the installation:

1. Renames the installation's directory from `~/.demi/runner/<id>` or
   `~/.demi/instances/<id>` to the same place under `~/.jush`; one that
   `DEMI_HOME` put elsewhere stays. It moves each other entry of `~/.demi`,
   such as the downloaded browsers and the plugins' files, which every
   installation of the device shares: on macOS and Linux it leaves a link
   at each earlier path for the installations still on a release from
   before, and the last to leave removes the links; on Windows, which makes
   no link without the right to, those entries move with the last
   installation.
2. Settles as an installation from before does: links `jush-runner`,
   writes the launcher anew and starts itself there, as no manager's
   runner, which registers `ai.jush.runner.<id>` and hands over to it.
3. The manager's runner removes the registration `sh.demi.runner.<id>`
   and `demi-runner`, and at each start what is left of `~/.demi`: the link
   at the earlier path once no job's shell runs, and empty directories.

A job runs on through the move. On macOS and Linux the directory renames
while a job's shell writes in it, the next runner takes the job over in the
new place, and a link at the earlier path serves the job that still names
it. Windows refuses to rename a directory in which a process holds a file
or works: the runner then settles where it is, under the new names, and
stays there. The move is tried only at that first start, whose settling and
handing over register the runner where it lies; a later move would leave
the running supervisor starting the launcher at its earlier path.

A job whose shell runs on through the update reaches the runner at an
address that the installation's directory and the earlier name give: once
the runner moved, that job's `demi` commands find no runner. The job itself
runs on; this happens once.

On a Mac the runner's path and signing identifier change, so macOS asks
once more for the folders it was allowed.

**A Cloud.** The rename changes a base's format: its user, its home and the
place of the programs. A Cloud on a base from before is reset to the
configured base at its next wake, keeping its home, as a base whose format
a release ends is: what was installed outside the home goes. Each boot
renames the user's directory in the home volume from `demi` to `jush`, and
the runner's `.demi` in it, its scratch and plugins' files, to `.jush`. The
manager's start removes the interfaces and the firewall table that the
earlier manager left. The backend's migration rewrites `/home/demi` to
`/home/jush` in each Cloud's stored home and in the paths that the Cloud's
projects, worktrees, workflows, conversations, pins and schedules keep,
and in each child agent's directory. A paired device whose user is named
`demi` keeps its paths: a row that names its device moves only a Cloud's.

**The backend's data.** Its migration also renames the plugin packages
recorded for each device, and moves the paths into a runner's installation,
`~/.demi/runner/…` and `~/.demi/instances/…`, such as a pinned document in
a conversation's scratch, to `~/.jush`. A schedule's command, which runs
unattended, takes the new paths and the variables a job gets, such as
`JUSH_SCRATCH`; a variable of the user's own keeps its name. Everything
else that holds the old name is history, such as transcripts and titles,
and stays as written.

**The browser.** The backend takes a `demi_session` cookie once and
answers with `jush_session`, so nobody signs in again. The web app moves
its `localStorage` keys from `demi.` and `demi-` to `jush.` and `jush-` as
it starts, and the drafts from the database `demi-drafts` to `jush-drafts`.
It deletes the offline copy and the plugins' cache under the earlier names,
which it fills again, and the service worker deletes its `demi-*` caches.

**Repositories.** Jush reads workflows and instructions from `.jush/` only.
The user renames `.demi/` in each repository, which is a change to commit
like any other.

**Agents.** A turn that starts after the update reads `jush` and
`JUSH_SCRATCH`. A conversation's earlier turns keep the old words, which
the model reads as history. A profile, a saved message or an instruction
the user wrote keeps the words the user wrote.

### The order

1. The rename and the migrations merge as one series.
2. The repository moves from `demi-sh` to `jush-ai` by GitHub's transfer, so
   the old address redirects. Nobody creates `demi-sh/demi` again, which
   would end the redirect.
3. The first Jush release publishes its image to `ghcr.io/jush-ai/jush`
   and the site to `jush.ai`.

## Spike

- **A directory renamed under a running job.** On macOS, a process running
  from a directory and serving a Unix socket in it went on after the
  directory's rename, and a client reached the socket by the new path. A
  job's shell records its endpoint in the system's temporary directory, so
  the next runner finds it after the move. On Windows 11, a directory whose
  only use was a running executable renamed; one in which a process held a
  file open, even with sharing for deletion, or worked, was refused with
  access denied. Hence a Windows device moves at a start that finds its
  installation free.
- **GitHub's redirect.** GitHub's documentation says a transferred
  repository's links redirect, the releases API's included. The transfer
  is followed at once by a check of the releases API, the feed and an
  asset's download at the earlier address, and a failed check transfers the
  repository back before any release.

## Prior art

- Homebrew renames a formula with `formula_renames.json` and moves a tap
  with `tap_migrations.json`
  ([docs](https://docs.brew.sh/Rename-A-Formula)): an installed copy follows
  the new name at its next update, as an installation here does.
- Biome, renamed from Rome, read `rome.json` for a while and offered
  `biome migrate` to rename it. Jush reads only `.jush/`, since a project
  with two names for one thing is what this project avoids.
- GitHub keeps a transferred repository's old address as a redirect, which
  the installed server relies on.

## Alternatives

- **Reinstall.** No migration code, but every server and device is set up
  and paired again, and the Clouds' paths and the browser's drafts are lost.
- **A bridge release.** A last Demi release that already looks for Jush's
  assets. It keeps the release's asset names clean, but an installation
  older than the bridge cannot upgrade directly, and the bridge must ship
  and reach every server first.
- **Keep the old names where nobody sees them.** No migration, but one
  concept would carry two names in the code forever.

## Open questions

None.
