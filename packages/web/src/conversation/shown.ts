import { computed, watch, type Ref } from 'vue'
import type { Router } from 'vue-router'
import { provideBlobUrl } from '@demicodes/web-ui/agent/media-source'
import { provideKeepPicture } from '@demicodes/web-ui/agent/comments/pictures'
import { provideSendTargets, type SendTargets } from '@demicodes/web-ui/agent/send-to-conversation'
import { pageMediaViewer, provideMediaViewer } from '@demicodes/web-ui/files/media-viewer'
import { keptComputed } from '@demicodes/web-ui/infra/keep-same'
import { providePageHost } from '@demicodes/web-ui/plugins/page'
import type { SearchRow } from '@demicodes/web-ui/search/search'
import type { SidebarConversation } from '@demicodes/web-ui/sidebar/types'
import { searchConversations } from '../api/search'
import { blobUrl, keepPicture } from '../api/uploads'
import { productPageHost } from '../plugins/host'
import { attend, attended } from '../state/attention'
import { useProduct } from '../state/product'
import { useResources } from '../state/resources'
import type { Conversation } from '../state/types'
import { useWorkspaces } from '../state/workspaces'
import { useConversationNavigation } from './navigation'
import { useConversations } from './store'
import { useWork } from './work'

/** How many recent conversations the search window lists before the user types. */
const RECENT_CONVERSATIONS = 20

/**
 * The sidebar's row of `conversation`: its own fields and the device it runs
 * on. Every field is named, so a field the row type gains fails to compile
 * here, and a row reads none of the conversation's other state, which
 * changes as it streams.
 */
export function sidebarRow(
  conversation: Conversation,
  resources: ReturnType<typeof useResources>,
): { [Field in keyof Required<SidebarConversation>]: SidebarConversation[Field] } {
  return {
    id: conversation.id,
    title: conversation.title,
    updatedAt: conversation.updatedAt,
    status: conversation.status,
    projectId: conversation.projectId,
    pinned: conversation.pinned,
    unread: conversation.unread,
    needsYou: conversation.needsYou,
    device: resources.conversationDevice(conversation.target),
    scheduled: conversation.scheduled,
    worktree: conversation.worktree,
  }
}

/** What the page around a shown conversation gives its views (`shownConversation`). */
export type ShownConversation = ReturnType<typeof shownConversation>

/**
 * What the views take from the page around the conversation it shows,
 * `activeId`, alike in the web app's window and in a program's elements
 * (`elements/main.ts`): the plugins' pages, the media viewer, Send to
 * Conversation, its work's tabs read and its read mark, through `router`.
 * Answers them, for `provideShownConversation` to give each view, with a
 * link's Open in Browser Window and what the search window lists before the
 * user types.
 */
export function shownConversation(activeId: Ref<string | null>, router: Router) {
  const product = useProduct()
  const conversations = useConversations()
  const resources = useResources()
  const workspaces = useWorkspaces()
  const work = useWork()
  const { open, create } = useConversationNavigation(router)
  const pageHost = productPageHost()
  // A conversation's pictures and videos open in its Attachments window,
  // and in the viewer's window while no page the user has on opens it.
  const mediaViewer = pageMediaViewer({
    reveal: () => {
      const conversationId = activeId.value
      if (conversationId === null || !work.canOpen('media')) {
        return false
      }
      work.openIn(conversationId, { intent: 'media', payload: {} })
      return true
    },
  })
  /** The conversations of the shown workspace the backend has, the most recently active first. */
  const activeConversations = computed<SearchRow[]>(() =>
    conversations.items
      .filter((conversation) => conversation.persistence === 'synced' && conversation.workspaceId === workspaces.current)
      .toSorted((a, b) => b.updatedAt.localeCompare(a.updatedAt))
      .map((conversation) => ({
        conversationId: conversation.id,
        title: conversation.title,
        titleRanges: [],
        archived: conversation.archived,
        lastActiveAt: conversation.updatedAt,
        match: null,
      })),
  )
  /** What the search window lists before the user types: the conversations most recently active. */
  const recentConversations = keptComputed(() => activeConversations.value.slice(0, RECENT_CONVERSATIONS))
  // Send to Conversation: the search window's sources, and the words go to the end of the chosen draft.
  const sendTargets: SendTargets = {
    recent: () => activeConversations.value,
    search: searchConversations,
    describe: (id) => {
      // A row the sidebar lists: none for an archived one a search found.
      const conversation = conversations.listed.find((item) => item.id === id && item.workspaceId === workspaces.current)
      if (!conversation)
        return null
      return {
        status: sidebarRow(conversation, resources),
        project: resources.projects.find((project) => project.id === conversation.projectId)?.name ?? null,
      }
    },
    shown: () => product.activeConversationId,
    send: (to, words) => {
      const shown = conversations.items.find((item) => item.id === product.activeConversationId)
      const id = to ?? create(shown?.projectId ?? null)
      if (to !== null)
        open(to)
      const target = conversations.items.find((item) => item.id === id)
      if (target)
        conversations.appendToDraft(target, words)
    },
  }
  // The workspace shown is the shown conversation's; this browser remembers
  // the one it shows last, and the conversation it showed last in each.
  watch(activeId, (id) => { workspaces.shownConversation = id }, { immediate: true })
  watch(
    () => conversations.items.find((item) => item.id === activeId.value),
    (shown) => {
      if (shown) {
        workspaces.showed(shown)
      }
    },
  )
  // Before its work arrives: what the work held when the conversation came on screen decides what opens it on a phone.
  watch(activeId, (conversationId) => work.shown(conversationId), { immediate: true })
  // What the viewer shows is the open conversation's: another's Media tab
  // shows the medium it showed, found again in its own transcript.
  watch(activeId, () => mediaViewer.close())
  // The open conversation's tabs are read with no window open too, since the frame then mounts no
  // window: a tab a kind asks to show opens its window.
  watch(
    [activeId, () => activeId.value !== null && work.recorded(activeId.value)],
    ([conversationId]) => {
      if (conversationId !== null) {
        work.load(conversationId)
      }
    },
    { immediate: true },
  )
  /** A link's Open in Browser Window: a new tab of the shown conversation's browser, while a page the user has on opens one. */
  const openInBrowserWindow = computed(() => {
    const conversationId = activeId.value
    if (conversationId === null || !work.canOpen('web')) {
      return undefined
    }
    return (url: string) => work.openIn(conversationId, { intent: 'web', payload: { url } })
  })
  // The shown conversation is read while the user attends the page, and a
  // change of it is the user's action, from the sidebar, a link, Back or a
  // notification.
  watch(activeId, () => attend(), { immediate: true })
  watch(
    [
      activeId,
      attended,
      () => conversations.items.find((item) => item.id === activeId.value)?.unread,
      () => conversations.items.find((item) => item.id === activeId.value)?.needsYou,
      () => conversations.items.find((item) => item.id === activeId.value)?.load,
    ],
    () => {
      if (activeId.value) {
        conversations.markRead(activeId.value)
      }
    },
    { immediate: true },
  )
  return { pageHost, mediaViewer, sendTargets, openInBrowserWindow, recentConversations }
}

/** Gives the views below what `shown` holds, with their messages' media and pictures. */
export function provideShownConversation(shown: ShownConversation): void {
  provideBlobUrl(blobUrl)
  provideKeepPicture(keepPicture)
  providePageHost(shown.pageHost)
  provideMediaViewer(shown.mediaViewer)
  provideSendTargets(shown.sendTargets)
}
