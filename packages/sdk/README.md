# @demicodes/sdk

Demi's agents in your program. The SDK starts `demi-core`, the process your
program's cores run in, and gives your program the cores, their
conversations, providers and devices, and plugins of your own whose commands
the agents run as `demi <group> <command>`.

A core is one Demi: its conversations, provider entries, devices and
settings, kept in a directory you choose. A team bot, for example, opens one
core for each team it serves.

## Install

```sh
npm install @demicodes/sdk zod
```

The package installs the executables of your machine's platform, macOS,
Linux or Windows on x86-64 or ARM64, from its optional platform packages,
such as `@demicodes/sdk-linux-x64`. It runs on Node.js 22 or later, Bun and
Deno. `zod` defines your plugins' command arguments.

## Quick start

The following program opens a core, gives it an Anthropic API key and a
device on your machine, and runs one turn:

```ts
import { Demi } from '@demicodes/sdk'

const demi = await Demi.start()
const core = await demi.open('./data/team-a')

const provider = await core.providers.create({
  source: 'vendor',
  vendorId: 'anthropic',
  apiKey: process.env.ANTHROPIC_API_KEY,
})
const device = await core.devices.create()
await device.spawn()

const conversation = await core.conversations.create({
  title: 'Release notes',
  model: { providerId: provider.id, modelId: 'claude-sonnet-4-5' },
  target: { kind: 'device', deviceId: device.id, path: '/srv/work' },
})
const turn = await conversation.run('Draft the notes for this week from the git log')
console.log(turn.outcome, turn.text)

await demi.close()
```

`turn.outcome` is `finished`, `failed` or `stopped`; `turn.text` is the text
the agent wrote after your message, and `turn.blocks` everything it did.

## Cores

`Demi.start()` starts one `demi-core` process for your program, and
`demi.open(dir)` opens a core in it, creating the core when `dir` is absent
or empty. A process holds any number of cores. Open the same directory again
later, and the core has everything it had: conversations, provider entries,
devices and settings.

A core is open in one process at a time; opening it in a second fails with a
`DemiError` whose `code` is `core_open`.

`core.close()` closes one core, and `demi.close()` closes every core and ends
the process. A turn still running when its core closes stops there, and Demi
takes it up again when the core opens again.

`demi-core` and the runners the SDK starts write their logs to your
program's standard error.

## A process of its own

`Demi.start()` ties the process to your program: when your program stops,
for example to deploy a new version, every core closes, and a running turn
stops until the core opens again. To keep the cores running while your
program restarts, or to share them between several services, run
`demi-core` on its own, as a server:

```sh
export DEMI_CORE_TOKEN=7f3c9e2a51d84b06
export DEMI_RELEASE=node_modules/@demicodes/sdk/release:node_modules/@demicodes/sdk-linux-x64/release
node_modules/@demicodes/sdk-linux-x64/bin/demi-core serve --listen 127.0.0.1:4100
```

`demi-core` is in your platform's package. `DEMI_RELEASE` lists the
`release` directories of the SDK and of each platform package, as `PATH`
does, so the cores' devices run Demi's commands. Each of your services
connects with the same token:

```ts
const demi = await Demi.connect({ url: 'http://127.0.0.1:4100', token, plugins: [chat] })
const core = await demi.open('/srv/bot/data/team-a')
```

- Opening a directory the process holds already answers the same core, with
  its conversations and turns as they went on while your program was away.
- `demi.close()` disconnects and closes nothing; `core.close()` closes the
  core for every service.
- Each connection declares its plugins again, and the last declaration of a
  plugin's id stands: a new version of your program changes the commands
  of every conversation at once.
- `SIGTERM` or Ctrl-C stops the process, which closes every core. Your
  program connects again by itself once the process is back, declares its
  plugins and opens its cores again, and goes on.

Set `DEMI_PUBLIC_URL` to where the process is reached from outside, such as
`https://bot.example.com/demi`, for schedules' URLs and plugins' sign-ins,
which the process then answers itself.

