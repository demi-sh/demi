import { dataChanges } from './work-changes'
import { pinnedData, type PinnedTabs } from './work-tabs'
import type { TabKind } from '../plugins/page'
import type { PlaceFields } from '../plugins/place'

/**
 * The work's part of the page's address: the tab of the window in front.
 *
 * ```text
 * /chat/c1                                no window in front
 * /chat/c1?tab=file&path=/work/main.ts    Files in front, at a file
 * /chat/c1?tab=t7                         the window of the tab t7 in front, on it
 * ```
 *
 * `tab` names a pinned tab by its kind, any other tab by its id. The rest of
 * the query is the pinned tab's place, which only its kind writes and reads
 * (`../plugins/place.ts`). An address kept from before the work had its name
 * says `panel` for `tab`: it is read as `tab`, and the store writes it anew
 * without it.
 *
 * While a conversation is shown, its address names the tab the windows say is
 * in front, and a move to another tab in front is a move of the address, a
 * step of the web browser's history unless it only changes how the same place
 * is seen. The address follows the windows and never moves them: a
 * navigation the store did not write is a request to them. This module says
 * what an address holds; the page's store moves it.
 */

/** The query field that names the tab in front. */
export const TAB_FIELD = 'tab'

/** The field an address kept from before the work had its name says for `tab`. */
const TAB_FIELD_BEFORE = 'panel'

/** The work's query: `tab` and the shown pinned tab's place, empty while no window is in front. */
export type WorkAddress = PlaceFields

/**
 * The address of the work whose window in front shows `inFront`, or none,
 * with a pinned kind's place read from `pinned`, through the pages' `kinds`.
 */
export function workAddress(inFront: string | null, pinned: PinnedTabs, kinds: readonly TabKind<unknown>[]): WorkAddress {
  if (inFront === null) {
    return {}
  }
  const selection = inFront
  const kind = kinds.find((candidate) => candidate.kind === selection && candidate.pinned)
  const data = kind?.place ? kind.schema.safeParse(pinnedData(pinned, kind)) : null
  if (!kind?.place || !data?.success) {
    return { [TAB_FIELD]: selection }
  }
  const { [TAB_FIELD]: _named, ...place } = kind.place.write(data.data)
  return { [TAB_FIELD]: selection, ...place }
}

/** The tab the address names in front; null while it names none. */
export function addressSelection(address: WorkAddress): string | null {
  return address[TAB_FIELD] ?? address[TAB_FIELD_BEFORE] ?? null
}

/**
 * The data a pinned `kind` shows at the place `address` names, from what it
 * shows now: `current` itself when the place is where it is, so its view
 * does not draw again; the kind's start when the address does not read.
 * Null when the kind has no place.
 */
export function placeData(kind: TabKind<unknown>, address: WorkAddress, current: unknown): unknown {
  if (!kind.place || !kind.pinned) {
    return null
  }
  const parsed = kind.schema.safeParse(current)
  const shown = parsed.success ? parsed.data : kind.pinned.data()
  const { [TAB_FIELD]: _named, [TAB_FIELD_BEFORE]: _namedBefore, ...fields } = address
  const next = kind.place.read(fields, shown) ?? kind.pinned.data()
  const changed = next !== null && typeof next === 'object' && Object.keys(dataChanges(current, { ...next })).length > 0
  return changed ? next : current
}

/** Whether two addresses say the same. */
export function sameAddress(a: WorkAddress, b: WorkAddress): boolean {
  const fields = Object.keys(a)
  return fields.length === Object.keys(b).length && fields.every((field) => a[field] === b[field])
}
