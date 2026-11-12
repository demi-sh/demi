import { nextTick, onBeforeUnmount, ref, type Ref } from 'vue'
import { tabMotion } from '../agent/tab-strip'
import { createLanding, DRAG_START_PX, dragShift, dragTarget, moved, swallowDropClick } from './drag-order'
import { prefersReducedMotion } from './stream-reveal'

/**
 * Drag to reorder the rows of a list, as a macOS list's rows are, down a
 * vertical list (`axis` y) or along a row of buttons, as a taskbar's (x).
 *
 * - A row pressed on its handle follows the pointer once it moves a few pixels.
 * - The other rows slide aside to show where it will land, and letting go
 *   glides it from where it was let go to its place. The click that letting go
 *   makes is not the row's.
 * - Escape, or the window losing focus, glides it back.
 * - Every slide keeps the window tab strip's motion (`tabMotion`), so a
 *   taskbar button moves as a window's tab does; none moves where the user
 *   asks for reduced motion.
 * - The handle also moves its row one place with the Up and Down arrow keys, or
 *   Left and Right along a row.
 *
 * The rows are the elements of `container` marked `data-reorder-id`, in order.
 * `commit` receives the ids in their new order, and the host's list shows it with
 * its next render.
 */
export function useListReorder(
  container: Readonly<Ref<HTMLElement | undefined | null>>,
  ids: () => readonly string[],
  commit: (ids: string[]) => void,
  axis: 'x' | 'y' = 'y',
) {
  const across = axis === 'x'
  /** The row being dragged, once it moved far enough to drag. */
  const dragging = ref<string | null>(null)
  let held: {
    pointerId: number
    at: number
    from: number
    to: number
    rows: HTMLElement[]
    centers: number[]
    step: number
  } | null = null

  function rows(): HTMLElement[] {
    return Array.from(container.value?.querySelectorAll<HTMLElement>('[data-reorder-id]') ?? [])
  }

  function unstyle(row: HTMLElement) {
    row.style.transition = ''
    row.style.transform = ''
    row.style.zIndex = ''
    row.style.position = ''
  }

  const landing = createLanding(axis, (_, value) => value, unstyle)

  /** Ends the press: the listeners go, and the rows stay where they show. */
  function release() {
    held = null
    dragging.value = null
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', finish)
    window.removeEventListener('pointercancel', putBack)
    window.removeEventListener('keydown', escape, true)
    window.removeEventListener('blur', putBack)
  }

  /** Glides a dragged row back where it was, and the others with it. */
  function putBack() {
    const listed = held?.rows ?? []
    const dragged = dragging.value !== null
    release()
    if (dragged) {
      landing.land(listed, landing.shownAt(listed))
    }
  }

  /** Puts every row in its place at once and ends the press, as the list goes. */
  function settle() {
    const listed = held?.rows ?? []
    release()
    landing.settle()
    listed.forEach(unstyle)
  }

  function move(event: PointerEvent) {
    if (!held || event.pointerId !== held.pointerId) {
      return
    }
    const offset = (across ? event.clientX : event.clientY) - held.at
    if (dragging.value === null) {
      if (Math.abs(offset) < DRAG_START_PX) {
        return
      }
      dragging.value = held.rows[held.from]?.dataset['reorderId'] ?? null
    }
    event.preventDefault()
    held.to = dragTarget(held.centers, held.from, held.centers[held.from]! + offset)
    const still = prefersReducedMotion()
    held.rows.forEach((row, index) => {
      if (index === held!.from) {
        row.style.transition = 'none'
        row.style.position = 'relative'
        row.style.zIndex = '1'
        row.style.transform = `${across ? 'translateX' : 'translateY'}(${offset}px)`
      } else {
        row.style.transition = still ? '' : tabMotion('transform')
        row.style.transform = `${across ? 'translateX' : 'translateY'}(${dragShift(index, held!.from, held!.to) * held!.step}px)`
      }
    })
  }

  function finish(event: PointerEvent) {
    if (!held || event.pointerId !== held.pointerId) {
      return
    }
    const { from, to, rows: listed } = held
    const dragged = dragging.value !== null
    release()
    if (!dragged) {
      return
    }
    swallowDropClick()
    const shown = landing.shownAt(listed)
    if (from !== to) {
      commit(moved(ids(), from, to))
    }
    // The host's order lands with the next render; until then the rows hold
    // where the drag left them, and then glide from there.
    void nextTick(() => landing.land(rows(), shown))
  }

  function escape(event: KeyboardEvent) {
    if (event.key !== 'Escape') {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    putBack()
  }

  /** The handle of row `id` was pressed. */
  function start(event: PointerEvent, id: string) {
    if (event.button !== 0) {
      return
    }
    // A row still landing from the last drag is put in its place, which this
    // drag starts from.
    settle()
    const listed = rows()
    const from = listed.findIndex((row) => row.dataset['reorderId'] === id)
    if (from < 0 || listed.length < 2) {
      return
    }
    const rects = listed.map((row) => row.getBoundingClientRect())
    held = {
      pointerId: event.pointerId,
      at: across ? event.clientX : event.clientY,
      from,
      to: from,
      rows: listed,
      centers: rects.map((rect) => (across ? rect.left + rect.width / 2 : rect.top + rect.height / 2)),
      // The others step aside by the dragged row's own length and the gap
      // between rows, whatever their own lengths.
      step: across
        ? rects[from]!.width + Math.max(0, rects[1]!.left - rects[0]!.right)
        : rects[from]!.height + Math.max(0, rects[1]!.top - rects[0]!.bottom),
    }
    event.preventDefault()
    window.addEventListener('pointermove', move, { passive: false })
    window.addEventListener('pointerup', finish)
    window.addEventListener('pointercancel', putBack)
    window.addEventListener('keydown', escape, true)
    window.addEventListener('blur', putBack)
  }

  /** A key on the handle of row `id`: Up and Down, or Left and Right along a row, move it one place. */
  function keydown(event: KeyboardEvent, id: string) {
    const [back, on] = across ? ['ArrowLeft', 'ArrowRight'] : ['ArrowUp', 'ArrowDown']
    const step = event.key === back ? -1 : event.key === on ? 1 : 0
    if (step === 0) {
      return
    }
    event.preventDefault()
    const order = ids()
    const from = order.indexOf(id)
    const to = from + step
    if (from < 0 || to < 0 || to >= order.length) {
      return
    }
    commit(moved(order, from, to))
  }

  onBeforeUnmount(settle)
  return { dragging, start, keydown }
}
