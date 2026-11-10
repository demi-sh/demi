<script setup lang="ts">
/**
 * The sidebar's workspace switcher, under the app's name. With one workspace
 * it is a row with its tile and name, whose menu makes another; with more,
 * one track of segments, one per workspace, a click on one switching to it.
 * The menu lists the workspaces, each with the keys that switch to it, then
 * New Workspace and Workspace Settings: the row's dropdown with one
 * workspace, a right-click on the segments with more.
 */
import { useResizeObserver } from '@vueuse/core'
import { computed, h, nextTick, ref, watch, type FunctionalComponent } from 'vue'
import { ChevronsUpDown, Plus, Settings } from '@lucide/vue'
import { appOverlayStore } from '../overlay/appOverlay'
import Dropdown from '../ui/Dropdown.vue'
import Menu from '../ui/Menu.vue'
import MenuEntries from '../ui/MenuEntries.vue'
import ConversationDot from '../sidebar/ConversationDot.vue'
import { groupDot, type DotSource } from '../sidebar/conversation-dot'
import Tooltip from '../ui/Tooltip.vue'
import { vContextMenu, type ContextMenuDeclaration, type ContextMenuGroup } from '../ui/context-menu'
import { pressOnKey } from '../ui/button-events'
import { revealScroll, sidewaysWheel } from '../agent/tab-strip'
import { usePhone } from '../ui/phone'
import { ICON_PX } from '../ui/icon-metrics'
import WorkspaceTile from './WorkspaceTile.vue'
import { t } from './locales/text'
import { workspaceName, type WorkspaceView } from './workspaces'

const props = defineProps<{
  workspaces: readonly WorkspaceView[]
  /** The workspace the tab shows. */
  currentId: string | null
  /** The conversation whose dot each workspace's tile carries, by workspace (`groupDot`); the shown one's tile carries none, since its rows show. */
  dots: Readonly<Record<string, DotSource>>
  /** The keys that switch to the workspace at each place of the list, as the keyboard settings name them. */
  shortcuts?: readonly (string | undefined)[]
}>()

const emit = defineEmits<{
  switch: [id: string]
  create: []
  openSettings: []
}>()

const current = computed(() => props.workspaces.find((workspace) => workspace.id === props.currentId) ?? props.workspaces[0] ?? null)
/**
 * How the switcher shows: one workspace as the row whose menu makes another;
 * two side by side, each with its tile and name; three and more as their
 * tiles alone, equally wide while they fit and scrolled sideways past that.
 */
const layout = computed(() => props.workspaces.length <= 1 ? 'row' : props.workspaces.length === 2 ? 'named' : 'tiles')

/** The switcher's menu, the row's dropdown and the segments' right-click alike: every workspace, then New Workspace and Workspace Settings. */
function menuGroups(): ContextMenuGroup[] {
  return [
    props.workspaces.map((workspace, index) => ({
      id: workspace.id,
      icon: tileOf(workspace),
      label: workspaceName(workspace),
      shortcut: props.shortcuts?.[index],
      checked: workspace.id === current.value?.id,
      run: () => emit('switch', workspace.id),
    })),
    [
      { icon: Plus, label: t('new'), run: () => emit('create') },
      { icon: Settings, label: t('settings'), run: () => emit('openSettings') },
    ],
  ]
}
const segmentsMenu: ContextMenuDeclaration = { entries: menuGroups }

const track = ref<HTMLElement | null>(null)
/** Which ends of the tiles have more past them, which those ends fade to say. */
const edges = ref({ start: false, end: false })
const phone = usePhone()
/** A segment's floor: the room its tile needs, or on a phone a finger's. */
const segmentMinPx = computed(() => phone.value ? 44 : 36)
/** The space between two segments. */
const SEGMENT_GAP_PX = 2
/** The width of the fade at an end with more past it; the shown segment is scrolled clear of it, as a tab is (`revealScroll`). */
const FADE_PX = 24
/**
 * Each tile's width while they do not all fit, null while they do: as many
 * whole tiles as the track holds and half of the next, so the cut tile says
 * that more lie past the end.
 */
