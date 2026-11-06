// `@demicodes/web-components`: the web app's conversation view, with its
// work's windows and taskbar, as a standard custom element, a second entry
// of the web app's build, for a program's page in any framework, or none:
//
// ```html
// <link rel="stylesheet" href="/assets/demi-web-components.css" />
// <script type="module" src="/assets/demi-web-components.js"></script>
// <demi-conversation api="/demi/api" conversation="..."></demi-conversation>
// ```
//
// It reaches one core through `api`, where the program's server forwards
// the core's routes and page connection (`@demicodes/sdk`'s `core.fetch`
// and `core.accept`), and the elements of a page share its stores
// (`page.ts`). It renders in the page's own tree, without a shadow root, so
// its menus and dialogs, which go to the body, keep the styles the
// stylesheet gives.

import { defineCustomElement, type App } from 'vue'
import ConversationElement from './ConversationElement.vue'
import { elementsApps } from './page'
import '../style.css'

/** Installs the page's stores, router and text in an element's own app. */
function configureApp(app: App): void {
  const { pinia, router, i18n } = elementsApps()
  app.use(pinia).use(router).use(i18n)
}

export const DemiConversation = defineCustomElement(ConversationElement, { shadowRoot: false, configureApp })

if (!customElements.get('demi-conversation')) {
  customElements.define('demi-conversation', DemiConversation)
}
