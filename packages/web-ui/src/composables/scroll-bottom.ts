/**
 * How close to the end a session scroller counts as "at the bottom"
 * (auto-follow re-engages here).
 */
export const BOTTOM_THRESHOLD_PX = 100

/**
 * The reader is at the end while the end of the list is in view or less than
 * this far below it: the list follows its end from there.
 */
export const AT_END_PX = 16

export function distanceFromBottom(el: HTMLElement): number {
  return el.scrollHeight - el.scrollTop - el.clientHeight
}
