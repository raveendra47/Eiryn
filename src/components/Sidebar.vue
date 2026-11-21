<script setup lang="ts">
import { computed } from 'vue'
import { Search, Settings, Sliders } from '@lucide/vue'
import SidebarNav from './SidebarNav.vue'
import SidebarPlaylists from './SidebarPlaylists.vue'
import SidebarItem from './SidebarItem.vue'
import { usePlayerStore } from '../stores/player'
import { useAppStore } from '../stores/app'

const player = usePlayerStore()
const app = useAppStore()

const soundActive = computed(() => player.bpActive || app.eqEnabled || player.crossfeedEnabled)
</script>

<template>
  <div class="flex flex-col h-full bg-background w-full">
    <div class="px-3 py-2">
      <SidebarItem to="/search" :icon="Search" label="Search" />
    </div>

    <div class="mx-3 border-t border-foreground/[0.06] mb-1" />

    <SidebarNav />

    <div class="mx-3 border-t border-foreground/[0.06] my-1" />

    <SidebarPlaylists />

    <div class="mx-3 border-t border-foreground/[0.06] my-1" />

    <div class="px-3 py-2 space-y-1">
      <SidebarItem to="/sound" :icon="Sliders" label="Sound" :show-dot="soundActive" />
      <SidebarItem to="/settings" :icon="Settings" label="Settings" />
    </div>
  </div>
</template>
