/**
 * Chrome-like tab strip motion: lock the current width, then collapse or grow
 * in flow.
 */

import type { InjectionKey } from 'vue'
import { clamp } from '@jush-ai/utils'
import { prefersReducedMotion } from '../ui/stream-reveal'

/**
 * Provided by a window whose tabs narrow to their icons on a phone (the
 * Agents and Running windows, `WorkWindow`): there only the selected
 * `TabItem` shows its name, as Chrome's narrowed tabs show the active one's
 * title. /
 */
export const iconTabsKey: InjectionKey<true> = Symbol('icon-tabs')

/**
 * The least width a strip's tab keeps, its mark, a few characters of its
 * title and its close slot: the strip scrolls before a tab narrows further.
 */
export const TAB_MIN_PX = 88

/** The most a tab is wide, `TabItem`'s cap (`--spacing(40)`). */
export const TAB_MAX_PX = 160

export const TAB_TRANSITION = 'tabs'

/**
 * How long every motion of a strip's tabs takes, and its curve: Chrome's tab
 * strip animates each tab to its place in 200 ms, easing out on
 * `1 - (1 - t)²`, which this Bézier draws exactly. A tab opening, closing,
 * stepping aside, landing where a drag leaves it and the strip's own scroll
 * all keep it, so they read as one motion.
 */
export const TAB_MOTION_MS = 200
export const TAB_EASING = 'cubic-bezier(0.333, 0.667, 0.667, 1)'

/** `TAB_EASING` at `progress`, from 0 to 1, for a motion drawn frame by frame. */
export function tabEased(progress: number): number {
  return 1 - (1 - progress) ** 2
}

/** A transition of `property` in the tabs' motion. */
export function tabMotion(property: string): string {
  return `${property} ${TAB_MOTION_MS}ms ${TAB_EASING}`
}

const leaveParents = new WeakMap<HTMLElement, HTMLElement>()
const enterWidths = new WeakMap<HTMLElement, number>()
/** The width a closing tab collapses from. */
const leaveWidths = new WeakMap<HTMLElement, number>()
/** The width a tab that stays moves to while others close. */
const closeWidths = new WeakMap<HTMLElement, number>()

function measureOpenWidth(tab: HTMLElement): number {
  const parent = tab.parentElement
  // In tab closing mode the others hold still at the widths it keeps, which
  // the tab's opening ends (`thawWidths`): they share the strip with it while
  // it is measured.
  const frozen = parent && frozenStrips.has(parent)
    ? [...parent.children].filter((child): child is HTMLElement => isStyledEl(child) && child !== tab && closeWidths.has(child))
    : []
  const held = frozen.map(({ style }) => [style.transition, style.width, style.flex] as const)
  const measure = (): number => {
    const prevWidth = tab.style.width
    const prevFlex = tab.style.flex
    const prevMin = tab.style.minWidth
    // No inline width or flex: the width the strip's sharing gives the tab is
    // what it opens to.
    tab.style.width = ''
    tab.style.flex = ''
    tab.style.minWidth = ''
    for (const child of frozen) {
      holdWidth(child)
      child.style.width = ''
      child.style.flex = ''
    }
    void tab.offsetWidth
    const width = tab.getBoundingClientRect().width
    tab.style.width = prevWidth
    tab.style.flex = prevFlex
    tab.style.minWidth = prevMin
    frozen.forEach((child, index) => {
      child.style.width = held[index]![1]
      child.style.flex = held[index]![2]
    })
    if (frozen.length > 0) {
      void tab.offsetWidth
      frozen.forEach((child, index) => {
        child.style.transition = held[index]![0]
      })
    }
    return width
  }
  return parent ? measureSettled(parent, [], measure) : measure()
}

/**
 * What `measure` reads of `parent`'s tabs laid out as they settle: the
 * closing ones, `closing` and those already on their way out, take no room,
 * and each tab on its way in the room it opens to, also one opened with it
 * whose motion has yet to start. Tabs opened together thus open to one share,
 * rather than each later one to the room the earlier ones, still narrow,
 * leave it.
 *
 * Their own motion runs on: those tabs are taken out of the flow, which a
 * motion does not move by, and a copy of each one on its way in stands in its
 * place at its open width, for `measure` to read (`opening`). The measure may
 * lay the strip out shorter, and the browser would pull a scroll at its end
 * back to fit that at once: the scroll is put back.
 */
