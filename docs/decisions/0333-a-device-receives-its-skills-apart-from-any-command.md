---
status: accepted
---

# A device receives its skills apart from any command

Supersedes: 0147 (A skill source is Global or a workspace's), the part that
a Host receives a skill's directory when a job runs there.

## Problem

Alex's agent runs `git log` on his build machine. The machine's runner
reconnected a minute earlier, so Demi first checks the skills' directories
there, finds all 111 of Work's missing, and writes them at once. The machine
finishes one or two a second; the writes still waiting after 60 seconds fail,
the rest are cancelled, and `git log` never starts: it ends with exit status
127 and *the device did not finish writing
~/.demi/plugins/skills/api-case-generation-… within 60 seconds*. The agent
tries again, and each try writes the directories still missing and stops at
the same wall. For ten minutes no command runs, though nothing is wrong with
them; the skills, which `git log` never needed, decided it.

A command and a skill's files have nothing to do with each other: a skill is
files the agent may read, and a command runs whether they are there or not.

## Decision

The installation of the plugins' directories on a device is its own work,
which no command starts, waits for or fails with.

* **What a device holds.** Every Global skill that is on, now on each device
  of the user rather than each one a command ran on, and, as 0147 says, the
  skills of each workspace whose conversations run on the device, as their
  primary device, the Cloud included, or an attached one.
* **When it is brought there.** When the device connects, and when a
  plugin's set of directories changes or the plugin is turned on or off.
  The connected devices are checked again when a message of the user's
  arrives, so a conversation that moved to a device, moved to another
  workspace or attached one has its skills written while the model reads the
  message, and when a command starts; neither waits. Each brings the device
  to its set: the directories it lacks written, those no set names removed.
  A device that is offline is brought there when it connects; a Cloud is
  never woken for it.
* **A command never waits.** A command starts at once, whatever the
  installation is doing. A skill whose directory is not there yet is a path
  that does not exist for that moment, as a file the agent has not written
  yet is.
* **Each directory alone.** The directories are written at most 8 at a time:
  more only queue on the device's one connection, and a write waiting there
  runs out of time while it has not started. A directory that fails is
  written again after a pause that doubles from 5 seconds to at most 5
  minutes, while the device stays online; a check when a message arrives or
  a command starts does not end that pause, a connection or a changed set
  does. A directory that cannot be removed, such as one a process still
  uses, keeps no other from being written. Each failure is logged with its
  directory and why.

The design to build from:

* `backend-device-access` keeps one installer per device of a user, in place
  of `install_directories` on the job's path: `PluginInstalls` holds per
  device the connection and the revision it was last brought to, and runs at
  most one pass at a time, in a task of the shard that ends with it.
* The workspaces of a device come from the control store at each pass
  (`ControlService::device_workspaces`): those of the unarchived
  conversations whose target or attached device it is.
* `ProductPort` gains `directories_changed`, which the plugin host calls when
  a plugin's set changes or the plugin is switched.
* `device_commands.rs` and `shells.rs`, and the conversation lane when it
  takes a message, only check, which starts the installer when it is idle.
* Nothing changes on the runner or on the wire.

## Spike

On a development Cloud whose skills folder was made read-only, a source of
20 skills was added: every command after it started at once and exited 0,
each pass failed with *Permission denied* and was tried again after a
pause. Once the folder was writable, the next pass wrote all 19 left, at most
8 at a time. With a stale directory that could not be removed, a pass still
wrote the skill turned on, and passes came 5, 10 and 20 seconds apart while
commands ran in between.

Why the device in the incident finished only one or two directories a
second, about 10 MB together, is still not known: it needs the device's far
link through the CDN, which the development setup does not have, and a
development runner on a person's machine shares that person's home, whose
skills a pass would remove. With the installation off the command path, that
speed delays only the skills.

## Prior art

* **VS Code Remote** installs the workspace's extensions on the remote
  server when it connects, in the background; a terminal opens at once and
  never waits for them
  ([remote extensions](https://code.visualstudio.com/api/advanced-topics/remote-extensions)).
* **Claude Code** and **Codex** read skills from the local disk and install
  nothing on another machine, so they have no such step.

## Alternatives

* **Keep installing before a command, with the limit and the retries.** The
  command would still wait for files it does not need, and still fail when
  they cannot be written.
* **Every device holds every workspace's skills.** Simpler, but 0147 keeps
  Work's skills off the Mac only Personal conversations use.

## Open questions

None.
