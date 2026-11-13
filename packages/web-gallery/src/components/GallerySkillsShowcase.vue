<script setup lang="ts">
import { reactive } from 'vue'
import SkillsSettings from '@jush-ai/plugin-skills/SkillsSettings.vue'
import { skillsPage } from '@jush-ai/plugin-skills'
import { PageScope, providePageHost } from '@jush-ai/web-ui/plugins/page'
import { galleryPageHost, skillsPlugin } from '../fixtures/plugins'
import { SKILLS_SHOWCASE_OPEN, skillsShowcase } from '../fixtures/settings'

/**
 * The Skills page with every source and skill state pinned at once, in a tab
 * of the Work workspace, over a skills plugin of its own: its controls act on
 * this showcase's state the way the backend's plugin would.
 */
const state = reactive(skillsShowcase())
providePageHost(galleryPageHost({ skills: skillsPlugin(state) }, { workspace: 'work' }))
</script>

<template>
  <PageScope :page="skillsPage" :component="SkillsSettings" :props="{ open: SKILLS_SHOWCASE_OPEN }" />
</template>
