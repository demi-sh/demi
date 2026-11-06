---
status: proposed
---

# A program's page shows a conversation with the web app's own views

## Problem

A program that runs Demi's agents, such as a bot's console, wants to show
a conversation as Demi shows it: the transcript as it streams, the
composer, the permission cards, and the conversation's work, the files,
the changes and the browser's live view, in its windows. Those views are
the web app's: they read and write through the web app's client at `/api`,
behind a Demi session, inside the web app's shell. A program would rebuild
them, and its copy would fall behind every change to Demi's.

## Decision

`@demicodes/web-components` defines a custom element, built from the web
app's own views, which talks to a core through the path where the program
forwards the core's routes (0301's `core.fetch` and `core.accept`):

```html
<link rel="stylesheet" href="/assets/demi-web-components.css" />
<script type="module" src="/assets/demi-web-components.js"></script>
<demi-conversation api="/demi/api" conversation="0193f1c2-..."></demi-conversation>
<script>
  document.addEventListener('navigate', (event) => showInConsole(event.detail.address))
</script>
```

```text
browser                          the program's server          demi-core
<demi-conversation api=/demi/api>
  GET /demi/api/conversations/x ---> its own sign-in check
                                     core.fetch(request) -------> core-api
  WS  /demi/api/connect ----------> core.accept(socket) -------> lanes
```

- `<demi-conversation>` is the web app's conversation view
  (`ChatPage.vue` over `ChatSession.vue`): the transcript, the composer
  and its cards, and the conversation's work in windows over and beside
  it, with the taskbar under them (0308), whose tabs are of the kinds the
  plugins' pages give, such as files, changes and the browser's live view.
  It has what the web app's window gives it around: the context menu, the
  toasts, the media viewer, Find in Conversation and the dialog that
  changes where it runs.
- The program checks who may see what before it forwards; the element
  carries no Demi session.
- Setting an element's `conversation` shows another conversation.

### One set of views

The elements are a second entry of the web app's build, not a copy:

- The web client's base path, `/api`, written once today in `apiUrl`
  (`api/client.ts`), which the requests and the page connection use,
  becomes `/api` in the web app and the elements' `api` attribute on a
  program's page (`program-page.ts`).
- The elements read the core's part of the product state (`CoreState`),
  which names none of the server's parts, such as the signed-in user. The
  data the web browser keeps for the page, such as drafts, which the web
  app keeps under the signed-in user, they keep under the core's `api`.
- One page of elements shares one set of stores, one router and one core,
  as the web app's window does: the first element that connects makes
  them, and an element with another `api` is refused. The router holds the
  address of the conversation the page shows, with the tab of its window
  in front, in memory.
- What the views ask of the app's shell becomes an event of the element:
  a link to another conversation, to a setting or to a file outside the
  conversation is a `navigate` event, whose `detail.address` is the
  address the web app would show, such as `/chat/0193f1c2-...` or
  `/settings/providers`, which the program handles. The page's first
  `demi-conversation` dispatches it, and it bubbles. The web app keeps
  doing what it does now.
- What the web app's window gives the views around the shown conversation,
  the plugins' pages, the media viewer, Send to Conversation, its work's
  tabs read and its read mark, moves out of `App.vue` into one function
  both use (`conversation/shown.ts`).
- The elements read the core's product state through the same
  synchronization lane, and register the same plugins' pages.
- The build is the web app's own in another mode (`vite build --mode
  elements`), which writes one module and one stylesheet, with what they
  load later beside them, into the package.

The elements are Vue's own custom elements (`defineCustomElement`), with
Vue and the views inside, so a page in any framework, or none, uses them.

They render in the page's own tree, not in a shadow root
(`shadowRoot: false`): their menus and dialogs go to the page's body
(`Teleport`, 46 times in `web-ui`) and keep their styles, as in the web
app. A `Teleport` to the body from such an element crashed in Vue 3.5.22
and was fixed by vuejs/core#13900, merged in September 2025; the web app
is on 3.5.38. Their styles and the page's meet: `web-ui`'s base styles
reach the page's own elements, and the page's rules can reach the views'.
A program keeps its rules to its own classes.

## Built on

| What the design uses | Where it is today | New |
| --- | --- | --- |
| The conversation view | `web/src/conversation/ChatPage.vue`, `web-ui/src/agent/ChatSession.vue` | in a custom element |
| The work's windows and taskbar | `web-ui/src/windows`, in `ChatSession.vue`, with the plugins' tab kinds | |
| The client's base path | `/api` in `apiUrl` (`api/client.ts`) | an attribute |
| Navigation | the web app's router, in `ChatPage.vue`, `navigation.ts`, `work.ts` | an event outside the app |
| The routes and sockets | `core.fetch`, `core.accept` (0301) | |
| A view rendered without the backend | the gallery's specimens of `ChatSession` | |

## Spike

None: the views run as they do in the web app, in the page's own tree,
against the same routes through the path the program forwards.

## Prior art

- **CopilotKit**: its React provider takes a `runtimeUrl`, such as
  `/api/copilotkit`, where the developer's server mounts the runtime's
  handler; the components talk only to that path.
- **Vercel AI SDK UI**: `useChat({ api: '/api/chat' })` talks to a route
  the developer writes.
- **Vue's `defineCustomElement`**: a Vue component as a standard custom
  element, used by any page.

Demi departs in shipping custom elements rather than components of one
framework, as the web app is Vue and a program's page may be anything.

## Alternatives

- **The views' components with props, as the gallery uses them.** A
  program would wire 36 props of `ChatSession` and the services the web
  app provides around it: the web app's store, written again.
- **Components for React and Vue.** Two more builds of the same views for
  two frameworks, and none for the others.
- **A second element for the work's windows and taskbar**, which the
  program places apart. The windows stand over the conversation as well as
  beside it, and an element cannot hold a window over another element: the
  windows would leave `ChatSession.vue` for a layout of their own, apart
  from the web app's.
- **The web app in a frame.** Its shell, sign-in and routes inside the
  program's page, with no way for the program to handle navigation.
- **A shadow root for each element.** The styles stay apart, but a menu
  or a dialog that goes to the body leaves the shadow root and its styles
  with it: each of the 46 would have to go into the shadow root instead.

## Open questions

- None.