function measureSettled<T>(
  parent: HTMLElement,
  closing: HTMLElement[],
  measure: (opening: Map<HTMLElement, HTMLElement>) => T,
): T {
  const children = [...parent.children].filter(isStyledEl)
  const leaving = children.filter((child) => closing.includes(child) || isLeavingTab(child))
  const entering = children.filter((child) => !leaving.includes(child) && enterWidths.has(child))
  // Opened in the same render, a tab whose motion has yet to start holds no
  // least width (`beforeEnterTab`): it takes its own while measured.
  const pending = children.filter(
    (child) =>
      !leaving.includes(child) && !entering.includes(child) && child.classList.contains(`${TAB_TRANSITION}-enter-active`),
  )
  const pendingLeast = pending.map((child) => child.style.minWidth)
  const scrolled: [HTMLElement, number][] = []
  for (let each: HTMLElement | null = parent; each; each = each.parentElement) {
    if (each.scrollLeft > 0) {
      scrolled.push([each, each.scrollLeft])
    }
  }
  const opening = new Map<HTMLElement, HTMLElement>()
  try {
    for (const child of entering) {
      const copy = child.cloneNode(true) as HTMLElement
      for (const property of ['width', 'flex', 'min-width', 'max-width', 'overflow', 'position']) {
        copy.style.removeProperty(property)
      }
      copy.style.transition = 'none'
      copy.removeAttribute('id')
      child.before(copy)
      opening.set(child, copy)
    }
    for (const child of [...leaving, ...entering]) {
      child.style.position = 'absolute'
    }
    for (const child of pending) {
      child.style.minWidth = ''
    }
    return measure(opening)
  } finally {
    for (const copy of opening.values()) {
      copy.remove()
    }
    for (const child of [...leaving, ...entering]) {
      child.style.position = ''
    }
    pending.forEach((child, index) => {
      child.style.minWidth = pendingLeast[index]!
    })
    void parent.offsetWidth
    for (const [each, left] of scrolled) {
      each.scrollLeft = left
    }
  }
}

function tabEl(el: Element): HTMLElement {
  return el as HTMLElement
}

/** The gap between a strip's tabs. */
export function stripGap(strip: HTMLElement): number {
  const gap = Number.parseFloat(getComputedStyle(strip).columnGap)
  return Number.isFinite(gap) ? gap : 0
}

function columnGap(el: HTMLElement): number {
  const parent = el.parentElement
  if (!parent) {
    return 0
  }
  return stripGap(parent)
}

/**
 * True while the tab is on its way out and no longer part of the settled
 * layout.
 */
export function isLeavingTab(tab: Element): boolean {
  return tab.classList.contains(`${TAB_TRANSITION}-leave-active`)
}

/**
 * Where `tab` will sit once the strip's motion ends: tabs on their way out
 * take no room, a tab on its way in takes the width it opens to.
 */
export function settledTabBounds(strip: HTMLElement, tab: HTMLElement): { left: number; right: number } {
  const gap = stripGap(strip)
  let left = 0
  for (const child of strip.children) {
    if (child === tab) {
      break
    }
    if (child instanceof HTMLElement && !isLeavingTab(child)) {
      left += settledTabWidth(child) + gap
    }
  }
  return { left, right: left + settledTabWidth(tab) }
}

/**
 * Where the strip scrolls to so the selected `tab` shows whole and clear of
 * the edge fades, or null when it already does. A view too narrow for the
 * tab and a fade on each side shows the tab whole without that room, and the
 * fades give way beside it (`fadeRoom`): the selected tab is never under a
 * fade. The tab is never wider than the view (`--tab-room`).
 */
