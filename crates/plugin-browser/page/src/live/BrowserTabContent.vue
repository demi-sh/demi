<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from 'vue'
import { ArrowDownToLine, Download, File, FolderSearch, Monitor, Ruler, Smartphone } from '@lucide/vue'
import { AddressBar } from '@jush-ai/plugin-sdk'
import { IconButton } from '@jush-ai/plugin-sdk'
import { Menu } from '@jush-ai/plugin-sdk'
import { MenuItem } from '@jush-ai/plugin-sdk'
import { Dropdown } from '@jush-ai/plugin-sdk'
import { ProgressLine } from '@jush-ai/plugin-sdk'
import { RegionStatus } from '@jush-ai/plugin-sdk'
import { Tooltip } from '@jush-ai/plugin-sdk'
import { PaneReading, downloadProgress, downloadUrl, formatByteRatio, formatBytes, usePage, usePaneReady, usePaneWaiting } from '@jush-ai/plugin-sdk'
import { clientPlatform } from '@jush-ai/utils'
import type { LiveDownload } from '../generated/plugin'
import LiveView from './LiveView.vue'
import WaitingNote from './WaitingNote.vue'
import { t } from '../locales/text'
import { browserShortcut } from './input'
import { startingLabel, type BrowserTabData, type BrowserTabsController, type KeptPicture } from './tabs'
import { BLANK_PAGE, deviceSnap, panelSize, viewportChoices, type PanelSize, type ViewportChoice } from './view'

/**
 * A `browser` tab's content: the device's browser tab its data names. It
 * shows at once what the tab's data says, and what the device sends
 * replaces it when it arrives:
 *
 * - While the browser does not have the tab, as while a browser that starts
 *   again opens the tabs of the one before: the panel's own background, which
 *   says what Jush is doing.
 * - Then a blank page until the browser tab's first picture.
 * - An address the user submits shows at once, with the page loading over the
 *   picture the tab still shows.
 *
 * The tab is there exactly while the device lists its browser tab: the
 * content changes no tab. A content stays mounted while another tab is
 * selected (`shown` false), so shown again it shows its picture, address and
 * title at once.
 *
 * The blank page has Chrome's empty-page color in the tab's scheme, so the
 * first picture does not flash.
 */
const props = defineProps<{
  conversation: string
  /** The conversation's browser, which the plugin's store keeps. */
  session: BrowserTabsController
  tabId: string
  data: BrowserTabData
  shown: boolean
}>()

const addressBar = ref<InstanceType<typeof AddressBar> | null>(null)

const { overlays, errors, files, intents } = usePage()
const platform = clientPlatform(navigator)
const view = computed(() => props.session.session.value)
/** The browser tab the work tab shows; the browser may not list it yet. */
const bound = computed(() => props.data.tab)
/** The bound tab as a view last reported it, kept while no view is open or the view reconnects. */
const live = computed(() => props.session.tab(bound.value))
const viewport = computed(() => live.value?.viewport ?? null)
const choices = computed(() => (viewport.value ? viewportChoices(viewport.value) : []))
/** A computer, a phone, or a size the agent set. */
const MODE_ICONS: Record<ViewportChoice['mode'], Component> = { web: Monitor, mobile: Smartphone, custom: Ruler }
// The pane waits for the browser's tab list, which comes with the conversation's opening: until
// it is in, the area under the address stays empty. A Cloud still starting, or
// a list that could not be read, says so in the area.
const pane = usePaneWaiting()
usePaneReady(() => props.session.list.value !== null || props.session.listError.value !== null || props.session.api.deviceStarting())
/** What Jush does before the browser has the tab, if it does not have it yet. */
const phase = computed(() => props.session.startingPhase(props.data))
/** The tab loads: Stop, the strip's spinner and the progress line show it. */
const busy = computed(() => props.session.busy(props.data))
/** Back and Forward are unavailable while the browser says the tab has no page that way, or has said nothing yet. */
const backReason = computed(() => (live.value?.canGoBack ? null : t('address.noBack')))
const forwardReason = computed(() => (live.value?.canGoForward ? null : t('address.noForward')))

// A shown tab is watched on the page's view, once its area measured the panel;
// before the browser has its tab, the view watches nothing and tells the device
// the panel's size for the tab it opens.
watch(
  [() => props.shown, bound],
  ([shown, tab], previous) => {
    if (previous?.[0] && (previous[1] !== tab || !shown)) {
      props.session.hide(props.tabId)
    }
    if (shown) {
      props.session.show(props.tabId, tab)
    }
  },
  { immediate: true, flush: 'post' },
)

