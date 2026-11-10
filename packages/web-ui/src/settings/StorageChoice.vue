<script setup lang="ts" generic="T">
import type { OverlayStore } from '../overlay/overlayStore'
import Dropdown from '../ui/Dropdown.vue'
import Menu from '../ui/Menu.vue'
import MenuItem from '../ui/MenuItem.vue'
import type { Action } from '../ui/operations'

/** One rule of Settings › Storage: a dropdown of its choices, which sets it as chosen. */
const props = defineProps<{
  value: T
  options: readonly { value: T; label: string }[]
  /** What the dropdown sets, for assistive technology, such as Delete Temporary Files Unused For. */
  label: string
  overlayStore: OverlayStore
  onChoose?: Action<[value: T]>
}>()

const shown = (value: T) => props.options.find((option) => option.value === value)?.label ?? ''
</script>

<template>
  <!-- As wide in every row, so a rule's dropdowns stand in columns whatever each shows. -->
  <div class="flex min-w-32 justify-end">
    <Dropdown size="sm" :overlay-store="overlayStore" variant="default" width="fill" :trigger-label="label">
      <template #trigger>{{ shown(value) }}</template>
      <template #content="{ close }">
        <Menu>
          <MenuItem
            v-for="option in options"
            :key="String(option.value)"
            :label="option.label"
            choice
            :is-selected="option.value === value"
            @select="close(); option.value === value || onChoose?.(option.value)"
          />
        </Menu>
      </template>
    </Dropdown>
  </div>
</template>
