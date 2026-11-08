import type { StatusDotTone } from '../ui/StatusDot.vue'
import { isRunning, statusTone, type ConversationStatus } from '../agent/conversation-status'
import type { SidebarConversation, UnreadEnd } from './types'

/**
 * The tone of a conversation's sidebar dot, which the row and the browser tab both
 * read:
 *
 * - Yellow while a permission request waits unread, over every other mark.
 * - The accent while it runs.
 * - Green or red by how the latest turn the user asked for ended, until it is read,
 *   whatever turns another conversation or a schedule started ran after it.
 * - None for a settled row.
 */
export function conversationDotTone(
  conversation: Pick<SidebarConversation, 'status' | 'needsYou' | 'unread'>,
): StatusDotTone | null {
  if (conversation.needsYou)
    return 'attention'
  if (isRunning(conversation.status))
    return statusTone(conversation.status)
  return conversation.unread === null ? null : statusTone(UNREAD_STATUS[conversation.unread])
}

/** The status whose tone each unread end wears. */
const UNREAD_STATUS: Record<UnreadEnd, ConversationStatus> = { answered: 'done', failed: 'error' }

/** The dots that say something is new, the most urgent first. */
export const NEWS_TONES = ['danger', 'attention', 'success'] as const
export type NewsTone = (typeof NEWS_TONES)[number]

/**
 * The conversation whose dot stands for `conversations` where their rows do not
 * show, such as under a folded label: the one with the most urgent news, the
 * first listed among equals, or null while none has any. The open conversation
 * shows its own row, so it lends none.
 */
export function mostUrgentNews<C extends Pick<SidebarConversation, 'id' | 'status' | 'needsYou' | 'unread'>>(
  conversations: readonly C[],
  openId: string | null,
): C | null {
  let found: C | null = null
  let rank: number = NEWS_TONES.length
  for (const conversation of conversations) {
    if (conversation.id === openId)
      continue
    const tone = conversationDotTone(conversation)
    const at = NEWS_TONES.indexOf(tone as NewsTone)
    if (at >= 0 && at < rank) {
      found = conversation
      rank = at
    }
  }
  return found
}
