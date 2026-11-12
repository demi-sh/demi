<script setup lang="ts">
import { t } from './locales/text'
import { computed, watch } from 'vue'
import { SquareTerminal } from '@lucide/vue'
import RegionStatus from '../ui/RegionStatus.vue'
import TerminalView from './TerminalView.vue'
import type { TerminalRecord } from './terminals'

/**
 * What the Running or the Services window shows: the command its tab names,
 * or that none runs. An ended command from the conversation's list asks for
 * its output (`read`) the first time it shows, and shows that it loads, or
 * why it could not, until it has it.
 */
const props = defineProps<{
  /** The window's commands, its tabs. */
  terminals: readonly TerminalRecord[]
  activeId: string | null
}>()

const emit = defineEmits<{
  /** Read the output of the ended command `id`. */
  read: [id: string]
}>()

const active = computed(() => props.terminals.find((terminal) => terminal.id === props.activeId) ?? props.terminals[0] ?? null)

watch(() => active.value?.outputRead?.state === 'unread' ? active.value.id : null, (unread) => {
  if (unread !== null) {
    emit('read', unread)
  }
}, { immediate: true })
</script>

<template>
  <RegionStatus
    v-if="active?.outputRead"
    class="h-full"
    :status="active.outputRead.state === 'failed' ? 'failed' : 'loading'"
    :label="active.outputRead.state === 'failed' ? t('terminals.outputUnavailable') : t('terminals.outputLoading')"
    :loading-label="t('terminals.outputLoading')"
    :detail="active.outputRead.state === 'failed' ? active.outputRead.message : null"
    :on-retry="() => emit('read', active!.id)"
  />
  <RegionStatus
    v-else-if="active?.notStored"
    class="h-full"
    status="note"
    :icon="SquareTerminal"
    :label="t('terminals.outputNotStored', { reason: active.notStored })"
  />
  <TerminalView v-else-if="active" :key="active.id" :terminal="active" />
  <RegionStatus v-else class="h-full" status="note" :icon="SquareTerminal" :label="t('terminals.none')" />
</template>
