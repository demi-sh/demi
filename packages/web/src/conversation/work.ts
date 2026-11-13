import { reactive, shallowRef, watch } from 'vue'
import { defineStore } from 'pinia'
import { z } from 'zod'
import type { LocationQuery, RouteLocationNormalized, Router } from 'vue-router'
import {
  openIntent,
  pendingShows,
  pinnedData,
  type TabsState,
  type WorkTab,
  type PinnedTabs,
} from '@jush-ai/web-ui/agent/work-tabs'
import {
  addressSelection,
  workAddress,
  placeData,
  sameAddress,
  type WorkAddress,
} from '@jush-ai/web-ui/agent/work-address'
import { WorkTabs, closeWorkTabs, updateWorkTab } from '@jush-ai/web-ui/agent/work-changes'
import { TAB_HISTORY, selectTab } from '@jush-ai/web-ui/agent/tab-close'
import { usePhone } from '@jush-ai/web-ui/ui/phone'
import { intentKind, type HistoryDirection, type TabUpdate } from '@jush-ai/web-ui/plugins/page'
import type { IntentName, IntentRequest } from '@jush-ai/web-ui/plugins/intents'
import type { FrontCause, WorkRequest } from '@jush-ai/web-ui/windows/session-work'
import { readTab } from '@jush-ai/web-ui/agent/tab-kinds/kind'
import { workBackend } from '../api/work'
import { reportError } from '@jush-ai/web-ui/infra/errors'
import { t } from './locales/text'
import { useResources } from '../state/resources'
import { useProduct } from '../state/product'
import { PLUGIN_PAGES } from '../plugins/generated/pages'
import { usePluginStates } from '../plugins/states'
import { pluginEnabled } from '../plugins/enabled'
import { useConversations } from './store'

/**
 * One conversation's work as the page shows it: the order the tabs were shown
 * in, its pinned tabs' data, the backend's tabs with this page's changes over
 * them, and what its windows have yet to take.
 */
export interface WorkState {
  /** The tabs' and pinned kinds' ids in the order they were shown, the newest last. */
  readonly history: readonly string[]
  /** The pinned tabs' data, by kind, for the page's lifetime. */
  readonly pinned: PinnedTabs
  /** The history and the tabs, as the windows show them. */
  readonly tabsState: TabsState
  /**
   * Whether the page read the conversation's tabs: the tabs the first read
   * brings were there before the page looked, and show at once.
   */
  readonly tabsRead: boolean
  /** A tab, or none, asked of the windows that they have not taken yet. */
  readonly request: WorkRequest | null
}

/** No pinned tab's data kept yet: one value, so a read of it changes nothing. */
const NO_PINNED: PinnedTabs = Object.freeze({})

/**
 * The mark of a history entry whose address the work wrote: an address
 * without `tab` then says no window is in front, where an address nobody
 * marked, such as one the sidebar opens, shows the windows as the layout
 * keeps them.
 */
const ENTRY_MARK = 'work'
/**
 * How many steps an entry is from the one before a window came to the front,
 * which a phone's Back to Conversation goes back to: 1 for the entry that
 * brought it. Null while none is in front, or for an entry an address nobody
 * marked opened, such as a link.
 */
const ENTRY_STEPS = 'workSteps'

/**
 * What the work reads of the router's current history entry: its mark and
 * steps, and the addresses of the entries before and after it, which
 * vue-router's web history keeps.
 */
const entryStateSchema = z.looseObject({
  back: z.string().nullable().optional(),
  forward: z.string().nullable().optional(),
  [ENTRY_MARK]: z.literal(true).optional(),
  [ENTRY_STEPS]: z.number().int().positive().nullable().optional(),
})

/** The conversation an address shows, and the work's part of it. */
interface ShownAddress {
  conversation: string
  /** The address's path, which a move of the work keeps. */
  path: string
  address: WorkAddress
  /** The work wrote the address, so one without `tab` says no window is in front. */
  marked: boolean
}

/**
 * The layout's field of what the conversation's tabs were shown in, newest
 * last, which each window shows its newest of: one record for every device.
 */
const RECENT_FIELD = 'recent'
const recentSchema = z.array(z.string()).max(TAB_HISTORY)