export function revealScroll(
  view: { scrollLeft: number; clientWidth: number; scrollWidth: number },
  tab: { left: number; right: number },
  fade: number,
): number | null {
  const margin = tab.right - tab.left + 2 * fade <= view.clientWidth ? fade : 0
  const latest = Math.max(0, tab.left - margin)
  const earliest = tab.right + margin - view.clientWidth
  const wanted = clamp(view.scrollLeft, earliest, latest)
  // The last tab's fade reaches past the strip's end: the end is as far as it
  // goes.
  const target = Math.min(wanted, Math.max(0, view.scrollWidth - view.clientWidth))
  return Math.abs(target - view.scrollLeft) < 1 ? null : target
}

/**
 * Where a dragged tab lands, its index among the others, as Chrome picks it:
 * the slot whose place, were the drag to end now, lies nearest the tab's
 * left edge where the pointer holds it. A tab thus takes a neighbor's slot
 * once it covers half of it. `widths` are the tabs' widths in order, the
 * dragged one at `from` included, the first tab's left edge at `start`.
 */
export function dropSlot(widths: readonly number[], gap: number, start: number, from: number, left: number): number {
  let best = 0
  let nearest = Number.POSITIVE_INFINITY
  let place = start
  const others = widths.filter((_, index) => index !== from)
  for (let slot = 0; slot <= others.length; slot++) {
    const distance = Math.abs(left - place)
    if (distance < nearest) {
      nearest = distance
      best = slot
    }
    place += (others[slot] ?? 0) + gap
  }
  return best
}

/**
 * How far, in px, the pointer moves after a dragged tab changed slots before
 * it may change again, so a pointer at rest on the line between two slots
 * does not swap the tabs back and forth: Chrome's 16 px, scaled down for a tab
 * narrower than a full one (`fullWidth`).
 */
export function reorderHysteresis(width: number, fullWidth: number): number {
  return Math.round((width / fullWidth) * 16)
}

/**
 * How far a wheel turned over a strip that scrolls sideways moves it, on
 * whichever axis the wheel turns, so a mouse whose wheel turns only up and
 * down moves it too; null when the strip has nothing to scroll that way, and
 * the page keeps the event.
 */
export function sidewaysWheel(strip: HTMLElement, event: WheelEvent): number | null {
  if (strip.scrollWidth <= strip.clientWidth) {
    return null
  }
  const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY
  const max = strip.scrollWidth - strip.clientWidth
  const atEdge = delta < 0 ? strip.scrollLeft <= 0 : strip.scrollLeft >= max
  return atEdge ? null : delta
}

/**
 * How wide each edge fade may be so it never lies over the selected tab:
 * the room between the view's edge and the tab, in the view's coordinates.
 * The fades give way to the selected tab while it shows whole, and while
 * the strip scrolls to reveal it (`revealing`). One the user scrolled under
 * an edge is cut there as any tab is, under that edge's fade; without a
 * selected tab in the strip the fades keep their own width.
 */
export function fadeRoom(
  clientWidth: number,
  selected: { left: number; right: number } | null,
  revealing: boolean,
): { before: number; after: number } {
  const whole = selected !== null && selected.left >= 0 && selected.right <= clientWidth
  if (!selected || (!whole && !revealing)) {
    return { before: Number.POSITIVE_INFINITY, after: Number.POSITIVE_INFINITY }
  }
  return { before: Math.max(0, selected.left), after: Math.max(0, clientWidth - selected.right) }
}

/** A mutation record of a strip, as `tabsChanged` reads it. */
export interface StripMutation<T> {
  type: string
  target: unknown
  addedNodes: Iterable<T>
  removedNodes: Iterable<T>
}

/**
 * Whether `records`, observed on `strip` and its tabs, hold a tab becoming
 * active, coming or going: what moves the strip's view. A tab's own content
 * changing, as a page's title does, is none of these. Nor is a node both
 * added and removed in the same batch: a render that takes a tab out and
 * puts it back, or the shallow copy of a tab that Vue's transition group puts
 * in the strip for a moment on each render to read its move class. Either
 * would pull the strip away from where the user scrolled it. `isTab` tells
 * an element from a text or comment node.
 */
