<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { ChartColumn, Flame, HardDrive, Disc, ListMusic, Timer, Clock, Music, AudioLines, Users, RefreshCw, ChevronUp, ChevronDown } from '@lucide/vue'
import { listen } from '@tauri-apps/api/event'
import * as api from '../lib/invoke'
import type { ListeningInsights, LibraryInsights } from '../lib/invoke'
import { usePlayerStore } from '../stores/player'
import TopArtistsCarousel from './TopArtistsCarousel.vue'
import ListeningActivityChart from './charts/ListeningActivityChart.vue'
import LibraryGrowthChart from './charts/LibraryGrowthChart.vue'
import GenreDistributionChart from './charts/GenreDistributionChart.vue'
import AudioQualityChart from './charts/AudioQualityChart.vue'
import PlaybackOutcomesChart from './charts/PlaybackOutcomesChart.vue'

const playerStore = usePlayerStore()

type Period = '7d' | '30d' | 'all'
const periodOptions = [
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: 'all', label: 'All time' },
] as const
const libraryPeriod = ref<Period>('7d')
const listeningPeriod = ref<Period>('7d')
const listening = ref<ListeningInsights | null>(null)
const library = ref<LibraryInsights | null>(null)
const loading = ref(false)
const libraryLoading = ref(false)

function formatTime(secs: number): string {
  const m = Math.round(secs / 60)
  const h = Math.floor(m / 60)
  if (h > 0) return `${h}h ${m % 60}m`
  return `${m}m`
}

function formatNum(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M'
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K'
  return String(n)
}

function formatBytes(b: number): string {
  if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB'
  if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB'
  return `${Math.round(b / 1e3)} KB`
}

async function loadListening(silent = false) {
  if (!silent) loading.value = true
  try {
    listening.value = await api.getListeningInsights(listeningPeriod.value)
  } catch (e) {
    console.error('Failed to load listening insights', e)
  } finally {
    loading.value = false
  }
}

async function loadLibrary(silent = false) {
  if (!silent) libraryLoading.value = true
  try {
    library.value = await api.getLibraryInsights(libraryPeriod.value)
  } catch (e) {
    console.error('Failed to load library insights', e)
  } finally {
    libraryLoading.value = false
  }
}

function refreshAll() {
  void loadListening()
  void loadLibrary()
}

watch(listeningPeriod, () => void loadListening())
watch(libraryPeriod, () => void loadLibrary())

let offStats: (() => void) | null = null
let offSync: (() => void) | null = null
onMounted(async () => {
  await Promise.all([loadListening(), loadLibrary()])
  offStats = await listen('stats-cleared', refreshAll)
  offSync = await listen('library:sync-finished', () => {
    void loadLibrary(true)
    void loadListening(true)
  })
})
onUnmounted(() => { offStats?.(); offSync?.() })

const totalQuality = computed(() =>
  (library.value?.quality ?? []).reduce((sum, item) => sum + item.count, 0),
)
const sortedQuality = computed(() => [...(library.value?.quality ?? [])].sort((a, b) => b.count - a.count))
const totalGenres = computed(() =>
  (listening.value?.genres ?? []).reduce((sum, item) => sum + item.seconds, 0),
)
const sortedGenres = computed(() => [...(listening.value?.genres ?? [])].sort((a, b) => {
  if (a.is_other !== b.is_other) return a.is_other ? 1 : -1
  return b.seconds - a.seconds
}))
const sortedPlaybackOutcomes = computed(() => [
  { key: 'completed', value: listening.value?.completed ?? 0 },
  { key: 'skipped', value: listening.value?.skipped ?? 0 },
  { key: 'stopped', value: listening.value?.stopped ?? 0 },
].sort((a, b) => b.value - a.value))

const QUALITY_LABELS: Record<string, string> = {
  lossy: 'Lossy',
  lossless: 'Lossless',
  hi_res: 'Hi-Res',
  dsd: 'DSD',
  unknown: 'Unknown',
}

