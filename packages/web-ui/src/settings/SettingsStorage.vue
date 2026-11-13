<script setup lang="ts">
import { computed, ref } from 'vue'
import { formatBytes } from '../files/format'
import DeviceIcon from '../devices/DeviceIcon.vue'
import type { OverlayStore } from '../overlay/overlayStore'
import { deviceIcon } from '../targets/icons'
import AsyncRegion from '../ui/AsyncRegion.vue'
import Button from '../ui/Button.vue'
import ConfirmDialog from '../ui/ConfirmDialog.vue'
import Meter from '../ui/Meter.vue'
import RelativeTime from '../ui/RelativeTime.vue'
import TruncatedText from '../ui/TruncatedText.vue'
import type { Action, Operation } from '../ui/operations'
import { formatPercent } from '../ui/percent'
import type { HeadlineText, TitleText } from '../ui/ui-text'
import SettingsGroup from './SettingsGroup.vue'
import SettingsPage from './SettingsPage.vue'
import SettingsRow from './SettingsRow.vue'
import StorageBar from './StorageBar.vue'
import StorageChoice from './StorageChoice.vue'
import { t } from './locales/text'
import {
  GB,
  LOG_DAYS,
  LOG_GB,
  MAX_GB,
  UNUSED_DAYS,
  overLimit,
  reportTotals,
  type ConversationFiles,
  type DeviceStorageReport,
  type LogRetention,
  type StorageAction,
  type StorageDevice,
  type StorageRule,
  type StorageRules,
  type StorageView,
} from './storage'

/**
 * Settings › Storage: what Jush keeps on the server, the rules every device
 * cleans up by, and each device's use; a device's page cleans it up by hand.
 *
 * A device's request walks its files again, which takes a while, so each of its
 * buttons is an operation on the device (`storage:<id>`), and one excludes the
 * others while it runs.
 */
const props = defineProps<{
  storage: StorageView | null
  load: 'loading' | 'ready' | 'failed'
  error?: string | null
  devices: readonly StorageDevice[]
  /** The device whose page is open; null for the list. */
  shown: string | null
  overlayStore: OverlayStore
  onSetRules?: Action<[rules: StorageRules]>
  onSetLog?: Action<[retention: LogRetention]>
  /** Has a device remove what the action names and measure again. */
  onAct?: Action<[device: string, action: StorageAction]>
}>()
const emit = defineEmits<{
  show: [device: string | null]
  retry: []
}>()

const SERIES = (n: number) => `var(--color-series-${n})`

const server = computed(() => props.storage?.server ?? null)
/**
 * The server's kinds the backend measured for the reader, each a part of the
 * bar and a row that says what removes it: a server without Clouds, and a
 * reader who is not an administrator, have fewer.
 */
const serverParts = computed(() => {
  const measured = server.value
  if (!measured) return []
  const kinds = [
    { kind: 'conversations', bytes: measured.conversationsBytes, color: SERIES(1) },
    { kind: 'attachments', bytes: measured.attachmentsBytes, color: SERIES(2) },
    { kind: 'clouds', bytes: measured.cloudsBytes, color: SERIES(3) },
    { kind: 'log', bytes: measured.logBytes, color: SERIES(4) },
    { kind: 'search', bytes: measured.searchBytes, color: SERIES(5) },
  ] as const
  return kinds.flatMap(({ kind, bytes, color }) =>
    bytes === null ? [] : [{ kind, bytes, color, label: t(`storage.${kind}`), description: t(`storage.${kind}Description`) }],
  )
})

function used(total: number | null, available: number | null): string | null {
  if (total === null || available === null) return null
  return t('storage.used', { used: formatBytes(total - available), total: formatBytes(total) })
}

const dayOptions = UNUSED_DAYS.map((days) => ({
  value: days,
  label: days === null ? t('storage.noTimeLimit') : t('storage.unusedFor', { count: days }, days),
}))
const sizeOptions = MAX_GB.map((gb) => ({
  value: gb === null ? null : gb * GB,
  label: gb === null ? t('storage.noSizeLimit') : t('storage.atMostPerDevice', { size: formatBytes(gb * GB) }),
}))
const logDayOptions = LOG_DAYS.map((days) => ({ value: days, label: t('storage.keepDays', { count: days }, days) }))
const logSizeOptions = LOG_GB.map((gb) => ({ value: gb * GB, label: t('storage.atMost', { size: formatBytes(gb * GB) }) }))

