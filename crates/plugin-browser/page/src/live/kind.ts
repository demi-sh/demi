import { computed, defineComponent, h, onBeforeUnmount, type PropType } from 'vue'
import { Globe, RotateCw } from '@lucide/vue'
import { ICON_PX, usePage, type KindTab, type TabKind, type WindowKind } from '@jush-ai/plugin-sdk'
import BrowserOpening from './BrowserOpening.vue'
import BrowserTabContent from './BrowserTabContent.vue'
import { t } from '../locales/text'
import { BROWSER_TAB_KIND, type BrowserStore } from './store'
import { browserTabDataSchema, type BrowserTabData, type BrowserTabsController } from './tabs'

const BrowserTabMark = defineComponent({
  setup: () => () => h(Globe, { size: ICON_PX.markIn28 }),
})

/**
 * A tab is named by its page's title as the browser reports it, so a tab
 * opened in the background takes its title as soon as its page has one, as
 * in any browser; until it has one, by its address. A tab its user sent
 * elsewhere is named after where it goes at once, never after the page it
 * leaves.
 */
export function browserTabTitle(data: BrowserTabData, session: BrowserTabsController): string {
  const title = session.title(data)
  if (title) {
    return title
  }
  const address = session.address(data)
  const url = URL.parse(address)
  return url ? url.host || url.href : address
}

/** What a new tab asks the device for: the address it opens on, or none for a blank page. */
export interface BrowserTabOpens {
  url: string | null
}

/** The browser's window, a tab per page. */
const browserWindow: WindowKind = { id: 'browser', name: () => t('window.name'), icon: Globe }

/**
 * The `browser` tab kind over `store`. Its content watches the conversation's
 * browser while it is mounted and reaches the device through it; its title, icon
 * and commands read it without watching; the panel sees only this
 * declaration.
 */
export function browserTabKind(store: BrowserStore): TabKind<BrowserTabData, BrowserTabOpens> {
  const browser = (tab: KindTab) => tab.page.store(store).of(tab.conversation).tabs
  const content = defineComponent({
    props: {
      conversation: { type: String, required: true },
      tabId: { type: String, required: true },
      data: { type: Object as PropType<BrowserTabData>, required: true },
      shown: { type: Boolean, required: true },
    },
    setup(props) {
      // Watched while mounted: the conversation's browser is the most recent to keep.
      const watched = usePage().store(store).watch(props.conversation)
      onBeforeUnmount(() => watched.end())
      return () => h(BrowserTabContent, {
        conversation: props.conversation,
        session: watched.session.tabs,
        tabId: props.tabId,
        data: props.data,
        shown: props.shown,
      })
    },
  })
  const opening = defineComponent({
    props: { conversation: { type: String, required: true } },
    emits: { failed: () => true },
    setup(props, { emit }) {
      const page = usePage()
      const watched = page.store(store).watch(props.conversation)
      onBeforeUnmount(() => watched.end())
      const held = computed(() => new Set(page.work.tabs(props.conversation, BROWSER_TAB_KIND).flatMap((data) => {
        const parsed = browserTabDataSchema.safeParse(data)
        return parsed.success ? [parsed.data.tab] : []
      })))
      return () => h(BrowserOpening, { session: watched.session.tabs, held: held.value, onFailed: () => emit('failed') })
    },
  })
  return {
    kind: BROWSER_TAB_KIND,
    schema: browserTabDataSchema,
    window: browserWindow,
    // A blank page, as a web browser's new tab shows before an address is typed.
    start: () => ({ url: null }),
    // Only the device makes a tab: the work has it once the device lists it.
    open: (opens, tab) => void browser(tab).open(opens.url),
    title: (data, tab) => browserTabTitle(data, browser(tab)),
    // The agent's showings, and the user's own opening, which the plugin carries from the tab list.
    shows: (data) => data.shows ?? 0,
    mark: BrowserTabMark,
    // The page's own icon, as a web browser's tab shows it; the mark only for a page without one.
    icon: (data, tab) => browser(tab).favicon(data),
    // The page loads: the strip shows it as a web browser's tab does.
    busy: (data, tab) => browser(tab).busy(data),
    content,
    opening,
    // A link's Open in Browser Window: a new tab on its page.
    intents: {
      web: (payload) => ({ url: payload.url }),
    },
    // A web browser's tab menu: Reload the page.
    commands: (data, tab) => [
      {
        label: t('menu.reload'),
        icon: RotateCw,
        run: () => browser(tab).reload(data),
      },
    ],
  }
}
