import { ref, shallowRef } from 'vue'
import type { HistoryDirection } from '@jush-ai/web-ui/plugins/page'

/** A step of a specimen's history: what it shows, open or closed. */
interface Shown {
  open: boolean
}

/**
 * The web browser's history of one specimen's view, as the product keeps it
 * in the page's address: `step` adds what `current` reads as a step after the
 * one shown, unless it says the same, `replace` takes the shown one's place,
 * and Back and Forward `restore` the step they reach, available while it
 * shows the view open. A step restored and then said again is no new step,
 * as the product's address does not change when it already says the same.
 */
export function galleryHistory<T extends Shown>(current: () => T, restore: (step: T) => void) {
  const steps = shallowRef<T[]>([current()])
  const at = ref(0)
  return {
    step(): void {
      const now = current()
      if (JSON.stringify(now) === JSON.stringify(steps.value[at.value])) {
        return
      }
      steps.value = [...steps.value.slice(0, at.value + 1), now]
      at.value = steps.value.length - 1
    },
    replace(): void {
      steps.value = steps.value.map((step, index) => (index === at.value ? current() : step))
    },
    canGo(direction: HistoryDirection): boolean {
      const next = steps.value[at.value + (direction === 'back' ? -1 : 1)]
      return current().open && next?.open === true
    },
    /** Goes back or forward a step, and says whether there was one. */
    go(direction: HistoryDirection): boolean {
      const index = at.value + (direction === 'back' ? -1 : 1)
      const next = steps.value[index]
      if (next) {
        at.value = index
        restore(next)
      }
      return next !== undefined
    },
    /** The history as a specimen starts it again: what it shows, alone. */
    reset(): void {
      steps.value = [current()]
      at.value = 0
    },
  }
}
