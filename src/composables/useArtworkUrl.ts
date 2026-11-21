import { ref, watch, onMounted, onUnmounted } from 'vue'
import * as api from '../lib/invoke'

// Shared md-thumbnail blob cache across all rows/cards/queues: one IPC fetch
// per track no matter how many components display it, LRU-evicted.
const BLOB_CACHE_MAX = 150
const blobCache = new Map<string, string>()
const inflight = new Map<string, Promise<string | null>>()

function evictIfNeeded() {
  while (blobCache.size > BLOB_CACHE_MAX) {
    const oldest = blobCache.keys().next().value
    if (!oldest) break
    URL.revokeObjectURL(blobCache.get(oldest)!)
    blobCache.delete(oldest)
  }
}

async function fetchArtworkUrl(id: string): Promise<string | null> {
  const hit = blobCache.get(id)
  if (hit) return hit
  let p = inflight.get(id)
  if (!p) {
    p = (async () => {
      try {
        const bytes = await api.getTrackArtwork(id, 'md')
        if (!bytes || bytes.length === 0) return null
        const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' }))
        blobCache.set(id, url)
        evictIfNeeded()
        return url
      } catch {
        return null
      } finally {
        inflight.delete(id)
      }
    })()
    inflight.set(id, p)
  }
  return p
}

/**
 * Windowed prefetch for queues: fills the shared cache for the given ids
 * (md thumbnails, bounded concurrency, in-flight deduped) and returns the
 * resolved id→URL map. Callers keep only the window in their local map so
 * a 5k-track queue never materializes 5k artworks.
 */
export async function primeArtworkCache(ids: string[], concurrency = 4): Promise<Map<string, string>> {
  const unique = [...new Set(ids)]
  for (let i = 0; i < unique.length; i += concurrency) {
    await Promise.all(unique.slice(i, i + concurrency).map((id) => fetchArtworkUrl(id)))
  }
  const out = new Map<string, string>()
  for (const id of unique) {
    const hit = blobCache.get(id)
    if (hit) out.set(id, hit)
  }
  return out
}

/**
 * Virtual-list-safe artwork: IntersectionObserver-gated, in-flight deduped,
 * generation-guarded so a recycled row's stale fetch can never paint the
 * wrong cover. URLs live in the shared cache (never revoked per instance).
 */
export function useArtworkUrl(getId: () => string | null, hasArtwork: () => boolean) {
  const artworkSrc = ref<string | null>(null)
  let observer: IntersectionObserver | null = null
  let visible = false
  let gen = 0

  async function resolve() {
    const my = ++gen
    const id = getId()
    if (!id || !hasArtwork()) {
      artworkSrc.value = null
      return
    }
    const hit = blobCache.get(id)
    if (hit) {
      artworkSrc.value = hit
      return
    }
    artworkSrc.value = null
    if (!visible) return
    const url = await fetchArtworkUrl(id)
    if (my === gen) artworkSrc.value = url
  }

  function observe(el: HTMLElement | null) {
    if (!el) return
    if (!observer) {
      observer = new IntersectionObserver(
        (entries) => {
          for (const e of entries) {
            if (e.isIntersecting) {
              visible = true
              void resolve()
            }
          }
        },
        { rootMargin: '200px' },
      )
    }
    observer.observe(el)
  }

  function reload() {
    void resolve()
  }

  watch(getId, () => {
    void resolve()
  })

  onMounted(() => {
    void resolve()
  })

  onUnmounted(() => {
    gen++
    observer?.disconnect()
    observer = null
  })

  return { artworkSrc, observe, reload }
}
