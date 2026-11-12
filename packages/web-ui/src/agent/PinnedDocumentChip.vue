<script setup lang="ts">
import { Pin, X } from '@lucide/vue'
import IconButton from '../ui/IconButton.vue'
import Tooltip from '../ui/Tooltip.vue'
import { ICON_PX } from '../ui/icon-metrics'
import { t } from './locales/text'

/** A document the conversation's main agent pinned, a file of a device. */
export interface PinnedDocumentView {
  /** What unpins it. */
  id: number
  /** Its absolute path on the device. */
  path: string
  /** What the user calls the device. */
  device: string
  deviceId: string
}

/**
 * A document the agent pinned, as the dock's chips row shows it: the pin's
 * mark, the file's name and a ghost ×, which unpins it. Its path and device
 * are in the tooltip. A click on the name opens the file where the page
 * opens one of the conversation's device; a file of another device only
 * shows.
 */
const props = defineProps<{
  pin: PinnedDocumentView
  /** Whether the page opens the file. */
  opens: boolean
}>()
const emit = defineEmits<{
  open: []
  unpin: []
}>()
const name = () => props.pin.path.split(/[\\/]/).pop() || props.pin.path
</script>

<template>
  <span data-pinned-document class="btn-solid inline-flex h-chip select-none items-center rounded-full text-chrome text-fg-body">
    <Tooltip :content="`${pin.path} · ${pin.device}`">
      <component
        :is="opens ? 'button' : 'span'"
        :type="opens ? 'button' : undefined"
        class="inline-flex h-full min-w-0 items-center gap-1.5 rounded-l-full pl-2.5 pr-1.5"
        :class="opens && 'hover:text-fg-emphasis'"
        @mousedown.prevent
        @click="opens && emit('open')"
      >
        <Pin :size="ICON_PX.in24" class="shrink-0 text-fg-subtle" aria-hidden="true" />
        <span class="min-w-0 truncate">{{ name() }}</span>
      </component>
    </Tooltip>
    <Tooltip :content="t('dock.unpin')">
      <IconButton
        :icon="X"
        size="xs"
        variant="ghost"
        class="mr-1"
        :aria-label="t('dock.unpin')"
        @mousedown.prevent
        @click="emit('unpin')"
      />
    </Tooltip>
  </span>
</template>
