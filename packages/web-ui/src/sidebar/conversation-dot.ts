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

/** What a dot that stands for several conversations reads of each. */
export type DotSource = Pick<SidebarConversation, 'status' | 'needsYou' | 'unread'>

/**
 * The conversation whose dot stands for `conversations` where their rows do not
 * show: under a folded label, on a folded project's row, on another
 * workspace's tile.
 *
 * - The one with the most urgent news, red, then yellow, then green.
 * - With no news, one that runs: one whose model works, so the dot breathes,
 *   before one that only waits for a command.
 * - The first listed among equals; null while none has news or runs.
 *
 * The open conversation shows its own row, so it lends none. Once its news is
 * read, a group whose other conversations still run turns back to the accent.
 */
export function groupDot<C extends { id: string } & DotSource>(
  conversations: readonly C[],
  openId: string | null,
): C | null {
  let found: C | null = null
  let rank = Number.POSITIVE_INFINITY
  for (const conversation of conversations) {
    if (conversation.id === openId)
      continue
    const at = groupDotRank(conversation)
    if (at < rank) {
      found = conversation
      rank = at
    }
  }
  return found
}

/** How urgent a conversation's dot is where it stands for others: news first, then a model at work, then a command. */
function groupDotRank(conversation: DotSource): number {
  const tone = conversationDotTone(conversation)
  const news = NEWS_TONES.indexOf(tone as NewsTone)
  if (news >= 0)
    return news
  if (conversation.status === 'active')
    return NEWS_TONES.length
  if (conversation.status === 'awaiting_commands')
    return NEWS_TONES.length + 1
  return Number.POSITIVE_INFINITY
}
