<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type ComponentPublicInstance } from 'vue'
import {
  TAB_EASING,
  TAB_MIN_PX,
  TAB_MOTION_MS,
  TAB_TRANSITION,
  afterEnterTab,
  afterLeaveTab,
  beforeEnterTab,
  beforeLeaveTab,
  closeWouldWiden,
  closingProgress,
  cutMarkCover,
  dropSlot,
  enterTab,
  freezeWidths,
  isLeavingTab,
  fadeRoom,
  reorderHysteresis,
  sidewaysWheel,
  keepingCloseMotion,
  leaveTab,
  resettleClosing,
  revealScroll,
  settledScrollWidth,
  settledTabBounds,
  stripGap,
  tabEased,
  tabMotion,
  tabsChanged,
  thawWidths,
  widthsFrozen,
} from './tab-strip'
import { createLanding, DRAG_START_PX, dragShift, swallowDropClick } from '../ui/drag-order'
import { usePhone } from '../ui/phone'

/** The edge fades' width; a revealed tab is scrolled clear of them. */
const FADE_PX = 24

/**
 * The row every tab bar uses.
 *
 * Width. Its tabs share its room equally (`TabItem`):
 *
 * - Each fits its content up to its maximum width, and the room a shorter one
 *   leaves goes to the others.
 * - None is narrower than `TAB_MIN_PX`, so a long title never widens its tab
 *   past the others', and selecting a tab changes no tab's width.
 *
 * Overflow. When the tabs cannot all fit at that least width:
 *
 * - The strip scrolls with no scrollbar and fades out at whichever edge has
 *   more behind it.
 * - The host's `overflow` slot, such as a button that lists every tab, shows at
 *   the strip's end.
 * - A tab mark that an edge cuts stays under the fade's solid start, so a mark
 *   at an edge shows whole or not at all, never sliced.
 *
 * The selected tab always shows whole, its close control included, and no fade
 * lies over it. Whenever the selected tab changes, a tab enters, the strip
 * resizes or the selected tab does (as when its title arrives), the strip
 * scrolls until that tab shows whole with a fade's width beside it. Where the
 * view has no room for that, the fades give way beside the tab. No tab is wider
 * than the strip's view, so a very narrow strip truncates its title instead of
 * cutting it. Unselected tabs may lie under the fades.
 *
 * The scroll is the strip's own, timed and eased like the tab motion, and aimed
 * at the settled layout: a tab on its way out takes no room and a tab on its
 * way in its full width, so one motion lands the tab where it ends up. Close
 * and insert collapse or grow from the current width. A close is one motion:
 * the tabs that stay move to their new widths as it collapses, and a scroll it
 * brings, such as back from an end that came into reach, keeps step with the
 * collapse, so the browser never pulls the strip back after it. The wheel
 * scrolls the strip along its axis, so a vertical wheel over the tabs moves
 * them sideways instead of the page.
 *
 * Slots. The host passes its `TabItem`s as children and sizes the strip in its
 * row. The `end` slot holds a control right after the last tab, as Chrome's
 * New Tab: while the tabs scroll it stays in view at the strip's end, the tabs
 * fading under its start. The `leading` slot holds tabs that stand ahead of
 * them and never scroll, such as a window's pinned tabs. They are elements of
 * `role="tab"` like the children, in the same tab list, each of one width
 * whatever is selected,
 * which the strip's own tabs never squeeze. A divider parts them from the
 * strip's own tabs while there are any.
 *
 * `surface` is what the row sits on. On the base surface the active tab is
 * raised to the surface color; on a raised surface it is pressed to the base
 * color, and hover uses the active color either way. The tabs and the edge
 * fades take their colors from the strip, so a host sets this once.
 *
 * The arrow keys, Home and End move the keyboard's focus between the tabs, the
 * leading ones included.
 *
 * On a phone:
 *
 * - The tabs never shrink: each keeps its content's width up to its maximum,
 *   the leading tabs too.
 * - The leading tabs and the strip's own scroll as one row under a finger, with
 *   no scrollbar and their overscroll kept to the row.
 * - The selected tab is revealed as above, a leading one included.
 * - A touch there scrolls the row and drags no tab.
 *
 * Motion. Every motion of the tabs is Chrome's: 200 ms, easing out
 * (`TAB_MOTION_MS`). Closing a tab with the pointer, by its close control or a
 * middle click, starts Chrome's tab closing mode while the strip has narrowed
 * its tabs: the others keep their widths, so the next tab's close control
 * slides under the pointer, until the pointer leaves the strip, the strip
 * resizes or a tab opens, when they move to their shares.
 *
 * Dragging. A host that listens to `reorder` lets the user drag a tab along
 * the strip, as in Chrome:
 *
 * - The tab follows the pointer anywhere along the strip, up to its end
 *   control, not only as far as the last tab.
 * - It takes a slot once it covers half of it (`dropSlot`), the others
 *   stepping aside, and a pointer at rest on the line between two slots does
 *   not swap them back and forth (`reorderHysteresis`).
 * - Held at the view's edge of a strip that overflows, it scrolls the strip.
 * - On release, `reorder` names where the tab was and its index among the
 *   others, which the host applies, and the tab glides from where it was let
 *   go to its place. Escape glides it back.
 */