export function tabsChanged<T>(
  strip: unknown,
  records: readonly StripMutation<T>[],
  isTab: (node: T) => boolean,
): boolean {
  if (records.some((record) => record.type === 'attributes')) {
    return true
  }
  const own = records.filter((record) => record.target === strip)
  const added = own.flatMap((record) => [...record.addedNodes]).filter(isTab)
  const removed = own.flatMap((record) => [...record.removedNodes]).filter(isTab)
  return added.some((node) => !removed.includes(node)) || removed.some((node) => !added.includes(node))
}

/**
 * How far an edge fade stays solid before it fades, so that it covers a tab
 * mark the edge cuts: at a scrolled edge a mark shows whole or not at all,
 * never sliced. `edge` and `marks` are in the strip's view coordinates;
 * `side` is the side of the view the edge is on.
 */
export function cutMarkCover(
  edge: number,
  side: 'start' | 'end',
  marks: readonly { left: number; right: number }[],
): number {
  const cut = marks.find((mark) => mark.left < edge && edge < mark.right)
  if (!cut) {
    return 0
  }
  return Math.ceil(side === 'start' ? cut.right - edge : edge - cut.left)
}

/**
 * The motions of `transition` other than a width's, such as a tab landing
 * from a drag, joined; empty when there are none.
 */
function otherMotion(transition: string): string {
  return transition
    // The commas between motions, not those within a curve's parentheses.
    .split(/,\s*(?![^()]*\))/)
    .map((part) => part.trim())
    .filter((part) => part !== '' && part !== 'none' && !part.startsWith('width'))
    .join(', ')
}

/** `tab`'s transition with its width held still and its other motions running on. */
function holdWidth(tab: HTMLElement): void {
  tab.style.transition = otherMotion(tab.style.transition) || 'none'
}

function clearWidthLock(tab: HTMLElement): void {
  closeWidths.delete(tab)
  tab.style.width = ''
  tab.style.flex = ''
  tab.style.transition = otherMotion(tab.style.transition)
}

/**
 * Whether a tab enters whole, in the frame it appears: only when motion is
 * reduced. Every other tab, the one the user is shown included, grows from
 * nothing into place, as a new tab does in Chrome: it is there from the first
 * frame, opaque, its content clipped while it grows.
 */
function entersWhole(): boolean {
  return prefersReducedMotion()
}

export function beforeEnterTab(el: Element): void {
  const tab = tabEl(el)
  if (entersWhole()) {
    // Over the transition classes the strip's group adds: no growth.
    tab.style.transition = 'none'
    return
  }
  tab.style.minWidth = '0'
  tab.style.overflow = 'hidden'
}

export function enterTab(el: Element): void {
  const tab = tabEl(el)
  if (entersWhole()) {
    return
  }
  const width = measureOpenWidth(tab)
  enterWidths.set(tab, width)
  // Its content does not cap it on the way: a short title's tab opens to its
  // least width, which is wider than the title.
  tab.style.maxWidth = 'none'
  tab.style.flex = '0 0 auto'
  tab.style.transition = 'none'
  tab.style.width = '0'
  void tab.offsetWidth
  tab.style.transition = ''
  tab.style.width = `${width}px`
}

/**
 * The width a tab will have once its entrance ends; its current width
 * otherwise.
 */
export function settledTabWidth(tab: HTMLElement): number {
  return enterWidths.get(tab) ?? closeWidths.get(tab) ?? tab.getBoundingClientRect().width
}

/**
 * How wide the content of `scroller`, which holds `strip`, will be once the
 * strip's motion ends: a closing tab takes no room and gives back its gap,
 * and every other tab takes its settled width.
 */
export function settledScrollWidth(scroller: HTMLElement, strip: HTMLElement): number {
  const gap = stripGap(strip)
  let change = 0
  for (const child of strip.children) {
    if (!isStyledEl(child)) {
      continue
    }
    const width = child.getBoundingClientRect().width
    if (isLeavingTab(child)) {
      const margin = Number.parseFloat(getComputedStyle(child).marginRight) || 0
      change -= width + margin + gap
    } else {
      change += settledTabWidth(child) - width
    }
  }
  return scroller.scrollWidth + change
}

/**
 * How far the strip's closing tabs have collapsed, from 0 to 1, the least of
 * them; null when none is closing. The tabs that stay move with them.
 */