/** The area the picture shows in, which the panel's size is: measured before any view exists. */
const area = ref<HTMLElement | null>(null)
const measured = ref<PanelSize>({ width: 1, height: 1 })
/** What moves the picture onto the screen's pixel grid, from where the area stands. */
const snap = ref({ x: 0, y: 0 })
let observer: ResizeObserver | null = null
let density: MediaQueryList | null = null

/** The area as it stands now, for the picture and, while the tab is shown, for the view. */
function measure(): void {
  const bounds = area.value?.getBoundingClientRect()
  // A hidden content measures nothing; its tab keeps the size it had.
  if (!bounds || bounds.width < 1 || bounds.height < 1) {
    return
  }
  measured.value = panelSize(bounds.width, bounds.height)
  snap.value = { x: deviceSnap(bounds.left, devicePixelRatio), y: deviceSnap(bounds.top, devicePixelRatio) }
  if (props.shown) {
    props.session.resize({
      panel: measured.value,
      devicePixelRatio,
      screen: panelSize(screen.width, screen.height),
    })
  }
}

/** The screen's density can change when the window moves to another display. */
function watchDensity(): void {
  density?.removeEventListener('change', watchDensity)
  density = matchMedia(`(resolution: ${devicePixelRatio}dppx)`)
  density.addEventListener('change', watchDensity)
  measure()
}

onMounted(() => {
  observer = new ResizeObserver(() => measure())
  if (area.value) {
    observer.observe(area.value)
  }
  watchDensity()
})

// Shown again, the tab's panel may have changed meanwhile.
watch(() => props.shown, (shown) => {
  if (shown) {
    measure()
  }
}, { flush: 'post' })

onBeforeUnmount(() => {
  observer?.disconnect()
  density?.removeEventListener('change', watchDensity)
  props.session.hide(props.tabId)
})

/**
 * The address shows at once, and the tab loads it; a refused one leaves the
 * tab on the page the browser still shows, with its address and title.
 */
function submit(url: string): void {
  props.session.navigate(bound.value, url).catch((error: unknown) => errors.report(t('failures.openAddress'), error))
}

/** The live view's last picture of the bound tab, for the next view of it, as after a conversation switch. */
function keepPicture(picture: KeptPicture): void {
  const tab = live.value
  if (tab) {
    props.session.keepPicture(tab.id, picture)
  }
}

/** What a toast names when Back, Forward or Reload was refused. */
const couldNot = (action: 'back' | 'forward' | 'reload') =>
  t(action === 'back' ? 'failures.goBack' : action === 'forward' ? 'failures.goForward' : 'failures.reloadPage')

/** Back, Forward and Reload, on the bound tab; a refusal is reported as any failed request is. */
function history(action: 'back' | 'forward' | 'reload'): void {
  props.session.history(bound.value, action).catch((error: unknown) => errors.report(couldNot(action), error))
}

/**
 * The browser's own shortcuts, with the focus anywhere in the tab's content:
 * they act here, never in the page, and never reach the page around the panel,
 * whose ⌘R would reload Jush itself.
 */
function shortcut(event: KeyboardEvent): void {
  const action = browserShortcut(event, platform)
  if (action === null) {
    return
  }
  event.preventDefault()
  event.stopPropagation()
  if (event.type !== 'keydown') {
    return
  }
  if (action === 'address') {
    addressBar.value?.focus()
  } else if (action === 'reload' || (action === 'back' ? backReason.value : forwardReason.value) === null) {
    history(action)
  }
}

/** The downloads the user started in this tab, the newest first, as a browser's bubble lists them. */
const downloads = computed(() => [...props.session.downloadsOf(bound.value)].reverse())
const bubble = ref(false)
// A download that starts opens the bubble, as a browser shows a download it starts.
watch(
  () => downloads.value[0]?.id,
  (id, before) => {
    if (id !== undefined && id !== before && props.shown) {
      bubble.value = true
    }
  },
)

/** What the bubble says of a download: its size, how much has come, or that it stopped. */
function downloadDetail(download: LiveDownload): string {
  if (download.state === 'canceled') {
    return t('downloads.canceled')
  }
  if (download.state === 'complete') {
    return formatBytes(download.total)
  }
  return download.total > 0
    ? formatByteRatio(download.received, download.total)
    : formatBytes(download.received)
}

/** Save: the file goes from the device to the user's computer through the file route, as Download does in Files. */
function save(download: LiveDownload): void {
  const contents = files(props.conversation).tree?.source.contents
  if (!contents) {
    errors.report(t('failures.saveDownload'), new Error(t('failures.deviceFilesUnreachable')))
    return
  }
  downloadUrl(contents.url(download.path, { download: true }))
}

/** Show in Files: the file the browser saved, on the device. */
function showInFiles(download: LiveDownload): void {
  bubble.value = false
  intents.open(props.conversation, { intent: 'file', payload: { path: download.path } })
}

