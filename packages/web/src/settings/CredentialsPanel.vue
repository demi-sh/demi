<script setup lang="ts">
import { appOverlayStore } from '@jush-ai/web-ui/overlay/appOverlay'
import SettingsCredentials from '@jush-ai/web-ui/settings/SettingsCredentials.vue'
import AsyncRegion from '@jush-ai/web-ui/ui/AsyncRegion.vue'
import { useProduct } from '../state/product'
import { useCredentialSettings } from './credentials'

const product = useProduct()
const credentials = useCredentialSettings()
</script>

<template>
  <AsyncRegion :state="product.listLoad('credentials')" :on-retry="() => product.retryList('credentials')">
    <SettingsCredentials
      :credentials="credentials.credentials"
      :overlay-store="appOverlayStore"
      :create-credential="credentials.createCredential"
      :change-credential="credentials.changeCredential"
      :delete-credential="credentials.deleteCredential"
    />
  </AsyncRegion>
</template>
