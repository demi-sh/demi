<script setup lang="ts">
import { WINDOW_SCOPE } from '@demicodes/web-ui/ui/window-stack'

withDefaults(defineProps<{
  variant?: string
  wide?: boolean
  tight?: boolean
}>(), {
  variant: '',
  wide: false,
  tight: false,
})
</script>

<template>
  <div
    class="gallery-specimen"
    :class="wide ? 'gallery-specimen-wide' : ''"
    v-bind="{ [WINDOW_SCOPE]: '' }"
  >
    <div
      v-if="variant"
      class="gallery-specimen-meta select-none"
      :class="tight ? 'gallery-specimen-meta-tight' : ''"
    >
      <div class="gallery-specimen-variant">{{ variant }}</div>
    </div>
    <div v-if="$slots.default" class="gallery-specimen-stage">
      <slot />
    </div>
  </div>
</template>
