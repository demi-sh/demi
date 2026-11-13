<script setup lang="ts">
import { t } from './locales/text'
import { appOverlayStore } from '@jush-ai/web-ui/overlay/appOverlay'
import SettingsUsers from '@jush-ai/web-ui/settings/SettingsUsers.vue'
import AsyncRegion from '@jush-ai/web-ui/ui/AsyncRegion.vue'
import { useProduct } from '../state/product'
import { useUserSettings } from './users'

const users = useUserSettings()
const product = useProduct()
</script>

<template>
  <!-- The list is read when the pane opens, and followed on the channel's `users` part. -->
  <AsyncRegion :state="product.listLoad('users')" :label="t('users.loading')" :error="t('users.loadFailed')" :on-retry="() => product.retryList('users')">
    <SettingsUsers v-if="users.host" :host="users.host" :overlay-store="appOverlayStore" />
  </AsyncRegion>
</template>