A runner reaches a process of its own as it reaches a Demi server: start it
anywhere with `demi-runner run --backend http://bot.example.com:4100`. It
prints a pairing code and waits; pair it to a core with
`core.devices.claim(code)`. The runner keeps its token, and connects again
by itself after it or the process restarts.

## Providers

A provider entry is what a core's agents call models with. Create one with
an API key of a vendor, such as `anthropic`, `openai` or `google`:

```ts
const openai = await core.providers.create({ source: 'vendor', vendorId: 'openai', apiKey: process.env.OPENAI_API_KEY })
```

Or with an endpoint that speaks a vendor's protocol, such as a gateway or a
model server of your own; `providerType` is the protocol, `anthropic` when
absent:

```ts
const gateway = await core.providers.create({
  source: 'compatible',
  providerType: 'openai',
  label: 'Team Gateway',
  baseUrl: 'https://gateway.example.com/v1',
  apiKey: process.env.GATEWAY_KEY,
})
```

The models each entry offers are in the core's state, as
`core.state.models.providers`, each with its `providerId` and `models`.
`core.providers` also lists, updates and removes entries.

### Subscriptions

A subscription entry signs in with an account instead of a key: `codex` for
ChatGPT, `claude-code` for Claude, or `grok-build` for Grok. The user
confirms the sign-in in a browser. Its stages are in `core.state.logins`:

```ts
const signIn = await core.providers.signIn({ providerType: 'codex' })
const stop = core.on('state', (state) => {
  const login = state.logins.find((each) => each.id === signIn.id)?.login
  if (login?.status === 'pending' && login.verificationUrl !== null) {
    showToUser(`Open ${login.verificationUrl} and enter ${login.userCode}`)
  } else if (login !== undefined && login.status !== 'pending') {
    stop()
  }
})
```

When a login's `needsCode` is true, the user signs in at the address
instead, and you hand the code the page shows to `signIn.submitCode(code)`.
`signIn.cancel()` stops it.

An entry can hold several accounts: `core.providers.accounts(id)` signs in
another one, and lists, activates, orders and removes them. Once an entry
holds two or more, its `rotation` says which account each agent uses:
`manual` the active one, `in_order` the first in their order that is not used
up, and `smart` first the one whose quota resets soonest, since what is left
at a reset is lost:

```ts
await core.providers.update(entry.id, { rotation: 'smart' })
```

## Devices

A device is a machine the agents run commands on, served by a
`demi-runner`. `device.spawn()` starts the package's own runner on your
program's machine. Give it a command to start a runner elsewhere, such as in
a container; each device's runner keeps its state in a directory of its own,
`--home`:

```ts
const box = await core.devices.create()
await box.spawn(['docker', 'exec', '-i', 'box-7', 'demi-runner', 'run', '--stdio', '--home', `/var/lib/demi/${box.id}`])
```

`spawn()` resolves once the core counts the device online, so a conversation
can be created on it, and answers the runner's child process. It fails when
the command exits before, or when no runner comes on its channel within 30
seconds, and then ends the command. The core counts a device online for a
short while after its runner drops, and after the core opens, if its runner
was connected when the core closed, waiting for the runner to come back;
`spawn()` of such a device resolves at once, so watch the child process for a
runner that exits.

A runner that reaches the core some other way, such as over SSH or a socket,
connects with `device.connect({ readable, writable })`, the standard output
and input of a runner already started. It resolves as `spawn()` does, with
`ended`, which resolves once either end closed.

A core keeps its devices, but not their runners: when your program starts
again, start a runner for each device the core lists:

```ts
for (const listed of core.state.devices) {
  await core.devices.of(listed).spawn()
}
```

## Conversations

`core.conversations.create()` takes the conversation's `title`, `model` and
`target`, the device and directory its commands run in. A conversation
without a target runs on the Cloud, which only Demi's server runs, so in a
program it is refused with `cloud_unavailable`, as one on a device that does
not exist is with `device_not_found`. `core.conversations.open(id)` takes a
conversation created earlier, and `conversation.update(patch)` changes the
same fields later.

