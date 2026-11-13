---
status: proposed
---

# A skill source of any size can be fetched

Supersedes: 0148 (A skill source may be fetched on one of the user's
devices), the repository's limit (64 MiB) and the files' limit (16 MiB) of
a fetch, which no longer hold.

## Problem

Alex's team keeps about 300 skills in one repository. Their directories hold
about 50 MiB of files, much of it the same reference files copied under two
skills. Settings › Skills adds the repository, fetched on Alex's laptop, and
shows it as Failed: *The skills' files are larger than 16 MB.* Not one of
its skills reaches Alex's conversations, and nothing Alex can do in Jush
gets them there.

The limits are there because a fetch holds every file of a source in the
backend's memory at once. Nothing else needs them: the server keeps each
file as a blob of its own, one copy per content, and a device receives a
skill's directory through a stream.

## Decision

A source is fetched whatever its size, on the server and on a device. The
page never shows a size failure: *The repository is larger than 64 MB* and
*The skills' files are larger than 16 MB* are gone. A device receives every
skill that is on, as today; a user who wants fewer files on their devices
turns skills off.

A fetch stores the files as it reads them instead of holding all of them:

```text
 device git                     backend                    plugin
 ──────────                     ───────                    ──────
 tree listing ────────────────► paths of every file
 objects' sizes ──────────────► runs of at most 8 MiB
 run 1's objects ─────────────► store each as a blob ─┐
 run 2's objects ─────────────► store each as a blob ─┼──► paths + blob refs
 ...                                                  ┘    reads each SKILL.md
                                                           from its blob
```

The design to build from:

* **One shape for both fetchers.** A fetch answers the commit, every file's
  path, and the blob of each file in a directory a `SKILL.md` marks. The
  skills are read from that by one function for both fetchers; it reads
  each `SKILL.md` from its blob.

  ```rust
  pub struct GitTree {
      pub commit: String,
      pub entries: Vec<GitEntry>,
      /// The files that were asked for, by path, each as the blob that
      /// holds its bytes.
      pub files: Vec<GitFile>,
  }

  pub struct GitFile {
      pub path: String,
      pub blob: BlobRef,
  }
  ```

* **A device's fetch.** `PortMessage::DeviceGitFetch` loses
  `repository_max` and `files_max`, and `GitRefusal` loses
  `RepositoryTooLarge` and `FilesTooLarge`. The size check after the
  clone, `git count-objects`, goes. After the tree, one
  `git cat-file --batch-check` gives the wanted objects' sizes, and
  `git cat-file --batch` reads them in runs of at most 8 MiB, a larger
  object in a run of its own. Device access stores a run's objects in the
  user's blobs before it starts the next run, so the backend holds one
  run's bytes at a time. The 8 MiB bounds that memory, not what a source
  may hold: a 50 MiB source takes seven runs.
* **The server's fetch.** The fetch no longer counts the bytes it receives.
  One blocking task clones, lists the tree and reads each wanted object,
  and hands the objects to the shard over a channel of one, which stores
  each with `PutBlob`. The clone is in a directory under the backend's
  data directory, `tmp/skill-fetches`, which the backend empties when it
  starts, not in the system's temporary directory, which a server may keep
  in memory.
* **Reading the skills.** Each `SKILL.md` is read from its blob and parsed
  before the next; which files belong to which skill is worked out on the
  blocking pool.
* **Memory.** The backend holds at most one run's objects, or a few files
  on their way from the server's clone to the blobs, as a blob is stored
  whole. A file's size is the repository's own, not a limit of Jush's.
* **Time.** A device's `git` is still killed when it stops, but no longer
  after a total time a size was measured by. Each run of a fetch is killed
  after five minutes in which `git` wrote nothing, with the helpers it
  started: `SpawnRequest` gains `process_group`, which the runner's
  `kill_process_group` already serves, and `git` runs in a group of its own.
  A failure keeps what `git` had said before it went silent. The transfers
  run with `--progress` and without `--quiet`, which would hide the meter
  of the objects being received, so a slow transfer that still moves keeps
  writing: the clone is
  `git clone --progress --bare --depth 1 --single-branch --no-tags
  --filter=blob:none`, and the fetch of the objects
  `git fetch --progress --no-tags --no-write-fetch-head
  --recurse-submodules=no --filter=blob:none --stdin origin`. A failure
  shows what `git` wrote to its standard error after the last line it
  redrew, ending it with a carriage return, as it draws a progress meter,
  without the line that names the clone's directory, *Cloning into…* in
  the device's language. The check, `git ls-remote`, keeps its limit of 60
  seconds.
