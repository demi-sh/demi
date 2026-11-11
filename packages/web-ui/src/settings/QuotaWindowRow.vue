<script setup lang="ts">
import { t } from './locales/text'
import Meter from '../ui/Meter.vue'
import ResetTime from '../ui/ResetTime.vue'
import { formatPercent } from '../ui/percent'
import { useClock } from '../composables/useClock'
import { afterReset, hasReset } from './quota-window'
import type { SettingsQuotaWindow } from './types'
import { windowName } from './window-name'
import { computed } from 'vue'

/**
 * One usage window as every surface shows it (`packages/web-ui/src/ui/dates.ts`):
 * one line over its bar, the window's name at the start, its reset and its use at
 * the end, "Resets in 4h 42m · 4% used". Settings and the context card both show
 * a window only through this row.
 *
 * - The reset is worded by `formatReset` and left out when unknown.
 * - Once the window's reset passes on the clock, it reads "0% used", with no
 *   reset (`afterReset`). The row updates when the reset passes, not on every
 *   tick.
 * - Where the line is short, the reset wraps at its spaces and the use, "4%
 *   used", drops under it whole, right-aligned. None of it is cut.
 */
const props = defineProps<{ window: SettingsQuotaWindow }>()
const name = computed(() => windowName(props.window.name))
const now = useClock()
const reset = computed(() => hasReset(props.window, now.value))
const shown = computed(() => (reset.value ? afterReset(props.window) : props.window))
</script>

<template>
  <div class="flex flex-col gap-1">
    <div class="flex items-baseline gap-3">
      <span class="shrink-0 text-fg-body">{{ name }}</span>
      <span class="ml-auto min-w-0 text-right">
        <ResetTime v-if="shown.resetsAt" :timestamp="shown.resetsAt" />
        <template v-if="shown.resetsAt">&nbsp;· </template><span class="whitespace-nowrap">{{ t('quota.used', { share: formatPercent(shown.used, shown.max) }) }}</span>
      </span>
    </div>
    <Meter :value="shown.used" :max="shown.max" :label="name" />
  </div>
</template>
