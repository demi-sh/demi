// `@demicodes/sdk`: Demi's agents in a program. It starts `demi-core`, the
// process a program's cores run in, speaks HTTP/2 to it on its standard input
// and output, and gives the program its cores, their conversations,
// providers, devices and product state, and the program's own plugins, with
// the clients the web app uses.
//
// ```text
// public API    Demi, Core, Conversation, definePlugin        this package
// clients       PageConnection, ConversationClient,           shared with
//               the product state's withPart, the schemas     the web app
// wire          HTTP/2 on the process's stdio                 process.ts
// ```
//
// ```ts
// const demi = await Demi.start({ plugins: [chat] })
// const core = await demi.open('/srv/bot/team-a')
// const provider = await core.providers.create({ source: 'vendor', vendorId: 'anthropic', apiKey })
// const device = await core.devices.create()
// await device.spawn()
// const conversation = await core.conversations.create({
//   model: { providerId: provider.id, modelId: 'claude-sonnet-4-5' },
//   target: { kind: 'device', deviceId: device.id, path: '/srv/bot/work' },
// })
// const turn = await conversation.run('Draft the notes for this week')
// ```
//
// The SDK writes no client of its own: what a page and a program both need
// is in `@demicodes/protocol` and `@demicodes/conversation-client`.

export { Demi, type DemiOptions } from './demi'
export {
  Core,
  Conversations,
  Device,
  Devices,
  Providers,
  type AcceptedSocket,
  type ConversationOptions,
  type Notified,
  type ProviderAccounts,
  type SignIn,
} from './core'
export { Conversation, type ConversationEvent, type Input, type Permission, type Turn } from './conversation'
export {
  definePlugin,
  leaf,
  PortFailure,
  type CommandCall,
  type GroupDefinition,
  type LeafDefinition,
  type Plugin,
  type PluginDefinition,
  type PluginPort,
} from './plugin'
export { DemiError } from './routes'
