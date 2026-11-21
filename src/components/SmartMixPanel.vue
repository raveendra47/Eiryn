<script setup lang="ts">
import { ref, shallowRef, onMounted } from 'vue'
import { Sparkles, RotateCw, Shuffle } from '@lucide/vue'
import * as api from '../lib/invoke'
import type { Track } from '../lib/invoke'
import { usePlayerStore } from '../stores/player'
import HomeSection from './HomeSection.vue'
import TrackCard from './TrackCard.vue'
import TrackContextMenu from './TrackContextMenu.vue'

const playerStore = usePlayerStore()

const state = ref<'idle' | 'seeding' | 'loading' | 'mix'>('idle')
const seedCandidates = shallowRef<Track[]>([])
const mixTracks = shallowRef<Track[]>([])
const seedTitle = ref('')
const trackContextMenu = ref<InstanceType<typeof TrackContextMenu> | null>(null)

onMounted(async () => {
  try {
    const recent = await api.getRecentlyPlayedTracks(28)
    if (recent && recent.length > 0) {
      seedCandidates.value = recent
      state.value = 'seeding'
    }
  } catch { /* no recent tracks */ }
})

async function pickSeed(track: Track) {
  state.value = 'loading'
  seedTitle.value = track.title
  try {
    const result = await api.generateSmartMix(track.id, 28)
    mixTracks.value = result || []
    state.value = result && result.length > 0 ? 'mix' : 'idle'
  } catch {
    state.value = 'seeding'
  }
}

function newMix() {
  state.value = 'seeding'
  mixTracks.value = []
}

function playTrack(track: Track) {
  if (state.value === 'mix') {
    playerStore.playTracks(mixTracks.value, mixTracks.value.indexOf(track))
  } else {
    playerStore.playTracks([track], 0)
  }
}

function playAll() {
  if (mixTracks.value.length > 0) {
    playerStore.playTracks(mixTracks.value, 0)
  }
}

function onTrackContextMenu(e: MouseEvent, track: Track) {
  trackContextMenu.value?.open(e, track)
}
</script>

<template>
  <div>
    <div v-if="state === 'idle' || (state === 'seeding' && seedCandidates.length === 0)">
      <section class="animate-section select-none">
        <div class="flex items-center gap-3 mb-6">
          <div class="p-2 bg-primary/10 rounded-lg">
            <Sparkles class="w-5 h-5 text-primary" />
          </div>
          <h2 class="text-2xl font-bold tracking-tight">Smart Mix</h2>
        </div>
        <p class="text-foreground opacity-60">Play more music to get Smart Mix suggestions.</p>
      </section>
    </div>

    <div v-else-if="state === 'seeding'">
      <HomeSection title="Smart Mix" :icon="Sparkles" :items="seedCandidates" id="carousel-smart-seed">
        <template #default="{ item: track }">
          <div class="relative cursor-pointer group" @click="pickSeed(track)">
            <TrackCard :track="track" @click="playTrack" @contextmenu="onTrackContextMenu" />
            <div
              class="absolute inset-0 bg-black/40 rounded-lg flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity">
              <Shuffle class="w-8 h-8 text-white" />
            </div>
          </div>
        </template>
      </HomeSection>
      <div class="mt-2 text-xs text-foreground/40 text-center select-none">Pick a seed track to generate a Smart Mix</div>
    </div>

    <div v-else-if="state === 'loading'" class="animate-section">
      <div class="flex items-center justify-between mb-6">
        <div class="flex items-center gap-3 select-none">
          <div class="p-2 bg-primary/10 rounded-lg">
            <Sparkles class="w-5 h-5 text-primary" />
          </div>
          <h2 class="text-2xl font-bold tracking-tight">Smart Mix</h2>
        </div>
      </div>
      <div class="flex items-center justify-center py-12">
        <div class="animate-spin rounded-full h-8 w-8 border-2 border-primary border-t-transparent" />
      </div>
    </div>

    <div v-else-if="state === 'mix'">
      <section class="animate-section">
        <div class="flex items-center justify-between mb-6">
          <div class="flex items-center gap-3 select-none">
            <div class="p-2 bg-primary/10 rounded-lg">
              <Sparkles class="w-5 h-5 text-primary" />
            </div>
            <div class="flex items-center gap-3">
              <h2 class="text-2xl font-bold tracking-tight">Smart Mix</h2>
              <button @click="playAll"
                class="group flex items-center gap-2 px-3 py-1 bg-foreground/5 hover:bg-primary hover:text-primary-foreground rounded-full text-xs font-semibold transition-all"
                title="Play all">
                <Shuffle class="w-3 h-3" />
                <span>Play All</span>
              </button>
              <button @click="newMix"
                class="group flex items-center gap-2 px-3 py-1 bg-foreground/5 hover:bg-foreground/10 rounded-full text-xs font-semibold transition-all"
                title="New Mix">
                <RotateCw class="w-3 h-3" />
                <span>New Mix</span>
              </button>
            </div>
          </div>
        </div>

        <HomeSection :items="mixTracks" id="carousel-smart-mix">
          <template #default="{ item: track }">
            <TrackCard :track="track" @click="playTrack" @contextmenu="onTrackContextMenu" />
          </template>
        </HomeSection>
      </section>
    </div>

    <TrackContextMenu ref="trackContextMenu" />
  </div>
</template>
