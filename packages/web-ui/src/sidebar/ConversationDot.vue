<script setup lang="ts">
import { t } from './locales/text'
import { computed } from 'vue'
import StatusDot from '@demicodes/web-ui/ui/StatusDot.vue'
import CornerDot from '@demicodes/web-ui/ui/CornerDot.vue'
import { conversationDotTone } from './conversation-dot'
import type { SidebarConversation } from './types'

// One quiet mark: yellow while a permission request waits for the user and has
// not been read, the same size as every other dot and with nothing around it,
// over every other mark. Otherwise it is the tone the conversation's state wears
// on its tab too: the accent breathing while it runs, green once finished and
// red once failed, until the user reads it.
//
// A read answer, failure or request, a turn the user stopped (their own
// decision), and a conversation that ran nothing leave the row settled: a faint
// ring in the dot's place, so the column under a project's icon is never empty.
//
// A folded group's label, a folded project's row and another workspace's tile
// carry the dot of the conversation that stands for those they hide
// (`groupDot`), so the mark is drawn here alone. On an icon's corner, as a workspace's tile
// carries it, a settled conversation draws nothing.
const props = defineProps<{
  conversation: Pick<SidebarConversation, 'status' | 'needsYou' | 'unread'>
  /** On the corner of the icon its parent holds (`CornerDot`). */
  corner?: boolean
}>()

const tone = computed(() => conversationDotTone(props.conversation))
</script>

<template>
  <template v-if="corner">
    <CornerDot
      v-if="tone"
      :tone="tone"
      :label="tone === 'attention' ? t('dot.needsYou') : undefined"
    />
  </template>
  <span v-else class="flex size-3.5 shrink-0 items-center justify-center phone:size-4 phone:[&>*]:scale-[1.35]">
    <StatusDot v-if="tone === 'attention'" tone="attention" :label="t('dot.needsYou')" />
    <StatusDot v-else-if="tone" :tone="tone" :breathing="conversation.status === 'active'" />
    <StatusDot v-else tone="muted" ring />
  </span>
</template>
