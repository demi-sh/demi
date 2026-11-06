<script setup lang="ts">
import { computed, onUnmounted, ref, useHost, watch } from 'vue'
import SearchDialog from '@demicodes/web-ui/search/SearchDialog.vue'
import type { MessageRow, MessageSource, SearchRow } from '@demicodes/web-ui/search/search'
import { appOverlayStore } from '@demicodes/web-ui/overlay/appOverlay'
import ChatPage from '../conversation/ChatPage.vue'
import { useConversationNavigation } from '../conversation/navigation'
import { provideShownConversation } from '../conversation/shown'
import { searchConversation, searchConversations } from '../api/search'
import PageHosts from './PageHosts.vue'
import { elementsPage } from './page'

/**
 * `<demi-conversation>`: the web app's conversation view, its transcript,
 * composer and cards, its work's windows and taskbar, with Find in
 * Conversation, and what the web app's window gives it around while it
 * leads the page (`PageHosts.vue`).
 */
const props = defineProps<{
  /** Where the program forwards the core's routes, such as `/demi/api`. */
  api: string
  /** The conversation it shows. */
  conversation: string
}>()

const page = elementsPage(props.api)
provideShownConversation(page.shown)
const element = useHost()!
watch(() => props.conversation, (id) => page.show(id, element), { immediate: true })
onUnmounted(() => page.leave(element))

const { openAt } = useConversationNavigation()
/** Find in Conversation's window, which searches the conversation shown. */
const searchOpen = ref(false)
const searchIn = computed<MessageSource>(() => (query, signal) => searchConversation(props.conversation, query, signal))
function openSearchResult(row: SearchRow): void {
  searchOpen.value = false
  openAt(row.conversationId, null, row.match?.blockId ?? null)
}
function openFoundMessage(row: MessageRow): void {
  searchOpen.value = false
  openAt(props.conversation, null, row.blockId)
}
</script>

<template>
  <div class="demi-conversation flex h-full min-h-0 flex-col">
    <ChatPage :id="conversation" @find="searchOpen = true" />
  </div>
  <SearchDialog
    :is-open="searchOpen"
    :overlay-store="appOverlayStore"
    :recent="page.shown.recentConversations.value"
    :search="searchConversations"
    :search-in="searchIn"
    @close="searchOpen = false"
    @open="openSearchResult"
    @open-message="openFoundMessage"
  />
  <PageHosts v-if="page.leader.value === element" :page="page" />
</template>
