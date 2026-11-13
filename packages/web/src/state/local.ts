import { z } from 'zod'
import { logStorageFailed } from '@jush-ai/web-ui/infra/log'
import { SIDEBAR_GROUPS } from '@jush-ai/web-ui/sidebar/list-model'
import { holds } from './account-owner'

const localStateSchema = z.object({
  foldedProjects: z.array(z.string()),
  /** The sidebar's groups folded under their label; absent until the reader folds one. */
  foldedGroups: z.array(z.enum(SIDEBAR_GROUPS)).optional(),
  /** The sidebar's width in px; absent until the reader resizes it. */
  sidebarWidth: z.number().int().optional(),
  /** Whether a computer's sidebar is collapsed; absent until the reader collapses it. */
  sidebarCollapsed: z.boolean().optional(),
  /**
   * Whether this browser was offered a passkey and answered, by either
   * button: it is not offered again here.
   */
  passkeyOfferAnswered: z.boolean().optional(),
  /**
   * Each tab's count of the times its kind asked that the user see it, as
   * this page's work last applied it, by tab id, keyed by conversation
   * id.
   */
  tabsShown: z.record(z.string(), z.record(z.string(), z.number().int().min(1))).optional(),
  /** The workspace this browser last switched to, which a page that shows no conversation opens. */
  lastWorkspace: z.string().optional(),
  /** The conversation this browser last showed in each workspace, which a switch to it shows again, keyed by workspace id. */
  workspaceConversations: z.record(z.string(), z.string()).optional(),
})
export type LocalState = z.infer<typeof localStateSchema>

export function emptyLocalState(): LocalState {
  return {
    foldedProjects: [],
  }
}

export function readLocalState(userId: string): LocalState {
  try {
    const raw = localStorage.getItem(`jush.preferences.${userId}`)
    return raw ? localStateSchema.parse(renamed(JSON.parse(raw))) : emptyLocalState()
  } catch (error) {
    // The web browser's storage is optional. Invalid external entries are
    // discarded as a whole; no legacy keys or partially repaired records enter
    // product state.
    logStorageFailed('local_preferences', 'read', error)
    return emptyLocalState()
  }
}

/**
 * `raw` with the names a browser kept before the conversation's work had its
 * own: what the work panel applied of each tab's showings is `tabsShown`, so
 * a tab shown once is not shown again. The next write keeps only the new name.
 */
function renamed(raw: unknown): unknown {
  if (raw === null || typeof raw !== 'object' || !('workPanelShown' in raw) || 'tabsShown' in raw) {
    return raw
  }
  const { workPanelShown, ...rest } = raw as Record<string, unknown>
  return { ...rest, tabsShown: workPanelShown }
}

export function writeLocalState(userId: string, value: LocalState): void {
  // A page whose account no longer holds the browser's data writes none of it.
  if (!holds(userId)) {
    return
  }
  try {
    localStorage.setItem(`jush.preferences.${userId}`, JSON.stringify(value))
  } catch (error) {
    // Private browsing and storage quotas must not prevent server operations.
    logStorageFailed('local_preferences', 'write', error)
  }
}