Each conversation's browser has a profile of its own, so a sign-in made in
one conversation is not seen in another. Conversations that name the same
`browserProfile` on one device share its browser instead: a site one of them
signs in to is signed in for the others, and each keeps its own tabs. The
profile is the directory `~/.demi/browser-profiles/<name>` on the device,
made at its first use; list, copy or remove it as any directory.

```ts
const triage = await core.conversations.create({ title: 'Triage', model, target, browserProfile: 'support' })
const refunds = await core.conversations.create({ title: 'Refunds', model, target, browserProfile: 'support' })
// Back to a profile of its own, from its next browser command on:
await refunds.update({ browserProfile: null })
```

An agent introduces itself as Demi unless its conversation names an
`identity`, the sentences that open its system prompt instead. Demi's rules
for running commands and replying still follow them. `plugins` lists which
of your program's plugins the conversation's agents have, by id; without it
they have all of them. Demi's own plugins follow the user's switches either
way. A change made with `update` reaches each agent from its next request.

```ts
const desk = await core.conversations.create({
  model,
  target,
  identity: "You are the help desk's assistant. You answer the team's questions about their accounts.",
  plugins: ['tickets'],
})
// Try the next version of the commands in this conversation alone:
await desk.update({ plugins: ['tickets-next'] })
```

`conversation.run(input)` sends a message and resolves once the turn it
starts ended. It fails when the message leaves the queue without running, as
an abort that clears the queue makes it, or when the conversation closes
first. The other actions are the ones the app has:

| Method | What it does |
| --- | --- |
| `send(input)` | Sends a message, queued behind a running turn; resolves once the conversation holds it |
| `steer(input)` | Hands a message to the running turn; resolves once the turn took it |
| `abort()` | Stops the running turn |
| `retry()` | Runs the latest turn again from your message |
| `resume()` | Finishes a turn that did not finish |
| `compact()` | Summarizes the conversation so far, to free its context |

A message is text, or content blocks, such as a file on one of the core's
devices:

```ts
await conversation.run([
  { type: 'text', text: 'Summarize this report' },
  { type: 'remote_file', deviceId: device.id, path: '/srv/work/report.md' },
])
```

`conversation.blocks` is what the conversation holds, oldest first.
`conversation.events()` yields everything that happens in it, as the app
shows it live; a `transcript_patch` event says the blocks changed, such as
while the agent writes:

```ts
for await (const event of conversation.events()) {
  if (event.type === 'transcript_patch') {
    render(conversation.blocks)
  }
}
```

The loop ends when you leave it or the conversation closes.
`conversation.close()` stops following the conversation; its agents go on in
the core, and the next action follows it again.

`conversation.sendToAgent(node, text)` sends a message to one subagent, named
by its id in the conversation's agents. A subagent at work takes it as a
steer, an idle one starts a turn with it, and one that ended is resumed with
it. Give the message an id when you may send it again, such as after a lost
answer: the subagent takes a message once, however often it arrives.

```ts
await conversation.sendToAgent(node, 'The build finished.', 'reminder-7')
```

## Permission requests

A command that acts on Demi itself, such as `demi credentials`, which hands
the agent a stored secret, or one of your plugin's commands that declares a
permission, waits for the user's permission in the conversation. `events()` yields a `permission`
event for each request, and `conversation.permissions` lists those waiting:

```ts
for await (const event of conversation.events()) {
  if (event.type === 'permission') {
    await event.permission.allow()
  }
}
```

`deny()` refuses it. A request for a credential takes the values it asks
for: `allow({ values: { token: 'xoxb-example' } })`.

## Plugins

A plugin gives the agents commands that your program runs. The following
plugin adds `demi chat send`, which posts to the chat a conversation answers
in:

