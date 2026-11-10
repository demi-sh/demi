<script setup lang="ts">
import { computed, ref } from 'vue'
import { appOverlayStore } from '@demicodes/web-ui/overlay/appOverlay'
import SettingsStorage from '@demicodes/web-ui/settings/SettingsStorage.vue'
import {
  GB,
  type ConversationFiles,
  type DeviceStorageReport,
  type LogRetention,
  type StorageAction,
  type StorageDevice,
  type StorageRules,
  type StorageView,
} from '@demicodes/web-ui/settings/storage'
import { GALLERY_MARKS } from '../fixtures/device-marks'
import { slow } from '../fixtures/slow'
import { productWould } from '../product-would'
import { DAY, HOUR, MINUTE, ago } from '../fixtures/time'

/**
 * Settings › Storage: a Mac near its temporary files' limit, the Cloud, a
 * Linux machine offline since it last measured, and a Windows PC that never
 * measured. An administrator sees the whole server; anyone else sees only
 * their own conversations and attachments on it. Every control acts on the
 * specimen as the product's runner would:
 *
 * - A rule's dropdown sets it.
 * - Measure measures again; Clean Up…, Clear… and Delete… remove what is not
 *   in use; Empty… removes a conversation's files, and a running
 *   conversation's Clean Up only what it did not use in the last day.
 */
const props = withDefaults(
  defineProps<{
    /** Who reads the page: an administrator, one on a server that runs no Clouds, or a member. */
    viewer?: 'administrator' | 'noClouds' | 'member'
  }>(),
  { viewer: 'administrator' },
)

const devices: StorageDevice[] = [
  { id: 'cloud', name: 'Cloud', system: 'Ubuntu 24.04', state: 'online', mark: null, cloud: true },
  { id: 'mac', name: 'alex-mbp', system: 'macOS 26.5 · Apple silicon', state: 'online', mark: GALLERY_MARKS.mac, cloud: false },
  { id: 'build', name: 'build-01', system: 'Ubuntu 24.04', state: 'offline', mark: GALLERY_MARKS.ubuntu, cloud: false },
  { id: 'studio', name: 'studio-pc', system: 'Windows 11', state: 'online', mark: GALLERY_MARKS.windows, cloud: false },
]

const files = (name: string, temporary: number, cache: number, used: number): ConversationFiles => ({
  name,
  temporaryBytes: temporary * GB,
  cacheBytes: cache * GB,
  lastUsedAt: ago(used),
})

const storage = ref<StorageView>({
  server: {
    measuredAt: ago(HOUR),
    diskBytes: 96 * GB,
    diskAvailableBytes: 54 * GB,
    conversationsBytes: 3.2 * GB,
    attachmentsBytes: 15 * GB,
    cloudsBytes: 13 * GB,
    logBytes: 0.4 * GB,
    searchBytes: 0.4 * GB,
    log: { keepDays: 14, maxBytes: 2 * GB },
  },
  rules: {
    temporary: { unusedDays: 30, maxBytes: 20 * GB },
    cache: { unusedDays: 30, maxBytes: 10 * GB },
  },
  devices: [
    {
      deviceId: 'mac',
      report: {
        measuredAt: ago(3 * MINUTE),
        diskBytes: 1800 * GB,
        diskAvailableBytes: 410 * GB,
        componentsBytes: 0.7 * GB,
        conversations: [
          files('a1b2c3d4e5f6', 9.4, 2.1, 2 * HOUR),
          files('b2c3d4e5f6a1', 7.8, 0.4, 20 * MINUTE),
          files('c3d4e5f6a1b2', 1.2, 3.3, 9 * DAY),
          files('d4e5f6a1b2c3', 0.3, 1.2, 26 * DAY),
          files('e5f6a1b2c3d4', 0.9, 0, 14 * DAY),
        ],
        lastCleanup: { at: ago(5 * HOUR), cause: 'rules', removedBytes: 6.2 * GB },
      },
    },
    {
      deviceId: 'cloud',
      report: {
        measuredAt: ago(40 * MINUTE),
        diskBytes: 40 * GB,
        diskAvailableBytes: 31 * GB,
        componentsBytes: 0.5 * GB,
        conversations: [files('b2c3d4e5f6a1', 1.1, 0.5, 20 * MINUTE)],
        lastCleanup: null,
      },
    },
    {
      deviceId: 'build',
      report: {
        measuredAt: ago(2 * DAY),
        diskBytes: 250 * GB,
        diskAvailableBytes: 120 * GB,
        componentsBytes: 0.3 * GB,
        conversations: [files('f6a1b2c3d4e5', 4.2, 0.6, 3 * DAY)],
        lastCleanup: null,
      },
    },
    { deviceId: 'studio', report: null },
  ],
  conversations: [
    { name: 'a1b2c3d4e5f6', id: 'a1b2c3d4-e5f6-4000-8000-000000000001', title: 'Plan the onboarding flow', archived: false, running: false },
    { name: 'b2c3d4e5f6a1', id: 'b2c3d4e5-f6a1-4000-8000-000000000002', title: 'Fix the checkout total', archived: false, running: true },
    { name: 'c3d4e5f6a1b2', id: 'c3d4e5f6-a1b2-4000-8000-000000000003', title: 'Compare chart libraries', archived: true, running: false },
    { name: 'd4e5f6a1b2c3', id: 'd4e5f6a1-b2c3-4000-8000-000000000004', title: 'Scrape the price list', archived: false, running: false },
    { name: 'f6a1b2c3d4e5', id: 'f6a1b2c3-d4e5-4000-8000-000000000006', title: 'Build the release notes', archived: false, running: false },
  ],
})
const shown = ref<string | null>(null)