function playTopTrack(idx: number) {
  const tracks = (listening.value?.top_tracks ?? []).map((t) => t.track)
  if (tracks.length > 0) playerStore.playTracks(tracks, idx)
}

const topTracksPreviewLimit = 5
const topTracksExpanded = ref(false)
const visibleTopTracks = computed(() => topTracksExpanded.value
  ? (listening.value?.top_tracks ?? [])
  : (listening.value?.top_tracks ?? []).slice(0, topTracksPreviewLimit))

const trackArtworkUrls = ref(new Map<string, string>())

async function loadTrackArtwork(id: string) {
  if (trackArtworkUrls.value.has(id)) return
  try {
    const bytes = await api.getTrackArtwork(id, 'md')
    if (bytes && bytes.length > 0) {
      trackArtworkUrls.value.set(id, URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' })))
    }
  } catch { /* artwork fallback */ }
}

watch(() => listening.value?.top_tracks, (list) => {
  topTracksExpanded.value = false
  for (const t of (list ?? []).slice(0, 10)) void loadTrackArtwork(t.track.id)
})

watch(visibleTopTracks, (list) => {
  for (const t of list) void loadTrackArtwork(t.track.id)
})

onUnmounted(() => {
  for (const url of trackArtworkUrls.value.values()) URL.revokeObjectURL(url)
  trackArtworkUrls.value.clear()
})
</script>

<template>
  <section class="space-y-12 pb-12">
    <div class="space-y-8">
      <section class="space-y-6">
        <header class="flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-xl font-semibold">Library</h2>
          <div class="flex gap-1.5">
            <button
              v-for="p in periodOptions"
              :key="p.value"
              @click="libraryPeriod = p.value"
              :class="['px-3.5 py-1.5 rounded-xl text-xs font-medium transition-colors',
                libraryPeriod === p.value ? 'bg-primary text-primary-foreground' : 'bg-foreground/[0.04] text-foreground/70 hover:text-foreground']">
              {{ p.label }}
            </button>
          </div>
        </header>

        <div v-if="libraryLoading && !library" class="flex items-center justify-center py-8">
          <div class="animate-spin rounded-full h-8 w-8 border-2 border-primary border-t-transparent" />
        </div>

        <template v-else-if="library">
          <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <article class="rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
              <div class="flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <HardDrive class="h-4 w-4" />Library size
              </div>
              <p class="mt-3 text-2xl font-semibold">{{ formatBytes(library.total_bytes) }}</p>
            </article>
            <article class="rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
              <div class="flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Disc class="h-4 w-4" />Total albums
              </div>
              <p class="mt-3 text-2xl font-semibold">{{ formatNum(library.albums) }}</p>
            </article>
            <article class="rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
              <div class="flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Users class="h-4 w-4" />Total artists
              </div>
              <p class="mt-3 text-2xl font-semibold">{{ formatNum(library.artists) }}</p>
            </article>
            <article class="rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
              <div class="flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <ListMusic class="h-4 w-4" />Total playlists
              </div>
              <p class="mt-3 text-2xl font-semibold">{{ formatNum(library.playlists) }}</p>
            </article>
          </div>

          <div class="grid gap-4 lg:grid-cols-8">
            <article class="relative flex min-h-[17rem] flex-col overflow-hidden rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-6 backdrop-blur-[30px] lg:col-span-5">
              <div class="relative mb-6 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <HardDrive class="h-4 w-4" />Library growth
              </div>
              <div v-if="library.growth.length" class="relative grid flex-1 grid-cols-1 items-stretch gap-4 md:grid-cols-[auto_minmax(0,1fr)] md:gap-6">
                <div>
                  <p class="text-4xl font-bold tracking-[-0.03em]">{{ formatNum(library.tracks) }}</p>
                  <p class="mt-2 text-xs text-[color:var(--text-muted)]">tracks in library</p>
                </div>
                <LibraryGrowthChart class="h-full w-full self-stretch" :growth="library.growth" />
              </div>
              <div v-else class="relative flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
                <ChartColumn class="mb-2 h-6 w-6 opacity-60" />
                <p class="text-sm">No growth data yet</p>
              </div>
            </article>

            <article class="flex min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px] lg:col-span-3">
              <h2 class="mb-4 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <AudioLines class="h-4 w-4" />Audio quality
              </h2>
              <div v-if="library.quality.length" class="flex flex-1 flex-col gap-5 md:flex-row md:items-center md:gap-8">
                <AudioQualityChart :quality="sortedQuality" />
                <div class="min-w-0 flex-1 space-y-2.5">
                  <div v-for="q in sortedQuality" :key="q.kind"
                    class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-3 text-xs">
                    <span class="truncate text-[color:var(--text-muted)]">{{ QUALITY_LABELS[q.kind] ?? q.kind }}</span>
                    <span class="text-right tabular-nums text-[color:var(--text-muted)]">{{ formatNum(q.count) }}</span>
                    <span class="w-9 text-right tabular-nums text-[color:var(--text-muted)]">{{ totalQuality ? Math.round(q.count / totalQuality * 100) : 0 }}%</span>
                  </div>
                </div>
              </div>
              <div v-else class="flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
                <AudioLines class="mb-2 h-6 w-6 opacity-60" />
                <p class="text-sm">No quality data yet</p>
              </div>
            </article>
          </div>
        </template>
      </section>

      <section class="space-y-6 border-t border-[var(--border-glass)] pt-8">
        <header class="flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-xl font-semibold">Listening</h2>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <button type="button" aria-label="Refresh insights" title="Refresh" :disabled="loading"
              class="flex h-10 w-10 items-center justify-center rounded-full border border-foreground/[0.08] bg-foreground/[0.05] text-sm text-foreground opacity-80 transition-colors hover:bg-foreground/[0.1] hover:text-foreground" @click="refreshAll">
              <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': loading }" />
            </button>
            <div class="flex gap-1.5">
              <button
                v-for="p in periodOptions"
                :key="p.value"
                @click="listeningPeriod = p.value"
                :class="['px-3.5 py-1.5 rounded-xl text-xs font-medium transition-colors',
                  listeningPeriod === p.value ? 'bg-primary text-primary-foreground' : 'bg-foreground/[0.04] text-foreground/70 hover:text-foreground']">
                {{ p.label }}
              </button>
            </div>
          </div>
        </header>

        <div v-if="loading && !listening" class="flex items-center justify-center py-8">
          <div class="animate-spin rounded-full h-8 w-8 border-2 border-primary border-t-transparent" />
        </div>

        <template v-else-if="listening">
          <div class="grid gap-4 lg:grid-cols-8">
            <article class="relative flex min-h-[17rem] flex-col overflow-hidden rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-6 backdrop-blur-[30px] lg:col-span-6">
              <div class="relative mb-6 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Clock class="h-4 w-4" />Total time
                <span class="ml-auto inline-flex items-center gap-1 rounded-full border border-foreground/[0.08] bg-foreground/[0.05] px-2 py-1 tabular-nums text-foreground/70">
                  <Music class="h-3.5 w-3.5" />{{ formatNum(listening.plays) }} plays
                </span>
              </div>
              <div v-if="listening.seconds > 0" class="relative grid flex-1 grid-cols-1 items-stretch gap-4 md:grid-cols-[auto_minmax(0,1fr)] md:gap-6">
                <div class="flex items-baseline gap-3">
                  <p class="text-4xl font-bold tracking-[-0.03em]">{{ formatTime(listening.seconds) }}</p>
                  <p v-if="listening.change_percent !== null" class="text-xs tabular-nums text-[color:var(--text-muted)]">{{
                    listening.change_percent >= 0 ? '+' : '' }}{{ Math.round(listening.change_percent) }}%</p>
                </div>
                <ListeningActivityChart class="h-full w-full self-stretch" :activity="listening.activity" />
              </div>
              <div v-else class="relative flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
                <ChartColumn class="mb-2 h-6 w-6 opacity-60" />
                <p class="text-sm">No listening data yet</p>
              </div>
            </article>

            <article class="streak-card group relative flex min-h-[17rem] flex-col overflow-hidden rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-6 backdrop-blur-[30px] lg:col-span-2">
              <div aria-hidden="true" class="streak-glow pointer-events-none absolute -right-16 -bottom-16 h-52 w-52 rounded-full" />
              <Flame aria-hidden="true" class="streak-watermark pointer-events-none absolute -right-10 -bottom-8 h-40 w-40" stroke-width="0.25" fill="currentColor" />
              <div class="relative mb-6 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Flame class="h-4 w-4" />Streak
              </div>
              <div class="relative flex flex-1 flex-col justify-center">
                <p class="streak-value text-4xl font-bold tracking-[-0.03em]">{{ formatNum(listening.streak_days) }}</p>
                <p class="mt-2 text-xs text-[color:var(--text-muted)]">days</p>
              </div>
            </article>
          </div>

          <div class="grid gap-4 lg:grid-cols-8">
            <article class="flex min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px] lg:col-span-3">
              <div class="mb-4 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <ListMusic class="h-4 w-4" />Genre distribution
              </div>
              <div v-if="listening.genres?.length" class="flex flex-1 flex-col gap-5 md:flex-row md:items-center md:gap-6">
                <GenreDistributionChart :genres="sortedGenres" />
                <div class="min-w-0 flex-1 space-y-2.5">
                  <div v-for="genre in sortedGenres" :key="genre.is_other ? 'other' : genre.name"
                    class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-3 text-xs">
                    <span class="truncate text-[color:var(--text-muted)]">{{ genre.is_other ? 'Other' : genre.name }}</span>
                    <span class="text-right tabular-nums text-[color:var(--text-muted)]">{{ formatTime(genre.seconds) }}</span>
                    <span class="w-9 text-right tabular-nums text-[color:var(--text-muted)]">{{ totalGenres ?
                      Math.round(genre.seconds / totalGenres * 100) : 0 }}%</span>
                  </div>
                </div>
              </div>
              <div v-else class="flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
                <ListMusic class="mb-2 h-6 w-6 opacity-60" />
                <p class="text-sm">No genre data yet</p>
              </div>
            </article>

            <article class="flex min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px] lg:col-span-3">
              <div class="mb-4 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Music class="h-4 w-4" />Playback outcomes
              </div>
              <div v-if="(listening.completed + listening.skipped + listening.stopped) > 0" class="flex flex-1 flex-col items-center gap-5 md:flex-row md:gap-6">
                <PlaybackOutcomesChart :completed="listening.completed" :skipped="listening.skipped" :stopped="listening.stopped" />
                <div class="min-w-0 flex-1 space-y-2.5 text-xs">
                  <div v-for="item in sortedPlaybackOutcomes" :key="item.key" class="grid grid-cols-[minmax(0,1fr)_auto_auto] gap-x-3">
                    <span class="truncate capitalize text-[color:var(--text-muted)]">{{ item.key }}</span>
                    <span class="tabular-nums text-[color:var(--text-muted)]">{{ formatNum(item.value) }}</span>
                    <span class="w-9 text-right tabular-nums text-[color:var(--text-muted)]">{{ Math.round(item.value / Math.max(1, listening.completed + listening.skipped + listening.stopped) * 100) }}%</span>
                  </div>
                </div>
              </div>
              <div v-else class="flex flex-1 items-center justify-center text-sm text-[color:var(--text-muted)]">No playback outcomes yet</div>
              <p class="text-xs text-[color:var(--text-muted)] mt-4">
                Avg session {{ formatTime(listening.avg_session_secs) }} · Skip rate {{ Math.round(listening.skip_rate) }}%
              </p>
            </article>

            <article class="flex min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px] lg:col-span-2">
              <div class="flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Timer class="h-4 w-4" />Average session
              </div>
              <div class="flex flex-1 flex-col justify-center">
                <p class="text-3xl font-semibold tracking-[-0.03em]">{{ listening.avg_session_secs ? formatTime(listening.avg_session_secs) : '—' }}</p>
                <p class="mt-2 text-xs text-[color:var(--text-muted)]">per playback attempt</p>
              </div>
            </article>
          </div>

          <TopArtistsCarousel :artists="listening.top_artists" />

          <div class="grid gap-4">
            <article class="flex min-w-0 min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
              <h2 class="mb-4 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
                <Music class="h-4 w-4" />Top tracks
                <button v-if="listening.top_tracks.length > topTracksPreviewLimit" type="button" :aria-expanded="topTracksExpanded"
                  class="ml-auto inline-flex items-center gap-1 rounded-md px-2 py-1 text-[color:var(--text-muted)]"
                  @click="topTracksExpanded = !topTracksExpanded">
                  <span>{{ topTracksExpanded ? 'Show less' : 'Show more' }}</span>
                  <ChevronUp v-if="topTracksExpanded" class="h-3.5 w-3.5" aria-hidden="true" />
                  <ChevronDown v-else class="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </h2>
              <ol v-if="visibleTopTracks.length" class="min-w-0 space-y-1 max-h-[32rem] overflow-y-auto custom-scrollbar pr-1">
                <li v-for="(t, i) in visibleTopTracks" :key="t.track.id">
                  <button @click="playTopTrack(i)" class="w-full flex items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-foreground/[0.04]">
                    <span class="w-6 shrink-0 text-xs tabular-nums text-foreground/40">{{ i + 1 }}</span>
                    <span class="h-10 w-10 shrink-0 overflow-hidden rounded-lg bg-foreground/5 flex items-center justify-center">
                      <img v-if="trackArtworkUrls.get(t.track.id)" :src="trackArtworkUrls.get(t.track.id)" :alt="t.track.title"
                        class="h-full w-full object-cover" loading="lazy" decoding="async" />
                      <Music v-else class="h-4 w-4 text-foreground/30" />
                    </span>
                    <span class="flex-1 min-w-0">
                      <span class="block truncate text-sm font-medium">{{ t.track.title || 'Unknown' }}</span>
                      <span class="block truncate text-xs text-foreground/50">{{ t.track.artist || 'Unknown artist' }}</span>
                    </span>
                    <span class="hidden sm:block w-16 shrink-0 text-right text-xs tabular-nums text-foreground/50">{{ formatTime(t.seconds) }}</span>
                    <span class="w-10 shrink-0 text-right text-xs tabular-nums text-foreground/50">{{ t.plays }}×</span>
                  </button>
                </li>
              </ol>
              <div v-else class="flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
                <Music class="mb-2 h-6 w-6 opacity-60" />
                <p class="text-sm">No top tracks yet</p>
              </div>
            </article>
          </div>
        </template>
      </section>
    </div>
  </section>
</template>

<style scoped>
.streak-card {
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.streak-glow {
  background: radial-gradient(circle, rgb(251 146 60 / 0.28), rgb(244 63 94 / 0.1) 42%, transparent 70%);
  filter: blur(12px);
  opacity: 0.7;
  transform: scale(0.9);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.streak-watermark {
  color: rgb(251 146 60 / 0.12);
  transform: rotate(-12deg);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.streak-card:hover {
  border-color: rgb(251 146 60 / 0.3);
}

.streak-card:hover .streak-glow {
  opacity: 1;
  transform: scale(1.12);
}

.streak-card:hover .streak-watermark {
  color: rgb(253 186 116 / 0.2);
  transform: rotate(-5deg) scale(1.04);
}

.streak-card:hover .streak-value {
  text-shadow: 0 0 24px rgb(251 146 60 / 0.35);
}

@media (prefers-reduced-motion: reduce) {
  .streak-card,
  .streak-glow,
  .streak-watermark {
    transition: none;
  }
}
</style>
