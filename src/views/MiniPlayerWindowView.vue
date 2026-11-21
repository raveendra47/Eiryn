<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import {
  SkipBack, SkipForward, Play, Pause,
  Maximize2, X, Music
} from '@lucide/vue'
import LazyImg from '../components/LazyImg.vue'
import { usePlayerStore } from '../stores/player'
import { formatTime } from '../lib/utils'
import { determineAudioQuality } from '../lib/audioQuality'
import { useLyrics } from '../composables/useLyrics'
import * as api from '../lib/invoke'

const store = usePlayerStore()

// Lyrics state
const rawLyrics = ref<string>('')
const lyricsRef = computed(() => rawLyrics.value)
const { syncedLines } = useLyrics(lyricsRef)

const trackTitle = computed(() => {
  if (!store.currentTrack) return 'Nothing playing'
  return store.currentTrack.title || 'Nothing playing'
})

const trackArtist = computed(() => {
  if (!store.currentTrack) return 'Pick something in Eiryn'
  return store.currentTrack.artist || 'Pick something in Eiryn'
})

const currentLyricLine = computed(() => {
  if (!syncedLines.value || syncedLines.value.length === 0) return ''
  const pos = store.position + 0.25 // Look-ahead for musical timing
  let activeText = ''
  for (const l of syncedLines.value) {
    if (l.time > pos) break
    activeText = l.text
  }
  return activeText
})

const qualityLabel = computed(() => {
  const t = store.currentTrack
  if (!t) return ''
  const tier = determineAudioQuality(t)
  const rateKhz = t.sample_rate ? `${(t.sample_rate / 1000).toFixed(0)}kHz` : ''
  const bits = t.bit_depth ? `${t.bit_depth}-bit` : ''
  const fmt = (t.file_format || '').toUpperCase()
  if (tier === 'HI-RES' || tier === 'LOSSLESS') {
    return `${fmt} ${bits ? `${bits}/` : ''}${rateKhz}`.trim()
  }
  if (t.bitrate) {
    return `${fmt} ${Math.round(t.bitrate / 1000)}kbps`
  }
  return fmt || (tier !== 'UNKNOWN' ? tier : '')
})

const artworkUrl = computed(() => store.artworkUrlMd || store.artworkUrl || store.artworkUrlSm)

const washStyle = computed(() => {
  if (!artworkUrl.value) return {}
  return {
    backgroundImage: `url('${artworkUrl.value}')`
  }
})

async function fetchLyricsForTrack(trackId?: string) {
  if (!trackId) {
    rawLyrics.value = ''
    return
  }
  try {
    const row = await api.getTrackLyrics(trackId)
    rawLyrics.value = row?.content || ''
  } catch {
    rawLyrics.value = ''
  }
}

watch(() => store.currentTrack?.id, (newId) => {
  fetchLyricsForTrack(newId)
}, { immediate: true })

if (typeof document !== 'undefined') {
  document.documentElement.classList.add('transparent-window')
}

async function restoreMain() {
  try {
    await api.showMainWindow()
  } catch (e) {
    console.warn('Failed to restore main window:', e)
  }
}

async function closeMini() {
  await restoreMain()
}

function onSeekClick(e: MouseEvent) {
  const bar = e.currentTarget as HTMLElement
  const r = bar.getBoundingClientRect()
  if (store.duration && r.width > 0) {
    const fraction = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width))
    store.seek(fraction * store.duration)
  }
}

let _unlisteners: (() => void)[] = []

const displayPosition = computed(() => {
  return store.position
})

function onMiniKeyDown(e: KeyboardEvent) {
  const target = e.target as HTMLElement
  if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) return

  if (e.code === 'Escape') {
    e.preventDefault()
    closeMini()
  } else if (e.code === 'Space') {
    e.preventDefault()
    store.togglePlayPause()
  } else if (!e.ctrlKey && !e.altKey && !e.shiftKey && e.key === 'ArrowRight') {
    e.preventDefault()
    store.seekRelative(5)
  } else if (!e.ctrlKey && !e.altKey && !e.shiftKey && e.key === 'ArrowLeft') {
    e.preventDefault()
    store.seekRelative(-5)
  } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === 'ArrowRight') {
    e.preventDefault()
    store.next()
  } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === 'ArrowLeft') {
    e.preventDefault()
    store.previous()
  }
}

