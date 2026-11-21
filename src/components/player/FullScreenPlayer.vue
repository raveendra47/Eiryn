<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue'
import { ListMusic, Mic2, MicOff, ChevronDown, Volume2, VolumeX } from '@lucide/vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import TabSwitcher from '../TabSwitcher.vue'
import type { TabOption } from '../TabSwitcher.vue'
import { usePlayerStore } from '../../stores/player'
import { useAppStore } from '../../stores/app'
import { trackTelemetry } from '../../lib/utils'
import BlurredArtworkBackdrop from './BlurredArtworkBackdrop.vue'
import PlayerArtwork from './PlayerArtwork.vue'
import PlayerSeekBar from './PlayerSeekBar.vue'
import PlayerPlaybackControls from './PlayerPlaybackControls.vue'
import PlayerQueuePanel from './PlayerQueuePanel.vue'
import PlayerLyricsPanel from './PlayerLyricsPanel.vue'
import SleepTimerMenu from '../SleepTimerMenu.vue'
import Slider from '../Slider.vue'
import TrackContextMenu from '../TrackContextMenu.vue'
import FindLyricsDialog from '../FindLyricsDialog.vue'

const player = usePlayerStore()
const app = useAppStore()
const trackContextMenu = ref<InstanceType<typeof TrackContextMenu> | null>(null)
const findLyricsOpen = ref(false)

const emit = defineEmits<{ close: [] }>()

const leftColRef = ref<HTMLElement | null>(null)
const infoColRef = ref<HTMLElement | null>(null)
const leftW = ref(0)
const leftH = ref(0)
const infoH = ref(0)

let ro: ResizeObserver | null = null

function updateMeasurements() {
  if (leftColRef.value) {
    leftW.value = leftColRef.value.clientWidth
    leftH.value = leftColRef.value.clientHeight
  }
  if (infoColRef.value) {
    infoH.value = infoColRef.value.clientHeight
  }
}

function onFullscreenKeyDown(e: KeyboardEvent) {
  if (e.code === 'Escape') {
    e.preventDefault()
    player.setMode('sticky')
  }
}

onMounted(() => {
  window.addEventListener('keydown', onFullscreenKeyDown)
  ro = new ResizeObserver(() => updateMeasurements())
  if (leftColRef.value) ro.observe(leftColRef.value)
  if (infoColRef.value) ro.observe(infoColRef.value)
  setTimeout(updateMeasurements, 50)
})

onUnmounted(() => {
  if (ro) ro.disconnect()
  window.removeEventListener('keydown', onFullscreenKeyDown)
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
})

const COLUMN_GAP = 20
const dynamicCoverSize = computed(() => {
  if (!showRightColumn.value || !leftH.value || !infoH.value) return null
  return Math.round(Math.max(120, Math.min(leftW.value, leftH.value - infoH.value - COLUMN_GAP)))
})

function openContextMenu(e: MouseEvent) {
  if (!player.currentTrack) return
  trackContextMenu.value?.open(e, player.currentTrack, { excludePlayNext: true })
}

function openTrackInfo() {
  if (!player.currentTrack) return
  if (player.playerMode === 'fullscreen') {
    player.playerMode = 'sticky'
  }
  player.openTrackInfo(player.currentTrack)
}

function onClose() {
  player.setMode('sticky')
  emit('close')
}

const activeTab = computed({
  get: () => {
    if (player.isLyricsOpen) return 'lyrics'
    if (player.isQueueOpen) return 'queue'
    return null
  },
  set: (val: string | null) => {
    if (val === 'lyrics') {
      player.isLyricsOpen = true
      player.isQueueOpen = false
    } else if (val === 'queue') {
      player.isQueueOpen = true
      player.isLyricsOpen = false
    } else {
      player.isQueueOpen = false
      player.isLyricsOpen = false
    }
  },
})

async function toggleMaximize() {
  try { await getCurrentWindow().toggleMaximize() } catch {}
}

