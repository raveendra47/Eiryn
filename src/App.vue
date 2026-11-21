<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import MainLayout from './layouts/MainLayout.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import CommandPalette from './components/CommandPalette.vue'
import { usePlayerStore } from './stores/player'
import { useAppStore } from './stores/app'
import { useDeviceStore } from './stores/device'
import { useLibraryStore } from './stores/library'
import { useFavoritesStore } from './stores/favorites'
import { useLibrarySync } from './composables/useLibrarySync'
import { useAutoScan } from './composables/useAutoScan'
import { applyThemeColors } from './lib/artwork'
import * as api from './lib/invoke'

let _unlistenExternal: (() => void) | null = null

const route = useRoute()
const router = useRouter()
const playerStore = usePlayerStore()
const appStore = useAppStore()
const deviceStore = useDeviceStore()
const libraryStore = useLibraryStore()
const favoritesStore = useFavoritesStore()

const isCommandPaletteOpen = ref(false)

useLibrarySync()
useAutoScan()

const isMiniPlayer = computed(() => route.name === 'mini-player' || window.location.hash.includes('mini-player'))
const isFilePlayer = computed(() => route.name === 'file-player' || window.location.hash.includes('file-player'))

window.onerror = (_msg, _url, _line, _col, err) => {
  console.error('[GLOBAL]', err?.stack || err)
}
window.addEventListener('unhandledrejection', (e) => {
  console.error('[UNHANDLED]', e.reason?.stack || e.reason)
})

const handleKeyDown = (e: KeyboardEvent) => {
  const target = (e.target instanceof HTMLElement ? e.target : document.body) as HTMLElement

  // Range sliders, checkboxes, buttons shouldn't lock out playback shortcuts
  const typing =
    (target.tagName === 'INPUT' && !['range', 'checkbox', 'radio', 'button'].includes((target as HTMLInputElement).type)) ||
    target.tagName === 'TEXTAREA' ||
    target.tagName === 'SELECT' ||
    target.isContentEditable

  const isMac = deviceStore.isMac
  const ctrlKey = isMac ? e.metaKey : e.ctrlKey
  const shiftKey = e.shiftKey
  const altKey = e.altKey

  // ── COMMAND PALETTE (Ctrl+K) ──────────────────────────────────
  if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    isCommandPaletteOpen.value = !isCommandPaletteOpen.value
    return
  }

  // If Command Palette is open, let it handle its own keys
  if (isCommandPaletteOpen.value) {
    if (e.key === 'Escape') {
      e.preventDefault()
      isCommandPaletteOpen.value = false
    }
    return
  }

  // If user is typing in a text field, ignore the general playback shortcuts
  if (typing) return

  // ── PLAYBACK CONTROL ──────────────────────────────────────────
  if (e.code === 'Space' || (e.key === ' ' && !typing)) {
    // Only yield if focusing on a button or link
    if (target.tagName !== 'BUTTON' && target.tagName !== 'A') {
      e.preventDefault()
      playerStore.togglePlayPause()
    }
  } else if (ctrlKey && !shiftKey && (e.key === 'ArrowRight' || e.key.toLowerCase() === 'n')) {
    e.preventDefault()
    playerStore.next()
  } else if (ctrlKey && !shiftKey && (e.key === 'ArrowLeft' || e.key.toLowerCase() === 'p')) {
    e.preventDefault()
    playerStore.previous()
  } else if (!ctrlKey && !altKey && !shiftKey && e.key === 'ArrowRight') {
    e.preventDefault()
    playerStore.seekRelative(5)
  } else if (!ctrlKey && !altKey && !shiftKey && e.key === 'ArrowLeft') {
    e.preventDefault()
    playerStore.seekRelative(-5)
  } else if (shiftKey && !ctrlKey && !altKey && e.key === 'ArrowRight') {
    e.preventDefault()
    playerStore.seekRelative(30)
  } else if (shiftKey && !ctrlKey && !altKey && e.key === 'ArrowLeft') {
    e.preventDefault()
    playerStore.seekRelative(-30)
  } else if (ctrlKey && !shiftKey && e.key === 'ArrowUp') {
    e.preventDefault()
    playerStore.increaseVolume()
  } else if (ctrlKey && !shiftKey && e.key === 'ArrowDown') {
    e.preventDefault()
    playerStore.decreaseVolume()
  } else if ((ctrlKey && e.key.toLowerCase() === 'm') || (!ctrlKey && !altKey && !shiftKey && e.key.toLowerCase() === 'm')) {
    e.preventDefault()
    playerStore.toggleMute()
  } else if ((ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 's') || (altKey && !ctrlKey && !shiftKey && e.key.toLowerCase() === 's')) {
    // Toggle shuffle (Ctrl+S and Alt+S)
    e.preventDefault()
    playerStore.setShuffle(!playerStore.shuffle)
  } else if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'r') {
    // Cycle repeat (Ctrl+R, Cadence parity)
    e.preventDefault()
    playerStore.cycleRepeat()
  } else if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'l') {
    // Toggle lyrics / living artwork view (Cadence parity)
    e.preventDefault()
    if (playerStore.playerMode !== 'fullscreen') {
      playerStore.setMode('fullscreen')
    }
    playerStore.isLyricsOpen = !playerStore.isLyricsOpen
    if (playerStore.isLyricsOpen) playerStore.isQueueOpen = false
  } else if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'u') {
    // Up Next / Queue (Ctrl+U, Cadence parity)
    e.preventDefault()
    playerStore.toggleQueue()
  }

  // ── LIBRARY & NAVIGATION ──────────────────────────────────────
  else if ((ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'f') || (!ctrlKey && !shiftKey && !altKey && e.key === '/')) {
    e.preventDefault()
    router.push('/search')
  } else if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'd') {
    e.preventDefault()
    const track = playerStore.currentTrack
    if (track) {
      api.appendQueueTracks([track]).catch(() => {})
      playerStore.syncQueueFromBackend().catch(() => {})
    }
  } else if (ctrlKey && shiftKey && !altKey && e.key.toLowerCase() === 'l') {
    e.preventDefault()
    const track = playerStore.currentTrack
    if (track) {
      favoritesStore.toggle(track.id)
    }
  } else if (ctrlKey && altKey && !shiftKey && e.key.toLowerCase() === 'm') {
    e.preventDefault()
    playerStore.toggleMiniWindow()
  } else if (e.code === 'F11' || (ctrlKey && shiftKey && !altKey && e.key.toLowerCase() === 'f')) {
    e.preventDefault()
    playerStore.setMode(playerStore.playerMode === 'fullscreen' ? 'sticky' : 'fullscreen')
  } else if (e.code === 'Escape') {
    if (playerStore.playerMode === 'fullscreen') {
      e.preventDefault()
      playerStore.setMode('sticky')
    } else if (playerStore.isQueueOpen || playerStore.isLyricsOpen || playerStore.isTrackInfoOpen) {
      e.preventDefault()
      playerStore.closeAllDrawers()
    }
  }

  // ── WINDOW & APP ──────────────────────────────────────────────
  else if (ctrlKey && !shiftKey && !altKey && e.key.toLowerCase() === 'q') {
    e.preventDefault()
    api.quitApp()
  } else if (ctrlKey && e.key === ',') {
    e.preventDefault()
    router.push('/settings')
  } else if (ctrlKey && shiftKey && !altKey && e.key.toLowerCase() === 'i') {
    e.preventDefault()
    api.toggleDevTools()
  }
}

