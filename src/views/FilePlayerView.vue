<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { listen } from '@tauri-apps/api/event'
import {
  SkipBack, SkipForward, Play, Pause,
  Pin, PinOff, X, Music, Info,
  Volume2, VolumeX,
} from '@lucide/vue'
import LazyImg from '../components/LazyImg.vue'
import { usePlayerStore } from '../stores/player'
import { formatTime } from '../lib/utils'
import * as api from '../lib/invoke'
import MarqueeText from '../components/MarqueeText.vue'
import Slider from '../components/Slider.vue'

const store = usePlayerStore()

// Local track state (not shared with main app store)
const trackMeta = ref<Record<string, unknown> | null>(null)
const localPosition = ref(0)
const localDuration = ref(0)
const localIsPlaying = ref(false)
const isSeeking = ref(false)
const artworkObjectUrl = ref<string | null>(null)
const seekValue = ref(0)
const alwaysOnTop = ref(localStorage.getItem('resona-file-always-on-top') === 'true')
const localError = ref<string | null>(null)
const showFileInfoPanel = ref(false)

const displayPosition = computed(() =>
  isSeeking.value ? (seekValue.value / 100) * localDuration.value : localPosition.value,
)

const trackTitle = computed(() => {
  const t = trackMeta.value
  if (!t) return 'Not Playing'
  const title = t.title as string
  if (title) return title
  const fp = t.file_path as string
  if (fp) {
    const name = fp.replace(/\\/g, '/').split('/').pop()!
    return name.replace(/\.[^.]+$/, '')
  }
  return 'Not Playing'
})
const trackArtist = computed(() => (trackMeta.value?.artist as string) || '')

const progressPct = computed(() =>
  localDuration.value > 0 ? (localPosition.value / localDuration.value) * 100 : 0,
)

const fileInfoFields = computed(() => {
  const m = trackMeta.value
  if (!m) return []
  return [
    { label: 'Title', value: (m.title as string) || '—' },
    { label: 'Artist', value: (m.artist as string) || '—' },
    { label: 'Album', value: (m.album as string) || '—' },
    { label: 'Album Artist', value: (m.album_artist as string) || '—' },
    { label: 'Composer', value: (m.composer as string) || '—' },
    { label: 'Genre', value: (m.genre as string) || '—' },
    { label: 'Track #', value: m.track_number != null ? String(m.track_number) : '—' },
    { label: 'Duration', value: formatTime((m.duration as number) || 0) },
    { label: 'Sample Rate', value: m.sample_rate ? `${m.sample_rate} Hz` : '—' },
    { label: 'Bit Depth', value: m.bit_depth ? `${m.bit_depth}-bit` : '—' },
    { label: 'Bitrate', value: m.bitrate ? `${m.bitrate} kbps` : '—' },
    { label: 'Format', value: (m.file_format as string) || '—' },
    { label: 'File Size', value: m.file_size ? `${Math.round((m.file_size as number) / 1024)} KB` : '—' },
    { label: 'File Path', value: (m.file_path as string) || '—', class: 'text-[10px] text-white/50 break-all' },
  ]
})

// Position interpolation
let lastSyncPos = 0
let lastSyncTime = performance.now()
let _posInterval: ReturnType<typeof setInterval> | null = null
let _rafId: number | null = null
let _unlisteners: (() => void)[] = []

async function toggleAlwaysOnTop() {
  alwaysOnTop.value = !alwaysOnTop.value
  localStorage.setItem('resona-file-always-on-top', String(alwaysOnTop.value))
  try {
    await getCurrentWebviewWindow().setAlwaysOnTop(alwaysOnTop.value)
  } catch { /* noop */ }
}

function onSeekStart() { isSeeking.value = true }
async function onSeekEnd() {
  if (localDuration.value > 0) {
    await store.seek((seekValue.value / 100) * localDuration.value)
  }
  isSeeking.value = false
}

async function closeWindow() {
  try {
    await api.stopPlayback()
    await getCurrentWebviewWindow().close()
  } catch (e) {
    console.warn('[filePlayer] close error:', e)
  }
}

function showFileInfo() {
  showFileInfoPanel.value = !showFileInfoPanel.value
}

function closeFileInfo() {
  showFileInfoPanel.value = false
}

async function togglePlayPause() {
  try {
    await api.togglePlay()
    localIsPlaying.value = !localIsPlaying.value
  } catch (e) {
    console.error('[filePlayer] togglePlay failed:', e)
  }
}

async function loadArtwork(filePath: string) {
  // Revoke previous to avoid leaks
  if (artworkObjectUrl.value) URL.revokeObjectURL(artworkObjectUrl.value)
  artworkObjectUrl.value = null
  try {
    const bytes = await api.getFileArtwork(filePath)
    if (bytes && bytes.length > 0) {
      const blob = new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' })
      artworkObjectUrl.value = URL.createObjectURL(blob)
    }
  } catch { /* no artwork */ }
}