const showRightColumn = computed(() => player.isQueueOpen || player.isLyricsOpen)

const tabOptions: TabOption[] = [
  { value: 'lyrics', icon: Mic2, label: 'Lyrics' },
  { value: 'queue', icon: ListMusic, label: 'Queue' },
]

const bpLabel = computed(() => {
  const fmt = ['', 'S32', 'S24', 'S16'][player.bpFormat] || ''
  const khz = player.bpRate ? `${Math.round(player.bpRate / 1000)}k` : ''
  return [khz, fmt].filter(Boolean).join(' · ')
})

// --- Cadence-style drag-down-to-close ---
const pull = ref(0)
const pullStatus = ref<'idle' | 'dragging' | 'closing'>('idle')
let startX = 0
let startY = 0
let startT = 0

const NO_PULL = 'input, button, a, textarea, .custom-slider, .custom-scrollbar, .scrollbar-hide, .dnd-handle, [data-no-pull]'

function onPointerDown(e: PointerEvent) {
  if (e.button !== 0 || pullStatus.value !== 'idle') return
  if ((e.target as HTMLElement).closest(NO_PULL)) return
  startX = e.clientX
  startY = e.clientY
  startT = performance.now()
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('pointercancel', onPointerCancel)
}

function onPointerMove(e: PointerEvent) {
  const dx = e.clientX - startX
  const dy = e.clientY - startY
  if (pullStatus.value === 'idle') {
    if (dy < 12 || Math.abs(dx) > dy) {
      // Not a downward drag — detach before it triggers.
      detach()
      return
    }
    pullStatus.value = 'dragging'
  }
  pull.value = Math.max(0, dy)
}

function onPointerUp() {
  detach()
  if (pullStatus.value !== 'dragging') return
  const speed = pull.value / Math.max(1, performance.now() - startT)
  const crossed = pull.value > window.innerHeight / 3
  const flick = pull.value > 60 && speed > 0.6
  if (crossed || flick) {
    closeByPull()
  } else {
    settle()
  }
}

function onPointerCancel() {
  detach()
  if (pullStatus.value === 'dragging') settle()
}

function detach() {
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  window.removeEventListener('pointercancel', onPointerCancel)
}

function settle() {
  pullStatus.value = 'closing'
  pull.value = 0
  setTimeout(() => { pullStatus.value = 'idle' }, 320)
}

function closeByPull() {
  pullStatus.value = 'closing'
  pull.value = window.innerHeight
  setTimeout(() => {
    onClose()
    pull.value = 0
    pullStatus.value = 'idle'
  }, 250)
}
</script>

