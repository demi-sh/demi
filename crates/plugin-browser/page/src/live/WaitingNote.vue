<script setup lang="ts">
import { t } from '../locales/text'

/** What a tab says, quietly over what it shows, while its view waits for the device (`stalled`). */
</script>

<template>
  <div class="pointer-events-none absolute inset-x-0 top-0 flex justify-center p-2">
    <span class="rounded-md bg-surface px-2 py-1 text-[12px] text-fg-subtle shadow">
      {{ t('region.waiting') }}
    </span>
  </div>
</template>
