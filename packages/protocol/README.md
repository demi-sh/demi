# @jush-ai/protocol

The TypeScript form of Jush's Rust contracts. It holds:

- The Zod schemas and types of the conversation lane's frames, the
  transcript, the content and the browser's live view
- The tables and lookups the page shares with the backend, such as the
  file-type table and `MAX_PAGE_MESSAGE_BYTES`

`bun run contracts` generates every file from the Rust contract crates. The
package has no hand-written source, so a Rust contract change is a change
of this package.

Parse a value from the network with its schema, and take its type from the
schema:

```ts
import { serverFrameSchema, type ServerFrame } from '@jush-ai/protocol'

const frame: ServerFrame = serverFrameSchema.parse(JSON.parse(message))
```

Jush has no TypeScript SDK. This package and
[`@jush-ai/conversation-client`](../conversation-client/README.md) are the
web app's client of the conversation lane, not a way to run Jush's agent in
another program ([ARCHITECTURE.md](../../ARCHITECTURE.md)).

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
For the text, see [LICENSE](../../LICENSE).
