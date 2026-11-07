/**
 * A conversation's work as the backend keeps it, and the browser
 * plugin's part in its tabs, in memory: the gallery's windows change the way the product's do, a beat late, over the
 * gallery's own browser.
 */
import { BrowserTabsError, addedWorkTab, browserTabDataSchema, type BrowserTabData, type TabFailure } from '@demicodes/plugin-browser/live/tabs'
import { SERVER_FAILED } from './plugins'
import { applyWorkChange, type WorkAnswer, type WorkBackend, type WorkChange, type WorkRead } from '@demicodes/web-ui/agent/work-changes'
import type { WorkTab } from '@demicodes/web-ui/agent/work-tabs'
import type { BrowserTab } from '@demicodes/plugin-browser/generated/plugin'
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

/** The browser tab a work tab shows, while the browser has it. */
function live(data: BrowserTabData): string | undefined {
  return data.closed ? undefined : data.tab
}

/**
 * The work tab that shows the browser tab whose page opened `tab`; none for
 * a tab no page opened, or whose opener the work does not have.
 */
function openerFor(record: GalleryWorkRecord, tab: BrowserTab): string | undefined {
  if (tab.createdBy.kind !== 'page') {
    return undefined
  }
  const opener = tab.createdBy.opener
  return record.all.find((each) => {
    const data = each.kind === 'browser' ? browserData(each) : null
    return data !== null && live(data) === opener
  })?.id
}

/**
 * Where a tab `opener` opened goes: right after it, behind the tabs it opened
 * before that still stand right after it, as Chrome places the tabs a link
 * opens and the backend's plugin places them; after the others (undefined)
 * when the work no longer has the opener.
 */
function openedIndex(record: GalleryWorkRecord, opener: string): number | undefined {
  const tabs = record.all
  const at = tabs.findIndex((tab) => tab.id === opener)
  if (at < 0) {
    return undefined
  }
  let end = at + 1
  while (end < tabs.length && browserData(tabs[end]!)?.openedBy === opener) {
    end += 1
  }
  return end
}

/** The browser plugin's part in the work's `browser` tabs, as the backend's plugin does it. */
export interface GalleryBrowserPlugin {
  /** Opens a browser tab for the work tab, unless it shows one; answers the tab it shows then, as the plugin does. */
  bind(workTab: string): Promise<string | null>
  /** Closes the browser tab of a work tab its user removed. */
  removed(tab: WorkTab): Promise<void>
  /**
   * The browser told the plugin its tabs changed: adds the agent's and the
   * pages' tabs, removes the ones closed on purpose and marks the ones the browser lost.
   */
  tabsChanged(): Promise<void>
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

  return {
    bind: (workTab) => inTurn(async () => {
      const data = browserData(record.tab(workTab))
      if (!data) {
        return null
      }
      const shown = live(data)
      if (shown !== undefined && !data.failure) {
        return shown
      }
      try {
        // The tab whose page failed to open loads it again; any other gets a
        // new tab, bound before its page opens, as the backend's plugin does.
        const made = shown === undefined
        const tab = shown ?? (await browser.newTab()).id
        record.apply({ type: 'update', id: workTab, data: { tab, closed: null, failure: null } })
        const now = browserData(record.tab(workTab))
        if (!now) {
          await browser.close(tab)
          return null
        }
        // A new tab is blank already.
        if (!made || now.url !== 'about:blank') {
          await browser.navigate(tab, now.url)
        }
        return tab
      } catch (error) {
        // The browser's or the device's own reason, as the backend's plugin keeps
        // it, or the status a call would answer a failure without one with.
        const message = error instanceof Error ? error.message : String(error)
        const reason = error instanceof BrowserTabsError ? error.reason : null
        const failure: TabFailure = reason ? { code: reason, message } : { status: SERVER_FAILED, message }
        record.apply({ type: 'update', id: workTab, data: { failure } })
        return null
      }
    }),
    removed: (tab) => inTurn(async () => {
      const data = browserData(tab)
      const shown = data ? live(data) : undefined
      if (shown !== undefined) {
        await browser.close(shown)
      }
    }),
    settled: () => turn,
    tabsChanged: () => inTurn(async () => {
      const listed = browser.listed.value.tabs
      const bound = record.all
        .filter((tab) => tab.kind === 'browser')
        .map((tab) => ({ id: tab.id, data: browserData(tab) }))
      const shown = new Set(bound.map((tab) => tab.data?.tab))
      for (const tab of listed) {
        if (tab.createdBy.kind !== 'user' && !shown.has(tab.id)) {
          const data: BrowserTabData = { url: tab.url, tab: tab.id, title: tab.title }
          if (tab.shows > 0) {
            data.shows = tab.shows
          }
          const opener = openerFor(record, tab)
          if (opener !== undefined) {
            data.openedBy = opener
          }
          // Beside its opener, as the backend's plugin places it.
          const index = opener === undefined ? undefined : openedIndex(record, opener)
          record.apply({ type: 'create', tab: { id: addedWorkTab(tab.id), kind: 'browser', data }, index })
        }
      }
      for (const { id, data } of bound) {
        const tab = data ? live(data) : undefined
        if (tab === undefined) {
          continue
        }
        const present = listed.find((candidate) => candidate.id === tab)
        if (!present && browser.closedOnPurpose.has(tab)) {
          record.apply({ type: 'remove', id })
        } else if (!present) {
          record.apply({ type: 'update', id, data: { closed: true } })
        } else {
          // The tab follows its page's address and title, and the agent's
          // showings, which each page selects once. One whose page failed to
          // open keeps its own address, as the backend's plugin keeps it.
          const follows: Partial<BrowserTabData> = {}
          if (!data?.failure && present.url !== data?.url) {
            follows.url = present.url
          }
          if (!data?.failure && present.title !== data?.title) {
            follows.title = present.title
          }
          if (present.shows > (data?.shows ?? 0)) {
            follows.shows = present.shows
          }
          if (Object.keys(follows).length > 0) {
            record.apply({ type: 'update', id, data: follows })
          }
        }
      }
    }),
  }
}