onMounted(async () => {
  // Consume pending external file metadata and play it
  let pending: Record<string, unknown> | null = null
  try {
    pending = await api.consumePendingExternalFile()
  } catch (e) {
    console.error('[filePlayer] Failed to consume pending file:', e)
  }
  if (pending && typeof pending.file_path === 'string') {
    trackMeta.value = pending
    localDuration.value = (pending.duration as number) || 0
    loadArtwork(pending.file_path as string)

    // Check if audio engine is alive before attempting playback
    let engineReady = false
    for (let attempt = 0; attempt < 5; attempt++) {
      try {
        engineReady = await api.isAudioReady()
        if (engineReady) break
      } catch { /* backend may still be starting */ }
      if (attempt < 4) await new Promise(r => setTimeout(r, 1000))
    }
    if (!engineReady) {
      localError.value = 'Audio engine failed to initialize. Try restarting the app.'
      console.error('[filePlayer] Audio engine not ready')
    }

    if (!localError.value) {
      try {
        const result = await api.playExternalFile(pending.file_path as string)
        if (result) {
          trackMeta.value = { ...pending, ...result }
          localDuration.value = (result.duration as number) || 0
          loadArtwork(result.file_path as string)
        }
        localIsPlaying.value = true
      } catch (e) {
        localError.value = String(e)
        console.error('[filePlayer] Failed to play:', e)
      }
    }
  }

  // Sync initial volume from backend player
  try {
    const ps = await api.getPlayerState()
    if (ps && typeof ps.volume === 'number') {
      store.rawVolume = ps.volume
      store.volume = ps.volume
    }
  } catch { }

  // Listen for backend events
  _unlisteners = [
    await listen<Record<string, unknown>>('external-file-received', async (event) => {
      const data = event.payload
      if (data && typeof data.file_path === 'string') {
        localError.value = null
        trackMeta.value = data
        localDuration.value = (data.duration as number) || 0
        loadArtwork(data.file_path as string)
        try {
          const result = await api.playExternalFile(data.file_path as string)
          if (result) {
            trackMeta.value = { ...data, ...result }
            localDuration.value = (result.duration as number) || 0
            loadArtwork(result.file_path as string)
          }
          localIsPlaying.value = true
        } catch (e) {
          localError.value = String(e)
          console.error('[filePlayer] Failed to play external file:', e)
        }
      }
    }),
    await listen('track-started', async () => {
      localIsPlaying.value = true
      try {
        const pos = await api.getPosition()
        localPosition.value = pos
        lastSyncPos = pos
        lastSyncTime = performance.now()
      } catch { /* noop */ }
    }),
    await listen('playback-ended', async () => {
      localIsPlaying.value = false
      localPosition.value = 0
    }),
  ]

  // Periodic position sync (every 2s)
  _posInterval = setInterval(async () => {
    try {
      const pos = await api.getPosition()
      lastSyncPos = pos
      lastSyncTime = performance.now()
      localPosition.value = pos
    } catch { /* noop */ }
  }, 2000)

  // RAF interpolation
  function tick() {
    if (localIsPlaying.value) {
      const now = performance.now()
      const elapsed = (now - lastSyncTime) / 1000
      localPosition.value = Math.min(lastSyncPos + elapsed, localDuration.value)
    }
    _rafId = requestAnimationFrame(tick)
  }
  _rafId = requestAnimationFrame(tick)

  // Restore always-on-top
  try {
    await getCurrentWebviewWindow().setAlwaysOnTop(alwaysOnTop.value)
  } catch { /* noop */ }
})

onUnmounted(() => {
  _unlisteners.forEach(fn => fn())
  if (_posInterval) clearInterval(_posInterval)
  if (_rafId !== null) cancelAnimationFrame(_rafId)
  if (artworkObjectUrl.value) URL.revokeObjectURL(artworkObjectUrl.value)
})
</script>