/** What the backend answers the viewer. */
const seen = computed<StorageView>(() =>
  props.viewer === 'administrator'
    ? storage.value
    : props.viewer === 'noClouds'
      ? { ...storage.value, server: { ...storage.value.server, cloudsBytes: null } }
      : {
        ...storage.value,
        server: {
          ...storage.value.server,
          diskBytes: null,
          diskAvailableBytes: null,
          conversationsBytes: 0.8 * GB,
          attachmentsBytes: 2.4 * GB,
          cloudsBytes: null,
          logBytes: null,
          searchBytes: null,
          log: null,
        },
      },
)

async function setRules(rules: StorageRules) {
  await slow(300)
  storage.value = { ...storage.value, rules }
}

async function setLog(log: LogRetention) {
  await slow(300)
  storage.value = { ...storage.value, server: { ...storage.value.server, log } }
}

const removedBy = (before: DeviceStorageReport, after: DeviceStorageReport) => {
  const sum = (report: DeviceStorageReport) =>
    report.componentsBytes + report.conversations.reduce((total, entry) => total + entry.temporaryBytes + entry.cacheBytes, 0)
  return sum(before) - sum(after)
}

/** What the runner would remove, then its new report. */
async function act(device: string, action: StorageAction) {
  await slow(action.type === 'measure' ? 900 : 1500)
  const running = (name: string) => storage.value.conversations.find((entry) => entry.name === name)?.running ?? false
  const usedToday = (entry: ConversationFiles) => entry.lastUsedAt !== null && Date.now() - Date.parse(entry.lastUsedAt) < DAY
  storage.value = {
    ...storage.value,
    devices: storage.value.devices.map((entry) => {
      if (entry.deviceId !== device) return entry
      const before: DeviceStorageReport = entry.report ?? {
        measuredAt: ago(0),
        diskBytes: 1000 * GB,
        diskAvailableBytes: 620 * GB,
        componentsBytes: 0.6 * GB,
        conversations: [],
        lastCleanup: null,
      }
      let conversations = before.conversations
      let componentsBytes = before.componentsBytes
      switch (action.type) {
        case 'cleanUpTemporary':
          conversations = conversations.map((files) => (usedToday(files) ? files : { ...files, temporaryBytes: 0 }))
          break
        case 'clearCache':
          conversations = conversations.map((files) => (running(files.name) ? files : { ...files, cacheBytes: 0 }))
          break
        case 'deleteComponents':
          componentsBytes = 0.2 * GB
          break
        case 'conversation': {
          const name = storage.value.conversations.find((entry) => entry.id === action.conversation)?.name
          conversations = conversations.map((files) =>
            files.name !== name ? files : action.empty ? { ...files, temporaryBytes: 0, cacheBytes: 0 } : { ...files, temporaryBytes: files.temporaryBytes / 2 },
          )
          break
        }
        case 'measure':
          break
      }
      conversations = conversations.filter((files) => files.temporaryBytes + files.cacheBytes > 0)
      const after: DeviceStorageReport = { ...before, measuredAt: ago(0), conversations, componentsBytes }
      const removed = removedBy(before, after)
      return {
        deviceId: device,
        report: {
          ...after,
          diskAvailableBytes: after.diskAvailableBytes === null ? null : after.diskAvailableBytes + removed,
          lastCleanup: action.type === 'measure' ? before.lastCleanup : { at: ago(0), cause: 'user', removedBytes: removed },
        },
      }
    }),
  }
}
</script>

<template>
  <SettingsStorage
    :storage="seen"
    load="ready"
    :devices="devices"
    :shown="shown"
    :overlay-store="appOverlayStore"
    @retry="productWould('Load Storage Again')"
    @show="shown = $event"
    @set-rules="setRules"
    @set-log="setLog"
    @act="act"
  />
</template>
