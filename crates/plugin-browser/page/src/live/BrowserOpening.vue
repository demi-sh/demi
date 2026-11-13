<script setup lang="ts">
import { computed, watch } from 'vue'
import { AddressBar, RegionStatus, downloadProgress } from '@jush-ai/plugin-sdk'
import { t } from '../locales/text'
import { BLANK_URL, startingLabel, type BrowserTabsController } from './tabs'
import { BLANK_PAGE } from './view'

/**
 * What the browser window shows over its tabs while a tab the user opens
 * comes, the latest one asked: the tab as it will look, an address bar on
 * what it opens and below it what Jush does, or a blank page for a blank
 * tab the browser opens now, so the tab that comes moves nothing. An
 * address typed meanwhile is where the tab goes once it opens. An opening
 * that ends without a tab, refused or closed before the work showed it,
 * says `failed` once no other is still opening, so a window it left empty
 * closes and one that waits for another tab stays.
 */
const props = defineProps<{
  session: BrowserTabsController
  /** The browser tabs the work holds now. */
  held: ReadonlySet<string>
}>()
const emit = defineEmits<{ failed: [] }>()

// The tab may come before or after the open answers, so both are watched.
watch([() => props.held, props.session.openings], ([tabs]) => props.session.held(tabs), { immediate: true })
watch(props.session.unopened, () => {
  if (props.session.openings.value.length === 0) {
    emit('failed')
  }
})

const latest = computed(() => props.session.openings.value.at(-1) ?? null)
const phase = computed(() => props.session.openingPhase())
</script>

<template>
  <div v-if="latest" class="absolute inset-0 z-10 flex flex-col bg-surface">
    <AddressBar
      :address="latest.url ?? BLANK_URL"
      :back-reason="t('address.noBack')"
      :forward-reason="t('address.noForward')"
      :can-reload="false"
      @submit="(url) => session.goOpening(url)"
    />
    <div class="relative flex min-h-0 flex-1 flex-col border-t border-line">
      <RegionStatus
        v-if="phase !== null"
        class="min-h-0 flex-1"
        status="loading"
        :label="startingLabel(phase)"
        :progress="downloadProgress(session.api.downloading())"
      />
      <div v-else class="min-h-0 flex-1" :class="BLANK_PAGE" />
    </div>
  </div>
</template>