export function closingProgress(strip: HTMLElement): number | null {
  let progress: number | null = null
  for (const child of strip.children) {
    const from = isStyledEl(child) ? leaveWidths.get(child) : undefined
    if (from === undefined || from <= 0) {
      continue
    }
    const done = clamp(1 - child.getBoundingClientRect().width / from, 0, 1)
    progress = progress === null ? done : Math.min(progress, done)
  }
  return progress
}

export function afterEnterTab(el: Element): void {
  const tab = tabEl(el)
  enterWidths.delete(tab)
  tab.style.width = ''
  tab.style.minWidth = ''
  tab.style.maxWidth = ''
  tab.style.overflow = ''
  tab.style.flex = ''
  tab.style.transition = ''
  // The others moved beside it, as when it ended tab closing mode: the last
  // tab in hands them back to the strip, unless a close still runs, which
  // does so when it ends.
  const parent = tab.parentElement
  const children = parent ? [...parent.children].filter(isStyledEl) : []
  if (parent && children.some((child) => closeWidths.has(child))
    && !children.some((child) => enterWidths.has(child) || isLeavingTab(child))) {
    releaseStrip(parent)
  }
}

/** Whether `node` is one of the strip's tabs, not the control after them (`data-strip-end`). */
function isStyledEl(node: unknown): node is HTMLElement {
  return (
    typeof node === 'object' &&
    node !== null &&
    'style' in node &&
    'getBoundingClientRect' in node &&
    !(node as unknown as Element).hasAttribute('data-strip-end')
  )
}

function stillLeaving(parent: HTMLElement): boolean {
  return [...parent.children].some(
    (child) => isStyledEl(child) && child.classList.contains('tabs-leave-active'),
  )
}

/**
 * Holds `tab` and each other tab that stays at its width, out of the strip's
 * sharing (`flex: 0 0 auto`), all read before any is written, so the strip
 * is laid out once and not once per tab. A tab on its way in keeps growing.
 */
function lockWidths(tab: HTMLElement): void {
  const siblings = [...tab.parentElement?.children ?? []].filter(
    (child): child is HTMLElement =>
      isStyledEl(child) && child !== tab && !child.classList.contains('tabs-leave-active') && !enterWidths.has(child),
  )
  const held = [tab, ...siblings]
  const widths = held.map((each) => each.getBoundingClientRect().width)
  held.forEach((each, index) => {
    each.style.width = `${widths[index]}px`
    each.style.flex = '0 0 auto'
  })
}

/**
 * After a close, hands the remaining tabs back to the strip's sharing. They
 * already moved with the close (`settleStaying`); only a change the close
 * brought about, such as the strip no longer overflowing, eases them on.
 */
function releaseStrip(parent: HTMLElement | null): void {
  if (!parent?.children) {
    return
  }
  if (stillLeaving(parent) || frozenStrips.has(parent)) {
    return
  }

  const tabs = [...parent.children].filter(isStyledEl)
  if (tabs.length === 0) {
    return
  }

  const from = tabs.map((tab) => tab.getBoundingClientRect().width)

  for (const tab of tabs) {
    closeWidths.delete(tab)
    holdWidth(tab)
    tab.style.width = ''
    tab.style.flex = ''
  }
  void parent.offsetWidth
  const to = tabs.map((tab) => tab.getBoundingClientRect().width)

  if (
    prefersReducedMotion() ||
    tabs.every((_, index) => Math.abs(from[index]! - to[index]!) < 0.5)
  ) {
    for (const tab of tabs) {
      clearWidthLock(tab)
    }
    return
  }

  for (let index = 0; index < tabs.length; index++) {
    const tab = tabs[index]!
    tab.style.flex = '0 0 auto'
    tab.style.width = `${from[index]}px`
  }
  void parent.offsetWidth
  for (let index = 0; index < tabs.length; index++) {
    const tab = tabs[index]!
    // A tab that keeps its width has no motion to end it: it is released now.
    if (Math.abs(from[index]! - to[index]!) < 0.5) {
      clearWidthLock(tab)
      continue
    }
    // Kept as a close's motion is, so another motion of the tab, such as a
    // drag's, runs beside it rather than cutting it.
    closeWidths.set(tab, to[index]!)
    tab.style.transition = keepingCloseMotion(tab, otherMotion(tab.style.transition))
    tab.style.width = `${to[index]}px`
    const finish = (event: TransitionEvent): void => {
      if (event.target !== tab || event.propertyName !== 'width') {
        return
      }
      tab.removeEventListener('transitionend', finish)
      tab.removeEventListener('transitioncancel', finish)
      // Unless a later motion took the tab over.
      if (closeWidths.get(tab) === to[index]) {
        clearWidthLock(tab)
      }
    }
    tab.addEventListener('transitionend', finish)
    tab.addEventListener('transitioncancel', finish)
  }
}

