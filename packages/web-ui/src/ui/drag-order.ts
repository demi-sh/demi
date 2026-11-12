import { tabMotion } from '../agent/tab-strip'
import { prefersReducedMotion } from './stream-reveal'

/**
 * Where a dragged item of a strip or a list lands, and how the others make
 * room for it, as a macOS list's rows do, along either axis
 * (`useListReorder`); a tab strip steps its tabs aside the same way, and picks
 * the slot as Chrome does (`TabStrip`). Both glide their items to their places
 * when a drag ends or is put back (`createLanding`).
 */

/** How far the pointer moves before a pressed item starts to drag, so a click with a tremor stays a click. */
export const DRAG_START_PX = 4

/**
 * Takes the click that letting go of a dragged item makes, before the item
 * sees it, so ending a drag neither selects nor opens what was dragged.
 */
export function swallowDropClick(): void {
  const swallow = (event: MouseEvent) => {
    event.stopPropagation()
    event.preventDefault()
  }
  window.addEventListener('click', swallow, { capture: true, once: true })
  // A release off the item makes no click; the next one is the user's.
  setTimeout(() => window.removeEventListener('click', swallow, { capture: true }))
}

/**
 * Where a dragged item lands: its index among the others, the `move` index, by how
 * many of their centers its own center has passed.
 *
 * `centers` are the items' centers in order, the dragged one at `from` included, and
 * `center` is the dragged item's center where the pointer holds it.
 */
export function dragTarget(centers: readonly number[], from: number, center: number): number {
  return centers.filter((other, index) => index !== from && other < center).length
}

/**
 * Which way the item at `index` steps aside while the item at `from` is held
 * over the slot `to` (`dragTarget`): -1 toward the start, 1 toward the end,
 * 0 when it stays.
 */
export function dragShift(index: number, from: number, to: number): -1 | 0 | 1 {
  if (index > from && index - 1 < to) {
    return -1
  }
  if (index < from && index >= to) {
    return 1
  }
  return 0
}

/**
 * `all` with the items of `order`, a reordering of some of them, in that
 * order, each in a slot one of them held; the others keep theirs.
 */
export function rearranged<T>(all: readonly T[], order: readonly T[]): T[] {
  const moving = new Set(order)
  let next = 0
  return all.map((item) => (moving.has(item) ? order[next++]! : item))
}

/** `ids` with the one at `from` moved to the slot `to` among the others. */
export function moved<T>(ids: readonly T[], from: number, to: number): T[] {
  const others = ids.filter((_, index) => index !== from)
  others.splice(to, 0, ids[from]!)
  return others
}

/**
 * The items of a strip or a list gliding to their places once a drag ends or
 * is put back, as Chrome's tabs and a macOS list's rows do: each from where it
 * showed to where the new order puts it, in the tabs' motion (`tabMotion`).
 * None glides where the user asks for reduced motion.
 *
 * - `transition` gives the inline transition an item takes for `value`, so a
 *   host keeps a motion of its own beside it, as a closing tab's width.
 * - `rest` takes the drag's styles off an item once it is in place, gliding
 *   or not.
 * - `done` hears that the last glide ended, or that none started.
 */
export function createLanding(
  axis: 'x' | 'y',
  transition: (item: HTMLElement, value: string) => string,
  rest: (item: HTMLElement) => void,
  done: () => void = () => {},
) {
  const across = axis === 'x'
  /** The items gliding, each with what ends its glide. */
  const gliding = new Map<HTMLElement, () => void>()

  function position(item: HTMLElement): number {
    const rect = item.getBoundingClientRect()
    return across ? rect.left : rect.top
  }

  /** Where each of `items` shows now, its motion included. */
  function shownAt(items: readonly HTMLElement[]): Map<HTMLElement, number> {
    return new Map(items.map((item) => [item, position(item)]))
  }

  /**
   * Takes the drag's offsets off `items`, now in the order they land in, and
   * glides each from where it showed (`shown`) to its place.
   */
  function land(items: readonly HTMLElement[], shown: Map<HTMLElement, number>): void {
    for (const item of items) {
      item.style.transition = transition(item, 'none')
      item.style.transform = ''
    }
    const moving = items.filter((item) => {
      const was = shown.get(item)
      const from = was === undefined ? 0 : was - position(item)
      if (!item.isConnected || Math.abs(from) < 0.5 || prefersReducedMotion()) {
        rest(item)
        return false
      }
      item.style.transform = `${across ? 'translateX' : 'translateY'}(${from}px)`
      return true
    })
    // The offsets apply before the glide back from them starts.
    void items[0]?.offsetWidth
    for (const item of moving) {
      item.style.transition = transition(item, tabMotion('transform'))
      item.style.transform = ''
      const ended = (event: TransitionEvent): void => {
        if (event.target === item && event.propertyName === 'transform') {
          landed()
        }
      }
      const landed = (): void => {
        item.removeEventListener('transitionend', ended)
        item.removeEventListener('transitioncancel', ended)
        gliding.delete(item)
        rest(item)
        if (gliding.size === 0) {
          done()
        }
      }
      item.addEventListener('transitionend', ended)
      item.addEventListener('transitioncancel', ended)
      gliding.set(item, landed)
    }
    if (gliding.size === 0) {
      done()
    }
  }

  /** Ends every glide at once, each item in its place. */
  function settle(): void {
    for (const [item, landed] of [...gliding]) {
      item.style.transition = transition(item, 'none')
      item.style.transform = ''
      landed()
    }
  }

  return {
    shownAt,
    land,
    settle,
    /** Whether an item is still gliding. */
    busy: () => gliding.size > 0,
  }
}