const scrolledWidth = ref<number | null>(null)
function measureEdges(): void {
  const box = track.value
  const room = box?.clientWidth ?? 0
  const count = props.workspaces.length
  if (layout.value !== 'tiles' || count * (segmentMinPx.value + SEGMENT_GAP_PX) - SEGMENT_GAP_PX <= room) {
    scrolledWidth.value = null
  } else {
    const whole = Math.max(1, Math.floor(room / (segmentMinPx.value + SEGMENT_GAP_PX) - 0.5))
    scrolledWidth.value = room / (whole + 0.5) - SEGMENT_GAP_PX
  }
  const start = box !== null && box.scrollLeft > 0
  const end = box !== null && box.scrollLeft + box.clientWidth < box.scrollWidth - 1
  if (start !== edges.value.start || end !== edges.value.end) {
    edges.value = { start, end }
  }
  measurePast(box)
}

/**
 * The workspaces each end of the track hides, wholly or in part, by ID: their
 * dots stand at that end (`groupDot`), so a workspace that needs the user is
 * seen while its tile is scrolled away.
 */
const past = ref<{ start: readonly string[]; end: readonly string[] }>({ start: [], end: [] })
function measurePast(box: HTMLElement | null): void {
  const found = { start: [] as string[], end: [] as string[] }
  if (box && scrolledWidth.value !== null) {
    const room = box.getBoundingClientRect()
    for (const segment of box.querySelectorAll<HTMLElement>('[data-workspace]')) {
      const place = segment.getBoundingClientRect()
      const id = segment.dataset.workspace!
      if (place.left < room.left - 0.5) {
        found.start.push(id)
      } else if (place.right > room.right + 0.5) {
        found.end.push(id)
      }
    }
  }
  if (found.start.join() !== past.value.start.join() || found.end.join() !== past.value.end.join()) {
    past.value = found
  }
}
/** The dot each end of the track carries for the workspaces it hides. */
const pastDots = computed(() => {
  const at = (ids: readonly string[]) => groupDot(
    props.workspaces.filter((workspace) => ids.includes(workspace.id)).flatMap((workspace) => {
      const dot = dotOf(workspace)
      return dot ? [{ id: workspace.id, status: dot.status, needsYou: dot.needsYou, unread: dot.unread }] : []
    }),
    null,
  )
  return { start: at(past.value.start), end: at(past.value.end) }
})
/** The shown workspace's segment stays in view when the tiles scroll, and once more after their width changes. */
async function revealChosen(): Promise<void> {
  await nextTick()
  const width = scrolledWidth.value
  measureEdges()
  if (scrolledWidth.value !== width) {
    await nextTick()
  }
  bringIntoTrack(track.value?.querySelector<HTMLElement>('[aria-selected="true"]'))
  measureEdges()
}
/**
 * Scrolls the track, and nothing around it, until `segment` shows whole and
 * clear of the fades: `scrollIntoView` would scroll every box that holds the
 * switcher too.
 */
function bringIntoTrack(segment: HTMLElement | null | undefined): void {
  const box = track.value
  if (!box || !segment) {
    return
  }
  const room = box.getBoundingClientRect()
  const place = segment.getBoundingClientRect()
  const left = place.left - room.left + box.scrollLeft
  const target = revealScroll(box, { left, right: left + place.width }, FADE_PX)
  if (target !== null) {
    box.scrollLeft = target
  }
}
/** A wheel over the tiles moves them, whichever axis it turns on, as over a strip of tabs. */
function onWheel(event: WheelEvent): void {
  const box = track.value
  const delta = box ? sidewaysWheel(box, event) : null
  if (box && delta !== null) {
    event.preventDefault()
    box.scrollLeft += delta
  }
}
useResizeObserver(track, () => void revealChosen())
watch([current, () => props.workspaces.map((workspace) => workspace.id).join(), phone], revealChosen, { immediate: true })