<template>
  <div class="relative h-full w-full bg-[#0A0A0A] text-white overflow-hidden dark select-none">
    <div class="w-full h-full flex flex-col">
      <!-- Top bar: drag handle + buttons -->
      <div class="flex items-center justify-between px-4 pt-3 shrink-0" data-tauri-drag-region>
        <span class="text-[10px] text-white/30 tracking-widest uppercase">Now Playing</span>
        <div class="flex items-center gap-1.5" style="-webkit-app-region: no-drag">
          <button
            class="w-7 h-7 flex items-center justify-center rounded-full text-white/40 hover:text-white/80 hover:bg-white/10 transition-all"
            @click="showFileInfo" title="File Info">
            <Info class="w-3.5 h-3.5" />
          </button>
          <button
            class="w-7 h-7 flex items-center justify-center rounded-full transition-all"
            :class="alwaysOnTop ? 'text-amber-400 hover:text-amber-300 bg-white/10' : 'text-white/40 hover:text-white/80 hover:bg-white/10'"
            @click="toggleAlwaysOnTop()" title="Always on Top">
            <Pin v-if="alwaysOnTop" class="w-3.5 h-3.5" />
            <PinOff v-else class="w-3.5 h-3.5" />
          </button>
          <button
            class="w-7 h-7 flex items-center justify-center rounded-full text-white/40 hover:text-red-400 hover:bg-white/10 transition-all"
            @click="closeWindow" title="Close (stops playback)">
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Artwork area -->
      <div class="flex-1 min-h-0 flex items-center justify-center px-6 py-2">
        <div class="relative h-full max-h-[220px] aspect-square rounded-2xl overflow-hidden shadow-2xl">
          <LazyImg
            v-if="artworkObjectUrl"
            :src="artworkObjectUrl"
            :alt="trackTitle"
            class="w-full h-full object-cover"
          />
          <div v-else class="w-full h-full flex items-center justify-center bg-white/5 rounded-2xl">
            <Music class="w-16 h-16 text-white/20" />
          </div>
        </div>
      </div>

      <!-- Track info -->
      <div class="px-6 mb-1 shrink-0">
        <MarqueeText :text="trackTitle" content-class="text-base font-semibold leading-tight text-white" />
        <MarqueeText :text="trackArtist" content-class="text-sm text-white/50 leading-tight mt-0.5" />
      </div>

      <!-- Seek bar -->
      <div class="px-6 mb-2 shrink-0" style="-webkit-app-region: no-drag">
        <div class="flex items-center gap-2">
          <span class="text-[11px] text-white/40 tabular-nums w-8 text-right shrink-0">
            {{ formatTime(displayPosition) }}
          </span>
          <Slider
            :model-value="isSeeking ? seekValue : progressPct"
            :min="0"
            :max="100"
            :step="0.1"
            class="flex-1"
            @update:model-value="(v: number) => (seekValue = v)"
            @mousedown="onSeekStart"
            @mouseup="onSeekEnd"
          />
          <span class="text-[11px] text-white/40 tabular-nums w-8 shrink-0">
            {{ formatTime(localDuration) }}
          </span>
        </div>
      </div>

      <!-- Controls -->
      <div class="flex items-center justify-center gap-5 px-6 mb-3 shrink-0" style="-webkit-app-region: no-drag">
        <button class="text-white/60 hover:text-white/90 transition-colors" @click="store.previous()">
          <SkipBack class="w-5 h-5" />
        </button>
        <button
          class="w-12 h-12 bg-white rounded-full flex items-center justify-center hover:scale-105 active:scale-95 transition-transform shrink-0 shadow-lg"
          @click="togglePlayPause()">
          <Pause v-if="localIsPlaying" class="w-5 h-5 fill-current text-[#0A0A0A]" />
          <Play v-else class="w-5 h-5 fill-current text-[#0A0A0A] ml-0.5" />
        </button>
        <button class="text-white/60 hover:text-white/90 transition-colors" @click="store.next()">
          <SkipForward class="w-5 h-5" />
        </button>
      </div>

      <!-- Error message -->
      <div v-if="localError" class="px-4 pb-2 shrink-0">
        <div class="text-[11px] text-red-400/90 text-center leading-tight bg-red-500/10 rounded-lg px-3 py-2">
          {{ localError }}
        </div>
      </div>

      <!-- Volume -->
      <div class="flex items-center justify-center gap-2 px-6 pb-4 shrink-0" style="-webkit-app-region: no-drag">
        <button class="text-white/40 hover:text-white/80 transition-colors"
          @click="store.toggleMute()">
          <VolumeX v-if="store.isMuted" class="w-4 h-4" />
          <Volume2 v-else class="w-4 h-4" />
        </button>
        <Slider
          :model-value="store.isMuted ? 0 : Math.round(store.rawVolume * 100)"
          :min="0"
          :max="100"
          :step="1"
          class="w-28"
          @update:model-value="(v: number) => store.setVolume(v / 100)"
        />
      </div>
    </div>

    <!-- File Info Overlay -->
    <div v-if="showFileInfoPanel && trackMeta" class="absolute inset-0 z-50 bg-[#0A0A0A]/95 flex flex-col">
      <div class="flex items-center justify-between px-4 pt-3 pb-2 shrink-0" data-tauri-drag-region>
        <span class="text-[10px] text-white/30 tracking-widest uppercase">File Info</span>
        <button class="w-7 h-7 flex items-center justify-center rounded-full text-white/40 hover:text-white/80 hover:bg-white/10 transition-all"
          @click="closeFileInfo">
          <X class="w-4 h-4" />
        </button>
      </div>
      <div class="flex-1 overflow-y-auto px-4 pb-4 space-y-2 text-sm">
        <div v-for="(item, key) in fileInfoFields" :key="key" class="flex items-start gap-2">
          <span class="text-white/30 text-[10px] uppercase tracking-wider w-24 shrink-0 pt-0.5">{{ item.label }}</span>
          <span class="text-white/80 break-all min-w-0">{{ item.value }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
