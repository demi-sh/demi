import { computed, shallowRef, type ComputedRef, type ShallowRef } from 'vue'
import { Slot, type SlotChange } from '../infra/slot'
import { removeTabs, type WorkTab } from './work-tabs'

/**
 * One change of the conversation's tabs, or of where its windows stand. An
 * update sets the fields of `data` it names and removes the null ones.
 */
export type WorkChange =
  | { type: 'create'; tab: WorkTab; index?: number }
  | { type: 'update'; id: string; data: Record<string, unknown> }
  | { type: 'remove'; id: string }
  | { type: 'move'; id: string; index: number }
  | { type: 'windows'; windows: Record<string, unknown> }

/**
 * The backend's work: its tabs, how many changes made them, and where its
 * windows stand. It comes with the conversation's entry in the list, so the
 * windows draw as soon as the conversation is listed.
 */
export interface WorkRead {
  revision: number
  tabs: WorkTab[]
  windows?: Record<string, unknown> | null
}

/**
 * What a request of changes answers: the work's revision once they are in it,
 * and whether they changed it.
 */
export interface WorkAnswer {
  revision: number
  changed: boolean
}

/**
 * Where the work's changes go: the backend's route, or a fixture's. The
 * work itself comes to the page as its owner announces it
 * (`WorkTabs.receive`), never by a read.
 */
export interface WorkBackend {
  /**
   * Applies the changes in order, all or none, as one change of the work,
   * and answers the work's revision once they are in it and whether they
   * changed it.
   */
  send(changes: readonly WorkChange[]): Promise<WorkAnswer>
}

/**
 * `tabs` after `change`, as the backend applies it; a change with nothing to do
 * leaves them.
 */
export function applyWorkChange(tabs: readonly WorkTab[], change: WorkChange): readonly WorkTab[] {
  const index = (id: string) => tabs.findIndex((tab) => tab.id === id)
  switch (change.type) {
    case 'create': {
      if (index(change.tab.id) >= 0) {
        return [...tabs]
      }
      const at = Math.min(change.index ?? tabs.length, tabs.length)
      return [...tabs.slice(0, at), change.tab, ...tabs.slice(at)]
    }
    case 'update':
      return tabs.map((tab) => (tab.id === change.id ? { ...tab, data: mergeData(tab.data, change.data) } : tab))
    case 'remove':
      return tabs.filter((tab) => tab.id !== change.id)
    case 'move': {
      const from = index(change.id)
      if (from < 0) {
        return [...tabs]
      }
      const moving = tabs[from]!
      const others = tabs.filter((tab) => tab.id !== change.id)
      const at = Math.min(change.index, others.length)
      return [...others.slice(0, at), moving, ...others.slice(at)]
    }
    // The layout is no tab: the same tabs, so nothing that reads them reads them again.
    case 'windows':
      return tabs
  }
}

/** `data` with each field of `fields` set, and each null one removed. */
export function mergeData(data: unknown, fields: Record<string, unknown>): Record<string, unknown> {
  const merged: Record<string, unknown> = data !== null && typeof data === 'object' ? { ...data } : {}
  for (const [field, value] of Object.entries(fields)) {
    if (value === null) {
      delete merged[field]
    } else {
      merged[field] = value
    }
  }
  return merged
}

/**
 * The update that turns `before` into `after`: each field that changed, and
 * each removed one as null.
 */
export function dataChanges(before: unknown, after: Record<string, unknown>): Record<string, unknown> {
  const previous: Record<string, unknown> = before !== null && typeof before === 'object' ? { ...before } : {}
  const fields: Record<string, unknown> = {}
  for (const [field, value] of Object.entries(after)) {
    if (JSON.stringify(previous[field]) !== JSON.stringify(value)) {
      fields[field] = value
    }
  }
  for (const field of Object.keys(previous)) {
    if (!Object.hasOwn(after, field)) {
      fields[field] = null
    }
  }
  return fields
}

/**
 * A change of the page's that is not in the work it read yet, and the request
 * it will go in.
 */
interface Unsent {
  change: WorkChange
  shown: SlotChange<readonly WorkTab[]>
}

/**
 * The work's tabs as a page shows them: a slot (`infra/slot`) of the
 * work it last read, its revision the work's, with the page's own changes
 * that are not in it yet on top.
 *
 * - A change shows at once. The changes made while none is on its way go
 *   together, in order, as one request, so Close Others is one request.
 * - An answer that changed the work one revision past the one the page holds
 *   is the work with the request in it. Any other answer waits for the work
 *   it names, which the conversation's entry in the list brings.
 * - A change leaves once the work at least as new as its answer is held, so a
 *   work received after a removal never brings the tab back.
 * - A request the backend refuses leaves at once, and the windows show as the
 *   backend has it.
 */
export class WorkTabs {
  /** Before the first read the work is empty, so a change shows over it. */
  private readonly slot = new Slot<readonly WorkTab[]>([])
  /** The changes made and not yet sent, oldest first. */
  private unsent: Unsent[] = []
  private sending = false
  private isStarted = false
  /** The tabs the page shows. */
  readonly tabs: ComputedRef<readonly WorkTab[]>
  /**
   * Where the windows stand, null while the work says none: the page's own
   * layout at once, as it writes it, else the work's as last received. A
   * work received while the page's layout is on its way, or older than the
   * revision it landed at, leaves the page's.
   */
  readonly windows: ShallowRef<Record<string, unknown> | null> = shallowRef(null)
  /** The revision `windows` is known at; -1 before any. */
  private windowsRevision = -1
  /** Whether a layout of the page's is sent or to send, its answer not come. */
  private windowsSending = false