const props = withDefaults(defineProps<{
  surface?: 'base' | 'raised'
  /**
   * A tab dragged from `from` to `to`, its index among the others; without it
   * tabs do not drag.
   */
  onReorder?: (from: number, to: number) => void
  /**
   * Tabs come and go at once, with no motion, while this holds: as when a menu
   * replaces the tab shown with another, or the strip shows another
   * conversation's tabs, so nothing opened or closed. Nothing of a tab is
   * measured or locked then. The strip reveals its selected tab once, after the
   * change.
   */
  still?: boolean
}>(), {
  surface: 'base',
})
// The tabs' colors, and the least width a tab keeps (`TabItem`).
const vars = computed(() => ({
  '--tab-min': `${TAB_MIN_PX}px`,
  '--tab-motion-ms': `${TAB_MOTION_MS}ms`,
  '--tab-easing': TAB_EASING,
  ...(props.surface === 'base'
    ? {
        '--tab-row': 'var(--surface-base)',
        '--tab-active': 'var(--surface)',
        '--tab-hover': 'var(--surface)',
      }
    : {
        '--tab-row': 'var(--surface)',
        '--tab-active': 'var(--surface-base)',
        '--tab-hover': 'var(--surface-base)',
      }),
}))
const phone = usePhone()
const root = ref<HTMLElement | null>(null)
const list = ref<HTMLElement | null>(null)
const leading = ref<HTMLElement | null>(null)
const el = ref<HTMLElement | null>(null)
const fades = ref<HTMLElement | null>(null)
/**
 * The selected tab the resize observer watches, so a title that widens it
 * reveals it again.
 */
let watchedTab: HTMLElement | null = null
const moreBefore = ref(false)
const moreAfter = ref(false)
/**
 * Whether the tabs cannot all fit at their least width, so the strip scrolls
 * and shows its `overflow` slot.
 */
const overflowing = ref(false)
let resizeObserver: ResizeObserver | null = null
let mutationObserver: MutationObserver | null = null
let scrollFrame: number | null = null
/** Where a scroll that keeps step with closing tabs ends. */
let followTarget: number | null = null

function cancelScroll(): void {
  if (scrollFrame !== null) {
    cancelAnimationFrame(scrollFrame)
    scrollFrame = null
  }
  followTarget = null
}

/**
 * Lands a scroll that kept step with tabs that have now closed where it was
 * going, before anything else moves the strip: the close may end between two
 * of its frames.
 */
function finishFollow(strip: HTMLElement): void {
  const tabs = el.value
  if (followTarget !== null && (!tabs || closingProgress(tabs) === null)) {
    strip.scrollLeft = followTarget
    cancelScroll()
  }
}

// The same length and curve as the tabs' motion, so the two read as one.
// While tabs close, the scroll follows their collapse itself: a timed scroll
// would run ahead of it or behind, and the tabs on one side would move first.
function scrollStripTo(strip: HTMLElement, target: number): void {
  cancelScroll()
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
    strip.scrollLeft = target
    return
  }
  const start = strip.scrollLeft
  const startedAt = performance.now()
  const tabs = el.value
  const closedAt = tabs ? closingProgress(tabs) : null
  followTarget = closedAt === null ? null : target
  const step = (now: number): void => {
    let eased: number
    if (closedAt !== null && tabs) {
      const closed = closingProgress(tabs)
      eased = closed === null || closedAt >= 1 ? 1 : Math.min(1, Math.max(0, (closed - closedAt) / (1 - closedAt)))
    } else {
      eased = tabEased(Math.min(1, (now - startedAt) / TAB_MOTION_MS))
    }
    strip.scrollLeft = start + (target - start) * eased
    if (eased < 1) {
      scrollFrame = requestAnimationFrame(step)
    } else {
      scrollFrame = null
      followTarget = null
    }
  }
  scrollFrame = requestAnimationFrame(step)
}

function bindEl(instance: Element | ComponentPublicInstance | null): void {
  el.value =
    instance instanceof HTMLElement
      ? instance
      : instance && '$el' in instance
        ? instance.$el
        : null
}

/**
 * What scrolls: the strip's own tabs, or on a phone the whole tab list, the
 * leading tabs with them.
 */
function scroller(): HTMLElement | null {
  return phone.value ? list.value : el.value
}

