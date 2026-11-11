import { computed, nextTick, reactive, ref, watch } from 'vue'
import { browserPage } from '@demicodes/plugin-browser'
import { mobilePage } from '@demicodes/plugin-mobile'
import { WorkTabs, closeWorkTabs, updateWorkTab } from '@demicodes/web-ui/agent/work-changes'
import {
  openIntent,
  pendingShows,
  type AppliedShows,
  type TabsState,
  type PinnedTabs,
} from '@demicodes/web-ui/agent/work-tabs'
import { selectTab } from '@demicodes/web-ui/agent/tab-close'
import type { RequestEditSelection } from '@demicodes/web-ui/files/request-changes'
import type { IntentRequest } from '@demicodes/web-ui/plugins/intents'
import type { FrontCause, WorkRequest } from '@demicodes/web-ui/windows/session-work'
import { readTab } from '@demicodes/web-ui/agent/tab-kinds/kind'
import type { TabUpdate } from '@demicodes/web-ui/plugins/page'
import { disposePluginStores } from '@demicodes/web-ui/plugins/stores'
import {
  intentKind,
  useBoundKinds,
  type AnyPluginPage,
  type ConversationFileService,
} from '@demicodes/web-ui/plugins/page'
import type { ChangeSetSource } from '@demicodes/web-ui/files/changes'
import { PLUGIN_PAGES } from '../generated/pages'
import { productWould } from '../product-would'
import { readGalleryEdit } from './blobs'
import { galleryRequest } from './transcripts'
import { galleryBrowser, type GalleryBrowser } from './live-browser'
import { galleryMobile, type GalleryScreen } from './live-mobile'
import { GalleryWorkRecord, galleryBrowserPlugin } from './work-record'
import { SLOT_ROOT, createSlotChangeSet, type createGalleryProject } from './project'
import { sharedHistory } from './history'
import { galleryHistory } from './browser-history'
import { browserPlugin, galleryPageHost } from './plugins'
import { galleryWidgets } from './widgets'

/**
 * The gallery project as the tab kinds read a conversation's files.
 * `directories` lists the directories its agents work in, the project's alone
 * by default; the repository's checkout in the other checkout's folder is on
 * the branch `slotBranch` names.
 */
export function galleryFiles(
  project: ReturnType<typeof createGalleryProject>,
  { directories, slotBranch }: { directories?: () => readonly string[]; slotBranch?: () => string | null } = {},
): ConversationFileService {
  const slotHistory = sharedHistory.sourceOn(slotBranch ?? (() => 'main'))
  // The other checkout's working tree, made once a view shows it.
  let slot: ChangeSetSource | null = null
  return {
    tree: { source: project.source, root: project.root },
    root: project.root,
    get directories() {
      return (directories?.() ?? [project.root]).map((path) => ({ path }))
    },
    changes: (directory) => directory === undefined || directory === project.root ? project.changes : (slot ??= createSlotChangeSet(200)),
    // One repository stands behind every working tree of the gallery.
    history: (directory) => directory !== undefined && (directory === SLOT_ROOT || directory.startsWith(`${SLOT_ROOT}/`)) ? slotHistory : sharedHistory.source,
    // The project is its own repository, and holds no other.
    repositories: () => ({ list: [{ root: project.root, name: 'shop' }], preferred: project.root }),
    showRepositories: () => {},
    edit: readGalleryEdit,
    request: galleryRequest,
    // The gallery's working tree changes only when a specimen changes it.
    showChanges: () => {},
  }
}

/** The conversation every gallery frame shows. */
const CONVERSATION = 'gallery'

/**
 * One conversation's work the way the product's work store holds it:
 *
 * - Its selection history, the pinned tabs' data, and the tabs of a conversation's work kept
 *   as the backend keeps them, with the specimen's changes shown at once over
 *   them.
 * - The plugin pages' kinds, bound to it over the specimen's files and its own
 *   conversation browser, whose plugin opens and closes browser tabs for the
 *   work's tabs as the backend's does.
 *
 * Intents open in it as they open in the product.
 */