onMounted(async () => {
  // Transparent window document styling for clean rounded border and drop shadow
  document.documentElement.classList.add('transparent-window')
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  const appEl = document.getElementById('app')
  if (appEl) {
    appEl.style.background = 'transparent'
    appEl.style.backgroundColor = 'transparent'
  }
  try {
    await api.removeWindowBorder('mini-player')
  } catch {}

  await store.restoreSession()
  store.startPolling()
  await store.syncState()

  window.addEventListener('keydown', onMiniKeyDown)

  _unlisteners = [
    await listen('track-started', () => { store.syncState() }),
    await listen('track-changed', () => { store.syncState() }),
  ]
})

onUnmounted(() => {
  document.documentElement.classList.remove('transparent-window')
  window.removeEventListener('keydown', onMiniKeyDown)
  _unlisteners.forEach(fn => fn())
  store.dispose()
})
</script>

<template>
  <div class="mini-window select-none" data-tauri-drag-region>
    <!-- Blurred artwork background wash -->
    <div
      v-if="artworkUrl"
      class="wash"
      :style="washStyle"
      data-tauri-drag-region
    />
    <!-- Dark gradient veil for readability -->
    <div class="veil" data-tauri-drag-region />

    <!-- Left: 104x104px Album Artwork Thumbnail -->
    <button
      class="art-thumb group"
      @click="restoreMain"
      title="Open Eiryn"
      style="-webkit-app-region: no-drag"
    >
      <LazyImg
        v-if="artworkUrl"
        :src="artworkUrl"
        :alt="trackTitle"
        class="w-full h-full object-cover transition-transform duration-200 group-hover:scale-105"
      />
      <div v-else class="w-full h-full flex items-center justify-center bg-white/5 text-white/30">
        <Music class="w-8 h-8" />
      </div>
    </button>

    <!-- Center/Right: Details and Controls -->
    <div class="body-content" data-tauri-drag-region>
      <!-- Title -->
      <div
        class="title-text"
        :title="trackTitle"
        data-tauri-drag-region
      >
        {{ trackTitle }}
      </div>

      <!-- Artist -->
      <div
        class="artist-text"
        :title="trackArtist"
        data-tauri-drag-region
      >
        {{ trackArtist }}
      </div>

      <!-- Live Synced Lyric or Quality Badge -->
      <div class="meta-line" data-tauri-drag-region>
        <span v-if="currentLyricLine" class="lyric-chip" :title="currentLyricLine">
          <span class="text-primary text-[11px] shrink-0">♪</span>
          <span class="truncate">{{ currentLyricLine }}</span>
        </span>
        <span v-else-if="qualityLabel" class="quality-chip">
          {{ qualityLabel }}
        </span>
      </div>

      <!-- Controls Row: Prev, Play/Pause, Next, EQ Bars, Elapsed Time -->
      <div class="controls-row" style="-webkit-app-region: no-drag">
        <button
          class="ctrl-btn"
          @click="store.previous()"
          title="Previous Track"
        >
          <SkipBack class="w-4 h-4 fill-current" />
        </button>

        <button
          class="play-btn"
          @click="store.togglePlayPause()"
          :title="store.isPlaying ? 'Pause' : 'Play'"
        >
          <Pause v-if="store.isPlaying" class="w-4 h-4 fill-current text-black" />
          <Play v-else class="w-4 h-4 fill-current text-black ml-0.5" />
        </button>

        <button
          class="ctrl-btn"
          @click="store.next()"
          title="Next Track"
        >
          <SkipForward class="w-4 h-4 fill-current" />
        </button>

        <!-- Animated EQ Visualizer Bars -->
        <span
          class="eq-container"
          :class="{ 'eq-playing': store.isPlaying }"
          aria-hidden="true"
        >
          <i class="eq-bar eq-bar-1" />
          <i class="eq-bar eq-bar-2" />
          <i class="eq-bar eq-bar-3" />
          <i class="eq-bar eq-bar-4" />
        </span>

        <!-- Time Display -->
        <span class="time-text ml-auto" data-tauri-drag-region>
          {{ formatTime(displayPosition) }} / {{ formatTime(store.duration) }}
        </span>
      </div>
    </div>

    <!-- Top-Right Window Controls -->
    <div class="top-controls" style="-webkit-app-region: no-drag">
      <button
        class="win-btn"
        @click="restoreMain"
        title="Open Main Window"
      >
        <Maximize2 class="w-3.5 h-3.5" />
      </button>
      <button
        class="win-btn win-btn-close"
        @click="closeMini"
        title="Close Mini Player"
      >
        <X class="w-3.5 h-3.5" />
      </button>
    </div>

    <!-- Bottom Seek Progress Bar -->
    <div
      class="progress-bar group"
      @click="onSeekClick"
      title="Seek"
      style="-webkit-app-region: no-drag"
    >
      <div
        class="progress-fill"
        :style="{ width: `${Math.min(100, store.progressPercent)}%` }"
      />
    </div>
  </div>