/**
 * Where the selected `tab` sits in the scroller's content once the strip's
 * motion ends.
 */
function activeBounds(scrolling: HTMLElement, tab: HTMLElement): { left: number; right: number } {
  const strip = el.value
  if (!strip || scrolling === strip) {
    return settledTabBounds(scrolling, tab)
  }
  // On a phone: a leading tab where it stands, a tab of the strip after the
  // leading ones.
  const origin = scrolling.getBoundingClientRect().left - scrolling.scrollLeft
  if (!strip.contains(tab)) {
    const rect = tab.getBoundingClientRect()
    return { left: rect.left - origin, right: rect.right - origin }
  }
  const offset = strip.getBoundingClientRect().left - origin
  const own = settledTabBounds(strip, tab)
  return { left: own.left + offset, right: own.right + offset }
}

function updateEdges(): void {
  const strip = scroller()
  if (!strip) {
    return
  }
  moreBefore.value = strip.scrollLeft > 0
  moreAfter.value = strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1
  const view = strip.getBoundingClientRect()
  const marks = [...strip.querySelectorAll('[data-tab-mark]')].map((mark) => {
    const rect = mark.getBoundingClientRect()
    return { left: rect.left - view.left, right: rect.right - view.left }
  })
  const selected = strip.querySelector<HTMLElement>('[aria-selected="true"]')?.getBoundingClientRect()
  const room = fadeRoom(
    strip.clientWidth,
    selected ? { left: selected.left - view.left, right: selected.right - view.left } : null,
    scrollFrame !== null,
  )
  // Written to the style, not kept as state: they change on every frame of
  // a scroll, and a render of the strip then would disturb its tabs' motion.
  const style = fades.value?.style
  style?.setProperty('--fade-before', `${cutMarkCover(0, 'start', marks)}px`)
  style?.setProperty('--fade-after', `${cutMarkCover(strip.clientWidth, 'end', marks)}px`)
  style?.setProperty('--fade-before-room', Number.isFinite(room.before) ? `${room.before}px` : '100%')
  style?.setProperty('--fade-after-room', Number.isFinite(room.after) ? `${room.after}px` : '100%')
}

/**
 * The widest a tab may be, the strip's view (`--tab-room`), so a very narrow
 * strip cuts its selected tab's title rather than the tab. Also whether the
 * tabs overflow.
 *
 * The tabs overflow when they cannot all fit at their least width in the
 * strip's width after the leading tabs and their divider, which keep their
 * width, and before the `overflow` slot. The width is read without that slot,
 * so its showing never changes the answer. A tab on its way out is not counted.
 */
function measureRoom(): void {
  const strip = root.value
  const view = fades.value
  const own = el.value
  if (!strip || !view || !own) {
    return
  }
  let room: number
  if (phone.value) {
    // On a phone no tab shrinks and the leading tabs scroll away with the rest:
    // a tab may take the whole view.
    room = strip.clientWidth
    overflowing.value = (list.value?.scrollWidth ?? 0) > strip.clientWidth
  } else {
    room = view.clientWidth
    const tabs = tabElements().length
    const base = strip.clientWidth - (view.getBoundingClientRect().left - strip.getBoundingClientRect().left)
    overflowing.value = tabs * TAB_MIN_PX + Math.max(0, tabs - 1) * stripGap(own) > base
  }
  const value = `${room}px`
  // Unchanged, it is not written again: writing it resizes the tabs, which the
  // observer reports.
  if (strip.style.getPropertyValue('--tab-room') !== value) {
    strip.style.setProperty('--tab-room', value)
  }
}

/**
 * The strip or its selected tab changed size: the selected tab is revealed
 * again.
 */
function resized(): void {
  // The strip's own width changing ends tab closing mode: the widths it froze
  // were the old room's. The overflow button coming or going is no change of
  // the strip's.
  const width = root.value?.clientWidth ?? 0
  if (width !== viewWidth) {
    viewWidth = width
    endClosingMode()
  }
  measureRoom()
  revealActive()
}

/** The strip's width, as `resized` last saw it. */
let viewWidth = 0

