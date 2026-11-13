<script setup lang="ts">
import { computed } from 'vue'
import { formatBytes } from '../files/format'

/**
 * A disk's use as macOS's Storage draws it: one bar, a color for each kind of
 * what Jush keeps, then what other programs take in grey, then the free room,
 * with a legend under it. A kind's dot in the legend is the color of its part
 * of the bar and of its row's dot below.
 */
const props = defineProps<{
  /** Jush's kinds, each with the series color it is drawn in. */
  parts: readonly { label: string; bytes: number; color: string }[]
  /** The volume's size; null draws the parts against their sum. */
  total: number | null
  available: number | null
  otherLabel: string
  availableLabel: string
}>()

const sum = computed(() => props.parts.reduce((total, part) => total + part.bytes, 0))
const total = computed(() => Math.max(props.total ?? sum.value, sum.value, 1))
const other = computed(() =>
  props.total !== null && props.available !== null ? Math.max(0, props.total - props.available - sum.value) : 0,
)
const share = (bytes: number) => `${(bytes / total.value) * 100}%`
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex h-2.5 w-full overflow-hidden rounded-full bg-overlay/8" aria-hidden="true">
      <div v-for="part in parts" :key="part.label" class="h-full" :style="{ width: share(part.bytes), background: part.color }" />
      <div v-if="other" class="h-full bg-fg-subtle/50" :style="{ width: share(other) }" />
    </div>
    <ul class="flex flex-wrap gap-x-4 gap-y-1 text-[12px] leading-4 text-fg-muted">
      <li v-for="part in parts" :key="part.label" class="flex items-center gap-1.5">
        <span class="size-2 shrink-0 rounded-full" :style="{ background: part.color }" />{{ part.label }} {{ formatBytes(part.bytes) }}
      </li>
      <li v-if="other" class="flex items-center gap-1.5">
        <span class="size-2 shrink-0 rounded-full bg-fg-subtle/50" />{{ otherLabel }} {{ formatBytes(other) }}
      </li>
      <li v-if="available !== null" class="flex items-center gap-1.5">
        <span class="size-2 shrink-0 rounded-full bg-overlay/8" />{{ availableLabel }} {{ formatBytes(available) }}
      </li>
    </ul>
  </div>
</template>