* **Left behind.** A fetch marks its directory's modification time every
  ten minutes while it runs, so a later fetch, which removes the fetches'
  directories an hour old, never removes one in use, however long it runs.
  Age, not the backend's memory, tells them apart, since every runner on a
  machine shares `~/.jush/tmp`.
* **Removal.** Removing a source stops its fetch, which saves nothing, and
  takes a device's `git` with it.
* **Stored data.** A migration turns a stored failure whose problem is
  `repository_too_large` or `files_too_large` into `failed`, with its
  message kept; the source fetches again on Update. `FetchProblem` loses
  both codes, the page loses both messages, and the gallery's failed source
  shows a failure in `git`'s words.
* **Older parts.** A runner meets nothing new: `git` runs through the same
  spawn. The page is served with the backend that answers it.

## Spike

* **Where the server's clone keeps the pack.** gix writes the pack it
  receives to a temporary file in the clone's directory as it arrives
  (`gix_pack::Bundle::write_to_directory`, gix-pack 0.74.2), so the
  server's memory does not grow with the repository; the limit's comment,
  *held in memory while they are unpacked*, was wrong.
* **Progress without a terminal.** A blobless clone of a public repository
  with `--progress` and its standard error sent to a file wrote 9.8 KB of
  progress, its updates separated by carriage returns, so a moving
  transfer writes output a stall can be measured by. With `--quiet` as
  well, only the remote's meters came, not *Receiving objects*, which runs
  while the bytes move.
* **A clone, not an init and a fetch.** `git init --bare`,
  `git remote add` and `git fetch origin HEAD` gave clean words, with no
  *Cloning into…*, but a SHA-256 repository then failed with
  *fatal: mismatched algorithms: client sha1; server sha256*, where a
  clone follows the remote's format; telling `git init` the format needs
  Git 2.29.
* **How long a connection takes to fail.** On Linux, `git ls-remote` of an
  address nothing answers failed after 133 seconds, *Failed to connect …
  after 132483 ms*, so a two-minute silence would have hidden those words.
* **The server's temporary directory.** A Linux server's `/tmp` may be a
  tmpfs, as Debian's is by default: one measured held 3.9 GB, on a machine
  with 7 GB of memory.
* **A directory's age.** The modification time of a fetch's directory did
  not change while the later fetch and reads wrote into `objects/pack/`, so
  a fetch longer than an hour would have looked left behind.
* **What a device's output waits in.** A spawned process's output waits in
  an unbounded queue (`Shared` in `crates/runner-client/src/link.rs`), and
  storing a blob on S3 is a round trip of its own. One stream of every
  object, stored as it arrives, would gather in that queue whenever the
  device sends faster than the backend stores, so the reads are runs whose
  size the backend picks.
* **A kill's reach.** A fetch from a server that never answered was killed
  after its silence, but its `git remote-https` and
  `git-remote-https` stayed, orphaned, in the runner's process group, as
  they did after the earlier total time limit, holding the connection on.
* **Copies.** A blob is named by the SHA-256 of its bytes (`BlobRef::of`),
  so a file copied under several skills is stored once.

## Prior art

* **Vercel's `skills` CLI** (`npx skills add`,
  [src/git.ts](https://github.com/vercel-labs/skills/blob/main/src/git.ts))
  clones a source with `git clone --depth 1` and no size limit. Its clone
  through simple-git is killed after five minutes without output, which
  is the rule this design takes for a device's `git`.

Jush departs in where the clone runs: the server or a paired device, not the
machine that uses the skills, so the files reach the devices as blobs.

## Alternatives

* **Raise the limits.** Any number fails the next larger source, and keeps
  the whole source in memory, which is the reason for a limit in the first
  place.
* **Keep a total time limit.** A large transfer that is still moving would
  be killed for its size by another name.
* **A limit per file.** The repository's own files bound a file; one file
  in memory is what storing it as a blob takes.
* **One `git cat-file --batch` stored as it streams.** Its output would
  wait in the process's unbounded queue whenever storing is slower than
  the device, as on S3, which brings back the whole source in memory.

## Open questions

None.
