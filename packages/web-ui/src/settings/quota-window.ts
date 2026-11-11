import type { SettingsQuotaWindow } from './types'

/**
 * Whether a usage window's reset has passed at `now`, in milliseconds. Such a
 * window has reset, as the account's routing counts it
 * (`crates/provider-registry/src/rotation.rs`), though the vendor's last
 * report, made before, still says how much of it was used.
 */
export function hasReset(window: SettingsQuotaWindow, now: number): boolean {
  return window.resetsAt !== null && Date.parse(window.resetsAt) <= now
}

/**
 * A window that has reset, until the vendor reports it again: nothing of it
 * is used, and when it next resets is not known yet.
 */
export function afterReset(window: SettingsQuotaWindow): SettingsQuotaWindow {
  return { ...window, used: 0, resetsAt: null }
}
