// The program's page Demi's elements run in: what every element of the
// page shares, as the web app's window shares its own (`main.ts`), made by
// the first element that connects.
//
// - One core: the elements reach it below their `api`, where the program
//   forwards its routes, through the page's one connection.
// - One store of each kind, one router and one text instance, which each
//   element's own Vue app installs.
// - One shown conversation, the leading element's: the first
//   `demi-conversation`. The router's address is its own, with the tab of
//   its window in front in the query. A navigation elsewhere, a link to
//   another conversation or to settings, is the program's to carry out: the
//   leading element dispatches a `navigate` event with the address the web
//   app would show, and the router stays.
// - What the web app's window has once around its views, its menus, toasts,
//   media viewer and dialogs, the leading element has (`PageHosts.vue`).
// - The user's attention to the page, which marks the shown conversation
//   read, and the file tree's choice, which the user's preferences keep, as
//   the web app's window follows them.
//
// ```text
// <demi-conversation api=/demi/api>  --\                  /--> GET /demi/api/state
//                                       +-- one page ----+---> WS  /demi/api/connect
// <demi-conversation api=/demi/api>  --/  (stores, router) \--> navigate event
// ```

import { effectScope, ref, shallowReadonly, shallowRef, type Ref } from 'vue'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { configurePageConnection } from '@demicodes/web-ui/transport/page-connection'
import { createPageI18n, setPageLanguage } from '@demicodes/web-ui/i18n/text'
import { lastShownLanguage, resolveLanguage } from '@demicodes/web-ui/i18n/languages'
import { apiUrl } from '../api/client'
import { useConversations } from '../conversation/store'
import { keepFileTree } from '../conversation/file-tree'
import { shownConversation, type ShownConversation } from '../conversation/shown'
import { useWork } from '../conversation/work'
import { programPage, showInProgramPage } from '../program-page'
import { SETTINGS_ROUTE } from '../settings/address'
import { followAttention } from '../state/attention'
import { followAppearance, followLanguage } from '../state/look'
import { usePreferences } from '../state/preferences'
import { useProduct } from '../state/product'
import { useResources } from '../state/resources'

/** What the page's elements share before any of them knows its core. */
export interface ElementsApps {
  pinia: Pinia
  router: Router
  i18n: ReturnType<typeof createPageI18n>
}

/** The page's elements together, once one connected. */
export interface ElementsPage extends ElementsApps {
  shown: ShownConversation
  /** The element whose conversation the page shows. */
  leader: Readonly<Ref<HTMLElement | null>>
  /** `element` shows the conversation `id`, which the page shows while it leads. */
  show(id: string, element: HTMLElement): void
  /** `element` left the page. */
  leave(element: HTMLElement): void
}

/** The route of every address: the elements draw their views themselves, and the router holds the address. */
const NOWHERE = { render: () => null }

let apps: ElementsApps | null = null
let page: ElementsPage | null = null

/** What each element's app installs, made once. */
export function elementsApps(): ElementsApps {
  if (apps) {
    return apps
  }
  const languages = navigator.languages
  const start = lastShownLanguage() ?? resolveLanguage('system', languages)
  const i18n = createPageI18n(start)
  setPageLanguage(start)
  const pinia = createPinia()
  setActivePinia(pinia)
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/chat/:id?', component: NOWHERE },
      { path: '/settings/:section?/:detail?', name: SETTINGS_ROUTE, component: NOWHERE },
      { path: '/:pathMatch(.*)*', component: NOWHERE },
    ],
  })
  apps = { pinia, router, i18n }
  return apps
}

/**
 * The page, made by the first element that connects with `api`, where the
 * program forwards its core's routes. Every element of a page reaches the
 * same core: another `api` is refused.
 */
export function elementsPage(api: string | undefined): ElementsPage {
  if (api === undefined) {
    throw new Error('Demi\'s elements need their `api` attribute: where the program forwards its core\'s routes, such as /demi/api')
  }
  if (page) {
    if (programPage()?.api !== api.replace(/\/+$/, '')) {
      throw new Error(`Demi's elements of a page reach one core: this one names ${api}, another ${programPage()?.api}`)
    }
    return page
  }
  const { pinia, router, i18n } = elementsApps()
  showInProgramPage(api)
  // The program's server decides who reaches the core: a refused upgrade is
  // its answer, which the connection's state shows, with no session to check.
  configurePageConnection({ url: () => apiUrl('/connect'), failedToOpen: () => {} })
  const shownId = ref<string | null>(null)
  const leader = shallowRef<HTMLElement | null>(null)
  /** The page's elements in the order they connected, each with its conversation. */
  const elements = new Map<HTMLElement, string>()
  router.beforeEach((to) => {
    if (shownId.value !== null && to.path === `/chat/${shownId.value}`) {
      return true
    }
    leader.value?.dispatchEvent(new CustomEvent('navigate', { detail: { address: to.fullPath }, bubbles: true }))
    return false
  })
  const conversations = useConversations(pinia)
  // Each element's view opens its own conversation, as the web app's does.
  const follow = (): void => {
    const [element, id] = [...elements][0] ?? [null, null]
    leader.value = element
    if (id === shownId.value) {
      return
    }
    shownId.value = id
    if (id !== null) {
      void router.replace(`/chat/${id}`)
    }
  }
  // The page's own scope: what follows the shown conversation lives as long
  // as the page, whichever element made it.
  const scope = effectScope(true)
  const shown = scope.run(() => {
    useWork(pinia).attach(router)
    keepFileTree()
    const product = useProduct(pinia)
    const preferences = usePreferences(pinia)
    followAppearance(useResources(pinia))
    followLanguage(product, preferences, shallowRef(navigator.languages))
    // The page lives as long as the window.
    followAttention(new AbortController().signal)
    conversations.initialize()
    return shownConversation(shownId, router)
  })!
  page = {
    pinia,
    router,
    i18n,
    shown,
    leader: shallowReadonly(leader),
    show(id, element) {
      elements.set(element, id)
      follow()
    },
    leave(element) {
      elements.delete(element)
      follow()
    },
  }
  return page
}