/** What `layout` keeps the tabs were shown in; none while it keeps nothing readable. */
function storedRecent(layout: Record<string, unknown> | null): readonly string[] {
  const parsed = recentSchema.safeParse(layout?.[RECENT_FIELD])
  return parsed.success ? parsed.data : []
}

function sameHistory(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, index) => id === b[index])
}

/** The conversation a route shows, or null for any other page. */
function routeConversation(route: Pick<RouteLocationNormalized, 'path' | 'params'>): string | null {
  const { id } = route.params
  return route.path.startsWith('/chat/') && typeof id === 'string' && id !== '' ? id : null
}

/** The address's query as the work reads it: a field given twice, or without a value, is none. */
function queryAddress(query: LocationQuery): WorkAddress {
  return Object.fromEntries(Object.entries(query).flatMap(([field, value]) => typeof value === 'string' ? [[field, value]] : []))
}

/**
 * The work's state per conversation, for the page's lifetime.
 *
 * - The windows alone say which is in front (`SessionWindows.vue`). While
 *   the page's address shows a conversation, it names the tab of the work's
 *   window in front as they last said (`inFront`), a step of the web
 *   browser's history unless a close or a correction writes it in place, or
 *   it only changes how the same place is seen; a move that joins one on its
 *   way keeps it a step.
 * - What asks for a tab from outside the windows, a pill, a link, a kind's
 *   `shows`, a plugin, or a navigation of the history the store did not
 *   write, is a request they take once its tab's window can show it. An
 *   address naming a tab the work does not have is corrected in place.
 * - A show of a tab the first read of the tabs brings happened before the
 *   page read them, and never overrides an address that names a tab.
 * - What the tabs were shown in, newest last, is the conversation's, kept in
 *   its layout with its windows (`recent`), so every device shows each window
 *   on the same tab; a phone keeps its own in memory, as it writes no layout.
 *   Its pinned tabs' data is kept in memory. An address without `tab`
 *   that the work did not write, such as one the sidebar opens, names
 *   nothing: the windows stand as the layout says, and the address then
 *   names the tab of the one in front.
 * - The backend's tabs are taken from the conversation's entry in the list
 *   when their revision rises, with this page's changes shown at once over
 *   them.
 */
