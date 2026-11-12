<script setup lang="ts">
import { t } from './locales/text'
import { computed } from 'vue'
import { Activity, History, SquareTerminal } from '@lucide/vue'
import { appOverlayStore } from '../overlay/appOverlay'
import Dropdown from '../ui/Dropdown.vue'
import IconButton from '../ui/IconButton.vue'
import HighlightText from '../ui/HighlightText.vue'
import Menu from '../ui/Menu.vue'
import Tag from '../ui/Tag.vue'
import { formatSpan } from '../ui/dates'
import { elapsedBetween } from '../composables/useElapsedTime'
import type { CommandKind, TerminalRecord } from './terminals'

/**
 * The Completed list of the Running or the Services window, the counterpart
 * of Agents' (`SubagentHistoryMenu`): the commands of its kind that ended,
 * newest first, each with how long it ran. Picking one shows it in the
 * window's ended tab.
 */
const props = defineProps<{
  kind: CommandKind
  /** The ended commands, newest first. */
  terminals: readonly TerminalRecord[]
  activeId: string | null
}>()

const emit = defineEmits<{
  select: [id: string]
}>()

const items = computed(() => props.terminals.map((terminal) => ({
  id: terminal.id,
  label: terminal.title,
  icon: props.kind === 'service' ? Activity : SquareTerminal,
  value: formatSpan(elapsedBetween(Date.parse(terminal.startedAt), Date.parse(terminal.endedAt ?? terminal.startedAt)) ?? 0),
  phase: terminal.phase,
  failed: terminal.phase === 'exited' && terminal.exitCode !== undefined && terminal.exitCode !== 0,
})))

function select(id: string, close: () => void): void {
  emit('select', id)
  close()
}
</script>

<template>
  <Dropdown :overlay-store="appOverlayStore" :offset="6">
    <template #trigger="{ isOpen }">
      <IconButton
        :icon="History"
        size="sm"
        variant="ghost"
        :pressed="isOpen"
        :aria-label="t('terminals.completed')"
      />
    </template>
    <template #content="{ close }">
      <Menu
        class="w-80"
        filterable
        :filter-placeholder="t('terminals.searchCompleted')"
        :empty-text="t('terminals.noneCompleted')"
        :items="items"
        :selected-id="activeId ?? undefined"
        :item-height="28"
        @select="select($event, close)"
      >
        <!--
          A command that failed, was stopped or was lost says so after its
          title; one that finished says nothing, as every row here ended.
        -->
        <template #item="{ item, query }">
          <span class="flex min-w-0 flex-1 items-center gap-1.5">
            <span class="min-w-0 truncate"><HighlightText :text="item.label" :query="query" /></span>
            <Tag v-if="item.failed" tone="danger" class="shrink-0">{{ t('transcript.runEnd.failed') }}</Tag>
            <Tag v-else-if="item.phase === 'aborted'" class="shrink-0">{{ t('transcript.runEnd.stopped') }}</Tag>
            <Tag v-else-if="item.phase === 'lost'" class="shrink-0">{{ t('transcript.runEnd.lost') }}</Tag>
          </span>
        </template>
      </Menu>
    </template>
  </Dropdown>
</template>