// Only the strip scrolls, never the page: scrollIntoView would move both.
function revealActive(): void {
  const strip = scroller()
  if (strip) {
    finishFollow(strip)
  }
  const active = strip?.querySelector<HTMLElement>('[aria-selected="true"]') ?? null
  if (active !== watchedTab) {
    if (watchedTab) {
      resizeObserver?.unobserve(watchedTab)
    }
    if (active) {
      resizeObserver?.observe(active)
    }
    watchedTab = active
  }
  // With the selected tab outside the scrolling tabs, as Changes is, a close
  // still brings the end back in step with it.
  if (strip && !active && el.value && closingProgress(el.value) !== null) {
    const end = Math.max(0, settledScrollWidth(strip, el.value) - strip.clientWidth)
    if (strip.scrollLeft - end >= 1) {
      scrollStripTo(strip, end)
    }
  }
  if (strip && active) {
    // The end control stands over the view's end: the tab shows whole before it.
    const end = strip.querySelector<HTMLElement>(':scope > [data-strip-end]')?.offsetWidth ?? 0
    const view = {
      scrollLeft: strip.scrollLeft,
      clientWidth: strip.clientWidth - end,
      scrollWidth: (el.value ? settledScrollWidth(strip, el.value) : strip.scrollWidth) - end,
    }
    const target = revealScroll(view, activeBounds(strip, active), FADE_PX)
    if (target !== null) {
      scrollStripTo(strip, target)
    }
  }
  updateEdges()
}

// A wheel over the tabs moves them, whichever axis it turns on (`sidewaysWheel`).
function onWheel(event: WheelEvent): void {
  const strip = scroller()
  const delta = strip ? sidewaysWheel(strip, event) : null
  if (!strip || delta === null) {
    return
  }
  event.preventDefault()
  cancelScroll()
  strip.scrollLeft += delta
}

/**
 * The tabs whose entrance or exit started, which alone end one. A tab that
 * comes or goes while the strip is still has no motion, and its hooks do
 * nothing.
 *
 * Each ending hook reads the strip's layout, so one per tab of a whole strip
 * replaced at once would lay the strip out once per tab. The mutation observer
 * reveals the selected tab once for all of them instead.
 */
const moving = new WeakSet<Element>()

// Each hook takes the element alone, so Vue ends a transition without CSS at
// once.
function onBeforeEnter(el: Element): void {
  if (props.still) {
    endClosingMode()
    return
  }
  moving.add(el)
  beforeEnterTab(el)
}

function onEnter(el: Element): void {
  if (moving.has(el)) {
    enterTab(el)
    // Tabs closing as it opens move to the room it leaves them.
    if (el.parentElement) {
      resettleClosing(el.parentElement)
    }
  }
  // A tab coming ends tab closing mode, as in Chrome: the others move to their
  // shares beside it as it grows.
  endClosingMode()
}

// The strip's extent settles only after the motion; reveal again then.
function onAfterEnter(el: Element): void {
  if (!moving.delete(el)) {
    return
  }
  afterEnterTab(el)
  revealActive()
}

function onBeforeLeave(node: Element): void {
  if (props.still) {
    return
  }
  const strip = el.value
  const byPointer = performance.now() - pointerClose.at < POINTER_CLOSE_MS
  pointerClose.at = Number.NEGATIVE_INFINITY
  // Tabs at their least width, the strip overflowing, may also widen: the
  // close may end the overflow, and the overflow button goes.
  if (byPointer && strip && !phone.value && node instanceof HTMLElement
    && (widthsFrozen(strip) || closeWouldWiden(strip, node, overflowing.value))) {
    freezeWidths(strip)
    if (pointerClose.touch) {
      endClosingModeLater()
    } else {
      watchClosingZone()
    }
  }
  moving.add(node)
  beforeLeaveTab(node)
}

function onLeave(el: Element): void {
  if (moving.has(el)) {
    leaveTab(el)
  }
}

function onAfterLeave(el: Element): void {
  if (!moving.delete(el)) {
    return
  }
  afterLeaveTab(el)
  revealActive()
}

/**
 * When the pointer last closed a tab, by its close control or a middle click,
 * and whether by touch: the close the host then applies starts tab closing
 * mode (`freezeWidths`).
 */
const pointerClose = { at: Number.NEGATIVE_INFINITY, touch: false }

/**
 * How long a host may take to apply a close and still have it count as the
 * pointer's: one that asks a device first applies it a moment later.
 */
const POINTER_CLOSE_MS = 1000

function notePointerClose(event: MouseEvent): void {
  // A click the keyboard made, on a control that has the focus, counts zero
  // clicks, and keeps nothing under the pointer.
  if (event.detail === 0) {
    return
  }
  const target = event.target instanceof Element ? event.target : null
  const control = event.type === 'click' && target?.closest('[role="tab"] [role="button"], [role="tab"] button')
  // A middle click closes only a tab that has a close control.
  const middle = event.type === 'auxclick' && event.button === 1
    && target?.closest('[role="tab"]')?.querySelector('[role="button"], button')
  if (control || middle) {
    pointerClose.at = performance.now()
    pointerClose.touch = event instanceof PointerEvent && event.pointerType === 'touch'
  }
}

/**
 * How long tab closing mode lasts after a close by touch, which leaves no
 * pointer to leave the strip: Chrome's 2 s, started again by each close.
 */
