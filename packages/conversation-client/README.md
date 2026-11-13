# @jush-ai/conversation-client

The browser's client of Jush's conversation lane. It exports three things:

- `ConversationClient` opens a conversation, sends messages, edits, steers and
  aborts, and keeps the transcript current from the backend's patches.
- `createWebSocketTransport` carries its frames over anything shaped as a
  WebSocket, such as a conversation's lane of the page connection.
- `applyTranscriptPatches` applies a patch list to a transcript.

The client validates every frame the backend sends against the schemas of
[`@jush-ai/protocol`](../protocol/README.md) before it acts on the frame. It
refuses a frame over the backend's message limit locally, as the backend would
refuse it.

The following example opens a conversation and sends a message:

```ts
import { ConversationClient, createWebSocketTransport } from '@jush-ai/conversation-client'
import { openLane } from '@jush-ai/web-ui/transport/page-connection'

// Each connection opens the conversation's lane, and carries frames once it is open.
const client = new ConversationClient(() => new Promise((resolve) => {
  const lane = openLane({ kind: 'conversation', conversation: id }, 'conversation lane')
  lane.addEventListener('open', () => resolve(createWebSocketTransport(lane)), { once: true })
}))
client.subscribe((event) => {
  if (event.type === 'transcript_patch') {
    render(event.blocks)
  }
})
await client.open()
await client.submit([{ type: 'text', text: 'Fix the failing test.' }])
```

Jush has no TypeScript SDK. The client drives a conversation the backend runs.
It does not run Jush's agent itself
([ARCHITECTURE.md](../../ARCHITECTURE.md)).

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
For the text, see [LICENSE](../../LICENSE).