function setRule(kind: keyof StorageRules, change: Partial<StorageRule>) {
  const rules = props.storage?.rules
  if (!rules) return
  return props.onSetRules?.({ ...rules, [kind]: { ...rules[kind], ...change } })
}
function setLog(change: Partial<LogRetention>) {
  const log = server.value?.log
  if (!log) return
  return props.onSetLog?.({ ...log, ...change })
}

const reportOf = (device: string): DeviceStorageReport | null =>
  props.storage?.devices.find((entry) => entry.deviceId === device)?.report ?? null

/** Each device's row: its system, its disk's use and what Jush keeps on it. */
const deviceRows = computed(() =>
  props.devices.map((device) => {
    const report = reportOf(device.id)
    const disk = report && report.diskBytes !== null && report.diskAvailableBytes !== null
      ? { used: report.diskBytes - report.diskAvailableBytes, total: report.diskBytes }
      : null
    const diskUsed = disk ? t('storage.diskUsed', { percent: formatPercent(disk.used, disk.total) }) : null
    const description = [device.system, report ? diskUsed : t('storage.notMeasured')].filter(Boolean).join(' · ')
    return { device, disk, diskUsed, description, jush: report ? reportTotals(report).jush : null }
  }),
)

// The device's page.
const device = computed(() => props.devices.find((entry) => entry.id === props.shown) ?? null)
const report = computed(() => (device.value ? reportOf(device.value.id) : null))
const totals = computed(() => (report.value ? reportTotals(report.value) : null))
const offline = computed(() => device.value?.state === 'offline')
const kinds = computed(() => {
  const measured = totals.value
  const rules = props.storage?.rules
  if (!measured || !rules || !report.value) return []
  return [
    { kind: 'temporary', label: t('storage.temporary'), description: t('storage.temporaryDescription'), bytes: measured.temporary, color: SERIES(1), over: overLimit(measured.temporary, rules.temporary), action: 'cleanUpTemporary', button: t('storage.cleanUp') },
    { kind: 'cache', label: t('storage.cache'), description: t('storage.cacheDescription'), bytes: measured.cache, color: SERIES(2), over: overLimit(measured.cache, rules.cache), action: 'clearCache', button: t('storage.clear') },
    { kind: 'components', label: t('storage.components'), description: t('storage.componentsDescription'), bytes: report.value.componentsBytes, color: SERIES(3), over: false, action: 'deleteComponents', button: t('storage.delete') },
  ] as const
})

interface ConversationRow {
  files: ConversationFiles
  id: string | null
  title: string
  archived: boolean
  running: boolean
}
const conversationRows = computed<ConversationRow[]>(() =>
  (report.value?.conversations ?? [])
    .map((files) => {
      const conversation = props.storage?.conversations.find((entry) => entry.name === files.name)
      return {
        files,
        id: conversation?.id ?? null,
        title: conversation?.title ?? t('storage.deletedConversation'),
        archived: conversation?.archived ?? false,
        running: conversation?.running ?? false,
      }
    })
    .sort((a, b) => b.files.temporaryBytes + b.files.cacheBytes - (a.files.temporaryBytes + a.files.cacheBytes)),
)

const operation = (action: string): Operation => ({ action, target: `storage:${props.shown}`, group: 'storage' })
function act(action: StorageAction) {
  if (!props.shown) return
  return props.onAct?.(props.shown, action)
}

// The question before a removal, one dialog for every button.
interface Confirming {
  title: HeadlineText
  body: string
  button: TitleText
  action: StorageAction
  operation: Operation
}
const confirming = ref<Confirming | null>(null)
const confirmOpen = ref(false)
function ask(kind: 'temporary' | 'cache' | 'components') {
  const name = device.value?.name ?? ''
  const actions = { temporary: 'cleanUpTemporary', cache: 'clearCache', components: 'deleteComponents' } as const
  confirming.value = {
    title: t(`storage.${kind}Title`),
    body: t(`storage.${kind}Body`, { device: name }),
    button: t(`storage.${kind}Action`),
    action: { type: actions[kind] },
    operation: operation(actions[kind]),
  }
  confirmOpen.value = true
}
function askEmpty(row: ConversationRow) {
  if (!row.id) return
  confirming.value = {
    title: t('storage.emptyTitle', { title: row.title }),
    body: t('storage.emptyBody', { device: device.value?.name ?? '' }),
    button: t('storage.emptyAction'),
    action: { type: 'conversation', conversation: row.id, empty: true },
    operation: operation(`conversation:${row.files.name}`),
  }
  confirmOpen.value = true
}
async function confirm() {
  const asked = confirming.value
  if (!asked) return
  try {
    await act(asked.action)
  } finally {
    if (confirming.value === asked) confirmOpen.value = false
  }
}
</script>