const TOUCH_CLOSING_MS = 2000
let touchClosing: number | null = null

function endClosingModeLater(): void {
  if (touchClosing !== null) {
    window.clearTimeout(touchClosing)
  }
  touchClosing = window.setTimeout(() => {
    touchClosing = null
    endClosingMode()
  }, TOUCH_CLOSING_MS)
}

/** A thaw that waits for a drag and the tabs landing from it, which it would cut. */
let thawPending = false

/** Whether the closing zone is watched (`watchClosingZone`). */
let zoneWatched = false

/**
 * Chrome's closing zone, past which the pointer ends tab closing mode: the
 * strip, 40 px below it, so a pointer that drifts down on its way along the
 * tabs keeps the mode, and 60 px past its end, so one going for New Tab does.
 */
const CLOSING_ZONE_BELOW_PX = 40
const CLOSING_ZONE_AFTER_PX = 60

function watchClosingZone(): void {
  if (zoneWatched) {
    return
  }
  zoneWatched = true
  document.addEventListener('pointermove', onZonePointer, true)
  document.documentElement.addEventListener('pointerleave', endClosingMode)
}

function onZonePointer(event: PointerEvent): void {
  const zone = root.value?.getBoundingClientRect()
  // A drag ends the mode only once it is let go away from the strip.
  if (!zone || drag?.moving) {
    return
  }
  if (event.clientX < zone.left || event.clientX > zone.right + CLOSING_ZONE_AFTER_PX
    || event.clientY < zone.top || event.clientY > zone.bottom + CLOSING_ZONE_BELOW_PX) {
    endClosingMode()
  }
}

/**
 * Ends tab closing mode: the tabs move to their shares of the strip, once a
 * drag and its tabs' landing are over.
 */
function endClosingMode(): void {
  if (zoneWatched) {
    zoneWatched = false
    document.removeEventListener('pointermove', onZonePointer, true)
    document.documentElement.removeEventListener('pointerleave', endClosingMode)
  }
  if (touchClosing !== null) {
    window.clearTimeout(touchClosing)
    touchClosing = null
  }
  if (drag?.moving || landing.busy()) {
    thawPending = true
    return
  }
  thawPending = false
  if (el.value) {
    thawWidths(el.value)
  }
}

/** Ends tab closing mode where it waited for a drag and its landing. */
function thawIfPending(): void {
  if (thawPending && !drag?.moving && !landing.busy()) {
    endClosingMode()
  }
}

/**
 * The strip's own tabs in order, the ones a drag moves among, without those on
 * their way out.
 */
function tabElements(): HTMLElement[] {
  const strip = el.value
  if (!strip) {
    return []
  }
  return [...strip.querySelectorAll<HTMLElement>(':scope > [role="tab"]')].filter((tab) => !isLeavingTab(tab))
}

// The arrows move the focus along the tabs, the leading ones first, wrapping at
// the ends. Enter or Space on a tab is the tab's own.
function onKeydown(event: KeyboardEvent): void {
  const tabs = [...leading.value?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [], ...tabElements()]
  const at = tabs.findIndex((tab) => tab === event.target)
  if (at < 0) {
    return
  }
  const next = {
    ArrowLeft: (at - 1 + tabs.length) % tabs.length,
    ArrowRight: (at + 1) % tabs.length,
    Home: 0,
    End: tabs.length - 1,
  }[event.key]
  if (next === undefined) {
    return
  }
  event.preventDefault()
  tabs[next]?.focus()
}

/** A full tab's width, `TabItem`'s cap, by which Chrome scales a drag's hysteresis. */
const FULL_TAB_PX = 160

/**
 * How far a drag scrolls an overflowing strip on each step while it holds the
 * tab at the view's edge, and how often: Chrome's 5 px every 20 ms.
 */
const DRAG_SCROLL_PX = 5
const DRAG_SCROLL_MS = 20

/**
 * A tab pressed with the primary button, which drags once the pointer moves far
 * enough. Places are in the strip's content, which scrolls under the view.
 */
interface Drag {
  pointerId: number
  /** Where the press was on screen, and where the pointer is now. */
  startX: number
  clientX: number
  /** Where the press was in the strip's content. */
  pressX: number
  from: number
  tabs: HTMLElement[]
  /** Each tab's left edge and width when the press began, in strip order. */
  lefts: number[]
  widths: number[]
  gap: number
  /**
   * How far the dragged tab's right edge may go: Chrome's tab drag area, the
   * whole strip but its end control, not only as far as the last tab.
   */
  end: number
  to: number
  /** Where the pointer was when the tab last changed slots (`reorderHysteresis`). */
  movedAt: number
  moving: boolean
  /** The scroll that runs while the tab is held at the view's edge, and its way. */
  scroll: number | null
  scrollWay: -1 | 0 | 1
}
let drag: Drag | null = null