  constructor(
    private readonly backend: WorkBackend,
    /** Reports a change the backend refused. */
    private readonly refused: (error: unknown) => void,
  ) {
    this.tabs = computed(() => this.slot.shown.value ?? [])
  }

  /** The revision of the work last received; -1 before the first. */
  get revision(): number {
    return this.slot.revision()
  }

  /** Whether the page sends its changes, as it does once it opened the conversation. */
  get started(): boolean {
    return this.isStarted
  }

  /** Whether the work was received once. */
  get read(): boolean {
    return this.slot.read.value !== null
  }

  /**
   * Starts sending: before it, as for a conversation that has no backend
   * record yet, changes only show.
   */
  start(): void {
    if (this.isStarted) {
      return
    }
    this.isStarted = true
    void this.flush()
  }

  /**
   * Shows `changes` at once and sends them, together, after the changes before
   * them.
   */
  change(...changes: WorkChange[]): void {
    // A layout replaces the one before it that has not left yet: only the last is sent.
    if (changes.some((change) => change.type === 'windows')) {
      const replaced = this.unsent.filter((entry) => entry.change.type === 'windows')
      if (replaced.length > 0) {
        this.slot.drop(replaced.map((entry) => entry.shown))
        this.unsent = this.unsent.filter((entry) => entry.change.type !== 'windows')
      }
    }
    for (const change of changes) {
      if (change.type === 'windows') {
        this.windows.value = change.windows
        this.windowsSending = true
      }
    }
    this.unsent.push(...changes.map((change) => ({
      change,
      shown: this.slot.change((tabs) => applyWorkChange(tabs, change)),
    })))
    void this.flush()
  }

  /**
   * Takes the work as its owner announced it, such as in the
   * conversation's entry in the list: one no newer than the work held
   * changes nothing.
   */
  receive(work: WorkRead): void {
    if (!this.windowsSending && work.revision > this.windowsRevision) {
      this.windows.value = work.windows ?? null
      this.windowsRevision = work.revision
    }
    this.slot.replace(work.tabs, { revisions: { '': work.revision } })
  }

  private async flush(): Promise<void> {
    if (this.sending || !this.isStarted) {
      return
    }
    this.sending = true
    try {
      for (;;) {
        const batch = this.unsent.splice(0)
        if (batch.length === 0) {
          return
        }
        const shown = batch.map((entry) => entry.shown)
        this.slot.send(shown)
        let answer: WorkAnswer
        try {
          answer = await this.backend.send(batch.map((entry) => entry.change))
        } catch (error) {
          this.slot.drop(shown)
          this.landedWindows(batch, null)
          this.refused(error)
          continue
        }
        this.landedWindows(batch, answer.revision)
        // A request that alone made the next revision leaves the work as the
        // tabs held with it; any other answer waits for the work it names.
        const alone = this.read && answer.changed && answer.revision === this.revision + 1
        this.slot.land(shown, {
          revisions: { '': answer.revision },
          ...(alone ? { value: (tabs: readonly WorkTab[]) => batch.reduce((next, entry) => applyWorkChange(next, entry.change), tabs) } : {}),
        })
      }
    } finally {
      this.sending = false
    }
  }

  /**
   * A batch was answered, at `revision`, or refused: once no layout of the
   * page's is left to send, the work received past the revision the page's
   * layout stands at shows again.
   */
  private landedWindows(batch: readonly Unsent[], revision: number | null): void {
    if (!batch.some((entry) => entry.change.type === 'windows') || this.unsent.some((entry) => entry.change.type === 'windows')) {
      return
    }
    this.windowsSending = false
    if (revision !== null) {
      this.windowsRevision = Math.max(this.windowsRevision, revision)
    }
  }
}

/**
 * The work's selection history, the tab or kind it selected last at its end,
 * which closing tabs changes.
 */
export interface SelectionHistory {
  history: readonly string[]
}

/**
 * Closes the tabs `ids` as one change of the work, one request, like Close
 * Others; the windows show what was selected before a closed one.
 */
export function closeWorkTabs(selection: SelectionHistory, tabs: WorkTabs, ids: readonly string[]): void {
  selection.history = removeTabs({ history: selection.history, tabs: tabs.tabs.value }, ids).history
  tabs.change(...ids.map((id) => ({ type: 'remove' as const, id })))
}

/** Whether `data` is an object a kind's tab keeps as its `data`. */
function isData(data: unknown): data is Record<string, unknown> {
  return data !== null && typeof data === 'object' && !Array.isArray(data)
}

/**
 * The tab's next `data`, from its kind: only what changed is sent; a tab gone,
 * or data that is no object, sends nothing.
 */
export function updateWorkTab(tabs: WorkTabs, id: string, data: unknown): void {
  const current = tabs.tabs.value.find((tab) => tab.id === id)
  if (!current || !isData(data)) {
    return
  }
  const fields = dataChanges(current.data, data)
  if (Object.keys(fields).length > 0) {
    tabs.change({ type: 'update', id, data: fields })
  }
}