onMounted(async () => {
  const t0 = performance.now()
  console.info(`[startup] frontend mounted +${Math.round(t0)}ms`)
  await appStore.loadAppearance()
  appStore.loadSavedSettings()
  appStore.loadLastfmConfig()
  deviceStore.init()

  if (!isMiniPlayer.value && !isFilePlayer.value) {
    window.addEventListener('keydown', handleKeyDown)
    await Promise.all([
      libraryStore.loadAll(),
      favoritesStore.load(),
      playerStore.restoreSession(),
    ])
    console.info(`[startup] library+session ready +${Math.round(performance.now())}ms`)
    playerStore.startPolling()

    // Listen for external file events from second instance (file association)
    const unlistenExternal = await listen<Record<string, unknown>>('external-file-received', async (event) => {
      const data = event.payload
      if (data && typeof data.file_path === 'string') {
        const newTitle = (data.title as string) || 'Unknown'
        const ok = playerStore.isPlaying && playerStore.currentTrack
          ? confirm(`Stop "${playerStore.currentTrack.title}" and play "${newTitle}"?`)
          : true
        if (ok) {
          if (playerStore.isPlaying) {
            await api.stopPlayback()
          }
          const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
          const win = new WebviewWindow(`file-player-${Date.now()}`, {
            url: '/#/file-player',
            width: 420,
            height: 640,
            resizable: false,
            decorations: false,
            center: true,
          })
          win.onCloseRequested(async () => {
            await api.stopPlayback()
          })
        }
      }
    })

    _unlistenExternal = unlistenExternal
  } else if (isMiniPlayer.value) {
    await libraryStore.loadAll()
  }
})

onUnmounted(() => {
  if (_unlistenExternal) {
    _unlistenExternal()
    _unlistenExternal = null
  }
  if (!isMiniPlayer.value && !isFilePlayer.value) {
    window.removeEventListener('keydown', handleKeyDown)
    playerStore.dispose()
  }
})

watch(() => playerStore.theme, (colors) => applyThemeColors(colors))
watch(() => appStore.theme, () => applyThemeColors(playerStore.theme))
</script>

<template>
  <RouterView v-if="isMiniPlayer || isFilePlayer" />
  <template v-else>
    <div v-if="playerStore.audioEngineDead" class="audio-error-banner">
      ⚠ Audio engine unavailable — playback disabled. Try restarting the app.
    </div>
    <ErrorBoundary>
      <MainLayout />
    </ErrorBoundary>
    <CommandPalette v-model:open="isCommandPaletteOpen" />
  </template>
</template>

<style scoped>
.audio-error-banner {
  background: #3d1f1f;
  color: #f0a0a0;
  padding: 8px 16px;
  text-align: center;
  font-size: 13px;
  border-bottom: 1px solid #5a2e2e;
}
</style>