/** The width the strip's end control takes, such as New Tab. */
function endControlWidth(strip: HTMLElement): number {
  return strip.querySelector<HTMLElement>(':scope > [data-strip-end]')?.offsetWidth ?? 0
}

/** Where the screen's `x` falls in `strip`'s content. */
function contentX(strip: HTMLElement, x: number): number {
  return x - strip.getBoundingClientRect().left + strip.scrollLeft
}

function onPointerdown(event: PointerEvent): void {
  const strip = el.value
  const tab = event.target instanceof Element ? event.target.closest<HTMLElement>('[role="tab"]') : null
  if (!props.onReorder || event.button !== 0 || !strip || !tab) {
    return
  }
  // A finger on a phone's strip scrolls it; the browser's pan would cut a drag
  // short anyway.
  if (phone.value && event.pointerType === 'touch') {
    return
  }
  // A tab still landing from the last drag is put in its place, which this
  // drag starts from, as Chrome stops the strip's motion when a drag begins.
  landing.settle()
  const tabs = tabElements()
  const from = tabs.indexOf(tab)
  if (from < 0) {
    return
  }
  const view = strip.getBoundingClientRect()
  const rects = tabs.map((each) => each.getBoundingClientRect())
  const pressX = contentX(strip, event.clientX)
  drag = {
    pointerId: event.pointerId,
    startX: event.clientX,
    clientX: event.clientX,
    pressX,
    from,
    tabs,
    lefts: rects.map((rect) => rect.left - view.left + strip.scrollLeft),
    widths: rects.map((rect) => rect.width),
    gap: stripGap(strip),
    end: Math.max(strip.clientWidth, strip.scrollWidth) - endControlWidth(strip),
    to: from,
    movedAt: pressX,
    moving: false,
    scroll: null,
    scrollWay: 0,
  }
}

/**
 * Puts the dragged tab under the pointer, within the drag area and the view,
 * the others aside where it would land, and scrolls an overflowing strip
 * while the tab is held at the view's edge.
 */
function placeDragged(held: Drag): void {
  const strip = el.value
  const view = scroller()
  if (!strip || !view) {
    return
  }
  // What shows of the strip, in its content: on a phone the whole tab list
  // scrolls, the strip within it.
  const shown = view.getBoundingClientRect()
  const pointer = contentX(strip, held.clientX)
  const width = held.widths[held.from]!
  const least = Math.max(0, contentX(strip, shown.left))
  const most = Math.max(least, Math.min(held.end, contentX(strip, shown.right - endControlWidth(strip))) - width)
  const left = Math.min(most, Math.max(least, held.lefts[held.from]! + pointer - held.pressX))
  const slot = dropSlot(held.widths, held.gap, held.lefts[0]!, held.from, left)
  if (slot !== held.to) {
    const others = held.widths.filter((_, index) => index !== held.from)
    const passed = others[Math.min(slot, others.length - 1)] ?? width
    if (Math.abs(pointer - held.movedAt) > reorderHysteresis(passed, FULL_TAB_PX)) {
      held.to = slot
      held.movedAt = pointer
    }
  }
  const offset = left - held.lefts[held.from]!
  const step = width + held.gap
  held.tabs.forEach((tab, index) => {
    if (index === held.from) {
      tab.style.transition = keepingCloseMotion(tab, 'none')
      // Over the others and the end control it passes, with a fill of its own:
      // a tab that was not selected has none but under the pointer, which the
      // strip holds while it drags.
      tab.style.zIndex = '3'
      tab.dataset['dragged'] = ''
      tab.style.transform = `translateX(${offset}px)`
      return
    }
    tab.style.transition = keepingCloseMotion(tab, tabMotion('transform'))
    tab.style.transform = `translateX(${dragShift(index, held.from, held.to) * step}px)`
  })
  const scrollable = view.scrollWidth - view.clientWidth
  const way = left <= least && view.scrollLeft > 0
    ? -1
    : left >= most && view.scrollLeft < scrollable - 0.5 ? 1 : 0
  scrollWhileHeld(held, way)
}

function scrollWhileHeld(held: Drag, way: -1 | 0 | 1): void {
  if (way === held.scrollWay) {
    return
  }
  stopDragScroll(held)
  if (way === 0) {
    return
  }
  held.scrollWay = way
  held.scroll = window.setInterval(() => {
    const view = scroller()
    if (!view || drag !== held) {
      stopDragScroll(held)
      return
    }
    view.scrollLeft += way * DRAG_SCROLL_PX
    placeDragged(held)
  }, DRAG_SCROLL_MS)
}

function stopDragScroll(held: Drag): void {
  if (held.scroll !== null) {
    window.clearInterval(held.scroll)
  }
  held.scroll = null
  held.scrollWay = 0
}