export function useGalleryWork(
  selection: string | null,
  { files, browser = galleryBrowser(), screen, pictures, pages, open: startsOpen = true }: {
    files: ConversationFileService
    /** Whether the windows are asked for `selection` at the start: a phone's shows the conversation instead. */
    open?: boolean
    browser?: GalleryBrowser
    /** The screen the simulators show in place of the gallery's own. */
    screen?: GalleryScreen
    /** Whether the web browser decodes the pictures; by default it asks the web browser, as the product does. */
    pictures?: () => Promise<boolean>
    /** The plugin pages whose kinds the windows show; every one the product shows by default. */
    pages?: readonly AnyPluginPage[]
  },
) {
  // The gallery's pages, with the browser's own made for a specimen that says how the pictures decode.
  const shown = pages ?? PLUGIN_PAGES.map((page) => {
    if (!pictures) {
      return page
    }
    return page.plugin === 'browser' ? browserPage({ pictures }) : page.plugin === 'mobile' ? mobilePage({ pictures }) : page
  })
  /** The plugins the specimen has turned off, as the user turns one off in Settings. */
  const off = reactive(new Set<string>())
  const enabled = (plugin: string) => !off.has(plugin)
  const backend = new GalleryWorkRecord()
  const plugin = galleryBrowserPlugin(browser, backend)
  // The conversation's simulators, whose plugin opens and closes devices for the work's `mobile` tabs.
  const mobile = galleryMobile(backend, screen)
  // The browser tells the plugin of every change of its tabs, as the browser package does
  // (`crates/command-protocol/src/messages.rs`).
  watch(browser.listed, () => void plugin.tabsChanged())
  // The browser's tabs stand in the browser window in its order, as the product's would, the plugin having
  // mirrored its list. A work without the browser's page has none.
  if (shown.some((page) => page.plugin === 'browser')) {
    plugin.mirror()
  }
  const tabs = new WorkTabs(backend, (error) => {
    productWould(`Report the Refused Work Change: ${error instanceof Error ? error.message : String(error)}`)
  })
  backend.removed = (tab) => {
    if (tab.kind === 'mobile') {
      mobile.removed(tab)
    } else if (tab.kind === 'browser') {
      void plugin.removed(tab)
    }
  }
  tabs.receive(backend.current())
  backend.changed = (work) => tabs.receive(work)
  tabs.start()

  const history = ref<readonly string[]>(selection === null ? [] : [selection])
  const pinned = ref<PinnedTabs>({})
  const tabsState = computed<TabsState>(() => ({ history: history.value, tabs: tabs.tabs.value }))
  /** The tab of the work's window in front, as the windows last said; none while no window of it is. */
  const front = ref<string | null>(null)
  /** What the windows have yet to take: a tab to have in front, or none. */
  const request = ref<WorkRequest | null>(null)
  let requested = 0
  /**
   * The web browser's history of the address, as the product's keeps it
   * (`packages/web-ui/src/agent/work-address.ts`): what the windows say is in
   * front is a step, and Back and Forward ask the windows for the tab there.
   */
  const steps = galleryHistory(() => ({ open: front.value !== null, front: front.value, pinned: pinned.value }), (step) => {
    // A step whose tab closed since says in its place what the windows show, as the product's address does.
    if (step.front !== null && !canShow(step.front)) {
      steps.replace()
      return
    }
    pinned.value = step.pinned
    ask(step.front, true)
  })
  /** The windows' next word starts the history over, as a specimen's Reset does. */
  let restarting = false

  /** Whether a window can show the tab `id`: a pinned kind, or a tab a kind on reads. */
  function canShow(id: string): boolean {
    return kinds.value.some((kind) => kind.pinned && kind.kind === id)
      || tabs.tabs.value.some((tab) => tab.id === id && readTab(tab, kinds.value) !== null)
  }
  function ask(tab: string | null, fromAddress = false) {
    requested += 1
    request.value = { id: requested, tab, fromAddress }
  }
  /** Asks the windows to bring the tab `id` to the front on it, once the work has it. */
  function select(id: string) {
    ask(id)
  }
  /** The windows took the request `id`. */
  function took(id: number) {
    if (request.value?.id === id) {
      request.value = null
    }
  }
  /**
   * The windows say `tab` is in front, or none, as the product's address
   * says it: in place for their first word and for a close, a step of the
   * history otherwise.
   */
  function inFront(tab: string | null, cause: FrontCause) {
    front.value = tab
    if (restarting) {
      restarting = false
      steps.reset()
    } else if (cause === 'moved') {
      steps.step()
    } else {
      steps.replace()
    }
  }
  /** The user picked `tab` in its window: the newest it showed. */
  function pick(tab: string) {
    history.value = selectTab(history.value, tab)
  }
  // The specimen opens on `selection` as the product opens on an address naming it.
  if (startsOpen && selection !== null) {
    ask(selection, true)
  }
  /** Each tab's showings the windows applied, as the product keeps them beside the history. */
  let applied: AppliedShows = {}
  // A tab its kind asks to show brings its window to the front and is selected once, as the product's work store does it.
  watch(() => tabs.tabs.value, (current) => {
    const pending = pendingShows(current, shown, enabled, applied)
    if (!pending) {
      return
    }
    applied = pending.applied
    for (const id of pending.shown) {
      select(id)
    }
  })
  /** A new tab, after the others or at `options.index` among them, selected unless `options` says not; returns its id. */
  function add(kind: string, data: unknown, options: { select: boolean; index?: number } = { select: true }): string {
    const id = crypto.randomUUID()
    tabs.change({ type: 'create', tab: { id, kind, data }, index: options.index })
    if (options.select) {
      select(id)
    }
    return id
  }
  /** Moves the tab `id` to `index` among the others, where the user dragged it. */
  function move(id: string, index: number) {
    tabs.change({ type: 'move', id, index })
  }
  function update(id: string, data: unknown) {
    updateWorkTab(tabs, id, data)
  }
  /** A pinned tab's data: a step of the history while its window is in front, unless `how` says it is none. */
  function updatePinned(kind: string, data: unknown, how?: TabUpdate) {
    pinned.value = { ...pinned.value, [kind]: data }
    if (front.value !== kind) {
      return
    }
    if (how?.replace) {
      steps.replace()
    } else {
      steps.step()
    }
  }
  /** Closed tabs go at once; each window shows the newest of its own tabs left. */
  function closeTabs(ids: string[]) {
    const selection = {
      get history() {
        return history.value
      },
      set history(next: readonly string[]) {
        history.value = next
      },
    }
    closeWorkTabs(selection, tabs, ids)
    if (request.value?.tab != null && ids.includes(request.value.tab)) {
      request.value = null
    }
  }
  /** Opens an intent as the product's work store does: what the windows already show stays as it is. */
  function openIn(intent: IntentRequest) {
    const opened = openIntent({ tabsState: tabsState.value, pinned: pinned.value }, shown, enabled, intent, front.value)
    if (!opened) {
      return
    }
    if (opened.pinned !== pinned.value) {
      pinned.value = opened.pinned
      if (front.value === opened.selection) {
        steps.step()
      }
    }
    const { created } = opened
    // A kind whose plugin makes its tabs is asked for the tab, and its window waits for it, as the product's work
    // store asks.
    if (created && kinds.value.some((kind) => kind.kind === created.kind && kind.open)) {
      requested += 1
      request.value = { id: requested, tab: null, opens: { kind: created.kind, data: created.data }, fromAddress: false }
      return
    }
    if (created) {
      tabs.change({ type: 'create', tab: created })
    }
    select(opened.selection)
  }

  const host = galleryPageHost({ browser: browserPlugin(browser), mobile: mobile.plugin, widget: galleryWidgets }, {
    files,
    enabled,
    deviceStarting: () => browser.deviceStarting.value,
    intents: {
      open: (_conversation, request) => openIn(request),
      canOpen: (intent) => intentKind(shown, enabled, intent) !== null,
    },
    work: {
      tabs: (_conversation, kind) => tabs.tabs.value.filter((tab) => tab.kind === kind).map((tab) => tab.data),
      select: (_conversation, _kind, id) => select(id),
      canGo: (_conversation, direction) => steps.canGo(direction),
      go: (direction) => steps.go(direction),
    },
  })
  const kinds = useBoundKinds(() => shown.filter((page) => enabled(page.plugin)), host, () => CONVERSATION)
  // A request the address made for a tab no window can show is dropped, and
  // the step says in its place what the windows show, as the product's work
  // store settles it.
  watch([request, () => tabs.tabs.value, kinds], ([asked]) => {
    if (asked?.fromAddress && asked.tab !== null && !canShow(asked.tab)) {
      request.value = null
      restarting = false
      steps.replace()
    }
  })
  /**
   * Turns `plugin` on or off while its window is open, as the user does in
   * Settings: turned off, the backend closes its kinds' tabs in the work and
   * releases what its packages hold on the device, such as the browser, and the
   * shell disposes its stores; the other plugins' tabs and stores stay.
   */
  function setEnabled(pluginId: string, on: boolean) {
    if (on) {
      off.delete(pluginId)
      return
    }
    off.add(pluginId)
    const owned = new Set(shown.find((page) => page.plugin === pluginId)?.kinds?.map((kind) => kind.kind) ?? [])
    for (const tab of tabs.tabs.value.filter((entry) => owned.has(entry.kind))) {
      if (tab.kind === 'mobile') {
        // Its package closes the simulator, as the release of the plugin's device resources does.
        mobile.removed(tab)
      }
      backend.apply({ type: 'remove', id: tab.id })
    }
    if (pluginId === 'browser') {
      browser.end()
    }
    disposePluginStores(host.pinia, pluginId)
  }
  /** A file pill, through the `edit` intent. */
  function selectEdit(edit: RequestEditSelection) {
    openIn({ intent: 'edit', payload: edit })
  }
  /** The work as it started: every tab closed, the first selection again. */
  function reset() {
    closeTabs(tabs.tabs.value.map((tab) => tab.id))
    history.value = selection === null ? [] : [selection]
    pinned.value = {}
    if (startsOpen && selection !== null) {
      restarting = true
      ask(selection, true)
    } else {
      steps.reset()
    }
  }
  /**
   * The agent shows `tab` with `demi browser show`: the browser tells the
   * plugin, which carries the count into the work tab, which the work then
   * selects.
   */
  async function agentShows(tab: string) {
    browser.show(tab)
    await nextTick()
    await plugin.settled()
  }
  /** The agent opens a page, shown with `--show` or not: the browser tells the plugin, which adds the tab. */
  async function agentOpens(url: string, show: boolean) {
    browser.agentOpens(url, { show })
    await nextTick()
    await plugin.settled()
  }
  return {
    tabsState,
    pinned,
    kinds,
    front,
    request,
    took,
    inFront,
    pick,
    browser,
    mobile,
    agentShows,
    agentOpens,
    enabled,
    setEnabled,
    host,
    select,
    add,
    move,
    update,
    updatePinned,
    closeTabs,
    openIn,
    selectEdit,
    reset,
  }
}

export type GalleryWork = ReturnType<typeof useGalleryWork>
