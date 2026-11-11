import { shallowRef, watch, type ShallowRef } from 'vue'
import type { TabsState, PinnedTabs } from '../agent/work-tabs'
import type { BoundTabKind } from '../agent/tab-kinds/kind'
import type { TabUpdate } from '../plugins/page'
import { useWindowStage, type StageState, type WindowStageModel } from './stage'

/**
 * A tab, or none, that something outside the windows asks to have in front:
 * a pill, a link, a kind's `shows`, a plugin, or a navigation of the web
 * browser's history. The windows take it once its tab's window can show it.
 */
export interface WorkRequest {
  id: number
  /**
   * The tab whose window comes to the front on it; none minimizes the work's
   * windows, unless `opens` names a tab to come.
   */
  tab: string | null
  /**
   * A tab of a kind whose plugin makes its tabs (`TabKind.open`), with its
   * data: the plugin is asked for it, and its window comes to the front to
   * wait for it.
   */
  opens?: { kind: string; data: unknown }
  /** The address names it already: the windows say what is in front only once they took it. */
  fromAddress: boolean
}

/**
 * Why the windows say what is in front: their first word since the
 * conversation came on screen (`came`), the window in front or its tab
 * closed (`closed`), or anything else (`moved`).
 */
export type FrontCause = 'came' | 'closed' | 'moved'

/**
 * What the host gives a conversation's windows: its tabs and their kinds, the
 * layout the conversation keeps, and the user's pins. The windows alone say
 * which of them is in front; the host writes the page's address from what
 * they say (`inFront`), and asks them for a tab through `request`. The
 * product's comes from the work store (`web/conversation/work.ts`), the
 * gallery's from its fixture.
 */
export interface SessionWork {
  /** The tabs, in order, and the order they were shown in, the newest last, which each window shows its newest of. */
  tabsState: TabsState
  pinned: PinnedTabs
  /** The kinds of the pages the user has on, bound to the conversation. */
  kinds: readonly BoundTabKind[]
  /** Whether the host has read the tabs: the ones a first read brings come with no motion. */
  tabsRead: boolean
  /** The conversation's first send has not created it yet: Files and Changes say when they come. */
  beforeFirstMessage: boolean
  /** The name of the primary device while its runner is not connected, which a window says over its tabs. */
  deviceOffline: string | null
  /** What the windows have yet to take, while there is something. */
  request: WorkRequest | null
  /** The windows took the request `id`. */
  took(id: number): void
  /** The tab of the work's window in front, or none: what the address names. */
  inFront(tab: string | null, cause: FrontCause): void
  /** The user picked `tab`: the newest its window showed. */
  pick(tab: string): void
  moveTab(id: string, index: number): void
  updateTab(id: string, data: unknown, how?: TabUpdate): void
  updatePinned(kind: string, data: unknown, how?: TabUpdate): void
  closeTabs(ids: string[]): void
  /** A new tab of `kind` with `data`, selected. */
  add(kind: string, data: unknown): void
  /** The layout the conversation keeps, null while it has none. */
  layout: StageState | null
  /** Keeps the layout with the conversation. */
  keepLayout(layout: StageState): void
  /** The windows the user pinned to the taskbar, for every conversation, in order. */
  pins: readonly string[]
  /** Keeps `pins` as the user's pins, in their order. */
  keepPins(pins: readonly string[]): void
}

/**
 * The stage of the conversation `conversation()` names: made from the layout
 * it keeps when it comes on screen, and kept with it at each change that ends
 * (`settled`). The layout is read once per showing: one kept by another
 * device while this one shows the conversation shows the next time, and one
 * that comes after the conversation does replaces the stage only while
 * nothing moved in it. A page that does not write the layout, a phone's,
 * opens the conversation with every window put away.
 */
export function useSessionStage(
  conversation: () => string,
  work: () => SessionWork | undefined,
  writes: () => boolean,
): ShallowRef<WindowStageModel> {
  const made = (layout: StageState | undefined) => useWindowStage(layout, { putAway: !writes() })
  const stage = shallowRef(made(work()?.layout ?? undefined))
  watch(conversation, () => {
    stage.value = made(work()?.layout ?? undefined)
  })
  watch(() => work()?.layout ?? null, (layout, before) => {
    if (layout && !before && stage.value.settled.value === 0) {
      stage.value = made(layout)
    }
  })
  // A stage made anew holds what was kept: only a change in it is written, and
  // an operation that moved nothing, such as a press in the window in front,
  // writes nothing.
  let kept = JSON.stringify(stage.value.state)
  watch([stage, () => stage.value.settled.value], ([now], [before]) => {
    const layout = JSON.stringify(now.state)
    const host = work()
    if (now === before && layout !== kept && host && writes()) {
      host.keepLayout(JSON.parse(layout) as StageState)
    }
    kept = layout
  })
  return stage
}
