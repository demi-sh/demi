# @jush-ai/utils

Small generic helpers that Jush's browser packages share:
`@jush-ai/conversation-client`, `@jush-ai/web-ui`, and the web application
and its component gallery.

Each is a pure function or a small primitive with no dependency and no domain
type, so a package imports it rather than writing its own copy.

## Modules

The package has these modules:

- **async**: `deferred` and its `Deferred`, `delay`, `waitFor`, and
  `SerialQueue`, which runs one task at a time in the order they were queued
- **errors**: `asError`
- **guards**: `nonEmptyString`, `numberOrNull`
- **strings**: `clamp`, `truncate`, `sliceHead`
- **bytes**: `utf8Bytes`
- **reorder**: `moveBefore`, a list with one item moved before another
- **id**: `createId`

Import a helper from the package root:

```ts
import { createId, deferred, truncate } from '@jush-ai/utils'
```

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
For the text, see [LICENSE](../../LICENSE).
