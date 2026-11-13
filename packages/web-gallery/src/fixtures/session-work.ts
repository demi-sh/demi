import { computed, ref, type ComputedRef } from 'vue'
import type { SessionWork } from '@jush-ai/web-ui/windows/session-work'
import { readStage, type StageState } from '@jush-ai/web-ui/windows/stage'
import type { GalleryWork } from './work'

const PINS_KEY = 'jush-gallery.taskbar-pins'
const LAYOUT_KEY = 'jush-gallery.windows.'

function readKept(key: string): unknown {
  try {
    return JSON.parse(localStorage.getItem(key) ?? 'null')
  } catch {
    // A value this gallery did not write is no layout.
    return null
  }
}

function readPins(): string[] {
  const kept = readKept(PINS_KEY)
  return Array.isArray(kept) ? kept.filter((pin): pin is string => typeof pin === 'string') : []
}

/** The user's pins, one list for every specimen, as the product's preferences keep one for every conversation. */
const pins = ref<readonly string[]>(readPins())

/**
 * A specimen's work as its windows read it, over `work`. The product keeps a
 * conversation's layout with it and the pins in the user's preferences; the
 * gallery keeps the pins in this browser, and the layout of a specimen that
 * names where (`keep`), so it opens as the user left it.
 */
export function gallerySessionWork(
  work: GalleryWork,
  { keep, beforeFirstMessage = () => false, deviceOffline = () => null }: {
    keep?: string
    beforeFirstMessage?: () => boolean
    deviceOffline?: () => string | null
  } = {},
): ComputedRef<SessionWork> {
  const layout = keep === undefined ? null : readStage(readKept(LAYOUT_KEY + keep))
  return computed(() => ({
    tabsState: work.tabsState.value,
    pinned: work.pinned.value,
    kinds: work.kinds.value,
    tabsRead: true,
    beforeFirstMessage: beforeFirstMessage(),
    deviceOffline: deviceOffline(),
    request: work.request.value,
    took: work.took,
    inFront: work.inFront,
    pick: work.pick,
    moveTab: work.move,
    updateTab: work.update,
    updatePinned: work.updatePinned,
    closeTabs: work.closeTabs,
    add: (kind, data) => {
      work.add(kind, data)
    },
    layout,
    keepLayout: (kept: StageState) => {
      if (keep !== undefined) {
        localStorage.setItem(LAYOUT_KEY + keep, JSON.stringify(kept))
      }
    },
    pins: pins.value,
    keepPins: (kept) => {
      pins.value = [...kept]
      localStorage.setItem(PINS_KEY, JSON.stringify(pins.value))
    },
  }))
}
