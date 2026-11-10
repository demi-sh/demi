<script setup lang="ts">
import { onBeforeUpdate, onUpdated, useTemplateRef, type Component } from 'vue'
import CornerDot from '../ui/CornerDot.vue'
import { ICON_PX } from '../ui/icon-metrics'
import type { TaskbarItem } from './Taskbar.vue'
import { resized } from './taskbar-motion'

/**
 * One button of the taskbar: its icon, then its name, then a count. A
 * window's button shows its tab's name and, holding more than one tab,
 * their count; a system button, its count alone; More, its name.
 *
 * The state says how it draws: closed and minimized windows are fainter; a
 * shown window wears a mark under it, the one in front the accent's, as More
 * does while the menu a click opens is open. The mark grows from its middle
 * as it comes and shrinks to it as it goes, with the windows' motion.
 *
 * A system button may wear the accent's dot on its icon's corner, breathing
 * or steady as `dot` says.
 */
defineProps<{
  icon: Component
  label?: string
  count?: number
  dot?: TaskbarItem['dot']
  state?: TaskbarItem['state']
  /** Keeps its width while the taskbar narrows its windows' buttons. */
  fixed?: boolean
}>()

// A new name or count takes the button to its new width with motion.
const button = useTemplateRef<HTMLButtonElement>('button')
let before = 0
onBeforeUpdate(() => {
  before = button.value?.getBoundingClientRect().width ?? 0
})
onUpdated(() => {
  if (button.value && before > 0) {
    resized(button.value, before)
  }
})
</script>

<template>
  <button
    ref="button"
    type="button"
    class="taskbar-button"
    :class="[state ? `taskbar-${state}` : '', { 'taskbar-fixed': fixed }]"
  >
    <span class="relative flex shrink-0">
      <component :is="icon" :size="ICON_PX.chip" />
      <CornerDot v-if="dot" tone="accent" size="xs" :breathing="dot === 'breathing'" />
    </span>
    <span v-if="label" class="min-w-0 truncate">{{ label }}</span>
    <span v-if="count !== undefined" class="taskbar-count">{{ count }}</span>
  </button>
</template>

<style scoped>
.taskbar-button {
  position: relative;
  display: inline-flex;
  height: 32px;
  flex: 0 1 auto;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  border-radius: 8px;
  padding-inline: 9px;
  white-space: nowrap;
  font-size: var(--text-chrome);
  color: var(--color-fg);
  cursor: default;
}

/* The fill the count steps from and a corner dot's ring cuts it out of. */
.taskbar-button:hover,
.taskbar-front {
  --surface-current: var(--color-surface-base);
  --fill-color: var(--color-surface-base);
}

.taskbar-button:hover {
  background: var(--color-surface-base);
}

.taskbar-fixed {
  flex: none;
}

.taskbar-closed {
  color: var(--color-fg-subtle);
}

.taskbar-minimized {
  color: var(--color-fg-muted);
}

.taskbar-front {
  background: var(--color-surface-base);
  color: var(--color-fg-emphasis);
}

/* Shown windows wear a mark under their button, the one in front the
   accent's and wider. Every button holds the mark, at no width while it has
   none, so it grows from its middle and shrinks back there; its color is the
   new state's at once, and the old one's while it shrinks away. */
.taskbar-button::after {
  content: "";
  position: absolute;
  left: 50%;
  bottom: 2px;
  height: 3px;
  width: 16px;
  margin-left: -8px;
  border-radius: 2px;
  background: var(--color-fg-faint);
  transform: scaleX(0);
  /* The windows' motion (`taskbar-motion.ts`); a mark that goes keeps its
     color until it has shrunk. */
  transition:
    transform 160ms cubic-bezier(0.2, 0, 0, 1),
    background-color 0s 160ms;
}

.taskbar-shown::after,
.taskbar-front::after {
  transition: transform 160ms cubic-bezier(0.2, 0, 0, 1);
}

.taskbar-shown::after {
  transform: scaleX(0.625);
}

.taskbar-front::after {
  transform: scaleX(1);
  background: var(--color-accent-fill);
}

@media (prefers-reduced-motion: reduce) {
  .taskbar-button::after {
    transition: none;
  }
}

.taskbar-count {
  display: inline-grid;
  min-width: 16px;
  height: 16px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  padding-inline: 4px;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  /* A track's fill on the button's own, so it reads on the hovered button
     and in light, where the raised surface is the taskbar's own white. */
  background: color-mix(in srgb, var(--surface-current), var(--overlay) 10%);
  color: var(--color-fg-muted);
}
</style>
