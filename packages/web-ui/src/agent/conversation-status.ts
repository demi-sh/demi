import type { SessionPhase } from '@demicodes/protocol'
import type { StatusDotTone } from '../ui/StatusDot.vue'
import type { ConversationState } from './types'
import { isSubagentRunning, type SubagentRecord } from './subagents'
import { sessionWorks } from './session-status'
import { isTerminalRunning, type TerminalRecord } from './terminals'

/**
 * Where a conversation, an agent or a command stands. `active` is a model at
 * work, whose dot breathes; `awaiting_commands` runs only for a command, as a
 * conversation whose agent waits for one, or the command itself, whose dot
 * is steady.
 */
export type ConversationStatus = 'idle' | 'active' | 'awaiting_commands' | 'done' | 'error' | 'aborted'

/** Whether it runs, a model at work or not: it cannot be archived, and its dot is the accent. */
export function isRunning(status: ConversationStatus): boolean {
  return status === 'active' || status === 'awaiting_commands'
}

/**
 * The tone of a conversation's status dot wherever it shows, a sidebar row's, a
 * tab's or an agent's mark, so one state has one look everywhere:
 *
 * - running: the accent, which breathes
 * - finished: green
 * - failed: red
 *
 * Green and red say there is something new to see. An agent's tab and link
 * have no read state and stay green or red; a sidebar row says what is unread
 * instead (`conversationDotTone`).
 *
 * A Stop is the user's own decision, neither finished nor failed. Like a
 * conversation that has run nothing yet, it has no tone: the place's settled
 * look.
 */
export function statusTone(status: ConversationStatus): StatusDotTone | null {
  switch (status) {
    case 'active':
    case 'awaiting_commands':
      return 'accent'
    case 'done':
      return 'success'
    case 'error':
      return 'danger'
    case 'aborted':
    case 'idle':
      return null
  }
}

/** Whether the session compacts, as the user asked or by itself while idle. */
export function isCompacting(phase: SessionPhase): boolean {
  return phase === 'compacting' || phase === 'compacting_idle'
}

/**
 * How a conversation runs, or null while it does not: active while a model
 * works, the root's or a running child's, and awaiting commands while it runs
 * only because a child is live or a command that is not a service runs, whose
 * end wakes its agent. A service runs until stopped, so it never keeps the
 * conversation running, nor does the root's compaction while idle, which
 * nobody asked for; the backend counts the same way
 * (`crates/agent-server/src/server/tree.rs`).
 */
export function conversationRun(
  phase: ConversationState['phase'],
  subagents: readonly Pick<SubagentRecord, 'phase' | 'sessionPhase'>[] = [],
  terminals: readonly Pick<TerminalRecord, 'phase' | 'service'>[] = [],
): 'active' | 'awaiting_commands' | null {
  const live = subagents.filter((agent) => isSubagentRunning(agent.phase))
  if (sessionWorks(phase) || live.some((agent) => sessionWorks(agent.sessionPhase))) {
    return 'active'
  }
  if (live.length > 0 || terminals.some((terminal) => isTerminalRunning(terminal.phase) && !terminal.service)) {
    return 'awaiting_commands'
  }
  return null
}