export const useWork = defineStore('work', () => {
  const resources = useResources()
  const product = useProduct()
  const conversations = useConversations()
  const pluginStates = usePluginStates()
  const enabled = (plugin: string) => pluginEnabled(product.snapshot, plugin)
  const states = reactive(new Map<string, WorkState>())
  /** Each conversation's tabs; reactive on their own, so they stand beside the states, not in them. */
  const tabsByConversation = new Map<string, WorkTabs>()
  /** Each conversation's pinned tabs' data as last shown, for the page's lifetime. */
  const keptPinned = reactive(new Map<string, PinnedTabs>())
  /**
   * The tabs this page showed in each conversation that it has not written
   * with the layout, oldest first: on a phone, which writes no layout, and
   * for a conversation the page has not opened, whose layout it would
   * otherwise write over another device's. Each is shown over the layout's
   * order, so what another device wrote meanwhile still counts.
   */
  const unwritten = reactive(new Map<string, readonly string[]>())
  /**
   * What each conversation's windows have yet to take: the newest request
   * only, as a later one overrides it. A page's tab may reach the work only
   * after the plugin that opened it asked for it, and the request waits.
   */
  const requests = reactive(new Map<string, WorkRequest>())
  /** The number of the last request made. */
  let requested = 0
  /** The tab of each conversation's work window in front, as its windows last said; none while none is. */
  const fronts = new Map<string, string | null>()
  const phone = usePhone()
  /** The conversation the page shows, as the frame tells it (`shown`). */
  let shownId: string | null = null
  /**
   * The work revision of the shown conversation the page last read, null
   * until it reads one after the conversation came on screen: a show the
   * page reads past it came while the user looked at that conversation.
   */
  let seenRevision: number | null = null
  /** The router whose address holds the work (`attach`). */
  let router: Router | null = null
  /**
   * The address that says what the windows show: the one the router reached,
   * or the one a move of the work is taking it to. Null while the page
   * shows no conversation.
   */
  const address = shallowRef<ShownAddress | null>(null)
  /** The move of the router on its way, which a later move joins: a step stays a step. */
  let moving: { how: 'push' | 'replace' } | null = null

  /** The kinds of the pages the user has on. */
  function kinds() {
    return PLUGIN_PAGES.filter((page) => enabled(page.plugin)).flatMap((page) => page.kinds ?? [])
  }

  /** The conversation's tabs, made the first time they are asked for. */
  function tabsOf(conversationId: string): WorkTabs {
    let tabs = tabsByConversation.get(conversationId)
    if (!tabs) {
      const created = new WorkTabs(workBackend(conversationId), (error) => {
        reportError(t('failures.changeTabs'), error, { site: 'failures.changeTabs', userVisible: true })
      })
      tabsByConversation.set(conversationId, created)
      let readBefore = false
      // A tab whose kind asks that the user see it brings its window to the front, open or
      // closed; the address then names what the page now knows of the tabs.
      watch(() => created.tabs.value, (current) => {
        const firstRead = !readBefore
        readBefore = created.read
        applyShows(conversationId, created.revision, current, firstRead)
        writeUnwritten(conversationId)
        settleRequest(conversationId)
      })
      tabs = created
    }
    return tabs
  }

  /** The address while it shows the conversation. */
  function addressOf(conversationId: string): ShownAddress | null {
    return address.value?.conversation === conversationId ? address.value : null
  }

  /** What the router's current history entry says of itself. */
  function entryState() {
    const parsed = entryStateSchema.safeParse(router?.options.history.state)
    const state = parsed.success ? parsed.data : {}
    return {
      marked: state[ENTRY_MARK] === true,
      steps: state[ENTRY_STEPS] ?? null,
      back: state.back ?? null,
      forward: state.forward ?? null,
    }
  }

  /** The address `route` shows, or null for a page that is no conversation. */
  function routeAddress(route: RouteLocationNormalized): ShownAddress | null {
    const conversationId = routeConversation(route)
    if (conversationId === null) {
      return null
    }
    const entry = entryState()
    return { conversation: conversationId, path: route.path, address: queryAddress(route.query), marked: entry.marked }
  }

  /**
   * Whether the page knows what the conversation's work has: the pages the
   * user has on, and the tabs once its record has them. Until then an
   * address naming what the page does not know yet stays as it is.
   */
  function ready(conversationId: string): boolean {
    return product.snapshot !== null && (!recorded(conversationId) || tabsOf(conversationId).read)
  }

  /** Whether a window can show the tab `id`: a pinned kind, or a tab of the work that a kind on reads. */
  function has(conversationId: string, id: string): boolean {
    const shown = kinds()
    return shown.some((kind) => kind.pinned && kind.kind === id)
      || tabsOf(conversationId).tabs.value.some((tab) => tab.id === id && readTab(tab, shown) !== null)
  }

  /** What the tabs were shown in, newest last: the layout's order, with what this page showed and did not write over it. */
  function historyOf(conversationId: string): readonly string[] {
    return (unwritten.get(conversationId) ?? []).reduce<readonly string[]>(selectTab, storedRecent(layoutOf(conversationId)))
  }

  /**
   * Whether the page writes the conversation's layout: on a computer, once
   * it opened the conversation and read its work. Before, a layout it wrote
   * would stand for one it has not read, or freeze the one it holds while
   * the write waits for the conversation to open.
   */
  function writes(conversationId: string): boolean {
    const tabs = tabsOf(conversationId)
    return !phone.value && tabs.started && tabs.read
  }

  /** Keeps what the tabs were shown in, newest last: with the layout where the page writes it, else in memory. */
  function keepHistory(conversationId: string, history: readonly string[]): void {
    if (sameHistory(history, historyOf(conversationId))) {
      return
    }
    if (writes(conversationId)) {
      unwritten.delete(conversationId)
      tabsOf(conversationId).change({ type: 'windows', windows: { ...layoutOf(conversationId), [RECENT_FIELD]: [...history] } })
      return
    }
    const shown = history.at(-1)
    if (shown !== undefined) {
      unwritten.set(conversationId, [...(unwritten.get(conversationId) ?? []).filter((id) => id !== shown), shown])
    }
  }

  /** Writes what the page showed and kept in memory, once it writes the conversation's layout. */
  function writeUnwritten(conversationId: string): void {
    if (unwritten.has(conversationId) && writes(conversationId)) {
      const history = historyOf(conversationId)
      unwritten.delete(conversationId)
      tabsOf(conversationId).change({ type: 'windows', windows: { ...layoutOf(conversationId), [RECENT_FIELD]: [...history] } })
    }
  }

  /**
   * Asks the conversation's windows for the tab `tab`, or none, in place of
   * what they have yet to take: a tab the work does not have yet shows once
   * it arrives.
   */
  function request(conversationId: string, tab: string | null, fromAddress = false): void {
    requested += 1
    requests.set(conversationId, { id: requested, tab, fromAddress })
  }

  /** The windows took the request `id`. */
  function took(conversationId: string, id: number): void {
    if (requests.get(conversationId)?.id === id) {
      requests.delete(conversationId)
    }
  }

  /**
   * Drops a request the address made for a tab no window can show, once the
   * page knows what the work has, and writes in place what the windows show.
   */
  function settleRequest(conversationId: string): void {
    const asked = requests.get(conversationId)
    if (asked?.fromAddress && asked.tab !== null && ready(conversationId) && !has(conversationId, asked.tab)) {
      requests.delete(conversationId)
      writeAddress(conversationId, 'replace')
    }
  }

  /**
   * The windows say `tab` is in front, or none: the address names it, in
   * place for their first word since the conversation came on screen and
   * for a close, a step of the history otherwise.
   */
  function inFront(conversationId: string, tab: string | null, cause: FrontCause): void {
    fronts.set(conversationId, tab)
    writeAddress(conversationId, cause === 'moved' ? 'push' : 'replace')
  }

  /** The user picked `tab` in its window: the newest it showed. */
  function pick(conversationId: string, tab: string): void {
    keepHistory(conversationId, selectTab(historyOf(conversationId), tab))
  }

  /**
   * While the address shows the conversation, writes into it the tab the
   * windows last said is in front, with its pinned kind's place: a step of
   * the history (`push`) or in place of the shown one (`replace`). An address
   * that says the same already, but that the work did not write, is marked
   * in place.
   */
  function writeAddress(conversationId: string, how: 'push' | 'replace'): void {
    const shown = addressOf(conversationId)
    if (!router || !shown) {
      return
    }
    const written = workAddress(fronts.get(conversationId) ?? null, keptPinned.get(conversationId) ?? NO_PINNED, kinds())
    if (sameAddress(written, shown.address)) {
      if (!shown.marked) {
        navigate(shown, written, 'replace')
      }
      return
    }
    navigate(shown, written, how)
  }

  /**
   * A navigation the work did not write, such as Back, Forward or a link: a
   * tab it names is asked of the windows, with its pinned kind's place; an
   * entry the work wrote that names none puts the work's windows away; an
   * address nobody marked that names none leaves them as the layout says.
   */
  function navigated(shown: ShownAddress): void {
    const conversationId = shown.conversation
    const tab = addressSelection(shown.address)
    if (tab === null) {
      if (shown.marked) {
        request(conversationId, null, true)
      } else {
        writeAddress(conversationId, 'replace')
      }
      return
    }
    if (ready(conversationId) && !has(conversationId, tab)) {
      writeAddress(conversationId, 'replace')
      return
    }
    readPlace(shown)
    request(conversationId, tab, true)
  }

  /**
   * Keeps the place the address names for its pinned kind, such as the
   * change Changes shows: read again once the pages the user has on are
   * known, as a page that opens on the address knows them only after.
   */
  function readPlace(shown: ShownAddress): void {
    const tab = addressSelection(shown.address)
    const kind = kinds().find((candidate) => candidate.pinned && candidate.kind === tab)
    if (!kind?.place) {
      return
    }
    const kept = keptPinned.get(shown.conversation) ?? NO_PINNED
    const current = pinnedData(kept, kind)
    const data = placeData(kind, shown.address, current)
    if (data !== current) {
      keptPinned.set(shown.conversation, { ...kept, [kind.kind]: data })
    }
  }

  /**
   * The router's move to `written` from `shown`. A move on its way that this
   * one replaces was a step, so this one is too. On a phone, putting the
   * window away goes back to the entry before it came, as its Back to
   * Conversation says.
   */
  function navigate(shown: ShownAddress, written: WorkAddress, asked: 'push' | 'replace'): void {
    const target = router
    if (!target) {
      return
    }
    const entry = entryState()
    const opens = addressSelection(written) !== null
    if (phone.value && !opens && asked === 'push' && moving === null && entry.steps !== null) {
      target.go(-entry.steps)
      return
    }
    const how: 'push' | 'replace' = moving?.how === 'push' || (asked === 'push' && shown.marked) ? 'push' : 'replace'
    const current = target.currentRoute.value
    const wasOpen = routeConversation(current) === shown.conversation && addressSelection(queryAddress(current.query)) !== null
    let steps: number | null = null
    if (opens && how === 'push') {
      steps = !wasOpen ? 1 : entry.steps === null ? null : entry.steps + 1
    } else if (opens) {
      steps = entry.steps
    }
    const token = { how }
    moving = token
    address.value = { conversation: shown.conversation, path: shown.path, address: written, marked: true }
    const location = { path: shown.path, query: { ...written }, state: { [ENTRY_MARK]: true, [ENTRY_STEPS]: steps }, force: true }
    const going = how === 'push' ? target.push(location) : target.replace(location)
    // A move nothing joined that the router did not make leaves the address
    // where the router is. An error a guard throws stays the router's, which
    // reports it as any navigation's.
    void going.then((failure) => {
      if (moving === token) {
        moving = null
        if (failure) {
          address.value = routeAddress(target.currentRoute.value)
        }
      }
    })
  }

  /** What the entry `direction` of the router's shows of the conversation's work: open, closed, or another page. */
  function neighbour(conversationId: string, direction: HistoryDirection): 'open' | 'closed' | null {
    const at = entryState()[direction]
    if (!router || !addressOf(conversationId) || !at) {
      return null
    }
    const route = router.resolve(at)
    if (routeConversation(route) !== conversationId) {
      return null
    }
    return addressSelection(queryAddress(route.query)) === null ? 'closed' : 'open'
  }

  /**
   * Opens the work on each tab whose kind counts more showings than this
   * page applied, once, and records the counts beside the selection history.
   *
   * - The shows the first read of the tabs brings happened before the page
   *   read them: an address that names what the windows show stands over
   *   them.
   * - On a phone only a show read while the user looks at the conversation
   *   opens it; one that came while the user was elsewhere, read before the
   *   conversation came on screen or by the first read after, is selected in
   *   the strip with no window brought to the front.
   * - On a computer, a show of a conversation the page has not opened waits
   *   until it opens it (`load`): the page writes no layout of a conversation
   *   it has not read as opened, which another device may change meanwhile.
   *
   * `revision` is the work's that `tabs` came with.
   */
  function applyShows(conversationId: string, revision: number, tabs: readonly WorkTab[], firstRead: boolean): void {
    if (!phone.value && !writes(conversationId)) {
      return
    }
    const looking = conversationId === shownId
    const whileLooking = looking && seenRevision !== null && revision > seenRevision
    if (looking && revision >= 0) {
      seenRevision = Math.max(seenRevision ?? -1, revision)
    }
    const applied = resources.local.tabsShown?.[conversationId] ?? {}
    const pending = pendingShows(tabs, PLUGIN_PAGES, enabled, applied)
    if (!pending) {
      return
    }
    resources.local.tabsShown ??= {}
    resources.local.tabsShown[conversationId] = { ...pending.applied }
    const shown = addressOf(conversationId)
    if (firstRead && shown && addressSelection(shown.address) !== null) {
      return
    }
    keepHistory(conversationId, pending.shown.reduce<readonly string[]>((selected, id) => selectTab(selected, id), historyOf(conversationId)))
    const last = pending.shown.at(-1)
    const anyInFront = (fronts.get(conversationId) ?? null) !== null
    if (last !== undefined && (!phone.value || whileLooking || anyInFront)) {
      request(conversationId, last)
    }
  }

  /**
   * The frame shows `conversationId` now, or no conversation: only what its
   * windows show from here on opens it on a phone.
   */
  function shown(conversationId: string | null): void {
    // A conversation off screen has no windows to say what is in front.
    if (shownId !== null && shownId !== conversationId) {
      fronts.delete(shownId)
    }
    shownId = conversationId
    const revision = conversationId === null ? -1 : (tabsByConversation.get(conversationId)?.revision ?? -1)
    seenRevision = revision >= 0 ? revision : null
  }

  /** The conversation's work as it shows. */
  function stateFor(conversationId: string): WorkState {
    if (!states.has(conversationId)) {
      const tabs = tabsOf(conversationId)
      states.set(conversationId, {
        get history(): readonly string[] {
          return historyOf(conversationId)
        },
        get pinned(): PinnedTabs {
          return keptPinned.get(conversationId) ?? NO_PINNED
        },
        get tabsState(): TabsState {
          return { history: this.history, tabs: tabs.tabs.value }
        },
        get tabsRead(): boolean {
          return tabs.read
        },
        get request(): WorkRequest | null {
          return requests.get(conversationId) ?? null
        },
      })
    }
    // The map's own (reactive) view of the entry, never the plain object it was made from.
    return states.get(conversationId)!
  }

  // Each conversation's work is the one its entry in the list brings, under
  // its revision, whatever the page shows: the work draws as soon as the
  // conversation is listed.
  watch(
    () => (product.snapshot?.conversations ?? []).map((entry) => [entry.id, entry.work] as const),
    (followed) => {
      for (const [id, work] of followed) {
        tabsOf(id).receive(work)
      }
    },
    { immediate: true, flush: 'sync' },
  )
  // The states the plugins hold for the tabs come with the conversation's
  // metadata, the disk's copy or the lane's opening, under their revisions.
  watch(
    () => conversations.items.map((item) => [item.id, item.metadata.shown.value?.tabStates] as const),
    (followed) => {
      for (const [id, held] of followed) {
        for (const { plugin, revision, state } of held ?? []) {
          pluginStates.hold(plugin, id, revision, state)
        }
      }
    },
    { immediate: true, flush: 'sync' },
  )
  // The pages the user has on say what an address names: a change of them
  // reads the place of its request and judges the request again.
  watch(
    () => product.snapshot === null ? null : kinds().map((kind) => kind.kind).join(),
    () => {
      const shown = address.value
      if (!shown) {
        return
      }
      if (requests.get(shown.conversation)?.fromAddress) {
        readPlace(shown)
      }
      settleRequest(shown.conversation)
    },
  )

  /**
   * Holds the work in `next`'s address: a navigation the router ends on a
   * conversation that the work did not write asks its windows for what the
   * address names (`navigated`). Returns what lets go of it.
   */
  function attach(next: Router): () => void {
    router = next
    const stop = next.afterEach((to, _from, failure) => {
      if (failure) {
        return
      }
      const ours = moving !== null
      moving = null
      address.value = routeAddress(to)
      if (!ours && address.value) {
        navigated(address.value)
      }
    })
    return () => {
      stop()
      router = null
      address.value = null
      moving = null
    }
  }

  /** Whether the web browser's Back or Forward stays in the conversation's work. */
  function canGo(conversationId: string, direction: HistoryDirection): boolean {
    const shownNow = addressOf(conversationId)
    return shownNow !== null && addressSelection(shownNow.address) !== null && neighbour(conversationId, direction) === 'open'
  }

  /** The web browser's Back or Forward, as the work's address bar takes it. */
  function go(direction: HistoryDirection): void {
    // Back and Forward ask for what the address names, in place of a request
    // that still waits for its tab.
    requests.clear()
    if (direction === 'back') {
      router?.back()
    } else {
      router?.forward()
    }
  }

  /**
   * Whether the conversation has its backend record, which its first send
   * creates. Before it, the work binds no page and reads or sends nothing.
   */
  function recorded(conversationId: string): boolean {
    return conversations.items.find((item) => item.id === conversationId)?.persistence === 'synced'
  }

  /** Reads the work and sends the page's changes, once the conversation has its record. */
  function load(conversationId: string): void {
    if (recorded(conversationId)) {
      const tabs = tabsOf(conversationId)
      tabs.start()
      writeUnwritten(conversationId)
      // The shows that came while the page had not opened it, as the first
      // read's: an address that names a tab stands over them.
      if (tabs.read) {
        applyShows(conversationId, tabs.revision, tabs.tabs.value, true)
      }
    }
  }

  /**
   * A new tab, after the others or at `options.index` among them, selected
   * unless `options` says not, with its window brought to the front; returns its id.
   */
  function add(
    conversationId: string,
    kind: string,
    data: unknown,
    options: { select: boolean; index?: number } = { select: true },
  ): string {
    const id = crypto.randomUUID()
    tabsOf(conversationId).change({ type: 'create', tab: { id, kind, data }, index: options.index })
    if (options.select) {
      request(conversationId, id)
    }
    return id
  }

  /** Moves the tab `id` to `index` among the others, where the user dragged it. */
  function moveTab(conversationId: string, id: string, index: number): void {
    tabsOf(conversationId).change({ type: 'move', id, index })
  }

  /** The tab's next `data`, from its kind: only what changed is sent. */
  function update(conversationId: string, id: string, data: unknown): void {
    updateWorkTab(tabsOf(conversationId), id, data)
  }

  /**
   * Closes the tabs `ids`: each window shows the newest of its own tabs left,
   * and one left with none closes; the windows then say what is in front.
   */
  function closeTabs(conversationId: string, ids: string[]): void {
    const selection = { history: historyOf(conversationId) }
    closeWorkTabs(selection, tabsOf(conversationId), ids)
    keepHistory(conversationId, selection.history)
    const asked = requests.get(conversationId)
    if (asked?.tab != null && ids.includes(asked.tab)) {
      requests.delete(conversationId)
    }
  }

  /** A pinned tab's data, as its kind replaces it: in front, a move to another place unless `how` says it is none. */
  function updatePinned(conversationId: string, kind: string, data: unknown, how?: TabUpdate): void {
    keptPinned.set(conversationId, { ...(keptPinned.get(conversationId) ?? NO_PINNED), [kind]: data })
    if (fronts.get(conversationId) === kind) {
      writeAddress(conversationId, how?.replace ? 'replace' : 'push')
    }
  }

  /**
   * Opens an intent in the conversation's work (`packages/web-ui/src/plugins/intents.ts`): the
   * kind that opens it shows it, in its pinned tab or a new one, whose window is asked to the
   * front on it. What the windows already show stays as it is (`openIntent`).
   */
  function openIn(conversationId: string, intent: IntentRequest): void {
    const state = stateFor(conversationId)
    const front = fronts.get(conversationId) ?? null
    const opened = openIntent(state, PLUGIN_PAGES, enabled, intent, front)
    if (!opened) {
      return
    }
    const { created } = opened
    // A kind whose plugin makes its tabs is asked for the tab, and its window
    // waits for it (`TabKind.open`).
    if (created && kinds().some((kind) => kind.kind === created.kind && kind.open)) {
      requested += 1
      requests.set(conversationId, {
        id: requested,
        tab: null,
        opens: { kind: created.kind, data: created.data },
        fromAddress: false,
      })
      return
    }
    if (created) {
      tabsOf(conversationId).change({ type: 'create', tab: created })
    }
    if (opened.pinned !== state.pinned) {
      keptPinned.set(conversationId, opened.pinned)
      if (front === opened.selection) {
        writeAddress(conversationId, 'push')
      }
    }
    request(conversationId, opened.selection)
  }

  /** Whether any page the user has on opens `intent`. */
  function canOpen(intent: IntentName): boolean {
    return intentKind(PLUGIN_PAGES, enabled, intent) !== null
  }

  /** Where the conversation's windows stand: as this page last wrote it, else as its work last received said; null while it says none. */
  function layoutOf(conversationId: string): Record<string, unknown> | null {
    return tabsOf(conversationId).windows.value
  }

  /** Keeps where the conversation's windows stand with its work, with what its tabs were shown in. */
  function keepLayout(conversationId: string, layout: Record<string, unknown>): void {
    tabsOf(conversationId).change({ type: 'windows', windows: { ...layout, [RECENT_FIELD]: [...historyOf(conversationId)] } })
  }

  return {
    stateFor,
    recorded,
    shown,
    attach,
    load,
    request,
    took,
    inFront,
    pick,
    add,
    move: moveTab,
    update,
    closeTabs,
    updatePinned,
    openIn,
    canGo,
    canOpen,
    go,
    layoutOf,
    keepLayout,
  }
})
