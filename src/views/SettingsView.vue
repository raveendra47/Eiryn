<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import {
  Search,
  Sliders,
  ChevronRight,
  Folder,
  Palette,
  Globe,
  Wifi,
  Keyboard,
  Info,
  X,
  Speaker,
} from '@lucide/vue'
import { usePlayerStore } from '../stores/player'
import GeneralSettings from '../components/settings/GeneralSettings.vue'
import LibrarySettings from '../components/settings/LibrarySettings.vue'
import IntegrationsSettings from '../components/settings/IntegrationsSettings.vue'
import RemoteServerSettings from '../components/settings/RemoteServerSettings.vue'
import KeyboardShortcutsSettings from '../components/settings/KeyboardShortcutsSettings.vue'
import AboutSettings from '../components/settings/AboutSettings.vue'

const router = useRouter()
const player = usePlayerStore()
const searchQuery = ref('')

const soundKeywords = ['sound', 'audio', 'eq', 'equalizer', 'crossfeed', 'bitperfect', 'exclusive', 'wasapi', 'lufs', 'replaygain', 'leveling', 'dac', 'headphone', 'autoeq', 'crossfade']

const matchesSound = computed(() => {
  if (!searchQuery.value.trim()) return false
  const q = searchQuery.value.toLowerCase().trim()
  return soundKeywords.some(k => k.includes(q) || q.includes(k))
})

const soundFeatures = [
  { title: 'Direct output', about: 'Straight to DAC, bit-perfect, WASAPI exclusive mode' },
  { title: 'Sound Check', about: 'LUFS loudness leveling, ReplayGain normalization' },
  { title: 'Tone per song', about: 'Fletcher-Munson equal-loudness contour & Smart EQ' },
  { title: 'Equalizer', about: '10-band presets, AutoEq & Equalizer APO headphone correction' },
  { title: 'Headphone crossfeed', about: 'Bauer stereophonic-to-binaural (bs2b) crossfeed' },
  { title: 'Crossfade', about: 'Gapless playback and smooth track transitions' },
]

function matchesQuery(text: string): boolean {
  if (!searchQuery.value.trim()) return true
  const q = searchQuery.value.toLowerCase().trim()
  return text.toLowerCase().includes(q)
}
</script>

