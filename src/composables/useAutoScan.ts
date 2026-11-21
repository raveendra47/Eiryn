import { onMounted, onUnmounted } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from '../stores/app'
import * as api from '../lib/invoke'

function waitForEvent(event: string): Promise<void> {
  return new Promise<void>(async (resolve) => {
    const unlisten = await listen(event, () => {
      unlisten()
      resolve()
    })
  })
}

export function useAutoScan() {
  const appStore = useAppStore()
  let unlistenSync: (() => void) | null = null
  let isRunning = false

  async function runPipeline() {
    if (isRunning) return
    isRunning = true
    try {
      if (appStore.autoFetchLyrics) {
        const done = waitForEvent('batch-lyrics-all-done')
        await api.batchFetchLyrics()
        await done
      }

      if (appStore.autoFetchArtistArtwork) {
        const done = waitForEvent('artist-artwork-done')
        await api.batchFetchArtistArtwork()
        await done
      }

      if (appStore.autoFetchGenre) {
        const done = waitForEvent('batch-genre-done')
        await api.batchFetchGenre()
        await done
      }

      if (appStore.autoAnalyzeAudio) {
        const done = waitForEvent('audio-analysis-done')
        await api.analyzeAudio()
        await done
      }
    } catch (e) {
      console.error('Auto-scan pipeline error:', e)
    } finally {
      isRunning = false
    }
  }

  onMounted(async () => {
    unlistenSync = await listen('library:sync-finished', () => {
      runPipeline()
    })
  })

  onUnmounted(() => {
    unlistenSync?.()
  })
}
