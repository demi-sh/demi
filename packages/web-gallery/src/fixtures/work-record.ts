/**
 * A conversation's work as the backend keeps it, and the browser
 * plugin's part in its tabs, in memory: the gallery's windows change the way the product's do, a beat late, over the
 * gallery's own browser.
 */
import { addedWorkTab, browserTabDataSchema, type BrowserTabData } from '@jush-ai/plugin-browser/live/tabs'
import { applyWorkChange, dataChanges, type WorkAnswer, type WorkBackend, type WorkChange, type WorkRead } from '@jush-ai/web-ui/agent/work-changes'
import type { WorkTab } from '@jush-ai/web-ui/agent/work-tabs'
import type { GalleryBrowser } from './live-browser'

/** How long the backend takes over a change of the work. */
const CHANGE_MS = 120

function beat(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, CHANGE_MS))
}


export class GalleryWorkRecord implements WorkBackend {
  private revision = 0
  private tabs: readonly WorkTab[] = []
  /** Every id the work had, which is never used again. */
  private readonly retired = new Set<string>()
  /**
   * Told of a tab its user removed, once the change is answered, as a kind's
   * plugin is; a tab the user created is none of the plugin's business.
   */
  removed: ((tab: WorkTab) => void) | null = null
  /** Each change, the plugin's included, with the work as it is then, as the conversation's entry in the list brings it. */
  changed: ((record: WorkRead) => void) | null = null

  /** The work as it is now, as the conversation's entry in the list carries it. */
  current(): WorkRead {
    return { revision: this.revision, tabs: this.tabs.map((tab) => ({ ...tab })) }
  }

  /**
   * A page's changes, as one change of the work: the kind's plugin hears
   * of each tab its user removed.
   */
  async send(changes: readonly WorkChange[]): Promise<WorkAnswer> {
    await beat()
    const before = this.revision
    const removed = changes.flatMap((change) => this.change(change) ?? [])
    if (this.revision > before) {
      // The request counts as one change, however many it made.
      this.revision = before + 1
      const record = this.current()
      queueMicrotask(() => this.changed?.(record))
    }
    for (const tab of removed) {
      queueMicrotask(() => this.removed?.(tab))
    }
    return { revision: this.revision, changed: this.revision > before }
  }

  /** Applies the plugin's `change`, which no plugin is told of. */
  apply(change: WorkChange): void {
    const before = this.revision
    this.change(change)
    if (this.revision > before) {
      const record = this.current()
      queueMicrotask(() => this.changed?.(record))
    }
  }

  /**
   * Makes the tabs of `kind` the tabs `listed`, the tabs a device has open,
   * as one change, as the backend mirrors them: creates each listed tab the
   * work never had, at its index; sets the data of each it has to the listed
   * data; and removes each tab of the kind the list does not name. Answers
   * the listed ids the work retired, tabs their user closed.
   */
  mirror(kind: string, listed: readonly { id: string; data: Record<string, unknown>; index?: number }[]): string[] {
    const before = this.revision
    const closed: string[] = []
    const named = new Set(listed.map((tab) => tab.id))
    for (const { id, data, index } of listed) {
      const current = this.tab(id)
      if (this.retired.has(id)) {
        closed.push(id)
      } else if (current) {
        this.change({ type: 'update', id, data: dataChanges(current.data, data) })
      } else {
        this.change({ type: 'create', tab: { id, kind, data }, index })
      }
    }
    for (const tab of this.tabs.filter((each) => each.kind === kind && !named.has(each.id))) {
      this.change({ type: 'remove', id: tab.id })
    }
    if (this.revision > before) {
      // The mirroring counts as one change, however many it made.
      this.revision = before + 1
      const record = this.current()
      queueMicrotask(() => this.changed?.(record))
    }
    return closed
  }

  /** Applies `change`, counting it in the revision when it changes the tabs; answers the tab it removed, if any. */
  private change(change: WorkChange): WorkTab | null {
    if (change.type === 'create' && (this.retired.has(change.tab.id) || this.tab(change.tab.id))) {
      return null
    }
    const removed = change.type === 'remove' ? this.tab(change.id) : null
    const next = applyWorkChange(this.tabs, change)
    if (JSON.stringify(next) === JSON.stringify(this.tabs)) {
      return null
    }
    this.tabs = next
    this.revision += 1
    if (removed) {
      this.retired.add(removed.id)
    }
    return removed
  }

