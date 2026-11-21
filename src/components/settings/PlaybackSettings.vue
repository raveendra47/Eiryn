<script setup lang="ts">
import { AudioLines, Settings2, Volume2, SkipForward } from '@lucide/vue'
import EQPanel from '../EQPanel.vue'
import { useAppStore } from '../../stores/app'
import { usePlayerStore } from '../../stores/player'

const appStore = useAppStore()
const player = usePlayerStore()
</script>

<template>
  <div class="space-y-10 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <Settings2 class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">General</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Prevent Sleep</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Prevent system from sleeping while playing</p>
          </div>
          <button @click="appStore.updatePreventSleepWhilePlaying(!appStore.preventSleepWhilePlaying)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.preventSleepWhilePlaying ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.preventSleepWhilePlaying ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Player Indicator</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Show player indicator in sidebar</p>
          </div>
          <button @click="appStore.updateShowPlayerIndicator(!appStore.showPlayerIndicator)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.showPlayerIndicator ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.showPlayerIndicator ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <SkipForward class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Gapless Playback</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Gapless</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Pre-decode next track for seamless transitions</p>
          </div>
          <button @click="player.setGapless(!player.gapless)"
            :class="['w-11 h-6 rounded-full transition-colors relative', player.gapless ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', player.gapless ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
        <div class="p-5">
          <div class="flex items-center justify-between mb-2">
            <div>
              <p class="text-sm font-semibold">Crossfade</p>
              <p class="text-xs text-foreground opacity-60 mt-1">Fade between tracks</p>
            </div>
            <span class="text-sm tabular-nums font-mono text-foreground/70">{{ player.crossfade.toFixed(1) }}s</span>
          </div>
          <input type="range" min="0" max="6" step="0.5" :value="player.crossfade"
            @input="player.setCrossfade(parseFloat(($event.target as HTMLInputElement).value))"
            class="w-full h-1.5 bg-foreground/[0.12] rounded-full appearance-none cursor-pointer accent-primary
              [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3.5 [&::-webkit-slider-thumb]:h-3.5
              [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-primary [&::-webkit-slider-thumb]:shadow" />
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <SkipForward class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Smart Queue</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Auto-Queue Continuation</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Automatically append similar tracks when queue ends</p>
          </div>
          <button @click="player.setAutoQueue(!player.autoQueue)"
            :class="['w-11 h-6 rounded-full transition-colors relative', player.autoQueue ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', player.autoQueue ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <Volume2 class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Volume Normalization</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] p-5">
        <div class="flex items-center justify-between mb-3">
          <div>
            <p class="text-sm font-semibold">ReplayGain</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Normalize playback volume across tracks</p>
          </div>
        </div>
        <div class="flex gap-2">
          <button v-for="mode in (['off', 'track', 'album'] as const)" :key="mode"
            @click="appStore.updateReplaygainMode(mode)"
            :class="['px-4 py-2 rounded-lg text-sm font-medium transition-colors capitalize', appStore.replaygainMode === mode ? 'bg-primary text-white' : 'bg-foreground/[0.06] hover:bg-foreground/[0.10]']">
            {{ mode === 'album' ? 'Album' : mode === 'track' ? 'Track' : 'Off' }}
          </button>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <AudioLines class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Equalizer</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
        <EQPanel />
      </div>
    </section>
  </div>
</template>
