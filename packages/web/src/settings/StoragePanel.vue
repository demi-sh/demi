<script setup lang="ts">
import { computed, onMounted, ref, shallowRef } from 'vue'
import { systemName } from '@jush-ai/web-ui/devices/report'
import { errorMessage, reportError } from '@jush-ai/web-ui/infra/errors'
import { appOverlayStore } from '@jush-ai/web-ui/overlay/appOverlay'
import SettingsStorage from '@jush-ai/web-ui/settings/SettingsStorage.vue'
import type { LogRetention, StorageAction, StorageDevice, StorageRules } from '@jush-ai/web-ui/settings/storage'
import { apiRequest, jsonBody, readResponse } from '../api/client'
import {
  deviceStorageReportSchema,
  logRetentionSchema,
  storageRulesSchema,
  storageSchema,
  type Storage,
} from '../api/generated/web-api'
import { useProduct } from '../state/product'
import { deviceLabel, deviceMarkOf, targetKindOf } from '../state/devices'
import { useSettingsAddress } from './address'
import { t } from './locales/text'

/**
 * Settings › Storage (`crates/backend-http/src/storage.rs`): read when the
 * page opens, and changed by the answer of each change it makes.
 */
const product = useProduct()
const address = useSettingsAddress()

/**
 * A device's request waits while its runner walks its files again, which on a
 * disk with hundreds of gigabytes of them takes minutes.
 */
const DEVICE_REQUEST_TIMEOUT_MS = 10 * 60_000

const storage = shallowRef<Storage | null>(null)
const load = ref<'loading' | 'ready' | 'failed'>('loading')
const failure = ref<string | null>(null)

async function read() {
  load.value = 'loading'
  try {
    storage.value = await readResponse(await apiRequest('/storage'), storageSchema)
    load.value = 'ready'
  } catch (error) {
    failure.value = errorMessage(error)
    load.value = 'failed'
  }
}
onMounted(read)

/** The user's devices, the Cloud first, as the snapshot names them. */
const devices = computed<StorageDevice[]>(() => {
  const known = product.snapshot?.devices ?? []
  return known
    .map((device): StorageDevice => {
      const kind = targetKindOf(device)
      return {
        id: device.id,
        name: deviceLabel(kind, device),
        system: systemName({ os: device.os, runnerVersion: null }),
        state: device.state,
        mark: deviceMarkOf(device, known),
        cloud: kind === 'cloud',
      }
    })
    .sort((a, b) => Number(b.cloud) - Number(a.cloud))
})

const shown = computed(() => (address.section.value === 'storage' ? address.detail.value : null))

async function setRules(rules: StorageRules) {
  try {
    const set = await readResponse(
      await apiRequest('/storage/rules', { method: 'PUT', ...jsonBody(rules) }),
      storageRulesSchema,
    )
    if (storage.value) storage.value = { ...storage.value, rules: set }
  } catch (error) {
    reportError(t('storage.saveFailed'), error, { site: 'storage.saveFailed', userVisible: true })
  }
}

async function setLog(retention: LogRetention) {
  try {
    const set = await readResponse(
      await apiRequest('/storage/log', { method: 'PUT', ...jsonBody(retention) }),
      logRetentionSchema,
    )
    if (storage.value) storage.value = { ...storage.value, server: { ...storage.value.server, log: set } }
  } catch (error) {
    reportError(t('storage.saveFailed'), error, { site: 'storage.saveFailed', userVisible: true })
  }
}

async function act(device: string, action: StorageAction) {
  try {
    const report = await readResponse(
      await apiRequest(`/devices/${encodeURIComponent(device)}/storage`, {
        method: 'POST',
        ...jsonBody(action),
        timeoutMs: DEVICE_REQUEST_TIMEOUT_MS,
      }),
      deviceStorageReportSchema,
    )
    const current = storage.value
    if (!current) return
    const others = current.devices.filter((entry) => entry.deviceId !== device)
    storage.value = { ...current, devices: [...others, { deviceId: device, report }] }
  } catch (error) {
    reportError(t('storage.cleanUpFailed'), error, { site: 'storage.cleanUpFailed', userVisible: true })
  }
}
</script>

<template>
  <SettingsStorage
    :storage="storage"
    :load="load"
    :error="failure"
    :devices="devices"
    :shown="shown"
    :overlay-store="appOverlayStore"
    @retry="read"
    @show="address.show('storage', $event)"
    @set-rules="setRules"
    @set-log="setLog"
    @act="act"
  />
</template>