/**
 * The segment the keys stand on, the one tab stop of the track: the shown
 * one until an arrow key moves it, and again once the focus leaves.
 */
const keyed = ref<string | null>(null)
const tabStop = computed(() => keyed.value ?? current.value?.id ?? null)
watch(current, () => {
  keyed.value = null
})
/**
 * Left and Right, Home and End move the focus along the segments, and Enter
 * or Space switches to the focused one, as tabs that wait to be chosen do:
 * the conversation a switch shows takes the focus, which would end the walk.
 */
function moveOnKey(event: KeyboardEvent, index: number): void {
  const count = props.workspaces.length
  const to = event.key === 'ArrowRight' ? (index + 1) % count
    : event.key === 'ArrowLeft' ? (index - 1 + count) % count
      : event.key === 'Home' ? 0
        : event.key === 'End' ? count - 1
          : null
  const next = to === null ? undefined : props.workspaces[to]
  if (!next) {
    return
  }
  event.preventDefault()
  keyed.value = next.id
  nextTick(() => {
    const segment = track.value?.querySelector<HTMLElement>(`[data-workspace="${CSS.escape(next.id)}"]`)
    segment?.focus({ preventScroll: true })
    bringIntoTrack(segment)
  })
}
/** The focus leaves the track: its tab stop is the shown segment again. */
function leaveTrack(event: FocusEvent): void {
  if (!(event.relatedTarget instanceof Node && track.value?.contains(event.relatedTarget))) {
    keyed.value = null
  }
}
/** The conversation whose dot a workspace's tile carries: none on the shown one's. */
function dotOf(workspace: WorkspaceView): DotSource | null {
  return workspace.id === current.value?.id ? null : props.dots[workspace.id] ?? null
}

/** A workspace's tile, with its dot, where a menu row draws its icon. */
function tileOf(workspace: WorkspaceView): FunctionalComponent {
  return () => {
    const dot = dotOf(workspace)
    return h('span', { class: 'relative inline-flex' }, [
      h(WorkspaceTile, { icon: workspace.icon, color: workspace.color, size: 16 }),
      dot ? h(ConversationDot, { conversation: dot, corner: true }) : null,
    ])
  }
}
</script>