export function beforeLeaveTab(el: Element): void {
  const tab = tabEl(el)
  if (tab.parentElement) {
    leaveParents.set(tab, tab.parentElement)
  }
  // A closing tab is no tab any more: the selection has passed on, so it
  // closes as an unselected one and the strip shows one selected tab throughout.
  tab.setAttribute('aria-selected', 'false')
  tab.style.setProperty('--tab-active', 'var(--tab-row)')
  // A tab closed with others, as Close Others closes them one by one, may
  // hold the width motion an earlier one gave it: its own close takes over
  // from the width it has reached.
  closeWidths.delete(tab)
  lockWidths(tab)
  tab.style.transition = ''
  leaveWidths.set(tab, Number.parseFloat(tab.style.width) || 0)
  tab.style.minWidth = '0'
  tab.style.overflow = 'hidden'
}

export function leaveTab(el: Element): void {
  const tab = tabEl(el)
  // It narrows to nothing, its content clipped, as a closing tab does in Chrome.
  tab.style.width = '0'
  tab.style.marginRight = `-${columnGap(tab)}px`
  if (tab.parentElement) {
    settleStaying(tab.parentElement, [tab])
  }
}

/**
 * `transition` for a tab, with the width motion a close gave it while that
 * runs: a transition that leaves width out would end it at once.
 */
export function keepingCloseMotion(tab: HTMLElement, transition: string): string {
  if (!closeWidths.has(tab)) {
    return transition
  }
  const width = tabMotion('width')
  return transition && transition !== 'none' ? `${transition}, ${width}` : width
}

/**
 * Aims the tabs that stay at their widths again while tabs close, once the
 * strip's room changed under them, as when a close ends its overflow and the
 * overflow button goes: the motion is retargeted, not followed by another.
 */
export function resettleClosing(strip: HTMLElement): void {
  if ([...strip.children].some(isLeavingTab)) {
    settleStaying(strip, [])
  }
}

/**
 * The strips whose tabs keep their widths after a close, Chrome's tab closing
 * mode (`freezeWidths`).
 */
const frozenStrips = new WeakSet<HTMLElement>()

/**
 * The widths the tabs that stay (`staying`, now `from` wide) take in tab
 * closing mode, as Chrome's strip, whose room for tabs the mode fixes, gives
 * them: they keep their widths, so the next tab slides under the pointer;
 * when the last tab closes, the room it leaves goes to them in equal parts,
 * each up to its share of the whole strip (`shares`), so the new last tab's
 * close control comes under the pointer instead.
 */
function frozenWidths(
  parent: HTMLElement,
  closing: HTMLElement[],
  staying: HTMLElement[],
  from: number[],
  shares: number[],
): number[] {
  const tabs = [...parent.children].filter(
    (child): child is HTMLElement => isStyledEl(child) && (closing.includes(child) || !isLeavingTab(child)),
  )
  const last = tabs.at(-1)
  if (!last || !closing.includes(last) || staying.length === 0) {
    // Where a close already aimed them, as a retargeting finds them.
    return staying.map((child, index) => closeWidths.get(child) ?? from[index]!)
  }
  const room = closing.reduce((sum, tab) => sum + (leaveWidths.get(tab) ?? 0) + stripGap(parent), 0)
  return from.map((width, index) => Math.min(shares[index]!, width + room / staying.length))
}

