# @demicodes/web-components

Demi's conversation view and work panel as custom elements, the web app's own
views, for a program's page in any framework, or none. They talk to a core
through the path where the program's server forwards it with
[`@demicodes/sdk`](../sdk/README.md).

The following page shows a conversation and its work panel:

```html
<link rel="stylesheet" href="/assets/demi-web-components.css" />
<script type="module" src="/assets/demi-web-components.js"></script>
<demi-conversation api="/demi/api" conversation="0193f1c2-..."></demi-conversation>
<demi-work-panel api="/demi/api" conversation="0193f1c2-..."></demi-work-panel>
```

The program's server forwards what the elements ask:

```ts
if (url.pathname === '/demi/api/connect') {
  const { socket, response } = Deno.upgradeWebSocket(request)
  core.accept(socket, request.url)
  return response
}
if (url.pathname.startsWith('/demi/api/')) {
  return core.fetch(request, '/demi/api')
}
```

- A link to another conversation or to settings is a `navigate` event with
  the address the web app would show, such as `/chat/0193f1c2-...`; setting
  an element's `conversation` shows another conversation.
- The work panel's Close dispatches `close`.
- The elements render in the page's own tree, without a shadow root, and
  their stylesheet also styles the page's own elements.

Part of [Demi](../../README.md). The package is under the Apache License 2.0.
