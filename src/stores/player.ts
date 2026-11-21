import { defineStore } from 'pinia'
import { ref, computed, watch, shallowRef } from 'vue'
import { listen } from '@tauri-apps/api/event'
import * as api from '../lib/invoke'
import type { ThemeColors } from '../lib/invoke'
import { useAppStore } from './app'

// Buffer for open-file events arriving before the store is initialized
let _pendingOpenFile: Record<string, unknown> | null = null

// Register early at module level so buffered Tauri events aren't missed
listen<Record<string, unknown>>('open-file', (event) => {
  if (_storeHandler) {
    _storeHandler(event.payload)
  } else {
    _pendingOpenFile = event.payload
  }
})

let _storeHandler: ((data: Record<string, unknown>) => void) | null = null

// Generation guard for the currentTrack watcher below: rapid skips fire the
// watcher again while an earlier run is still awaiting lyrics/artwork/palette.
// Stale resolutions must not overwrite the newer track's state (footer
// artwork mismatch). Capture `const gen = ++_artworkGen` on entry and bail
// after every await when `gen !== _artworkGen`.
let _artworkGen = 0

export type PlayerMode = 'sticky' | 'mini' | 'fullscreen'

export const usePlayerStore = defineStore('player', () => {
  const currentTrack = ref<api.Track | null>(null)
  const queue = ref<api.Track[]>([])
  const queueIndex = ref(0)
  const queueBusy = ref(false)
  const isPlaying = ref(false)
  const isRadioPlaying = ref(false)
  const isQueueOpen = ref(false)
  const isLyricsOpen = ref(false)
  const isTrackInfoOpen = ref(false)
  const playerMode = ref<PlayerMode>('sticky')

  const position = ref(0)
  const duration = ref(0)
  const lyricsContent = ref<string | undefined>(undefined)
  const isLoadingLyrics = ref(false)
  const volume = ref(0.8)
  const shuffle = ref(false)
  const repeatMode = ref<'off' | 'all' | 'one'>('off')
  const theme = shallowRef<ThemeColors | null>(null)
  const artworkUrl = ref<string | null>(null)
  const artworkUrlSm = computed(() => artworkUrl.value)
  const artworkUrlMd = computed(() => artworkUrl.value)
  const artworkBlobCache = new Map<string, string>()
  const ARTWORK_CACHE_MAX = 150
  const isMuted = ref(false)
  const preMuteVolume = ref(0.8)
  const isPaused = computed(() => !isPlaying.value && currentTrack.value !== null)
  const isStopped = computed(() => !isPlaying.value && currentTrack.value === null)

  const appStore = useAppStore()
  const rawVolume = ref(0.8)

  function computeEffectiveVolume(): number {
    if (appStore.replaygainMode === 'off' || !currentTrack.value) {
      return rawVolume.value
    }
    const gain = appStore.replaygainMode === 'track'
      ? currentTrack.value.replaygain_track_gain
      : currentTrack.value.replaygain_album_gain
    if (gain === 0) return rawVolume.value
    return Math.min(1, Math.max(0, rawVolume.value * Math.pow(10, gain / 20)))
  }

  function applyVolume() {
    if (isMuted.value) {
      volume.value = 0
      api.setVolume(0).catch(() => { })
    } else {
      const eff = computeEffectiveVolume()
      volume.value = eff
      api.setVolume(eff).catch(() => { })
    }
  }

  watch([() => currentTrack.value, () => appStore.replaygainMode], () => { applyVolume() })

  const audioEngineDead = ref(false)

  const gapless = ref(localStorage.getItem('resona-gapless') !== 'false')
  const crossfade = ref(parseFloat(localStorage.getItem('resona-crossfade') || '2'))
  const bpRequested = ref(false)
  const bpActive = ref(false)
  const bpRate = ref(0)
  const bpFormat = ref(0)
  const bpNote = ref<string | null>(null)
  const autoQueue = ref(localStorage.getItem('resona-auto-queue') !== 'false')
  const crossfeedEnabled = ref(false)
  const crossfeedLevel = ref('default')
  const karaokeEnabled = ref(false)

  async function loadKaraoke() {
    try {
      karaokeEnabled.value = await api.getKaraoke()
    } catch { }
  }

  async function setKaraoke(enabled: boolean) {
    karaokeEnabled.value = enabled
    await api.setKaraoke(enabled).catch(() => { })
  }

  async function loadCrossfeed() {
    try {
      const s = await api.getCrossfeedSettings()
      crossfeedEnabled.value = s.enabled
      crossfeedLevel.value = s.level || 'default'
    } catch { }
  }

  async function setCrossfeedSettings(enabled: boolean, level: string) {
    crossfeedEnabled.value = enabled
    crossfeedLevel.value = level
    await api.setCrossfeedSettings({ enabled, level }).catch(() => { })
  }

  // Session restore — position to seek to after first play
  const restoredPosition = ref(0)
  let _checkpointInterval: ReturnType<typeof setInterval> | null = null

  function persistSession() {
    api.saveSession(
      currentTrack.value?.id ?? null,
      position.value,
      rawVolume.value,
      repeatMode.value,
      shuffle.value,
      queueIndex.value,
      gapless.value,
      crossfade.value,
    ).catch(() => { })
  }

  const sleepTimerMinutes = ref(0) // 0 = off
  let sleepTimerInterval: ReturnType<typeof setInterval> | null = null
  let sleepTimerRemaining = ref(0)
  let sleepTimerBaseVolume = 0

  function setSleepTimer(minutes: number) {
    if (sleepTimerInterval) {
      clearInterval(sleepTimerInterval)
      sleepTimerInterval = null
    }
    sleepTimerMinutes.value = minutes
    if (minutes <= 0) {
      sleepTimerRemaining.value = 0
      return
    }
    sleepTimerRemaining.value = minutes * 60
    sleepTimerBaseVolume = rawVolume.value > 0 ? rawVolume.value : 0.8
    const fadeDurationSecs = Math.min(120, Math.max(10, Math.round(minutes * 60 * 0.3)))

    sleepTimerInterval = setInterval(() => {
      sleepTimerRemaining.value--

      // Cadence-style smooth fade-out over the final period
      if (sleepTimerRemaining.value <= fadeDurationSecs && sleepTimerRemaining.value > 0 && isPlaying.value) {
        const factor = sleepTimerRemaining.value / fadeDurationSecs
        const faded = Math.max(0.01, sleepTimerBaseVolume * factor)
        setVolume(faded).catch(() => { })
      }

      if (sleepTimerRemaining.value <= 0) {
        if (sleepTimerInterval) clearInterval(sleepTimerInterval)
        sleepTimerInterval = null
        pause()
        sleepTimerMinutes.value = 0
        sleepTimerRemaining.value = 0
        // Restore base volume for next listening session
        if (sleepTimerBaseVolume > 0) {
          setVolume(sleepTimerBaseVolume).catch(() => { })
        }
      }
    }, 1000)
  }

  function cancelSleepTimer() {
    if (sleepTimerInterval) {
      clearInterval(sleepTimerInterval)
      sleepTimerInterval = null
    }
    if (sleepTimerMinutes.value > 0 && sleepTimerBaseVolume > 0) {
      setVolume(sleepTimerBaseVolume).catch(() => { })
    }
    sleepTimerMinutes.value = 0
    sleepTimerRemaining.value = 0
  }

  const progressPercent = computed(() =>
    duration.value > 0 ? (position.value / duration.value) * 100 : 0,
  )

  // RAF interpolation state
  let lastSyncTime = performance.now()
  let lastSyncPosition = 0

  function syncInterpolation(pos: number) {
    lastSyncPosition = pos
    lastSyncTime = performance.now()
    position.value = lastSyncPosition
  }

  let _offFns: (() => void)[] = []
  let _initialized = false
  let _positionInterval: ReturnType<typeof setInterval> | null = null
  let _rafId: number | null = null

  // Connect the module-level open-file handler to this store instance
  const handleOpenFile = (data: Record<string, unknown>) => {
    if (data && typeof data.title === 'string' && typeof data.file_path === 'string') {
      const track = {
        id: (data.id as string) || '',
        file_path: data.file_path,
        title: data.title,
        artist: (data.artist as string) || 'Unknown',
        album: (data.album as string) || '',
        album_artist: (data.album_artist as string) || '',
        album_id: (data.album_id as string) || '',
        duration: (data.duration as number) || 0,
        composer: (data.composer as string) || '',
        track_number: (data.track_number as number) || 0,
        disc_number: (data.disc_number as number) || 0,
        year: (data.year as number) || 0,
        genre: (data.genre as string) || '',
        raw_genre_names: (data.raw_genre_names as string) || '',
        sample_rate: (data.sample_rate as number) || 0,
        bit_depth: (data.bit_depth as number) || 0,
        bitrate: (data.bitrate as number) || 0,
        file_size: (data.file_size as number) || 0,
        file_format: (data.file_format as string) || '',
        has_artwork: !!(data.has_artwork as boolean),
        channels: (data.channels as number) || 0,
        replaygain_track_gain: (data.replaygain_track_gain as number) || 0,
        replaygain_album_gain: (data.replaygain_album_gain as number) || 0,
        cue_parent_id: null,
        cue_offset: 0,
        artwork_key: null,
        bpm: 0,
        musical_key: '',
        energy: 0,
        analysis_failed_reason: null,
        file_modified: 0,
      } satisfies import('../lib/invoke').Track
      currentTrack.value = track
      duration.value = track.duration
      isPlaying.value = true
      syncInterpolation(0)
    }
  }
  _storeHandler = handleOpenFile

  // Process any open-file event that arrived before the store was ready
  if (_pendingOpenFile) {
    handleOpenFile(_pendingOpenFile)
    _pendingOpenFile = null
  }

  function updateInterpolatedPosition() {
    if (isPlaying.value) {
      const now = performance.now()
      const elapsed = (now - lastSyncTime) / 1000
      position.value = Math.min(lastSyncPosition + elapsed, duration.value)
    } else {
      position.value = lastSyncPosition
    }
    _rafId = requestAnimationFrame(updateInterpolatedPosition)
  }

  async function restoreSession() {
    if (_initialized) return
    _initialized = true

    // Check audio engine readiness with retries (handles race with thread startup)
    for (let attempt = 0; attempt < 5; attempt++) {
      try {
        const ready = await api.isAudioReady()
        if (ready) break
      } catch { /* backend may still be starting */ }
      if (attempt < 4) await new Promise(r => setTimeout(r, 1000))
    }
    try {
      const ready = await api.isAudioReady()
      if (!ready) {
        audioEngineDead.value = true
        console.warn('[audio] Engine not ready — audio unavailable')
      }
    } catch {
      audioEngineDead.value = true
      console.error('[audio] Failed to query audio engine')
    }

    // Sync gapless/crossfade from localStorage to backend
    api.setGapless(gapless.value).catch(() => { })
    api.setCrossfade(crossfade.value).catch(() => { })

    // Pull initial bit-perfect output state (armed by main.rs restore)
    try {
      applyOutputState(await api.getOutputState())
    } catch { }

    // Pull any external file metadata stored during Rust setup
    try {
      const pending = await api.consumePendingExternalFile()
      if (pending) {
        handleOpenFile(pending as Record<string, unknown>)
      }
    } catch { }

    // Restore session from disk (survives crashes)
    try {
      const result = await api.restoreSession()
      if (result.session) {
        const s = result.session
        rawVolume.value = s.volume
        volume.value = s.volume
        api.setVolume(s.volume).catch(() => { })
        repeatMode.value = s.repeat_mode as 'off' | 'all' | 'one'
        if (s.repeat_mode) {
          api.setRepeatMode(s.repeat_mode).catch(() => { })
        }
        shuffle.value = s.shuffle
        queueIndex.value = s.queue_index
        gapless.value = s.gapless
        crossfade.value = s.crossfade
        restoredPosition.value = s.position
        if (s.track_id) {
          try {
            const track = await api.getTrack(s.track_id)
            if (track) {
              currentTrack.value = track
              duration.value = track.duration
              position.value = s.position
              lastSyncPosition = s.position
              lastSyncTime = performance.now()
            }
          } catch { }
        }
      }
    } catch { }
    await loadCrossfeed()
    await loadKaraoke()

    let lastTrackId: string | null = null
    let lastPlayRowId: number | null = null

    let isMainWindow = true
    try {
      const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow')
      isMainWindow = getCurrentWebviewWindow().label === 'main'
    } catch {
      isMainWindow = true
    }

    const listeners = [
      await listen<api.OutputState>('output-updated', (event) => {
        applyOutputState(event.payload)
      }),
      await listen('player:status', (event) => {
        const data = event.payload as Record<string, unknown>
        if (typeof data.position === 'number') {
          syncInterpolation(data.position)
          if (isMainWindow) persistSession()
        }
        if (data.playing === false && data.track_id === null) {
          isPlaying.value = false
          position.value = 0
          lastSyncPosition = 0
          lastSyncTime = performance.now()
          if (isMainWindow) persistSession()
        }
        if (typeof data.volume === 'number') {
          rawVolume.value = data.volume
          volume.value = data.volume
          if (isMainWindow) persistSession()
        }
        if (typeof data.playing === 'boolean') {
          if (data.playing && !isPlaying.value) {
            lastSyncTime = performance.now()
          }
          isPlaying.value = data.playing
        }
        if (typeof data.track_id === 'string' && (!currentTrack.value || currentTrack.value.id !== data.track_id)) {
          api.getTrack(data.track_id).then(t => {
            if (t) {
              currentTrack.value = t
              duration.value = t.duration
            }
          }).catch(() => { })
        }
      }),
      await listen('player:position', (event) => {
        // ~4 Hz anchors from the audio thread; NO persistSession here (the
        // 30 s checkpoint covers persistence).
        const data = event.payload as Record<string, unknown>
        if (typeof data.position === 'number' && isPlaying.value) {
          syncInterpolation(data.position)
        }
      }),
      await listen<string>('track-changed', async (event) => {
        const newId = event.payload
        if (!newId) return
        if (isMainWindow) {
          if (lastTrackId && lastPlayRowId !== null) {
            const done = duration.value > 0 && position.value / duration.value > 0.9
            api.finishPlay(lastPlayRowId, position.value, duration.value, done ? 'completed' : 'skipped').catch(() => { })
            lastPlayRowId = null
          }
          const rowId = await api.recordPlay(newId).catch(() => null)
          if (rowId !== null) lastPlayRowId = rowId
          lastTrackId = newId
          await api.advanceQueue().catch(() => { })
        }
        const track = await api.getTrack(newId).catch(() => null)
        if (track) {
          currentTrack.value = track
          duration.value = track.duration
          isPlaying.value = true
          syncInterpolation(0)
          if (isMainWindow) {
            const qs = await api.getQueueState().catch(() => null)
            if (qs && qs.queue.length > 0) {
              let nextId: string | null = null
              if (repeatMode.value === 'one') {
                nextId = qs.queue[qs.queue_index]?.id ?? null
              } else if (qs.queue_index + 1 < qs.queue.length) {
                nextId = qs.queue[qs.queue_index + 1].id
              } else if (repeatMode.value === 'all') {
                nextId = qs.queue[0]?.id ?? null
              }
              if (nextId) {
                preloadArtwork(nextId)
                await api.prepareNextTrack(nextId).catch(() => { })
              }
            }
          }
        }
        await syncQueueFromBackend()
      }),
      await listen<string>('track-started', async (event) => {
        const newId = event.payload
        if (!newId) return
        if (isMainWindow) {
          if (lastTrackId && lastPlayRowId !== null) {
            const done = duration.value > 0 && position.value / duration.value > 0.9
            api.finishPlay(lastPlayRowId, position.value, duration.value, done ? 'completed' : 'skipped').catch(() => { })
            lastPlayRowId = null
          }
          const rowId = await api.recordPlay(newId).catch(() => null)
          if (rowId !== null) lastPlayRowId = rowId
          lastTrackId = newId
          if (currentTrack.value) {
            api.nowPlaying(
              currentTrack.value.artist,
              currentTrack.value.title,
              currentTrack.value.album,
              Math.round(currentTrack.value.duration),
            ).catch(() => { })
          }
        }
        if (!currentTrack.value || currentTrack.value.id !== newId) {
          const track = await api.getTrack(newId).catch(() => null)
          if (track) {
            currentTrack.value = track
            duration.value = track.duration
            isPlaying.value = true
            syncInterpolation(0)
          }
        }
      }),
    ]

    if (isMainWindow) {
      listeners.push(
        await listen<string>('media-key', (event) => {
          if (event.payload === 'NextTrack') playNext()
          else if (event.payload === 'PreviousTrack') playPrev()
          else if (event.payload === 'Stop') {
            isPlaying.value = false
            position.value = 0
            lastSyncPosition = 0
            lastSyncTime = performance.now()
            api.stopPlayback().catch(() => { })
          }
        }),
        await listen<string>('repeat-mode-changed', (event) => {
          if (event.payload) {
            repeatMode.value = event.payload as 'off' | 'all' | 'one'
          }
        }),
        await listen<boolean>('shuffle-changed', async (event) => {
          shuffle.value = event.payload
          await syncQueueFromBackend()
        }),
        await listen('playback-ended', async () => {
          if (!currentTrack.value) return
          if (lastTrackId && lastPlayRowId !== null) {
            const done = duration.value > 0 && position.value / duration.value > 0.9
            api.finishPlay(lastPlayRowId, position.value, duration.value, done ? 'completed' : 'stopped').catch(() => { })
            lastPlayRowId = null
            if (currentTrack.value) {
              if (position.value > 240 || (duration.value > 0 && position.value / duration.value > 0.5)) {
                api.scrobble(currentTrack.value.artist, currentTrack.value.title, currentTrack.value.album).catch(() => { })
              }
            }
            if (autoQueue.value && repeatMode.value === 'off' &&
              queue.value.length - queueIndex.value <= 2) {
              const mix = await api.generateSmartMix(lastTrackId, 20).catch(() => null)
              if (mix && mix.length > 1) {
                await api.appendQueueTracks(mix.slice(1)).catch(() => { })
                await syncQueueFromBackend()
              }
            }
            lastTrackId = null
          }
          if (repeatMode.value === 'one' && currentTrack.value) {
            await api.playTrack(currentTrack.value.id).catch(() => { })
            isPlaying.value = true
            syncInterpolation(0)
          } else {
            await playNext()
          }
        }),
      )
    }

    _offFns = listeners
  }

  function startPolling() {
    if (!_initialized) return

    // Rare fallback poll to correct drift (4 Hz player:position events are
    // the primary source now).
    _positionInterval = setInterval(async () => {
      try {
        const pos = await api.getPosition()
        if (isPlaying.value) {
          syncInterpolation(pos)
        }
      } catch { }
    }, 5000)
    // Start RAF interpolation for smooth UI
    _rafId = requestAnimationFrame(updateInterpolatedPosition)

    // Periodic session checkpoint (every 30s while playing)
    _checkpointInterval = setInterval(() => {
      if (isPlaying.value) {
        persistSession()
      }
    }, 30000)
  }

  watch(currentTrack, async (track) => {
    const gen = ++_artworkGen
    if (!track) {
      artworkUrl.value = null
      theme.value = null
      lyricsContent.value = ''
      isLoadingLyrics.value = false
      return
    }
    const trackId = track.id
    const stale = () => gen !== _artworkGen || currentTrack.value?.id !== trackId
    // Synchronous cache-first paint: correct art instantly, or blank instead
    // of the previous track's art while the fetch is in flight.
    artworkUrl.value = artworkBlobCache.get(trackId) ?? null
    if (track.file_path) {
      isLoadingLyrics.value = true
      try {
        let lrc: string | null = null
        if (appStore.preferLocalLyrics) {
          lrc = await api.getLyrics(track.file_path)
        }
        if (stale()) return
        if (lrc) {
          lyricsContent.value = lrc
        } else {
          const row = await api.getTrackLyrics(track.id)
          if (stale()) return
          lyricsContent.value = (row?.meta_content || row?.content) || ''
        }
        // Auto-fetch from providers if still empty
        if (!lyricsContent.value && appStore.enableLrclib && track.title) {
          try {
            const results = await api.findLyrics(track.title, track.artist || '', track.album || '', track.id)
            if (stale()) return
            if (results && results.length > 0) {
              const best = results[0]
              if (best.lyrics) {
                lyricsContent.value = best.lyrics
              }
            }
          } catch { }
        }
      } catch {
        if (!stale()) lyricsContent.value = ''
      } finally {
        if (!stale()) isLoadingLyrics.value = false
      }
    } else {
      lyricsContent.value = ''
      isLoadingLyrics.value = false
    }

    // Synchronous cache-first paint (instant when cached, blank instead of
    // the previous track's art otherwise), then resolve art + palette in
    // parallel — the palette used to wait behind the artwork fetch.
    const cached = artworkBlobCache.get(track.id)
    artworkUrl.value = cached ?? null
    const artPromise = (async (): Promise<string | null> => {
      if (cached) return cached
      try {
        const bytes = await api.getTrackArtwork(track.id, 'md')
        if (bytes && bytes.length > 0) {
          const uint8 = new Uint8Array(bytes)
          const blob = new Blob([uint8], { type: 'image/jpeg' })
          const url = URL.createObjectURL(blob)
          artworkBlobCache.set(track.id, url)
          if (artworkBlobCache.size > ARTWORK_CACHE_MAX) {
            const oldest = artworkBlobCache.keys().next().value
            if (oldest) {
              URL.revokeObjectURL(artworkBlobCache.get(oldest)!)
              artworkBlobCache.delete(oldest)
            }
          }
          return url
        }
        return null
      } catch {
        return null
      }
    })()
    const palettePromise: Promise<ThemeColors | null> = api
      .getTrackPalette(track.id)
      .catch(() => null)
    const [artUrl, palette] = await Promise.all([artPromise, palettePromise])
    if (stale()) return
    // A null art result means "no artwork" only if the cache still misses;
    // otherwise keep the synchronously painted cached art.
    if (artUrl !== null || !artworkBlobCache.has(track.id)) {
      artworkUrl.value = artUrl
    }
    theme.value = palette
    // Prime the next queue item's art (cache-only, no paint) so the
    // following skip resolves synchronously in the watcher above.
    void primeNextArtwork()
  })

  function primeNextArtwork() {
    try {
      const next = queue.value[queueIndex.value + 1]
      if (!next || artworkBlobCache.has(next.id)) return
      void api.getTrackArtwork(next.id, 'md').then((bytes) => {
        if (!bytes || bytes.length === 0 || artworkBlobCache.has(next.id)) return
        const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' }))
        artworkBlobCache.set(next.id, url)
        if (artworkBlobCache.size > ARTWORK_CACHE_MAX) {
          const oldest = artworkBlobCache.keys().next().value
          if (oldest && oldest !== next.id) {
            URL.revokeObjectURL(artworkBlobCache.get(oldest)!)
            artworkBlobCache.delete(oldest)
          }
        }
      }).catch(() => { })
    } catch { }
  }

  function dispose() {
    _storeHandler = null
    for (const url of artworkBlobCache.values()) URL.revokeObjectURL(url)
    artworkBlobCache.clear()
    _offFns.forEach(fn => fn())
    _offFns = []
    if (_positionInterval) clearInterval(_positionInterval)
    if (_rafId !== null) cancelAnimationFrame(_rafId)
    if (_checkpointInterval) clearInterval(_checkpointInterval)
    _initialized = false
  }

  async function syncState() {
    try {
      const ps = await api.getPlayerState()
      rawVolume.value = ps.volume
      volume.value = ps.volume
      position.value = ps.position
      lastSyncPosition = ps.position
      lastSyncTime = performance.now()
      isRadioPlaying.value = false
      repeatMode.value = ps.repeat_mode as 'off' | 'all' | 'one'
      shuffle.value = ps.shuffle
      queueIndex.value = ps.queue_index
      gapless.value = ps.gapless
      crossfade.value = ps.crossfade
      if (ps.track_id) {
        const track = await api.getTrack(ps.track_id)
        if (track) {
          currentTrack.value = track
          duration.value = track.duration
          isPlaying.value = ps.is_playing
          return
        }
      }
      if (!ps.track_id || !(await api.getTrack(ps.track_id))) {
        isPlaying.value = false
      }
    } catch (e) {
      console.error('syncState failed:', e)
    }
  }

  async function resolveTrackRef(ref: api.TrackRef): Promise<api.Track | null> {
    try {
      return await api.getTrack(ref.id)
    } catch {
      return null
    }
  }

  async function syncQueueFromBackend() {
    try {
      const qs = await api.getQueueState()
      const localById = new Map(queue.value.map(t => [t.id, t]))
      queue.value = qs.queue.map(tref => localById.get(tref.id) ?? ({
        id: tref.id, title: tref.title, artist: tref.artist, duration: tref.duration,
        file_path: tref.file_path, cue_parent_id: tref.cue_parent_id, cue_offset: tref.cue_offset,
        bpm: tref.bpm, energy: tref.energy,
        album: '', album_artist: '', album_id: '', composer: '', track_number: 0, disc_number: 0,
        year: 0, genre: '', raw_genre_names: '', sample_rate: 0, bit_depth: 0,
        channels: 0, bitrate: 0, file_size: 0, file_format: '', has_artwork: false,
        replaygain_track_gain: 0, replaygain_album_gain: 0, artwork_key: null,
        musical_key: '', file_modified: 0,
      } as api.Track))
      queueIndex.value = qs.queue_index
      if (qs.repeat_mode && (qs.repeat_mode === 'off' || qs.repeat_mode === 'all' || qs.repeat_mode === 'one')) {
        repeatMode.value = qs.repeat_mode
      }
      shuffle.value = qs.shuffle
    } catch { }
    queueBusy.value = false
  }

  async function preloadArtwork(trackId: string) {
    if (artworkBlobCache.has(trackId)) return
    try {
      const bytes = await api.getTrackArtwork(trackId, 'md')
      if (bytes && bytes.length > 0) {
        const uint8 = new Uint8Array(bytes)
        const blob = new Blob([uint8], { type: 'image/jpeg' })
        const url = URL.createObjectURL(blob)
        artworkBlobCache.set(trackId, url)
        if (artworkBlobCache.size > ARTWORK_CACHE_MAX) {
          const oldest = artworkBlobCache.keys().next().value
          if (oldest) {
            URL.revokeObjectURL(artworkBlobCache.get(oldest)!)
            artworkBlobCache.delete(oldest)
          }
        }
      }
    } catch { /* silent */ }
  }

  async function play() {
    if (audioEngineDead.value) {
      console.warn('[audio] Cannot play — engine unavailable')
      return
    }
    if (isPlaying.value) {
      return
    }
    if (currentTrack.value) {
      const wasPaused = await api.togglePlay()
      isPlaying.value = true
      if (wasPaused) {
        syncInterpolation(position.value)
      } else {
        syncInterpolation(restoredPosition.value > 0 ? restoredPosition.value : 0)
        if (currentTrack.value) {
          try {
            await api.playTrack(currentTrack.value.id)
            // Restore position from session if we just started playing after crash restore
            if (restoredPosition.value > 0) {
              await api.seek(restoredPosition.value)
              position.value = restoredPosition.value
              lastSyncPosition = restoredPosition.value
              restoredPosition.value = 0
            }
          } catch (e) {
            isPlaying.value = false
            console.error('Playback failed:', e)
          }
        }
      }
      persistSession()
    }
  }

  async function pause() {
    isPlaying.value = false
    lastSyncPosition = position.value
    lastSyncTime = performance.now()
    await api.togglePlay()
    persistSession()
  }

  async function togglePlayPause() {
    if (isPlaying.value) {
      await pause()
    } else if (currentTrack.value) {
      await play()
    }
  }

  async function playNext() {
    const track = await api.nextTrack().catch(e => {
      console.error('nextTrack failed', e)
      return null
    })
    if (track) {
      const full = await resolveTrackRef(track)
      if (full) preloadArtwork(full.id)
      currentTrack.value = full
      if (full) {
        duration.value = full.duration
        isPlaying.value = true
        syncInterpolation(0)
      }
    } else {
      isPlaying.value = false
    }
    await syncQueueFromBackend()
    persistSession()
  }

  async function playPrev() {
    const track = await api.prevTrack().catch(e => {
      console.error('prevTrack failed', e)
      return null
    })
    if (track) {
      const full = await resolveTrackRef(track)
      if (full) preloadArtwork(full.id)
      currentTrack.value = full
      if (full) {
        duration.value = full.duration
        isPlaying.value = true
        syncInterpolation(0)
      }
    }
    await syncQueueFromBackend()
    persistSession()
  }

  async function seek(pos: number) {
    syncInterpolation(pos)
    await api.seek(pos)
  }

  async function seekRelative(delta: number) {
    const newPos = Math.max(0, Math.min(duration.value, position.value + delta))
    await seek(newPos)
  }

  async function setVolume(v: number) {
    rawVolume.value = v
    if (v > 0) isMuted.value = false
    applyVolume()
    persistSession()
  }

  async function fastForward() {
    await seekRelative(10)
  }

  async function rewind() {
    await seekRelative(-10)
  }

  async function increaseVolume() {
    await setVolume(Math.min(1, rawVolume.value + 0.05))
  }

  async function decreaseVolume() {
    await setVolume(Math.max(0, rawVolume.value - 0.05))
  }

  async function setGapless(v: boolean) {
    gapless.value = v
    localStorage.setItem('resona-gapless', String(v))
    await api.setGapless(v).catch(() => { })
  }

  async function setCrossfade(v: number) {
    crossfade.value = v
    localStorage.setItem('resona-crossfade', String(v))
    await api.setCrossfade(v).catch(() => { })
  }

  function applyOutputState(os: api.OutputState) {
    bpRequested.value = os.bit_perfect_requested
    bpActive.value = os.bit_perfect_active
    bpRate.value = os.sample_rate
    bpFormat.value = os.format
    bpNote.value = os.note
  }

  async function setBitPerfect(v: boolean) {
    bpRequested.value = v
    if (!v) {
      bpActive.value = false
      bpRate.value = 0
      bpFormat.value = 0
    }
    await api.setBitPerfect(v).catch(() => { })
    // `output-updated` events cover the in-play flow; this backstop syncs the
    // stopped path, where the audio thread updates atoms without emitting.
    setTimeout(async () => {
      try {
        applyOutputState(await api.getOutputState())
      } catch { }
    }, 350)
  }

  function setAutoQueue(v: boolean) {
    autoQueue.value = v
    localStorage.setItem('resona-auto-queue', String(v))
  }

  async function toggleMute() {
    if (isMuted.value) {
      isMuted.value = false
      await setVolume(preMuteVolume.value || 0.8)
    } else {
      preMuteVolume.value = rawVolume.value
      isMuted.value = true
      volume.value = 0
      await api.setVolume(0)
    }
  }

  async function setMuted(m: boolean) {
    if (m !== isMuted.value) await toggleMute()
  }

  async function reorderQueue(newQueue: api.Track[]) {
    if (queueBusy.value) return
    queueBusy.value = true
    queue.value = newQueue
    await api.reorderQueue(newQueue).catch(() => { })
    await syncQueueFromBackend()
  }

  async function setShuffle(s: boolean) {
    if (s === shuffle.value) return
    shuffle.value = s
    await api.setShuffle_(s).catch(() => { })
    await syncQueueFromBackend()
    persistSession()
  }
  async function cycleRepeat() {
    const next = repeatMode.value === 'off' ? 'all' : repeatMode.value === 'all' ? 'one' : 'off'
    repeatMode.value = next
    await api.setRepeatMode(next).catch(() => { })
    persistSession()
  }

  async function toggleShuffle() {
    await setShuffle(!shuffle.value)
  }

  const isMiniWindowOpen = ref(false)

  function cycleMode() {
    switch (playerMode.value) {
      case 'sticky':
        playerMode.value = 'mini'
        break
      case 'mini':
        playerMode.value = 'fullscreen'
        break
      default:
        playerMode.value = 'sticky'
    }
  }

  function setMode(mode: PlayerMode) {
    playerMode.value = mode
  }

  async function toggleMiniWindow() {
    try {
      await api.toggleMiniWindow()
      isMiniWindowOpen.value = !isMiniWindowOpen.value
    } catch {
      const m = await import('../composables/useMiniPlayerWindow')
      const win = await m.getMiniWindow()
      if (win) {
        await m.closeMiniWindow()
        isMiniWindowOpen.value = false
      } else {
        const ok = await m.openMiniWindow()
        isMiniWindowOpen.value = ok
      }
    }
  }

  async function next() {
    await playNext()
  }

  async function previous() {
    await playPrev()
  }

  async function playTracks(tracks: api.Track[], startIndex: number) {
    if (audioEngineDead.value) {
      console.warn('[audio] Cannot play — engine unavailable')
      return
    }
    if (queueBusy.value) return
    queueBusy.value = true
    try {
      queue.value = tracks
      queueIndex.value = startIndex
      await api.setQueue(tracks, startIndex).catch(() => { })
      await api.setRepeatMode(repeatMode.value).catch(() => { })
      await syncQueueFromBackend()
    } catch { }
    const t = tracks[startIndex] ?? null
    if (t) preloadArtwork(t.id)
    const nextTrack = tracks[startIndex + 1]
    if (nextTrack) preloadArtwork(nextTrack.id)
    currentTrack.value = t
    isPlaying.value = true
    if (t) {
      duration.value = t.duration
      syncInterpolation(0)
      try {
        await api.playTrack(t.id)
      } catch (e) {
        isPlaying.value = false
        console.error('Playback failed:', e)
      }
    }
    persistSession()
  }

  function toggleQueue() {
    isQueueOpen.value = !isQueueOpen.value
    if (isQueueOpen.value) { isLyricsOpen.value = false; isTrackInfoOpen.value = false }
  }

  function toggleLyrics() {
    isLyricsOpen.value = !isLyricsOpen.value
    if (isLyricsOpen.value) { isQueueOpen.value = false; isTrackInfoOpen.value = false }
  }

  function toggleTrackInfoDrawer() {
    if (isTrackInfoOpen.value) {
      isTrackInfoOpen.value = false
    } else {
      openTrackInfo(currentTrack.value)
    }
  }

  const trackInfoTrack = ref<api.Track | null>(null)

  function openTrackInfo(track: api.Track | null) {
    if (!track) return
    trackInfoTrack.value = track
    isTrackInfoOpen.value = true
    isQueueOpen.value = false
    isLyricsOpen.value = false
  }

  function closeAllDrawers() {
    isQueueOpen.value = false
    isLyricsOpen.value = false
    isTrackInfoOpen.value = false
  }

  return {
    currentTrack, queue, queueIndex, isPlaying, isRadioPlaying, isPaused, isStopped,
    position, duration, volume, rawVolume, shuffle, repeatMode, playerMode,
    isQueueOpen, isLyricsOpen, isTrackInfoOpen, trackInfoTrack, theme,
    artworkUrl, artworkUrlSm, artworkUrlMd, isMuted, gapless, crossfade,
    bpRequested, bpActive, bpRate, bpFormat, bpNote,
    progressPercent, lyricsContent, isLoadingLyrics, restoreSession, startPolling, dispose,
    play, pause, togglePlayPause, playNext, playPrev, syncState,
    seek, seekRelative, setVolume, setShuffle, cycleRepeat, playTracks, reorderQueue,
    toggleQueue, toggleLyrics, toggleTrackInfoDrawer, openTrackInfo, closeAllDrawers,
    toggleShuffle, cycleMode, setMode, isMiniWindowOpen, toggleMiniWindow,
    next, previous, fastForward, rewind, increaseVolume, decreaseVolume, toggleMute, setMuted,
    sleepTimerMinutes, sleepTimerRemaining, setSleepTimer, cancelSleepTimer,
    setGapless, setCrossfade, setBitPerfect, autoQueue, setAutoQueue, audioEngineDead, syncQueueFromBackend, queueBusy,
    crossfeedEnabled, crossfeedLevel, loadCrossfeed, setCrossfeedSettings,
    karaokeEnabled, setKaraoke, loadKaraoke,
  }
})