/**
 * Whether closing `tab` would widen the tabs that stay: the strip has
 * narrowed one below what it fits, cutting its title short of its full width
 * (`TAB_MAX_PX`), or they are at their least width in a strip that
 * `overflows`; and the tab is not the last, whose close Chrome lets the others
 * grow into. It only reads the tabs, so a motion under way runs on.
 */
export function closeWouldWiden(strip: HTMLElement, tab: HTMLElement, overflows: boolean): boolean {
  const tabs = [...strip.children].filter(
    (child): child is HTMLElement => isStyledEl(child) && !isLeavingTab(child),
  )
  if (tabs.length < 2 || tabs.at(-1) === tab) {
    return false
  }
  if (overflows) {
    return true
  }
  return tabs.some((child) => {
    const title = child.querySelector<HTMLElement>('[data-tab-title]')
    return child !== tab && !enterWidths.has(child) && title !== null
      && child.getBoundingClientRect().width < TAB_MAX_PX - 0.5
      && title.scrollWidth > title.clientWidth + 0.5
  })
}

/**
 * Starts Chrome's tab closing mode on `strip`: a tab closed by the pointer
 * leaves the others at their widths, so the next tab, and its close control,
 * slides under the pointer, and a run of closes needs no aim.
 * `thawWidths` ends it.
 */
export function freezeWidths(strip: HTMLElement): void {
  frozenStrips.add(strip)
}

/** Whether `strip` is in tab closing mode (`freezeWidths`). */
export function widthsFrozen(strip: HTMLElement): boolean {
  return frozenStrips.has(strip)
}

/**
 * Ends tab closing mode: the tabs move to their shares of the strip, beside a
 * tab on its way in, which then ends their motion (`afterEnterTab`).
 */
export function thawWidths(strip: HTMLElement): void {
  if (!frozenStrips.delete(strip)) {
    return
  }
  if ([...strip.children].some((child) => isStyledEl(child) && enterWidths.has(child))) {
    settleStaying(strip, [])
  } else {
    releaseStrip(strip)
  }
}

export function afterLeaveTab(el: Element): void {
  const tab = tabEl(el)
  const parent = tab.parentElement ?? leaveParents.get(tab) ?? null
  leaveParents.delete(tab)
  leaveWidths.delete(tab)
  releaseStrip(parent)
}

/**
 * Starts the tabs that stay toward the widths they take once the closing
 * ones are gone, in the closing tab's motion, so the strip moves once rather
 * than closing first and widening after. A tab on its way in grows on, to
 * its share of the room the close leaves.
 */
function settleStaying(parent: HTMLElement, closing: HTMLElement[]): void {
  // Without motion the tabs take their widths when the close ends, but in
  // tab closing mode, which keeps them, only from here.
  const still = prefersReducedMotion()
  if (still && !frozenStrips.has(parent)) {
    return
  }
  const staying = [...parent.children].filter(
    (child): child is HTMLElement =>
      isStyledEl(child) && !closing.includes(child) && !isLeavingTab(child) && !enterWidths.has(child),
  )
  const from = staying.map((child) => child.getBoundingClientRect().width)
  const { to: shares, opened } = measureSettled(parent, closing, (opening) => {
    for (const child of staying) {
      holdWidth(child)
      child.style.width = ''
      child.style.flex = ''
    }
    const widths = staying.map((child) => child.getBoundingClientRect().width)
    const open = [...opening].map(([child, copy]) => [child, copy.getBoundingClientRect().width] as const)
    staying.forEach((child, index) => {
      child.style.width = `${from[index]}px`
      child.style.flex = '0 0 auto'
    })
    return { to: widths, opened: open }
  })
  const to = frozenStrips.has(parent) ? frozenWidths(parent, closing, staying, from, shares) : shares
  staying.forEach((child, index) => {
    closeWidths.set(child, to[index]!)
    const other = otherMotion(child.style.transition)
    child.style.transition = still ? other || 'none' : [other, tabMotion('width')].filter(Boolean).join(', ')
    child.style.width = `${to[index]}px`
  })
  // A tab on its way in grows on to the room the close leaves it.
  for (const [child, width] of opened) {
    enterWidths.set(child, width)
    child.style.width = `${width}px`
  }
}


