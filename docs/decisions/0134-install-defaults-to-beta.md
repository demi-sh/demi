---
status: proposed
---

# Installing without a channel installs Beta

Supersedes: 0109 (A server takes whole releases of the channel it names),
the part "Installing names its channel": an install without a channel no
longer refuses, and neither does an upgrade.

## Problem

Alex reads Jush's website and runs the command it shows:

```sh
curl -fsSL https://jush.ai/install.sh | sudo bash
```

`install.sh` installs nothing and answers that Jush has no stable release
yet, with two longer commands to choose from. Every first install starts
with a failure, and the shortest command, the one a website and a README
show, cannot be the one that works.

## Decision

`install.sh` without `--beta`, `--canary` or `--version` installs Beta, the
newest release Jush publishes, as `--beta` does:

```sh
curl -fsSL https://jush.ai/install.sh | sudo bash                    # Beta
curl -fsSL https://jush.ai/install.sh | sudo bash -s -- --canary     # Canary
curl -fsSL https://jush.ai/install.sh | sudo bash -s -- --version 0.1.22
```

* `--beta` stays, and does what no channel does.
* `--canary` and `--version` are unchanged, and one of them still excludes
  the others.
* Without a channel, the script passes `--beta` on to `jush-server setup`,
  so Settings › Updates starts on Beta, as 0116 decides for `--beta`.
* While Jush has published no release, Beta has nothing to install: the
  script says so and points to `--canary`, as `--beta` does today.
* Other parameters go to `setup` as before, so
  `curl … | sudo bash -s -- --domain jush.example.com` installs Beta with
  that domain.
* The script's comment, `docs/delivery/installation.md`, the upgrade guide
  and the README in each of its languages show the bare command for Beta
  and `--canary` beside it, and no longer say that an install must name a
  channel.

`jush-server upgrade` without a channel or a version no longer refuses
either: it moves to the newest release of the server's update channel, the
one Settings › Updates shows, as `apt upgrade` follows the sources the
machine names:

```text
$ sudo jush-server upgrade
Moving along the server's update channel, Beta
```

* The channel is the one an administrator chose in Settings › Updates, the
  `server_settings` row, or else the one the installation came from,
  `JUSH_UPGRADE_CHANNEL` in the configuration, and Beta when that names
  none, as the backend reads them (0116).
* `jush-server` reads the row from the control database read-only, while
  the backend runs; a database or a table that does not exist yet holds no
  choice. The row's reading is one function of the database crate, which
  the backend's `ControlService::upgrade_channel` uses too.
* `--beta`, `--canary` and a version name the release as before.

`jush-server setup` already sets up its own release without a channel, and
is unchanged.

## Spike

None.

## Prior art

* Tailscale's `curl -fsSL https://tailscale.com/install.sh | sh` installs
  the stable track; `TRACK=unstable` chooses the other.
* Docker's `get.docker.com` installs the stable channel unless `CHANNEL`
  names another.
* k3s's `get.k3s.io` installs the stable channel unless
  `INSTALL_K3S_CHANNEL` names another.

Each installer defaults to the channel most people want and takes the other
as a parameter, which is what Jush does now with Beta.

## Alternatives

* Default to Canary: the newest build reaches a first install before anyone
  ran it on a server, and Beta is the channel Jush publishes.
* Keep refusing: the shortest command fails, as the problem says.
* `upgrade` without a channel moving to Beta: a Canary server would be
  told that Beta's newest is older than what it runs, though its
  administrator chose Canary.

## Open questions

None.
