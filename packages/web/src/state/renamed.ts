// What the page kept under the names of before the product took the name
// Jush moves to the new names once, as the page starts. Its module runs
// first in `main.ts`, so every key moves before a module reads it.
//
// - `localStorage` keys that begin with `demi.` or `demi-` take `jush.` or
//   `jush-`, where no key of the new name is yet.
// - The drafts move from the database `demi-drafts` to `jush-drafts`
//   (`moveEarlierDrafts`); the offline copy and the plugins' cache, which
//   the page fills again, are deleted.
//
// It goes with the other migrations when the history starts again at the
// first formal release.

const EARLIER = 'demi'
const CURRENT = 'jush'

for (const key of Object.keys(localStorage)) {
  if (!key.startsWith(`${EARLIER}.`) && !key.startsWith(`${EARLIER}-`))
    continue
  const renamed = CURRENT + key.slice(EARLIER.length)
  const value = localStorage.getItem(key)
  if (value !== null && localStorage.getItem(renamed) === null)
    localStorage.setItem(renamed, value)
  localStorage.removeItem(key)
}

for (const name of ['offline', 'plugin-cache'])
  indexedDB.deleteDatabase(`${EARLIER}-${name}`)

/** The name of the earlier database of the drafts. */
export const EARLIER_DRAFTS = `${EARLIER}-drafts`
