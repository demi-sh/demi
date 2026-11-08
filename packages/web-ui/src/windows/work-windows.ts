import { activeTab } from '../agent/tab-close'
import { pinnedData, type TabsState, type PinnedTabs } from '../agent/work-tabs'
import { resolveTab, type BoundTabKind, type WindowKind } from '../agent/tab-kinds/kind'

/**
 * The windows the conversation's tabs open in: each kind names its window
 * (`BoundTabKind.window`), and the kinds of one window share it.
 *
 * - A pinned kind is one tab of its window, named by the kind's id, there in
 *   every conversation.
 * - A tab of a kind the page does not have, or whose data its kind does not
 *   read, has no window; it stays saved, as the backend keeps it.
 * - A window shows the newest of its tabs the history selected, else its
 *   first, as the tabs of one strip did.
 */

/** A tab of a work window, with its kind and checked data. */
export interface ShownTab {
  id: string
  kind: BoundTabKind
  data: unknown
  title: string
  /** What it shows, by name alone, such as a file's: what its pane says it reads while it waits, and a single view's title. */
  subject: string
  /** It is its kind's one tab, which no Close closes. */
  pinned: boolean
}

export interface WorkWindow {
  window: WindowKind
  tabs: ShownTab[]
  /** The tab it shows. */
  selected: string | null
  /** The kind whose new tab its + opens, if one of its kinds opens tabs. */
  starts: BoundTabKind | null
  /** The kind whose rows its entry in the taskbar's Open lists, if one of its kinds has some. */
  offers: BoundTabKind | null
}

/** The windows of `kinds` in their order, each with its tabs: its pinned kinds' first, then the work's in order. */
export function workWindows(tabsState: TabsState, pinned: PinnedTabs, kinds: readonly BoundTabKind[]): WorkWindow[] {
  const windows = new Map<string, WorkWindow>()
  const windowOf = (kind: BoundTabKind): WorkWindow => {
    let found = windows.get(kind.window.id)
    if (!found) {
      found = { window: kind.window, tabs: [], selected: null, starts: null, offers: null }
      windows.set(kind.window.id, found)
    }
    return found
  }
  for (const kind of kinds) {
    const window = windowOf(kind)
    if (kind.start && !kind.pinned && !window.starts) {
      window.starts = kind
    }
    if (kind.openMenu && !window.offers) {
      window.offers = kind
    }
    if (kind.pinned) {
      const data = pinnedData(pinned, kind)
      window.tabs.push({ id: kind.kind, kind, data, title: kind.title(data), subject: kind.subject?.(data) ?? kind.title(data), pinned: true })
    }
  }
  for (const tab of tabsState.tabs) {
    const resolved = resolveTab(tab, kinds)
    if (resolved.kind) {
      const { kind, data, title } = resolved
      windowOf(kind).tabs.push({ id: tab.id, kind, data, title, subject: kind.subject?.(data) ?? title, pinned: false })
    }
  }
  for (const window of windows.values()) {
    window.selected = activeTab(window.tabs.map((tab) => tab.id), tabsState.history)
  }
  return [...windows.values()]
}

/** The window that holds the tab `id`, or null. */
export function windowHolding(windows: readonly WorkWindow[], id: string): WorkWindow | null {
  return windows.find((window) => window.tabs.some((tab) => tab.id === id)) ?? null
}