function onPointermove(event: PointerEvent): void {
  const held = drag
  if (!held || event.pointerId !== held.pointerId) {
    return
  }
  // Let go where the strip did not hear it, as below the strip before the
  // drag began: the press is over.
  if ((event.buttons & 1) === 0) {
    cancelDrag()
    return
  }
  if (!held.moving) {
    if (Math.abs(event.clientX - held.startX) < DRAG_START_PX) {
      return
    }
    held.moving = true
    el.value?.setPointerCapture(event.pointerId)
    cancelScroll()
  }
  held.clientX = event.clientX
  placeDragged(held)
}

/**
 * The tabs gliding to their places after a drag ends or is put back, as
 * Chrome's tabs animate to theirs.
 */
const landing = createLanding(
  'x',
  keepingCloseMotion,
  (tab) => {
    tab.style.transition = keepingCloseMotion(tab, '')
    tab.style.zIndex = ''
    delete tab.dataset['dragged']
  },
  thawIfPending,
)

/**
 * Glides `tabs` to their places from where they showed (`shown`). A selected
 * tab that was dragged, which the drag may have scrolled away from, shows
 * whole again where it lands; another one leaves the strip where the user
 * scrolled it.
 */
function land(held: Drag, tabs: HTMLElement[], shown: Map<HTMLElement, number>): void {
  landing.land(tabs, shown)
  if (held.tabs[held.from]?.getAttribute('aria-selected') === 'true') {
    revealActive()
  }
}

function onPointerup(event: PointerEvent): void {
  const ended = drag
  if (!ended || event.pointerId !== ended.pointerId) {
    return
  }
  drag = null
  stopDragScroll(ended)
  if (!ended.moving) {
    return
  }
  // The tab carried is not selected by letting it go.
  swallowDropClick()
  const shown = landing.shownAt(ended.tabs)
  if (ended.to !== ended.from) {
    props.onReorder?.(ended.from, ended.to)
  }
  // The host's order lands with the next render; until then the tabs hold where
  // the drag left them, and then glide from there.
  void nextTick(() => land(ended, tabElements(), shown))
}

/** Puts a dragged tab back where it was, gliding, as Escape does. */
function cancelDrag(): void {
  const ended = drag
  drag = null
  if (!ended) {
    return
  }
  stopDragScroll(ended)
  if (ended.moving) {
    land(ended, ended.tabs, landing.shownAt(ended.tabs))
  }
}

function onDragKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape' && drag?.moving) {
    event.preventDefault()
    event.stopPropagation()
    cancelDrag()
  }
}

onMounted(() => {
  // Before any other key handler, so Escape during a drag puts the tab back
  // and nothing else: the window it is in stays.
  window.addEventListener('keydown', onDragKeydown, true)
  const strip = el.value
  if (!strip) {
    return
  }
  resizeObserver = new ResizeObserver(resized)
  for (const watched of [root.value, leading.value, fades.value]) {
    if (watched) {
      resizeObserver.observe(watched)
    }
  }
  // A tab becoming active, a leading one included, or tabs coming and going is
  // what moves the view. Tabs coming and going may also make the tabs overflow.
  // Any other change, such as a title that widens a tab, moves the marks.
  mutationObserver = new MutationObserver((records) => {
    if (tabsChanged(strip, records, (node) => node instanceof HTMLElement)) {
      measureRoom()
      revealActive()
    } else {
      updateEdges()
    }
  })
  mutationObserver.observe(list.value ?? strip, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['aria-selected'],
  })
  measureRoom()
  revealActive()
})

// The overflow button comes or goes with a close that starts or ends the
// overflow, and the room it takes or gives changes where the closing motion
// ends.
watch(
  overflowing,
  () => {
    if (el.value) {
      resettleClosing(el.value)
    }
  },
  { flush: 'post' },
)

// The leading tabs' width changes with their marks and badges, and they come
// and go with the host's.
watch(leading, (now, before) => {
  if (before) {
    resizeObserver?.unobserve(before)
  }
  if (now) {
    resizeObserver?.observe(now)
  }
}, { flush: 'post' })

// Crossing the phone's width changes what scrolls and the room a tab has.
watch(phone, () => {
  cancelScroll()
  void nextTick(resized)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onDragKeydown, true)
  cancelDrag()
  landing.settle()
  endClosingMode()
  resizeObserver?.disconnect()
  watchedTab = null
  mutationObserver?.disconnect()
  cancelScroll()
})

defineExpose({ el })
</script>

