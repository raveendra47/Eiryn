import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../lib/invoke'

export const useSearchStore = defineStore('search', () => {
  const query = ref('')
  const results = ref<api.SearchAllResults>({ tracks: [], albums: [], artists: [], genres: [], composers: [] })
  const isSearching = ref(false)
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  // Tauri invoke() is not abortable, so stale responses are dropped by
  // sequence: only the latest issued query may write results.
  let requestSeq = 0

  async function search(q: string) {
    query.value = q
    if (debounceTimer) clearTimeout(debounceTimer)
    requestSeq += 1
    if (!q.trim()) {
      results.value = { tracks: [], albums: [], artists: [], genres: [], composers: [] }
      isSearching.value = false
      return
    }
    const seq = requestSeq
    debounceTimer = setTimeout(async () => {
      isSearching.value = true
      try {
        const res = await api.searchAll(q.trim())
        if (seq === requestSeq) results.value = res
      } catch {
        if (seq === requestSeq) {
          results.value = { tracks: [], albums: [], artists: [], genres: [], composers: [] }
        }
      }
      if (seq === requestSeq) isSearching.value = false
    }, 300)
  }

  function clear() {
    query.value = ''
    requestSeq += 1
    results.value = { tracks: [], albums: [], artists: [], genres: [], composers: [] }
    isSearching.value = false
    if (debounceTimer) clearTimeout(debounceTimer)
  }

  return { query, results, isSearching, search, clear }
})
