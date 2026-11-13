<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { appOverlayStore } from '@jush-ai/web-ui/overlay/appOverlay'
import SettingsSubagents from '@jush-ai/web-ui/settings/SettingsSubagents.vue'
import AsyncRegion from '@jush-ai/web-ui/ui/AsyncRegion.vue'
import { useProduct } from '../state/product'
import { useResources } from '../state/resources'
import { useSubagentSettings } from './subagents'

const product = useProduct()
const resources = useResources()
const subagents = useSubagentSettings()
const { settings } = storeToRefs(subagents)
const { switchSubagents, setLimit, switchProfile, saveProfile, deleteProfile } = subagents
</script>

<template>
  <AsyncRegion :state="product.listLoad('subagents')" :on-retry="() => product.retryList('subagents')">
    <SettingsSubagents
      v-if="settings"
      :enabled="settings.enabled"
      :max-live="settings.maxLive"
      :max-depth="settings.maxDepth"
      :profiles="settings.profiles"
      :providers="resources.providerInfos"
      :models="resources.models"
      :catalog-models="resources.catalogModels"
      :catalog-ready="product.catalogLoad === 'ready'"
      :overlay-store="appOverlayStore"
      :save-profile="saveProfile"
      :delete-profile="deleteProfile"
      @switch="switchSubagents"
      @set-limit="setLimit"
      @switch-profile="switchProfile"
    />
  </AsyncRegion>
</template>
