import { onMounted, onUnmounted } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { useLibraryStore } from '../stores/library'

export function useLibrarySync() {
  const library = useLibraryStore()
  let unlistenFns: (() => void)[] = []

  onMounted(async () => {
    unlistenFns = [
      await listen('library:sync-finished', async () => {
        await library.loadAll()
      }),
      await listen<{ analyzed?: number }>('audio-analysis-done', async (event) => {
        if (event.payload?.analyzed && event.payload.analyzed > 0) {
          await library.loadAll()
        }
      }),
    ]
  })

  onUnmounted(() => {
    unlistenFns.forEach(fn => fn())
  })
}