```ts
import { Demi, definePlugin, leaf } from '@demicodes/sdk'
import { z } from 'zod'

const chat = definePlugin({
  id: 'chat',
  name: 'Chat Commands',
  description: 'Gives the agent `demi chat send`, which posts to the chat.',
  commands: {
    chat: {
      summary: 'The chat this conversation answers in',
      index: 'Posts to the chat this conversation answers in.',
      commands: {
        send: leaf({
          summary: 'Posts a message to the chat',
          input: z.object({ text: z.string().describe('The message, in Markdown') }),
          positionals: ['text'],
          output: 'the id of the message posted',
          async run({ text }, call) {
            const id = await postToChat(call.invocation.context.conversation, text)
            await call.stdout(`${id}\n`)
          },
        }),
      },
    },
  },
})

const demi = await Demi.start({ plugins: [chat] })
```

Every core of the process gives its agents the plugin's commands. The agent
learns them as it learns Demi's own: `index` is the group's entry in the
index of capabilities the agent reads, which says what the group is for, and
each command's `summary`, input and `output` are its `--help`.

- Each field of `input` is an option of the same name, such as `--text`, and
  its `describe` text is the option's help; `positionals` lists the fields
  given by their place, in order. Arguments that do not fit the schema fail
  before `run` is called.
- `run(input, call)` writes the command's output with `call.stdout()` and
  `call.stderr()`, and reads its standard input with `call.readStdin()` when
  the leaf names a `stdin` field. What it returns is the exit status, 0 when
  it returns nothing; a failure writes why to standard error and returns a
  status other than 0, as a shell command does.
- `call.signal` aborts when the agent stops the command.
- `jsonOutput` declares what the command's `--json` writes.
- A group's `permissions` declares categories, and a leaf's `permission`
  names the one it needs: the conversation then asks before the command
  runs.

A plugin can also add a context block before each request (`context`),
search a conversation's project (`projectSearch`), and follow conversations
being deleted or forked.

A command your program runs outlives your program's restart. If your
program stops while `run` is under way, the command waits, and once your
program serves the plugin again, `run` is called again for the same call,
with `call.resent` true. `call.request` is the call's number, which no other
call ever takes: use it to do the work once, for example when the command
posts a message. What the command already wrote stays in its output, and
what `run` writes now follows it; input the earlier call read is not read
again. While no program serves the plugin, a new command waits 10 seconds
for one, and then fails, telling the agent that the plugin's program is
unreachable. A `context` call does not wait: when your program stops
during one, the agent goes on without its block.

## Product state and notifications

`core.state` is the core's product state: its workspaces, projects, provider
entries and their models, devices, conversations and sign-ins, as the app
shows them. `core.on('state', listener)` calls the listener at each change,
and answers a function that stops it.

`core.on('notify', listener)` receives what the app would notify the user
of: a turn that finished or failed, a permission request, or a notification
the agent sent with `demi notify send`. A program has one such listener for
each core, and a new one replaces it; when several services listen to one
core, the one that listened last receives them. Answer how you delivered
it; the agent's `demi notify send` reports it:

```ts
core.on('notify', async (message) => {
  if (message.kind === 'agent') {
    await postToChat(message.conversationId, `${message.title}\n${message.body}`)
    return { type: 'sending', to: 1 }
  }
  return { type: 'nowhere' }
})
```

`{ type: 'shown' }` says every place that would show it shows its
conversation already.

## Showing conversations on your pages

[`@demicodes/web-components`](../web-components/README.md) shows a
conversation on your own pages with the app's own views: its transcript,
its composer, and its work in windows with a taskbar.
Your server forwards the pages' requests with `core.fetch(request, base)`
and their connection with `core.accept(socket, url)`, which takes a
WebSocket of the standard interface, as Deno's `upgradeWebSocket` and the
`ws` package give.

Demi can reach your program from outside when you tell it where your server
forwards its routes: `Demi.start({ publicUrl: 'https://bot.example.com/demi' })`.
A schedule's URL is then `/hooks/{id}` below it, which your server answers
with `demi.fetch(request, '/demi')`, and a plugin's sign-in returns there.

## Errors

A request the core refuses fails with a `DemiError`: `status` is the HTTP
status, `code` the refusal's code, such as `core_open`, and `message` says
why in words. `requestId` names the request in the core's log.

Part of [Demi](../../README.md). The package is under the Apache License 2.0.
