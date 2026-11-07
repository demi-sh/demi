<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch } from 'vue'
import { Toolbar, downloadUrl, usePage, type ToolbarAction } from '@demicodes/plugin-sdk'
import DeviceView from './DeviceView.vue'
import { drawsWebgl } from './webgl'
import type { MobileDevicesController } from './controller'
import { POSTURE_DEGREES, deviceControls, type DeviceControl } from './controls'
import type { MobileTabData } from './kind'
import type { Posture } from '../generated/plugin'
import { t } from '../locales/text'

/**
 * A device's tab (`crates/plugin-mobile/page/src/index.ts`): its toolbar, and the
 * device live. The agent opens the device; the tab goes when the device closes.
 *
 * The toolbar:
 *
 * - Starts with the panel's controls, each the operation of the command that
 *   does the same.
 * - Ends with the device's model in grey where it fits whole, when it says
 *   more than the tab's name, which the tab already shows with its system's
 *   logo; never on a phone.
 */
const props = defineProps<{
  conversation: string
  /** The conversation's simulators, which the plugin's store keeps. */
  session: MobileDevicesController
  tabId: string
  data: MobileTabData
  shown: boolean
}>()

const { files, errors, overlays } = usePage()
/** The device's live view, on its own stream, which the conversation's simulators keep. */
const device = props.session.device(props.data)
/** Whether the device records, as the device says, whoever started it. */
const recording = computed(() => device.recording)
/**
 * Whether this panel started the recording that runs. A recording it started
 * that ends without its Stop, at the limit of its time or its size, gives it
 * the video all the same.
 */
const startedHere = ref(false)
watch(recording, (now, was) => {
  if (was && !now && startedHere.value) {
    startedHere.value = false
    void props.session.collect(props.data.device).then(save)
  }
})
/**
 * The device's model as its driver reads it: the runner's once it said, the
 * tab's until then; none when the driver knows none, or when it is the
 * device's name, such as a simulator named iPhone 17 Pro.
 */
const model = computed(() => (device.model && device.model !== props.data.name ? device.model : null))
/** The controls of the device's platform, the tab's until the runner says: Back on Android alone. */
const controls = computed(() => deviceControls(device.platform ?? props.data.platform, device.fold?.kind ?? null))
/**
 * A device on a hinge shows in 3D with its bar, where the browser draws
 * WebGL 2, which three.js needs; in its lit display's 2D frame elsewhere.
 */
// three.js loads only for a device that folds on a hinge.
const HingedDevice = defineAsyncComponent(() => import('./HingedDevice.vue'))
const hinged = ref<{ posture(to: number): void } | null>(null)
const hinge = computed(() => device.fold?.kind === 'hinge' && drawsWebgl() ? device.fold : null)

/** Where the page reads the device's 3D body: the file on the runner's device, through its files. */
const bodyUrl = computed(() => {
  const body = hinge.value?.body
  return body ? files(props.conversation).tree?.source.contents?.url(body) ?? null : null
})

/** The file a control left on the runner's device goes to the user's computer, as a download does. */
function save(path: string | null): void {
  if (!path) {
    return
  }
  const contents = files(props.conversation).tree?.source.contents
  if (!contents) {
    errors.report(t('failures.save'), new Error(t('failures.deviceFilesUnreachable')))
    return
  }
  downloadUrl(contents.url(path, { download: true }))
}

/**
 * A toolbar control's call. Screenshot and Record leave a file, which the press
 * waits for and saves on this computer. The rest only send input, which waits
 * for nothing, the picture showing the effect
 * (`crates/plugin-mobile/page/src/index.ts`).
 */
function act(control: DeviceControl): Promise<void> | undefined {
  const state = { recording: recording.value, fold: device.fold }
  const call = control.call(props.data.device, state)
  if (call.method === 'fold') {
    // A posture the device is in moves it there again, as a fold outside
    // Demi may have left it elsewhere; a hinged one's view moves the hinge
    // as it shows it.
    if (hinged.value && hinge.value) {
      hinged.value.posture(POSTURE_DEGREES[call.params.to as Posture])
      return undefined
    }
  }
  if (call.method === 'record') {
    // Stop takes the video itself; only a start leaves one to come.
    startedHere.value = call.params.action === 'start'
  }
  if (call.method === 'screenshot' || call.method === 'record') {
    return props.session.run(call).then(save)
  }
  void props.session.run(call)
  return undefined
}

/** The toolbar's actions, in the controls' order; those a narrow panel has no room for go into More. */
const actions = computed<ToolbarAction[]>(() => controls.value.map((control) => {
  const state = { recording: recording.value, fold: device.fold }
  const pressed = control.pressed?.(state) ?? false
  return {
    label: t(control.label(state)),
    icon: control.icon(state),
    group: control.group,
    pressed,
    indicator: pressed && control.alerts ? 'danger' : undefined,
    run: () => act(control),
  }
}))
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <Toolbar :actions="actions" :overlay-store="overlays" :more-label="t('toolbar.more')">
      <template #trailing>
        <!-- The model shows whole or not at all: beside the empty first item it
             fits on the one line, or wraps to the next, which the row cuts off. -->
        <div v-if="model" class="flex h-10 min-w-0 flex-1 flex-wrap items-center justify-end overflow-hidden phone:invisible">
          <span class="h-10 w-0" />
          <span class="flex h-10 items-center whitespace-nowrap px-2 text-[13px] text-fg-subtle">{{ model }}</span>
        </div>
      </template>
    </Toolbar>
    <div
      v-if="session.pictures.value === 'unsupported'"
      class="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-[13px] text-fg-faint"
    >
      {{ t('region.unsupported') }}
    </div>
    <HingedDevice
      v-else-if="hinge"
      ref="hinged"
      :session="device"
      :fold="hinge"
      :shown="shown"
      :body-url="bodyUrl"
      :downloads="session.api.downloading()"
    />
    <DeviceView v-else :session="device" :shown="shown" :downloads="session.api.downloading()" />
  </div>
</template>