<template>
  <div ref="root" class="flex min-w-0 items-center" :style="vars">
    <!--
      On a phone the fades' frame: the whole tab list scrolls inside it, so the
      fades stay put over the row's ends.
    -->
    <div class="flex min-w-0 shrink grow phone:relative phone:isolate">
    <!--
      One tab list holds the leading tabs and the strip's own, so the keyboard
      and assistive technology meet them as one row of tabs. On a phone it is
      what scrolls.
    -->
    <div
      ref="list"
      role="tablist"
      class="flex min-w-0 shrink grow items-center gap-0.5 phone:overflow-x-auto phone:overscroll-x-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      @keydown="onKeydown"
      @scroll.passive="updateEdges"
    >
    <!--
      The leading tabs keep their width, the same gap apart as the strip's own
      tabs. On a phone they scroll with the rest.
    -->
    <div v-if="$slots.leading" ref="leading" class="flex shrink-0 items-center gap-0.5">
      <slot name="leading" />
    </div>
    <!--
      The divider after the leading tabs, while the strip has a tab of its own
      after it. Like the dividers between the strip's tabs, it fades out, its
      room kept, while the tab on either side of it is selected or hovered.
    -->
    <span
      v-if="$slots.leading"
      aria-hidden="true"
      data-leading-divider
      class="mx-0.5 hidden h-3.5 w-px shrink-0 bg-line transition-opacity duration-50 [&:has(+*_[role=tab])]:block [:has(>[role=tab]:last-child[aria-selected=true])+&]:opacity-0 [:has(>[role=tab]:last-child:hover)+&]:opacity-0 [&:has(+*>*>[role=tab]:first-child[aria-selected=true])]:opacity-0 [&:has(+*>*>[role=tab]:first-child:hover)]:opacity-0"
    />
    <!--
      Its own stacking context, so the fades lie over the scrolled tabs and
      nothing else. It takes the room the leading tabs leave, which its tabs
      share. On a phone it takes its tabs' whole width and the view holds the
      fades.
    -->
    <div
      ref="fades"
      class="relative isolate min-w-0 shrink grow [--fade-after:0px] [--fade-before:0px] phone:static phone:isolation-auto phone:shrink-0 phone:grow-0"
    >
    <TransitionGroup
      :ref="bindEl"
      :name="TAB_TRANSITION"
      :css="!still"
      tag="div"
      class="flex items-center gap-0.5 overflow-x-auto phone:overflow-x-visible [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      @scroll.passive="updateEdges"
      @wheel="onWheel"
      @pointerdown="onPointerdown"
      @pointermove="onPointermove"
      @pointerup="onPointerup"
      @pointercancel="cancelDrag"
      @click.capture="notePointerClose"
      @auxclick.capture="notePointerClose"
      @before-enter="onBeforeEnter"
      @enter="onEnter"
      @after-enter="onAfterEnter"
      @enter-cancelled="onAfterEnter"
      @before-leave="onBeforeLeave"
      @leave="onLeave"
      @after-leave="onAfterLeave"
      @leave-cancelled="onAfterLeave"
    >
      <slot />
      <div
        v-if="$slots.end"
        key="strip-end"
        data-strip-end
        class="sticky right-0 z-2 flex shrink-0 items-center bg-(--tab-row) pl-0.5"
        :class="{ 'strip-end-faded': moreAfter }"
      >
        <slot name="end" />
      </div>
    </TransitionGroup>
    <div
      v-if="moreBefore"
      aria-hidden="true"
      class="pointer-events-none absolute inset-y-0 left-0 z-1 w-[min(calc(var(--fade-before)_+_--spacing(6)),var(--fade-before-room))] bg-[linear-gradient(to_right,var(--tab-row)_var(--fade-before),transparent)]"
    />
    <div
      v-if="moreAfter"
      aria-hidden="true"
      class="pointer-events-none absolute inset-y-0 right-0 z-1 w-[min(calc(var(--fade-after)_+_--spacing(6)),var(--fade-after-room))] bg-[linear-gradient(to_left,var(--tab-row)_var(--fade-after),transparent)]"
    />
    </div>
    </div>
    </div>
    <div v-if="overflowing && $slots.overflow" class="ml-1 flex shrink-0 items-center">
      <slot name="overflow" />
    </div>
  </div>
</template>

<style>
.tabs-enter-active,
.tabs-leave-active {
  transition:
    width var(--tab-motion-ms) var(--tab-easing),
    margin var(--tab-motion-ms) var(--tab-easing);
}
.tabs-leave-active {
  pointer-events: none;
}
/* The tabs that scroll under the end control fade out before it. */
.strip-end-faded::before {
  content: "";
  position: absolute;
  inset-block: 0;
  right: 100%;
  width: 1.5rem;
  pointer-events: none;
  background: linear-gradient(to left, var(--tab-row), transparent);
}
@media (prefers-reduced-motion: reduce) {
  .tabs-enter-active,
  .tabs-leave-active {
    transition: none;
  }
}
</style>