  tab(id: string): WorkTab | null {
    return this.tabs.find((tab) => tab.id === id) ?? null
  }

  get all(): readonly WorkTab[] {
    return this.tabs
  }
}

/** The data of a `browser` tab; none for one that does not fit, which the plugin leaves as it is. */
function browserData(tab: WorkTab | null): BrowserTabData | null {
  const parsed = browserTabDataSchema.safeParse(tab?.data)
  return parsed.success ? parsed.data : null
}

/** A work tab as the mirroring places others beside it: the browser tab it shows, and the work tab that opened it. */
interface Placed {
  id: string
  shows: string | null
  openedBy: string | null
}

function placed(tab: WorkTab): Placed {
  const data = tab.kind === 'browser' ? browserData(tab) : null
  return { id: tab.id, shows: data?.tab ?? null, openedBy: data?.openedBy ?? null }
}

/**
 * Where a tab the work tab at `at` opened goes: right after it, behind the
 * tabs it opened before that still stand right after it, as Chrome places
 * the tabs a link opens and the backend's plugin places them.
 */
function afterOpened(order: readonly Placed[], at: number): number {
  const opener = order[at]!.id
  let end = at + 1
  while (end < order.length && order[end]!.openedBy === opener) {
    end += 1
  }
  return end
}

/** The browser tab a work tab the plugin made shows. */
function browserTab(workTab: string): string | null {
  return workTab.startsWith('browser-') ? workTab.slice('browser-'.length) : null
}

/** The browser plugin's part in the work's `browser` tabs, as the backend's plugin does it. */
export interface GalleryBrowserPlugin {
  /** Closes the browser tab of a work tab its user removed. */
  removed(tab: WorkTab): Promise<void>
  /** The browser told the plugin its tabs changed: the work's `browser` tabs become the ones it lists (`mirror`). */
  tabsChanged(): Promise<void>
  /**
   * Makes the work's `browser` tabs the browser's, now: a tab for each,
   * placed beside its opener or at the end, and none for a tab the list does
   * not name; a listed tab whose work tab the user closed is closed.
   */
  mirror(): void
  /** Resolves once the plugin did the work it was given so far. */
  settled(): Promise<void>
}

export function galleryBrowserPlugin(browser: GalleryBrowser, record: GalleryWorkRecord): GalleryBrowserPlugin {
  // One piece of the conversation's work at a time, as the plugin does it.
  let turn: Promise<void> = Promise.resolve()
  function inTurn<T>(work: () => Promise<T>): Promise<T> {
    const run = turn.then(work)
    // The next piece waits for this one however it ends; its caller hears how.
    turn = run.then(() => {}, () => {})
    return run
  }

  function mirror(): void {
    const order = record.all.map(placed)
    const shown = new Set(order.map((tab) => tab.shows))
    const tabs = browser.listed.value.tabs.map((tab) => {
      const id = addedWorkTab(tab.id)
      const opener = tab.createdBy.kind === 'page' ? tab.createdBy.opener : null
      const openedBy = opener === null ? null : addedWorkTab(opener)
      let index: number | undefined
      if (!shown.has(tab.id)) {
        // Beside its opener, as the backend's plugin places it.
        const at = opener === null ? -1 : order.findIndex((each) => each.shows === opener)
        index = at < 0 ? undefined : afterOpened(order, at)
        order.splice(index ?? order.length, 0, { id, shows: tab.id, openedBy })
      }
      const data: BrowserTabData = { tab: tab.id, url: tab.url, title: tab.title }
      if (tab.shows > 0) {
        data.shows = tab.shows
      }
      if (openedBy !== null) {
        data.openedBy = openedBy
      }
      return { id, data, index }
    })
    for (const closed of record.mirror('browser', tabs)) {
      const tab = browserTab(closed)
      if (tab !== null) {
        void browser.close(tab)
      }
    }
  }

  return {
    removed: (tab) => inTurn(async () => {
      const data = browserData(tab)
      if (data) {
        await browser.close(data.tab)
      }
    }),
    settled: () => turn,
    tabsChanged: () => inTurn(async () => mirror()),
    mirror,
  }
}