<template>
  <div
    v-if="layout !== 'row'"
    v-context-menu="segmentsMenu"
    class="relative h-9 w-full rounded-lg border border-line-subtle bg-(--fill-color) p-0.5 [--fill-color:color-mix(in_srgb,var(--surface-current),var(--overlay)_6%)] *:on-fill phone:h-11"
  >
    <!-- The dots of the workspaces each end hides, in the frame's top corners, over the fade. -->
    <ConversationDot v-if="pastDots.start" :conversation="pastDots.start" class="pointer-events-none absolute left-0 top-0 z-10" />
    <ConversationDot v-if="pastDots.end" :conversation="pastDots.end" class="pointer-events-none absolute right-0 top-0 z-10" />
    <div
      ref="track"
      role="tablist"
      :aria-label="t('switcher')"
      :class="{ 'fades-start': edges.start, 'fades-end': edges.end }"
      class="workspace-track flex size-full items-center overflow-x-auto overflow-y-hidden rounded-md"
      :style="{ gap: `${SEGMENT_GAP_PX}px`, '--fade': `${FADE_PX}px` }"
      @scroll="measureEdges"
      @wheel="onWheel"
      @focusout="leaveTrack"
    >
      <Tooltip
        v-for="(workspace, index) in workspaces"
        :key="workspace.id"
        class="flex h-full"
        :class="scrolledWidth !== null ? 'shrink-0' : layout === 'named' ? 'min-w-0 flex-1' : 'flex-1'"
        :style="scrolledWidth !== null ? { width: `${scrolledWidth}px` } : undefined"
      >
        <div
          role="tab"
          :aria-selected="workspace.id === current?.id"
          :data-workspace="workspace.id"
          :aria-label="workspaceName(workspace)"
          :tabindex="workspace.id === tabStop ? 0 : -1"
          :style="{ minWidth: `${segmentMinPx}px` }"
          class="relative flex h-full w-full cursor-default select-none items-center justify-center gap-2 rounded-md px-2 transition-[background-color,box-shadow] duration-200 ease-out"
          :class="workspace.id === current?.id ? 'workspace-chosen' : 'hover:bg-active'"
          @click="emit('switch', workspace.id)"
          @keydown="pressOnKey($event); moveOnKey($event, index)"
          @keyup="pressOnKey"
        >
          <span class="relative inline-flex shrink-0">
            <WorkspaceTile :icon="workspace.icon" :color="workspace.color" :size="20" />
            <!-- A tile an end cuts lends its dot to that end's corner. -->
            <ConversationDot
              v-if="dotOf(workspace) && !past.start.includes(workspace.id) && !past.end.includes(workspace.id)"
              :conversation="dotOf(workspace)!"
              corner
            />
          </span>
          <span
            v-if="layout === 'named'"
            class="min-w-0 truncate text-chrome font-medium phone:text-[16px]"
            :class="workspace.id === current?.id ? 'text-fg-emphasis' : 'text-fg-muted'"
          >{{ workspaceName(workspace) }}</span>
        </div>
        <template #overlay>
          <span class="flex items-center gap-2">
            {{ workspaceName(workspace) }}
            <span v-if="shortcuts?.[index]" class="text-fg-subtle">{{ shortcuts[index] }}</span>
          </span>
        </template>
      </Tooltip>
    </div>
  </div>
  <Dropdown
    v-else
    :overlay-store="appOverlayStore"
    side="bottom"
    :offset="4"
    width="fill"
  >
    <template #trigger="{ isOpen }">
      <div
        role="button"
        tabindex="0"
        :aria-label="t('switcher')"
        class="flex h-9 w-full cursor-default select-none items-center gap-2 rounded-lg border border-line-subtle px-2 transition-colors duration-200 ease-out phone:h-11"
        :class="isOpen ? 'bg-active' : 'bg-hover hover:bg-active'"
        @keydown="pressOnKey"
        @keyup="pressOnKey"
      >
        <WorkspaceTile v-if="current" :icon="current.icon" :color="current.color" :size="20" />
        <span class="min-w-0 flex-1 truncate text-chrome font-medium text-fg-emphasis phone:text-[16px]">
          {{ current ? workspaceName(current) : '' }}
        </span>
        <ChevronsUpDown :size="ICON_PX.in28" class="shrink-0 text-fg-muted" />
      </div>
    </template>
    <template #content>
      <Menu>
        <MenuEntries :groups="menuGroups()" />
      </Menu>
    </template>
  </Dropdown>
</template>

<style scoped>
/* The shown workspace's segment wears a segmented control's thumb (`Segmented.vue`). */
.workspace-chosen {
  --fill-color: color-mix(in srgb, var(--surface-current), var(--btn-mix));
  background-color: var(--fill-color);
}
html[data-theme="dark"] .workspace-chosen {
  --fill-color: color-mix(in srgb, var(--surface-current), var(--active-mix));
}

/* The tiles scroll sideways without a bar, as a browser's tabs do; the shown one stays in view. */
.workspace-track {
  scrollbar-width: none;
}
.workspace-track::-webkit-scrollbar {
  display: none;
}
.workspace-track.fades-start {
  mask-image: linear-gradient(to right, transparent, black var(--fade));
}
.workspace-track.fades-end {
  mask-image: linear-gradient(to left, transparent, black var(--fade));
}
.workspace-track.fades-start.fades-end {
  mask-image: linear-gradient(to right, transparent, black var(--fade), black calc(100% - var(--fade)), transparent);
}
</style>