<template>
  <div class="fixed inset-0 z-[100] bg-black overflow-hidden select-none" @pointerdown="onPointerDown">
    <div
      class="np dark flex flex-col h-full"
      :class="pullStatus === 'dragging' && pull > 0 ? 'np-pulled' : pullStatus === 'closing' ? 'np-closing' : 'np-settling'"
      :style="pull > 0
        ? { transform: `translateY(${pull}px)`, borderRadius: `${Math.min(28, pull / 4)}px` }
        : { borderRadius: '0px' }"
    >
      <BlurredArtworkBackdrop :artwork-url="player.artworkUrl" />

      <div class="relative z-10 flex flex-col h-full text-white">
        <!-- Top bar (window drag) -->
        <div class="flex items-center justify-between px-6 py-4"
          style="-webkit-app-region: drag" @dblclick="toggleMaximize">
          <div class="w-[120px]">
            <button class="p-2 rounded-full hover:bg-white/8 transition-all text-white/60 hover:text-white"
              @click="onClose">
              <ChevronDown class="w-5 h-5" />
            </button>
          </div>
          <span class="text-xs font-semibold text-white/40 uppercase tracking-[0.2em]">
            Now Playing
          </span>
          <div class="flex items-center gap-2 justify-end">
            <button
              class="p-1.5 rounded-full transition-all text-xs flex items-center gap-1.5 px-3 border"
              :class="player.karaokeEnabled ? 'bg-primary text-primary-foreground border-primary shadow-sm font-medium' : 'bg-white/5 hover:bg-white/10 text-white/60 hover:text-white border-white/10'"
              title="Toggle Karaoke Mode (Center Vocal Remover)"
              @click="player.setKaraoke(!player.karaokeEnabled)"
            >
              <MicOff class="w-3.5 h-3.5" />
              <span>Karaoke</span>
            </button>
            <TabSwitcher v-model="activeTab" :options="tabOptions" />
          </div>
        </div>

        <!-- Stage -->
        <div class="flex-1 flex items-center justify-center px-8 pb-10 w-full max-w-[1400px] mx-auto min-h-0">
          <div
            class="h-full flex items-center justify-center transition-all duration-500 ease-[cubic-bezier(0.4,0,0.2,1)] relative"
            :class="showRightColumn ? 'gap-12' : 'gap-0'">

            <!-- Left: cover + info (stacks when a right column is open) -->
            <div
              ref="leftColRef"
              class="flex items-center min-w-0 transition-all duration-500 ease-[cubic-bezier(0.4,0,0.2,1)]"
              :class="showRightColumn ? 'flex-col w-[min(32vw,54vh)] h-full justify-end min-h-0' : 'flex-row gap-[5vw] w-full justify-center'">

              <!-- Cover (dynamically sized to fit available column space) -->
              <div class="aspect-square cursor-pointer flex-shrink-0 transition-all duration-300"
                :class="showRightColumn ? '' : 'w-[min(40vw,62vh)]'"
                :style="dynamicCoverSize && showRightColumn ? { width: `${dynamicCoverSize}px`, height: `${dynamicCoverSize}px` } : (showRightColumn ? { width: '100%' } : undefined)"
                @click="openTrackInfo" @contextmenu.prevent="openContextMenu">
                <PlayerArtwork :artwork-url="player.artworkUrl"
                  :track-title="player.currentTrack?.title || 'Not Playing'"
                  :is-playing="player.isPlaying" size="full"
                  class="w-full h-full shadow-[0_30px_80px_rgba(0,0,0,0.55)]" />
              </div>

              <!-- Info column -->
              <div
                ref="infoColRef"
                class="flex flex-col items-start min-w-0 flex-shrink-0"
                :class="showRightColumn ? 'w-full mt-3' : 'w-[min(440px,34vw)]'">
                <h1 class="font-bold tracking-tight leading-[1.04] line-clamp-2 cursor-pointer mb-2"
                  :class="showRightColumn ? 'text-[clamp(20px,1.8vw,28px)]' : 'text-[clamp(26px,3vw,44px)]'"
                  @click="openTrackInfo" @contextmenu.prevent="openContextMenu">
                  {{ player.currentTrack?.title || 'Not Playing' }}
                </h1>
                <button v-if="player.currentTrack?.artist"
                  class="text-base md:text-lg text-white/85 hover:underline text-left truncate max-w-full"
                  @click="openTrackInfo" @contextmenu.prevent="openContextMenu">
                  {{ player.currentTrack.artist }}
                </button>
                <button v-if="player.currentTrack?.album"
                  class="text-sm text-white/55 hover:underline text-left truncate max-w-full mt-0.5"
                  @click="openTrackInfo">
                  {{ player.currentTrack.album }}
                </button>

                <!-- Seek bar -->
                <PlayerSeekBar variant="ambient" class="w-full mt-5"
                  :progress-percent="player.progressPercent" :position="player.position"
                  :duration="player.duration" @seek="(v) => player.seek(v)" />

                <!-- Transport spread -->
                <PlayerPlaybackControls spread class="mt-4" :is-playing="player.isPlaying"
                  :shuffle="player.shuffle" :repeat-mode="player.repeatMode"
                  :show-indicator="app.showPlayerIndicator"
                  @toggle-play="player.togglePlayPause()" @next="player.next()"
                  @previous="player.previous()" @toggle-shuffle="player.setShuffle(!player.shuffle)"
                  @cycle-repeat="player.cycleRepeat()" />

                <!-- Volume + sleep timer -->
                <div class="flex items-center gap-3 w-full max-w-[260px] mt-4">
                  <button
                    class="text-white/80 hover:text-white transition-all duration-100 active:scale-90 flex-shrink-0"
                    @click="player.toggleMute()">
                    <VolumeX v-if="player.isMuted" class="w-4 h-4" />
                    <Volume2 v-else class="w-4 h-4" />
                  </button>
                  <Slider :model-value="player.isMuted ? 0 : player.rawVolume" :min="0" :max="1" :step="0.01"
                    class="flex-1" @update:model-value="(v: number) => player.setVolume(v)" />
                  <SleepTimerMenu />
                </div>

                <!-- Facts row -->
                <div v-if="player.currentTrack" class="flex items-center flex-wrap gap-2 mt-4">
                  <span class="np-pill">{{ trackTelemetry(player.currentTrack) }}</span>
                  <span v-if="player.bpActive" class="np-pill np-gold">
                    Bit-perfect{{ bpLabel ? ` · ${bpLabel}` : '' }}
                  </span>
                </div>
              </div>
            </div>

            <!-- Right column -->
            <div
              class="h-full transition-all duration-500 ease-[cubic-bezier(0.4,0,0.2,1)] relative flex items-center justify-center min-h-0"
              :class="showRightColumn ? 'w-1/2 max-w-2xl' : 'w-0'">
              <Transition
                enter-active-class="transition-all duration-500 ease-[cubic-bezier(0.4,0,0.2,1)]"
                enter-from-class="opacity-0 translate-x-24"
                enter-to-class="opacity-100 translate-x-0"
                leave-active-class="transition-all duration-500 ease-[cubic-bezier(0.4,0,0.2,1)]"
                leave-from-class="opacity-100 translate-x-0"
                leave-to-class="opacity-0 translate-x-24">
                <PlayerQueuePanel v-if="player.isQueueOpen" key="queue" :queue="player.queue"
                  @play-track="(index) => player.playTracks(player.queue, index)" />
                <PlayerLyricsPanel v-else-if="player.isLyricsOpen" key="lyrics" :lyrics="player.lyricsContent"
                  :loading="player.isLoadingLyrics" :position="player.position"
                  @seek="(time) => player.seek(time)"
                  @search-lyrics="findLyricsOpen = true" />
              </Transition>
            </div>
          </div>
        </div>
      </div>
    </div>

    <TrackContextMenu ref="trackContextMenu" />
    <FindLyricsDialog v-model:open="findLyricsOpen" :track="player.currentTrack" />
  </div>
</template>

<style scoped>
.np {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: #10111a;
  animation: np-rise 0.3s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  will-change: transform;
}
@keyframes np-rise {
  from {
    transform: translateY(40px);
    opacity: 0;
  }
}
.np-pulled {
  animation: none;
  box-shadow: 0 -16px 60px rgba(0, 0, 0, 0.55);
}
.np-settling {
  transition: transform 0.28s cubic-bezier(0.2, 0.8, 0.2, 1),
    border-radius 0.28s cubic-bezier(0.2, 0.8, 0.2, 1);
}
.np-closing {
  transition: transform 0.24s cubic-bezier(0.3, 0, 0.7, 0.4),
    border-radius 0.24s cubic-bezier(0.3, 0, 0.7, 0.4);
  box-shadow: 0 -16px 60px rgba(0, 0, 0, 0.55);
}
.np-pill {
  font-size: 12px;
  font-weight: 600;
  padding: 4px 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.65);
  white-space: nowrap;
}
.np-gold {
  color: #e8c77a;
  background: rgba(232, 199, 122, 0.12);
}
</style>