/** Stop, on the bound tab; a refusal is a toast. */
function stop(): void {
  props.session.stop(bound.value).catch((error: unknown) => errors.report(t('failures.stopLoading'), error))
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col" @keydown.capture="shortcut" @keyup.capture="shortcut">
    <AddressBar
      ref="addressBar"
      :address="session.address(data)"
      :back-reason="backReason"
      :forward-reason="forwardReason"
      :loading="busy"
      @submit="submit"
      @back="history('back')"
      @forward="history('forward')"
      @reload="history('reload')"
      @stop="stop"
    >
      <template v-if="viewport || downloads.length > 0" #trailing>
        <!-- The downloads, once the user started one here, as a browser's toolbar shows them. -->
        <Dropdown v-if="downloads.length > 0" v-model:open="bubble" :overlay-store="overlays" class="shrink-0">
          <template #trigger="{ isOpen }">
            <Tooltip :content="t('downloads.title')" :disabled="isOpen">
              <IconButton
                :icon="ArrowDownToLine"
                variant="ghost"
                :aria-label="t('downloads.title')"
                aria-haspopup="menu"
                :pressed="isOpen"
              />
            </Tooltip>
          </template>
          <template #content>
            <Menu :aria-label="t('downloads.title')">
              <MenuItem
                v-for="download in downloads"
                :key="download.id"
                :icon="File"
                :label="download.name"
                :value="downloadDetail(download)"
                @select="download.state === 'complete' && showInFiles(download)"
              >
                <template #actions>
                  <Tooltip :content="t('downloads.save')" class="inline-flex">
                    <IconButton
                      :icon="Download"
                      variant="ghost"
                      size="xs"
                      :aria-label="t('downloads.save')"
                      :disabled="download.state !== 'complete'"
                      @click.stop="save(download)"
                    />
                  </Tooltip>
                  <Tooltip :content="t('downloads.showInFiles')" class="inline-flex">
                    <IconButton
                      :icon="FolderSearch"
                      variant="ghost"
                      size="xs"
                      :aria-label="t('downloads.showInFiles')"
                      :disabled="download.state !== 'complete'"
                      @click.stop="showInFiles(download)"
                    />
                  </Tooltip>
                </template>
              </MenuItem>
            </Menu>
          </template>
        </Dropdown>
        <!-- The mode alone, as its icon, on a button like the bar's others: the size is the panel's and
             says nothing the picture does not. -->
        <Dropdown v-if="viewport" :overlay-store="overlays" class="shrink-0">
          <template #trigger="{ isOpen }">
            <Tooltip :content="t('viewport.title')" :disabled="isOpen">
              <IconButton
                :icon="MODE_ICONS[viewport.mode]"
                variant="ghost"
                :aria-label="t('viewport.title')"
                aria-haspopup="menu"
                :pressed="isOpen"
              />
            </Tooltip>
          </template>
          <template #content="{ close }">
            <Menu>
              <MenuItem
                v-for="choice in choices"
                :key="choice.mode"
                :label="choice.label"
                :icon="MODE_ICONS[choice.mode]"
                choice
                :is-selected="choice.mode === viewport.mode"
                :disabled="!choice.selectable && choice.mode !== viewport.mode"
                @select="() => {
                  close()
                  if (live && view && choice.selectable) {
                    view.mode(live.id, choice.mode as 'web' | 'mobile')
                  }
                }"
              />
            </Menu>
          </template>
        </Dropdown>
      </template>
    </AddressBar>
    <div
      ref="area"
      class="relative flex min-h-0 flex-1 flex-col border-t border-line"
      :class="pane.waiting.value ? 'invisible' : ''"
    >
      <PaneReading />
      <ProgressLine :active="busy" />
      <div
        v-if="props.session.pictures.value === 'unsupported'"
        class="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-[13px] text-fg-faint"
      >
        {{ t('region.noH264') }}
      </div>
      <!-- While the browser does not have the tab: the panel's own background, which says what Jush does. -->
      <RegionStatus
        v-else-if="phase !== null"
        class="min-h-0 flex-1"
        status="loading"
        :label="startingLabel(phase)"
        :progress="downloadProgress(session.api.downloading())"
      />
      <LiveView
        v-else-if="live"
        :session="view"
        :tab="live"
        :kept="session.keptPicture(live.id)"
        :shown="shown"
        :panel="measured"
        :snap="snap"
        @history="history"
        @keep="keepPicture"
      />
      <!-- What a tab shows before its picture, and a blank tab: a blank page, which says so while the view
           waits for the device. -->
      <div
        v-else
        class="relative min-h-0 flex-1"
        :class="BLANK_PAGE"
      >
        <WaitingNote v-if="session.waiting()" />
      </div>
    </div>
  </div>
</template>