<template>
  <div class="settings-page h-full overflow-y-auto px-8 py-8 select-none">
    <div class="max-w-4xl mx-auto space-y-8">
      <!-- Title & Search Bar (Cadence Layout) -->
      <div>
        <h1 class="text-3xl font-bold tracking-tight text-foreground">Settings</h1>
        <div class="relative mt-4">
          <Search class="absolute left-3.5 top-1/2 -translate-y-1/2 w-4 h-4 text-foreground/40 pointer-events-none" />
          <input
            v-model="searchQuery"
            type="search"
            placeholder="Search settings..."
            class="w-full pl-10 pr-9 py-2.5 rounded-xl bg-foreground/[0.04] border border-foreground/[0.08] text-sm text-foreground placeholder:text-foreground/40 focus:outline-none focus:ring-1 focus:ring-primary focus:border-primary/50 transition-all"
          />
          <button
            v-if="searchQuery"
            @click="searchQuery = ''"
            class="absolute right-3 top-1/2 -translate-y-1/2 p-1 text-foreground/40 hover:text-foreground"
          >
            <X class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <!-- "On the Sound page" Hero Hit Card (Cadence format: appears when search matches audio terms) -->
      <div
        v-if="matchesSound"
        class="sound-hero-card rounded-2xl p-6 bg-gradient-to-br from-primary/10 via-foreground/[0.02] to-transparent border border-primary/20 space-y-4 cursor-pointer hover:border-primary/40 transition-all"
        @click="router.push('/sound')"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <div class="w-9 h-9 rounded-xl bg-primary/20 flex items-center justify-center text-primary">
              <Sliders class="w-5 h-5" />
            </div>
            <div>
              <h2 class="text-base font-bold text-foreground">On the Sound page</h2>
              <p class="text-xs text-foreground/60">Dedicated audio DSP, bit-perfect WASAPI, signal path, and equalizer controls</p>
            </div>
          </div>
          <div class="flex items-center gap-1 text-xs font-semibold text-primary">
            <span>Open Sound Page</span>
            <ChevronRight class="w-4 h-4" />
          </div>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2 pt-2 border-t border-foreground/[0.06]">
          <div
            v-for="f in soundFeatures"
            :key="f.title"
            class="p-2.5 rounded-xl bg-foreground/[0.03] border border-foreground/[0.06]"
          >
            <p class="text-xs font-semibold text-foreground">{{ f.title }}</p>
            <p class="text-[11px] text-foreground/50 mt-0.5 leading-tight">{{ f.about }}</p>
          </div>
        </div>
      </div>

      <!-- Settings Sections -->
      <div class="space-y-10">
        <!-- Section: Music Folders & Library -->
        <section
          v-if="matchesQuery('folders library scan music tags auto watch attention health')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Folder class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">Music Folders & Library</h2>
          </div>
          <LibrarySettings />
        </section>

        <!-- Section: Audio Output Overview (Cadence layout) -->
        <section
          v-if="matchesQuery('audio output device format sound mixer wasapi bitperfect sample rate')"
          class="settings-section"
        >
          <div class="flex items-center justify-between mb-4">
            <div class="flex items-center gap-2 text-foreground/80">
              <Speaker class="w-4 h-4 text-primary" />
              <div>
                <h2 class="text-lg font-bold">Audio Output</h2>
                <p class="text-xs text-foreground/60 mt-0.5">Eiryn plays through your system's default audio device</p>
              </div>
            </div>
            <button
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-foreground/10 text-xs font-semibold text-foreground/80 hover:text-foreground hover:border-foreground/30 hover:bg-foreground/[0.04] transition-all"
              @click="router.push('/sound')"
            >
              <Sliders class="w-3.5 h-3.5 text-primary" />
              <span>Sound settings</span>
            </button>
          </div>

          <div class="p-5 rounded-2xl bg-card border border-foreground/[0.06] space-y-3">
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 text-xs">
              <div>
                <span class="text-foreground/50 block">Device</span>
                <span class="font-medium text-foreground text-sm">Default Audio Device</span>
              </div>
              <div>
                <span class="text-foreground/50 block">Output Mode</span>
                <span class="font-medium text-foreground text-sm">
                  {{ player.bpActive ? 'Direct WASAPI Exclusive (Bit-perfect)' : 'Shared (Windows Audio Engine)' }}
                </span>
              </div>
              <div>
                <span class="text-foreground/50 block">Sample Rate</span>
                <span class="font-medium font-mono text-foreground">
                  {{ player.bpActive && player.bpRate ? `${(player.bpRate / 1000).toFixed(1)} kHz` : 'System Default (48.0 kHz)' }}
                </span>
              </div>
              <div>
                <span class="text-foreground/50 block">Bit Depth</span>
                <span class="font-medium font-mono text-foreground">
                  {{ player.bpActive && player.bpFormat ? `${['', '32-bit float', '24-bit', '16-bit'][player.bpFormat] || '24-bit'}` : '32-bit float' }}
                </span>
              </div>
            </div>
          </div>
        </section>

        <!-- Section: Appearance & System -->
        <section
          v-if="matchesQuery('theme color dark light accent appearance startup oled black glass zoom')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Palette class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">Appearance & System</h2>
          </div>
          <GeneralSettings />
        </section>

        <!-- Section: Integrations & Lyrics -->
        <section
          v-if="matchesQuery('lyrics lrclib kugou artwork deezer itunes lastfm scrobble')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Globe class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">Online Metadata & Lyrics</h2>
          </div>
          <IntegrationsSettings />
        </section>

        <!-- Section: Keyboard Shortcuts -->
        <section
          v-if="matchesQuery('shortcuts hotkeys keyboard media keys palette command')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Keyboard class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">Keyboard Shortcuts</h2>
          </div>
          <KeyboardShortcutsSettings />
        </section>

        <!-- Section: Remote Server -->
        <section
          v-if="matchesQuery('remote server browser phone lan wifi control')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Wifi class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">Remote Control Server</h2>
          </div>
          <RemoteServerSettings />
        </section>

        <!-- Section: About & Maintenance -->
        <section
          v-if="matchesQuery('about version updates stats licenses audiophile developer')"
          class="settings-section"
        >
          <div class="flex items-center gap-2 mb-4 text-foreground/80">
            <Info class="w-4 h-4 text-primary" />
            <h2 class="text-lg font-bold">About Eiryn</h2>
          </div>
          <AboutSettings />
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-section {
  padding-top: 24px;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
}
</style>
