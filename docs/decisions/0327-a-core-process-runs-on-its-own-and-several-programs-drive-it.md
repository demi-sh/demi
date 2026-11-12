---
status: proposed
---

# A core process runs on its own, and several programs drive it

Supersedes:

- 0299 (A program's plugins run in the program, over a lane of each
  core's connection), the parts that fix the plugins when the first core
  opens and let only the program that opened a core serve its plugins.
- 0304 (A program opens cores in a process of its own and drives them with
  the web API, over HTTP/2 on stdio), the part that ends the process with
  its program's connection, for a process that serves.

## Problem

A program deploys its service several times a day. Each deploy stops the
service, which ends `demi-core`'s standard input, so every core closes:

- Every running turn stops, and resumes at the next open with its model
  request made again.
- A command a plugin of the program was running fails.

A change to a plugin's commands, a new command or a new argument, is
such a restart too, since a process takes the program's plugins only before
its first core opens (0299).

A second service of the same program, such as one that sends scheduled
reminders into the same conversations, cannot reach them: a directory opens
in one process only, and only that process's program drives it.

## Decision

The process can run on its own, as a server does. Programs connect to it
and disconnect without closing anything, several at once, and declare
their plugins at any time. A program that starts its own process over
stdio (`Demi.start`) works as before, and both run the same routes.

```text
service A ---+                          +--- runner (device 1)
service A'---+--- demi-core serve ------+--- runner (device 2)
service B ---+    the cores, their      |
                  conversations and     |
                  turns go on while     |
                  any program restarts  |
```

### Serving

`demi-core serve --listen 127.0.0.1:4100` runs until it is stopped (`SIGTERM`
or Ctrl-C), which closes every core as a server's stop closes its users'.

- `DEMI_CORE_TOKEN` is the token every program presents, as
  `Authorization: Bearer`; the process does not start without one.
- `DEMI_PUBLIC_URL` is where it is reached from outside, for a schedule's
  URL and a plugin's sign-in, which it serves itself.
- `DEMI_RELEASE` names its release, as for a process over stdio.

The listener takes HTTP/1.1 and HTTP/2 in cleartext, as a server does
behind a proxy that terminates TLS:

| Who | Routes | Credential |
| --- | --- | --- |
| A program | the process's: `/plugins`, `/cores`, `/cores/{id}/api/...` | the process's token |
| A runner | a runner's, as on a server: `/api/runner`, its pipes, outputs and executables | its device's token, or none while it waits to be paired |
| Anyone | `/hooks/{id}`, the files a page's frames load | none, as on a server |
| Anyone | a plugin's sign-in callback | none: the sign-in's `state` names its core |

### Several programs

- Each program is a connection, and its end closes nothing.
- `POST /cores { dir }` for a directory the process has open answers its
  id, 200, so a program that reconnects opens its cores as it did at its
  start. A directory another process holds answers 409, as today.
- `DELETE /cores/{id}` closes the core, for every program.
- An open, a close, a declaration or a removal runs to its end once begun,
  also when its program goes away before the answer.
- The SDK's `Demi.connect({ url, token, plugins })` connects to a process;
  its `close()` disconnects, and closes no core. A connection that drops
  is made again, and begins by declaring the program's plugins and opening
  its cores, so a process that restarted serves the program as before.
  Until a program connects again, the process knows none of its plugins,
  and their commands are unknown.

### Plugins

- `PUT /plugins/{id}` declares a program's plugin, and `DELETE
  /plugins/{id}` removes it and closes its lanes, at any time; they replace
  `PUT /plugins`. The last declaration of an id stands.
- A declaration changes the commands of every live conversation at once,
  and leaves the list in their prompts until each next opens (0008). A
  removed plugin's commands leave at once, as when the user turns a plugin
  off.
- A core sends each request of a plugin on the newest open lane of that
  plugin (`{ kind: "plugin", plugin }`, 0299), whichever program opened it.
  A program that restarts opens its lanes again, and the requests go to the
  new ones.
- While no lane of the plugin is open, a new request waits as long as a
  device whose connection dropped counts as online, 10 seconds
  (`RunnerTuning::returning`), and then fails, telling the model that the
  plugin's program is unreachable. A context request does not wait: the
  agent asks its model without the plugin's block, also when the lane of
  one in flight closes and no other is open.
- A request in flight when its lane closes stays. Once a lane of the plugin
  opens, the request is sent on it again, under the same number, with
  `resent: true`; its port messages and its reply then come from that lane,
  and a stop's cancellation goes to it.
  - It has no deadline but the agent's stop, as while its program is
    connected.
  - A request whose reply came is over, and goes to no lane again.
  - Output the request wrote stays, and what it writes on the new lane
    follows it. Input it read is not read again: input a lane did not
    receive, and a read under way when its lane closed, answer its next
    read, in order.
  - The program makes its work idempotent by the number. Numbers count from
    the clock's milliseconds times 1000, below 2^53, so no other request
    takes a number, also after the process restarts.
- A core's notifications go on the newest open `starter` lane.

### Runners

- A runner dials the process as it dials a server: `demi-runner run
  --backend http://core.example.com:4100`. One not paired yet waits under
  the code it prints; a program pairs it to a core with
  `POST /cores/{id}/api/devices/claim { code }`, the route a page pairs one
  with. The device is made in that core, and the runner keeps its token.
- The core keeps the token's hash in the device's record, and the process
  finds a runner's device among its open cores, as it finds a schedule's.
- The process refuses no token: it cannot tell a removed device from one
  whose core is not open yet. A token no open core holds closes the socket,
  and the runner dials again, as after a dropped connection. A device
  removed while its runner is connected is revoked, as on a server.
- A channel a program gives (0305) still works, and ends with that
  program's connection; the device is then returning until a runner
  connects again.

### Build

- `crates/core-api/src/process`: the listener beside the stdio connection,
  each route's credential, and `POST /cores` answering an open core.
- The plugin registry (`crates/plugin-host/src/registry.rs`) takes, replaces
  and removes a program's plugin at any time, checked as at the start, and
  the toolset's revision names each program plugin's declaration. After a
  change, every open core adopts the toolset, as after the user's switch
  (`AgentServer::adopt_toolset`).
- A core's plugin lanes are kept per plugin, newest first, and a request in
  flight outlives its lane.
- Pairing (`crates/backend-runners/src/claims.rs`) moves into code both
  run. The server records a token's routing in its control database, as
  today; the process in the core's device record.
- The SDK's `Process` speaks HTTP/2 over a TCP connection as over the
  child's pipes.

## Spike

None.

## Prior art

- **Docker**: one daemon, and any number of clients over its socket's HTTP
  API; a client's end stops nothing.
- **OpenCode**: `opencode serve` runs headless, and its TUI, web app and
  SDK clients (`createOpencodeClient({ baseUrl })`) attach to it, with a
  password the server is started with.
- **Temporal**: several worker processes serve one task queue, each
  restarting on its own; a task waits while no worker polls it, and a
  worker's new build takes the tasks once it starts. Demi gives a request
  to the newest lane, not to any, as the newest program runs the newest
  code.
- **The Model Context Protocol**: a server changes its tools while connected
  and says so (`notifications/tools/list_changed`).

## Alternatives

- **A process per program, its conversations moved between them by
  closing and opening the directory.** Every move stops the running turn,
  and each directory keeps its own devices, providers and accounts.
- **Several processes on one directory, with a lock per conversation.**
  `core.sqlite` holds every conversation's devices, accounts and schedules,
  and the processes would have to keep it in step.
- **Only a channel the program gives (0305).** It ends with the program.
- **A request to any open lane.** Two releases of a plugin would run one
  conversation's commands by turns.

## Open questions

None.
