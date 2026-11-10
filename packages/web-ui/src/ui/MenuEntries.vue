<script setup lang="ts">
import { inject } from 'vue'
import { Check } from '@lucide/vue'
import { ICON_PX } from './icon-metrics'
import MenuDivider from './MenuDivider.vue'
import MenuItem from './MenuItem.vue'
import { menuRootKey } from './menu-context'
import type { ContextMenuEntry, ContextMenuGroup } from './context-menu'

/**
 * Rows given as data (`ContextMenuEntry`), a divider between each group and
 * the next: a context menu's, and the same rows wherever else they are
 * offered, such as a strip's More menu or a phone's title sheet.
 */
defineProps<{
  groups: readonly ContextMenuGroup[]
}>()

const menuRoot = inject(menuRootKey, null)

/**
 * A row with a check keeps its menu open as any row with a suffix does, so
 * choosing it closes the menu itself, first, as a macOS menu closes before
 * the choice acts.
 */
function choose(entry: ContextMenuEntry): unknown {
  menuRoot?.dismiss()
  return entry.run?.()
}
</script>

<template>
  <template v-for="(group, index) in groups" :key="index">
    <MenuDivider v-if="index > 0" />
    <MenuItem
      v-for="entry in group"
      :key="entry.id ?? entry.label"
      :icon="entry.icon"
      :label="entry.label"
      :shortcut="entry.shortcut"
      :disabled="entry.disabled"
      :disabled-reason="entry.disabledReason"
      :is-danger="entry.danger"
      :operation="entry.operation"
      :on-select="entry.checked !== undefined ? () => choose(entry) : entry.run"
    >
      <template v-if="entry.checked !== undefined" #suffix>
        <span class="flex items-center gap-2">
          <span v-if="entry.shortcut" class="text-[11px] text-fg-subtle phone:hidden">{{ entry.shortcut }}</span>
          <span class="flex size-3.5 shrink-0 items-center justify-center">
            <Check v-if="entry.checked" :size="ICON_PX.in28" class="text-fg-body" />
          </span>
        </span>
      </template>
      <template v-if="entry.submenu" #submenu>
        <component :is="entry.submenu" />
      </template>
    </MenuItem>
  </template>
</template>