</template>

<style scoped>
.mini-window {
  position: relative;
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 14px 16px 18px 14px;
  border-radius: 18px;
  overflow: hidden;
  background: rgba(18, 19, 28, 0.92);
  backdrop-filter: blur(28px);
  -webkit-backdrop-filter: blur(28px);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.65), inset 0 0 0 1px rgba(255, 255, 255, 0.06);
  box-sizing: border-box;
}

.wash {
  position: absolute;
  inset: -50%;
  background-size: cover;
  background-position: center;
  filter: blur(36px) saturate(1.6);
  opacity: 0.72;
  transform: scale(1.08) translateZ(0);
  will-change: transform;
  pointer-events: none;
}

.veil {
  position: absolute;
  inset: 0;
  background: linear-gradient(105deg, rgba(10, 11, 20, 0.42) 0%, rgba(10, 11, 20, 0.75) 100%);
  pointer-events: none;
}

.art-thumb {
  position: relative;
  width: 104px;
  height: 104px;
  flex-shrink: 0;
  border-radius: 12px;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 10px 24px rgba(0, 0, 0, 0.5);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.body-content {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
}

.title-text {
  font-size: 14px;
  font-weight: 600;
  color: #ffffff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  padding-right: 48px;
  line-height: 1.25;
}

.artist-text {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.6);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  padding-right: 48px;
  margin-top: 2px;
  line-height: 1.2;
}

.meta-line {
  height: 20px;
  display: flex;
  align-items: center;
  margin-top: 4px;
  padding-right: 48px;
  overflow: hidden;
}

.lyric-chip {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  font-weight: 500;
  color: rgba(255, 255, 255, 0.9);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.quality-chip {
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  border-radius: 4px;
  font-size: 9.5px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-weight: 600;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.75);
  border: 1px solid rgba(255, 255, 255, 0.12);
  letter-spacing: 0.02em;
}

.controls-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
}

.ctrl-btn {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  color: rgba(255, 255, 255, 0.65);
  transition: all 0.15s ease;
  cursor: pointer;
}
.ctrl-btn:hover {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.08);
}

.play-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  background: #ffffff;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
  transition: transform 0.15s ease;
  cursor: pointer;
}
.play-btn:hover {
  transform: scale(1.06);
}
.play-btn:active {
  transform: scale(0.96);
}

.eq-container {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 12px;
  padding: 0 4px;
}

.eq-bar {
  width: 2px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.75);
  height: 3px;
  transition: height 0.2s ease;
}

.eq-playing .eq-bar-1 { animation: eqAnim1 0.75s ease-in-out infinite; }
.eq-playing .eq-bar-2 { animation: eqAnim2 0.65s ease-in-out infinite; }
.eq-playing .eq-bar-3 { animation: eqAnim3 0.85s ease-in-out infinite; }
.eq-playing .eq-bar-4 { animation: eqAnim4 0.6s ease-in-out infinite; }

@keyframes eqAnim1 { 0%, 100% { height: 3px; } 50% { height: 11px; } }
@keyframes eqAnim2 { 0%, 100% { height: 11px; } 50% { height: 4px; } }
@keyframes eqAnim3 { 0%, 100% { height: 4px; } 50% { height: 12px; } }
@keyframes eqAnim4 { 0%, 100% { height: 10px; } 50% { height: 5px; } }

.time-text {
  font-size: 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  color: rgba(255, 255, 255, 0.5);
  letter-spacing: -0.01em;
}

.top-controls {
  position: absolute;
  top: 10px;
  right: 12px;
  display: flex;
  align-items: center;
  gap: 4px;
  z-index: 30;
}

.win-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.45);
  transition: all 0.15s ease;
  cursor: pointer;
}
.win-btn:hover {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.1);
}
.win-btn-close:hover {
  color: #f87171;
  background: rgba(239, 68, 68, 0.2);
}

.progress-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 3px;
  background: rgba(255, 255, 255, 0.12);
  cursor: pointer;
  transition: height 0.15s ease;
  z-index: 25;
}
.progress-bar:hover {
  height: 5px;
}

.progress-fill {
  height: 100%;
  background: var(--dynamic-primary, #60a5fa);
  transition: width 0.08s linear;
}
</style>
