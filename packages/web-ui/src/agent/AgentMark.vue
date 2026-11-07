<script setup lang="ts">
import { Bot } from '@lucide/vue'
import ConversationStatusDot from './ConversationStatusDot.vue'
import { subagentStatus, type AgentRun } from './subagents'

/**
 * An agent's mark: the robot, with the dot of its state on its corner.
 *
 * The dot changes as the agent runs and ends. The agent's tab in the Agents
 * window wears the mark, and so does a message's link to the agent. The
 * conversation's own agent, without a round, wears the robot alone.
 */
defineProps<{
  /** The agent's round, or null for the conversation's own agent. */
  run: Pick<AgentRun, 'phase' | 'sessionPhase'> | null
  size: number
}>()
</script>

<template>
  <span class="relative inline-flex shrink-0 items-center justify-center">
    <Bot :size="size" class="text-fg-subtle" />
    <ConversationStatusDot v-if="run" :status="subagentStatus(run)" />
  </span>
</template>