<template>
  <SettingsPage
    v-if="shown && device"
    :title="device.name"
    :back="t('storage.title')"
    @back="emit('show', null)"
  >
    <template #icon>
      <component :is="deviceIcon({ kind: 'cloud' })" v-if="device.cloud" :size="24" aria-hidden="true" />
      <DeviceIcon v-else :state="device.state" :mark="device.mark" :size="24" />
    </template>
    <template v-if="device.system" #status>{{ device.system }}</template>
    <SettingsGroup>
      <SettingsRow :label="report ? (used(report.diskBytes, report.diskAvailableBytes) ?? t('storage.jushSize', { size: formatBytes(totals!.jush) })) : t('storage.notMeasured')">
        <template #description>
          <template v-if="report">{{ t('storage.measured') }} <RelativeTime :timestamp="report.measuredAt" /></template>
          <template v-if="report?.lastCleanup"> · {{ t('storage.lastCleanup', { size: formatBytes(report.lastCleanup.removedBytes) }) }} <RelativeTime :timestamp="report.lastCleanup.at" /></template>
        </template>
        <Button
          size="sm"
          :disabled="offline"
          :disabled-reason="t('storage.offline')"
          :operation="operation('measure')"
          @click="act({ type: 'measure' })"
        >
          {{ t('storage.measure') }}
        </Button>
        <template v-if="report" #below>
          <StorageBar
            :parts="kinds"
            :total="report.diskBytes"
            :available="report.diskAvailableBytes"
            :other-label="t('storage.other')"
            :available-label="t('storage.available')"
          />
        </template>
      </SettingsRow>
    </SettingsGroup>
    <SettingsGroup v-if="report">
      <SettingsRow v-for="kind in kinds" :key="kind.kind" :label="kind.label">
        <template #leading><span class="mt-1.5 size-2 rounded-full" :style="{ background: kind.color }" /></template>
        <template #description>{{ kind.over ? t('storage.overLimit') : kind.description }}</template>
        <span class="w-16 text-right text-[13px] tabular-nums text-fg-muted">{{ formatBytes(kind.bytes) }}</span>
        <Button
          size="sm"
          class="w-24"
          :disabled="offline"
          :disabled-reason="t('storage.offline')"
          :operation="operation(kind.action)"
          @click="ask(kind.kind)"
        >
          {{ kind.button }}
        </Button>
      </SettingsRow>
    </SettingsGroup>
    <SettingsGroup v-if="report" :title="t('storage.byConversation')">
      <SettingsRow v-for="row in conversationRows" :key="row.files.name" :label="row.title" :muted="row.id === null">
        <!-- A title is a conversation's, as long as its first message: one line, the rest on hover. -->
        <template #label><TruncatedText class="max-w-full" :text="row.title" /></template>
        <template #description>
          <template v-if="row.archived">{{ t('storage.archived') }} · </template>
          <template v-if="row.running">{{ t('storage.running') }} · </template>
          <template v-if="row.files.lastUsedAt">{{ t('storage.lastUsed') }} <RelativeTime :timestamp="row.files.lastUsedAt" /></template>
        </template>
        <span class="w-16 text-right text-[13px] tabular-nums text-fg-muted">{{ formatBytes(row.files.temporaryBytes + row.files.cacheBytes) }}</span>
        <!-- Every row keeps the button's room, a deleted conversation's included. -->
        <div class="flex w-24 justify-end">
          <Button
            v-if="row.id && row.running"
            size="sm"
            class="w-24"
            :disabled="offline"
            :disabled-reason="t('storage.offline')"
            :operation="operation(`conversation:${row.files.name}`)"
            @click="act({ type: 'conversation', conversation: row.id, empty: false })"
          >
            {{ t('storage.cleanUpOld') }}
          </Button>
          <Button
            v-else-if="row.id"
            size="sm"
            class="w-24"
            :disabled="offline"
            :disabled-reason="t('storage.offline')"
            :operation="operation(`conversation:${row.files.name}`)"
            @click="askEmpty(row)"
          >
            {{ t('storage.empty') }}
          </Button>
        </div>
      </SettingsRow>
      <div v-if="!conversationRows.length" class="select-none px-4 py-6 text-center text-[13px] text-fg-subtle">
        {{ t('storage.noFiles') }}
      </div>
    </SettingsGroup>
    <ConfirmDialog
      :is-open="confirmOpen"
      :overlay-store="overlayStore"
      :title="confirming?.title ?? ''"
      :action="confirming?.button ?? ''"
      :operation="confirming?.operation"
      @close="confirmOpen = false"
      @confirm="confirm"
    >
      <p>{{ confirming?.body }}</p>
    </ConfirmDialog>
  </SettingsPage>
  <SettingsPage v-else :title="t('storage.title')" :description="t('storage.description')">
    <AsyncRegion :state="load" :label="t('storage.loading')" :detail="error ?? null" @retry="emit('retry')">
      <div v-if="storage && server" class="flex flex-col gap-6">
        <SettingsGroup :title="t('storage.server')">
          <SettingsRow :label="used(server.diskBytes, server.diskAvailableBytes) ?? t('storage.jushSize', { size: formatBytes(serverParts.reduce((sum, part) => sum + part.bytes, 0)) })">
            <template #description>{{ t('storage.measured') }} <RelativeTime :timestamp="server.measuredAt" /></template>
            <template #below>
              <StorageBar
                :parts="serverParts"
                :total="server.diskBytes"
                :available="server.diskAvailableBytes"
                :other-label="t('storage.other')"
                :available-label="t('storage.available')"
              />
            </template>
          </SettingsRow>
          <SettingsRow v-for="row in serverParts" :key="row.kind" :label="row.label" :description="row.description">
            <template #leading><span class="mt-1.5 size-2 rounded-full" :style="{ background: row.color }" /></template>
            <template v-if="row.kind === 'log' && server.log">
              <StorageChoice
                :value="server.log.keepDays"
                :options="logDayOptions"
                :label="t('storage.logKeepLabel')"
                :overlay-store="overlayStore"
                @choose="setLog({ keepDays: $event })"
              />
              <StorageChoice
                :value="server.log.maxBytes"
                :options="logSizeOptions"
                :label="t('storage.logMaxLabel')"
                :overlay-store="overlayStore"
                @choose="setLog({ maxBytes: $event })"
              />
            </template>
            <span class="w-16 text-right text-[13px] tabular-nums text-fg-muted">{{ formatBytes(row.bytes) }}</span>
          </SettingsRow>
        </SettingsGroup>
        <SettingsGroup :title="t('storage.devices')" :description="t('storage.rulesDescription')">
          <SettingsRow
            v-for="kind in (['temporary', 'cache'] as const)"
            :key="kind"
            :label="t(`storage.${kind}`)"
            :description="t(`storage.${kind}Description`)"
            :target="`storage-rules:${kind}`"
          >
            <StorageChoice
              :value="storage.rules[kind].unusedDays"
              :options="dayOptions"
              :label="t('storage.unusedLabel')"
              :overlay-store="overlayStore"
              @choose="setRule(kind, { unusedDays: $event })"
            />
            <StorageChoice
              :value="storage.rules[kind].maxBytes"
              :options="sizeOptions"
              :label="t('storage.maxLabel')"
              :overlay-store="overlayStore"
              @choose="setRule(kind, { maxBytes: $event })"
            />
          </SettingsRow>
        </SettingsGroup>
        <SettingsGroup>
          <SettingsRow
            v-for="row in deviceRows"
            :key="row.device.id"
            :label="row.device.name"
            :description="row.description"
            navigable
            @click="emit('show', row.device.id)"
          >
            <template #leading>
              <component :is="deviceIcon({ kind: 'cloud' })" v-if="row.device.cloud" :size="16" aria-hidden="true" />
              <DeviceIcon v-else :state="row.device.state" :mark="row.device.mark" />
            </template>
            <!-- Every row keeps the bar's room, a device that never measured included. -->
            <div class="w-16">
              <Meter v-if="row.disk" :value="row.disk.used" :max="row.disk.total" :label="row.diskUsed ?? undefined" />
            </div>
            <span class="w-24 text-right text-[13px] tabular-nums text-fg-muted">
              <template v-if="row.jush !== null">{{ t('storage.jushSize', { size: formatBytes(row.jush) }) }}</template>
            </span>
          </SettingsRow>
        </SettingsGroup>
      </div>
    </AsyncRegion>
  </SettingsPage>
</template>
