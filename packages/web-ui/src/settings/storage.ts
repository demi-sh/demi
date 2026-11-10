import type { DeviceMark } from '../devices/mark'
import type { DeviceState } from '../devices/state'

/**
 * Settings › Storage's view of what Demi keeps (`crates/web-api-protocol/src/storage.rs`):
 * the server's kinds, the rules for every device, and each device's report.
 */
export interface StorageView {
  server: {
    measuredAt: string
    diskBytes: number | null
    diskAvailableBytes: number | null
    conversationsBytes: number
    attachmentsBytes: number
    /** Null to anyone but an administrator, and on a server without Clouds. */
    cloudsBytes: number | null
    logBytes: number | null
    searchBytes: number | null
    /** The log's rules, which only an administrator sees and sets. */
    log: LogRetention | null
  }
  rules: StorageRules
  /** Each device with what its runner last measured, null before it measured. */
  devices: { deviceId: string; report: DeviceStorageReport | null }[]
  /** The conversations the reports name. */
  conversations: StorageConversation[]
}

export interface LogRetention {
  keepDays: number
  maxBytes: number
}

export interface StorageRule {
  unusedDays: number | null
  maxBytes: number | null
}

export interface StorageRules {
  temporary: StorageRule
  cache: StorageRule
}

export interface DeviceStorageReport {
  measuredAt: string
  diskBytes: number | null
  diskAvailableBytes: number | null
  componentsBytes: number
  conversations: ConversationFiles[]
  lastCleanup: { at: string; cause: 'rules' | 'lowDisk' | 'user'; removedBytes: number } | null
}

export interface ConversationFiles {
  name: string
  temporaryBytes: number
  cacheBytes: number
  lastUsedAt: string | null
}

export interface StorageConversation {
  /** The name of its directory on a device, which `ConversationFiles.name` names. */
  name: string
  id: string
  title: string
  archived: boolean
  running: boolean
}

/** A device as Storage lists it. */
export interface StorageDevice {
  id: string
  name: string
  /** Its system, such as `macOS 26.5`; null before its runner first said. */
  system: string | null
  state: DeviceState | null
  /** Null for a device never connected, which draws the plain computer. */
  mark: DeviceMark | null
  cloud: boolean
}

/** What a device's request removes before it measures again. */
export type StorageAction =
  | { type: 'measure' }
  | { type: 'cleanUpTemporary' }
  | { type: 'clearCache' }
  | { type: 'deleteComponents' }
  | { type: 'conversation'; conversation: string; empty: boolean }

/** A GB as the page shows sizes. */
export const GB = 1024 ** 3

/** The days a rule's Unused For offers; null is Never. */
export const UNUSED_DAYS: readonly (number | null)[] = [7, 14, 30, 60, 90, null]
/** The limits a rule's At Most offers; null is No Limit. */
export const MAX_GB: readonly (number | null)[] = [5, 10, 20, 50, 100, null]
/** The days the log's Keep offers. */
export const LOG_DAYS: readonly number[] = [3, 7, 14, 30, 90]
/** The limits the log's At Most offers. */
export const LOG_GB: readonly number[] = [1, 2, 5, 10]

/** A report's temporary files, cache and their sum on the device. */
export function reportTotals(report: DeviceStorageReport): { temporary: number; cache: number; demi: number } {
  const temporary = report.conversations.reduce((sum, files) => sum + files.temporaryBytes, 0)
  const cache = report.conversations.reduce((sum, files) => sum + files.cacheBytes, 0)
  return { temporary, cache, demi: temporary + cache + report.componentsBytes }
}

/** Whether a kind is over its rule's limit, which only files in use keep it. */
export function overLimit(bytes: number, rule: StorageRule): boolean {
  return rule.maxBytes !== null && bytes > rule.maxBytes
}
