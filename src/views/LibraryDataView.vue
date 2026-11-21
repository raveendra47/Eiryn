<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { Search, Database, ListMusic, FileText, Eye, EyeOff, Activity, AlertTriangle, ShieldAlert, CheckCircle2 } from '@lucide/vue'
import { listen } from '@tauri-apps/api/event'
import * as api from '../lib/invoke'
import type { LyricRow } from '../lib/invoke'

type Tab = 'tracks' | 'genres' | 'lyrics' | 'health'

const tabs: { key: Tab; label: string; icon: any }[] = [
  { key: 'tracks', label: 'Tracks', icon: Database },
  { key: 'genres', label: 'Genres', icon: ListMusic },
  { key: 'lyrics', label: 'Lyrics', icon: FileText },
  { key: 'health', label: 'Health & Forensics', icon: Activity },
]

const activeTab = ref<Tab>('tracks')
const searchQuery = ref('')
const isLoading = ref(false)

const allTracks = ref<api.Track[]>([])
const allGenres = ref<api.Genre[]>([])
const allLyrics = ref<LyricRow[]>([])
const healthSummary = ref<api.HealthSummary | null>(null)
const suspectTracks = ref<api.SuspectTrackItem[]>([])
const damagedTracks = ref<api.DamagedTrackItem[]>([])

let unlisten: (() => void) | null = null

function foldUnicode(s: string): string {
  return s.toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '')
}

function matchesQuery(row: Record<string, any>, q: string): boolean {
  for (const val of Object.values(row)) {
    if (val != null && foldUnicode(String(val)).includes(q)) return true
  }
  return false
}

const filteredTracks = computed(() => {
  if (!searchQuery.value) return allTracks.value
  const q = foldUnicode(searchQuery.value)
  return allTracks.value.filter(t => matchesQuery(t as any, q))
})

const filteredGenres = computed(() => {
  if (!searchQuery.value) return allGenres.value
  const q = foldUnicode(searchQuery.value)
  return allGenres.value.filter(g => matchesQuery(g as any, q))
})

const filteredLyrics = computed(() => {
  if (!searchQuery.value) return allLyrics.value
  const q = foldUnicode(searchQuery.value)
  return allLyrics.value.filter(l => matchesQuery(l as any, q))
})

const filteredSuspectTracks = computed(() => {
  if (!searchQuery.value) return suspectTracks.value
  const q = foldUnicode(searchQuery.value)
  return suspectTracks.value.filter(t => matchesQuery(t as any, q))
})

const filteredDamagedTracks = computed(() => {
  if (!searchQuery.value) return damagedTracks.value
  const q = foldUnicode(searchQuery.value)
  return damagedTracks.value.filter(t => matchesQuery(t as any, q))
})

// Sorting
const sortColumn = ref<string | null>(null)
const sortDir = ref<'asc' | 'desc'>('asc')

function toggleSort(col: string) {
  if (sortColumn.value === col) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortColumn.value = col
    sortDir.value = 'asc'
  }
}

function sortRows<T extends Record<string, any>>(rows: T[]): T[] {
  if (!sortColumn.value) return rows
  const col = sortColumn.value
  const dir = sortDir.value === 'asc' ? 1 : -1
  return [...rows].sort((a, b) => {
    const va = a[col]
    const vb = b[col]
    if (va == null && vb == null) return 0
    if (va == null) return 1
    if (vb == null) return -1
    if (typeof va === 'string' && typeof vb === 'string') {
      return va.localeCompare(vb) * dir
    }
    return (va < vb ? -1 : va > vb ? 1 : 0) * dir
  })
}

const sortedTracks = computed(() => {
  const rows = filteredTracks.value
  if (!sortColumn.value) return rows
  return sortRows(rows)
})

const sortedGenres = computed(() => {
  const rows = filteredGenres.value
  if (!sortColumn.value) return rows
  return sortRows(rows)
})

const sortedLyrics = computed(() => {
  const rows = filteredLyrics.value
  if (!sortColumn.value) return rows
  return sortRows(rows)
})

// --- Column definitions ---
type TrackKey = keyof api.Track

interface ColumnDef {
  key: TrackKey
  label: string
  group: string
  cls?: string
  fmt?: (v: any, t: api.Track) => string
  badge?: (v: any, t: api.Track) => { text: string; cls: string } | null
}

const columns: ColumnDef[] = [
  { key: 'title', label: 'Title', group: 'Info', cls: 'whitespace-nowrap' },
  { key: 'artist', label: 'Artist', group: 'Info', cls: 'whitespace-nowrap max-w-[160px] truncate' },
  { key: 'album', label: 'Album', group: 'Info', cls: 'whitespace-nowrap max-w-[160px] truncate' },
  { key: 'album_artist', label: 'Album Artist', group: 'Info', cls: 'whitespace-nowrap max-w-[140px] truncate' },
  { key: 'composer', label: 'Composer', group: 'Info', cls: 'whitespace-nowrap max-w-[140px] truncate' },

  { key: 'track_number', label: '#', group: 'Track', cls: 'tabular-nums text-center', fmt: v => v || '—' },
  { key: 'disc_number', label: 'Disc', group: 'Track', cls: 'tabular-nums text-center', fmt: v => v || '—' },
  { key: 'year', label: 'Year', group: 'Track', cls: 'tabular-nums', fmt: v => v || '—' },
  { key: 'genre', label: 'Genre', group: 'Track', cls: 'whitespace-nowrap', fmt: v => v || '—' },
  { key: 'raw_genre_names', label: 'Raw Genres', group: 'Track', cls: 'text-foreground/60 max-w-[120px] truncate', fmt: v => v || '—' },

  { key: 'duration', label: 'Duration', group: 'Audio', cls: 'tabular-nums text-right', fmt: v => `${Math.floor(v / 60)}:${String(Math.floor(v % 60)).padStart(2, '0')}` },
  { key: 'sample_rate', label: 'Sample Rate', group: 'Audio', cls: 'tabular-nums text-right', fmt: v => v ? `${(v / 1000).toFixed(1)}k` : '—' },
  { key: 'bit_depth', label: 'Bit Depth', group: 'Audio', cls: 'tabular-nums text-right', fmt: v => v ? `${v}-bit` : '—' },
  { key: 'bitrate', label: 'Bitrate', group: 'Audio', cls: 'tabular-nums text-right', fmt: v => v ? `${(v / 1000).toFixed(0)}k` : '—' },
  { key: 'file_size', label: 'Size', group: 'Audio', cls: 'tabular-nums text-right', fmt: v => v ? `${(v / 1048576).toFixed(1)} MB` : '—' },
  { key: 'file_format', label: 'Format', group: 'Audio', cls: 'whitespace-nowrap', fmt: v => v || '—' },
  { key: 'has_artwork', label: 'Art', group: 'Audio', badge: v => v ? { text: 'Yes', cls: 'bg-green-500/20 text-green-400' } : { text: 'No', cls: 'bg-foreground/10 text-foreground/40' } },

  { key: 'bpm', label: 'BPM', group: 'Analysis', cls: 'tabular-nums text-right', fmt: v => v > 0 ? v.toFixed(1) : '—' },
  { key: 'musical_key', label: 'Key', group: 'Analysis', fmt: v => v || '—' },
  { key: 'energy', label: 'Energy', group: 'Analysis', cls: 'tabular-nums text-right', fmt: v => v > 0 ? v.toFixed(2) : '—' },
  { key: 'analysis_failed_reason', label: 'Status', group: 'Analysis', badge: (v, t) =>
    v ? { text: v === 'file_missing' ? 'Missing' : 'Error', cls: 'bg-red-500/20 text-red-400' }
    : t.bpm > 0 ? { text: 'Analyzed', cls: 'bg-green-500/20 text-green-400' }
    : { text: 'Pending', cls: 'bg-yellow-500/20 text-yellow-400' } },

  { key: 'id', label: 'ID', group: 'Internal', cls: 'font-mono text-foreground/50 max-w-[80px] truncate', fmt: v => v ? v.slice(0, 8) + '…' : '—' },
  { key: 'file_path', label: 'File', group: 'Internal', cls: 'font-mono text-foreground/60 max-w-[200px] truncate', fmt: (v, t) => v ? v.replace(t.title, '…') : '—' },
  { key: 'artwork_key', label: 'Artwork Key', group: 'Internal', cls: 'font-mono text-foreground/50 max-w-[70px] truncate', fmt: v => v ? v.slice(0, 6) + '…' : '—' },
  { key: 'cue_parent_id', label: 'CUE Parent', group: 'Internal', cls: 'font-mono text-foreground/50 max-w-[70px] truncate', fmt: v => v ? v.slice(0, 6) + '…' : '—' },
  { key: 'cue_offset', label: 'CUE Offset', group: 'Internal', cls: 'tabular-nums text-right', fmt: v => v || '—' },
  { key: 'file_modified', label: 'Modified', group: 'Internal', cls: 'tabular-nums', fmt: v => v ? new Date(v * 1000).toLocaleDateString() : '—' },
  { key: 'replaygain_track_gain', label: 'RG Track', group: 'Internal', cls: 'tabular-nums text-right', fmt: v => v ? v.toFixed(1) + 'dB' : '—' },
  { key: 'replaygain_album_gain', label: 'RG Album', group: 'Internal', cls: 'tabular-nums text-right', fmt: v => v ? v.toFixed(1) + 'dB' : '—' },
]

const groups = ['Info', 'Track', 'Audio', 'Analysis', 'Internal']

const visibleGroups = ref<Record<string, boolean>>({
  Info: true, Track: true, Audio: true, Analysis: true, Internal: false,
})

const visibleColumns = computed(() => columns.filter(c => visibleGroups.value[c.group]))

function toggleGroup(g: string) {
  visibleGroups.value[g] = !visibleGroups.value[g]
}

const groupInfo = computed(() => {
  const vis = visibleColumns.value
  return groups.filter(g => visibleGroups.value[g]).map(g => ({
    label: g,
    colspan: vis.filter(c => c.group === g).length,
  }))
})

// Genres/Lyrics column defs
const genreColumns = [
  { key: 'id', label: 'ID', cls: 'font-mono text-foreground/50 max-w-[100px] truncate' },
  { key: 'name', label: 'Name', cls: 'whitespace-nowrap' },
  { key: 'normalization_key', label: 'Norm Key', cls: 'font-mono text-foreground/60' },
  { key: 'track_count', label: 'Tracks', cls: 'tabular-nums' },
]

const lyricColumns = [
  { key: 'track_id', label: 'Track ID', cls: 'font-mono text-foreground/50 max-w-[100px] truncate' },
  { key: 'content', label: 'Content', cls: 'max-w-[300px] truncate text-foreground/80' },
  { key: 'source', label: 'Source', cls: 'text-foreground/60' },
  { key: 'meta_content', label: 'Meta Content', cls: 'max-w-[200px] truncate text-foreground/60' },
  { key: 'meta_source', label: 'Meta Source', cls: 'text-foreground/60' },
]

// Track row bg
function rowCls(t: api.Track): string {
  if (t.analysis_failed_reason) return 'bg-red-500/5'
  if (!t.bpm && !t.musical_key) return 'bg-yellow-500/[0.03]'
  return ''
}

function val(t: api.Track, key: string): any {
  return (t as any)[key]
}

async function loadAll() {
  isLoading.value = true
  try {
    const [tracks, genres, lyrics, summary, suspects, damaged] = await Promise.all([
      api.getTracks(),
      api.getAllGenres(),
      api.getAllLyrics(),
      api.getLibraryHealthSummary().catch(() => null),
      api.getSuspectTracks(200).catch(() => []),
      api.getDamagedTracks().catch(() => []),
    ])
    allTracks.value = tracks
    allGenres.value = genres
    allLyrics.value = lyrics
    healthSummary.value = summary
    suspectTracks.value = suspects
    damagedTracks.value = damaged
  } catch (e) {
    console.error('Failed to load library data:', e)
  }
  isLoading.value = false
}

onMounted(async () => {
  await loadAll()
  const unsub = await listen('library:sync-finished', () => loadAll())
  unlisten = unsub
})

onUnmounted(() => {
  unlisten?.()
})
</script>

<template>
  <div class="h-full flex flex-col overflow-hidden bg-background">
    <!-- Header -->
    <div class="px-6 py-5 border-b border-foreground/[0.06] select-none bg-background/60 backdrop-blur-sm sticky top-0 z-10">
      <div class="flex items-center justify-between gap-4">
        <h1 class="text-2xl font-bold">Library Data</h1>
        <div class="flex items-center gap-3">
          <div class="relative w-52">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-foreground opacity-40 pointer-events-none" />
            <input v-model="searchQuery" type="text" placeholder="Filter rows..."
              class="w-full pl-9 pr-4 py-2 text-sm rounded-lg bg-foreground/[0.05] ring-1 ring-foreground/10 text-foreground placeholder:text-foreground/40 outline-none focus:ring-primary/40 transition-all" />
          </div>
          <span class="text-xs text-foreground opacity-50 tabular-nums">{{ activeTab === 'tracks' ? filteredTracks.length : activeTab === 'genres' ? filteredGenres.length : activeTab === 'lyrics' ? filteredLyrics.length : (filteredSuspectTracks.length + filteredDamagedTracks.length) }} rows</span>
        </div>
      </div>
      <!-- Tabs -->
      <div class="flex gap-1 mt-4">
        <button v-for="tab in tabs" :key="tab.key" @click="activeTab = tab.key"
          class="flex items-center gap-2 px-4 py-2 text-sm rounded-lg transition-all"
          :class="activeTab === tab.key ? 'bg-primary/15 text-primary font-semibold' : 'text-foreground/60 hover:text-foreground/90 hover:bg-foreground/5'">
          <component :is="tab.icon" class="w-4 h-4" />
          {{ tab.label }}
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-y-auto">
      <!-- Loading -->
      <div v-if="isLoading" class="flex items-center justify-center h-32">
        <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin" />
      </div>

      <!-- Tracks Table -->
      <div v-else-if="activeTab === 'tracks' && sortedTracks.length" class="overflow-auto">
        <!-- Column group toggles -->
        <div class="flex gap-2 px-4 pt-3 pb-2 border-b border-foreground/[0.04] bg-background/40">
          <button v-for="g in groups" :key="g" @click="toggleGroup(g)"
            class="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md transition-all"
            :class="visibleGroups[g] ? 'bg-primary/15 text-primary font-medium' : 'bg-foreground/[0.04] text-foreground/50 hover:text-foreground/80'">
            <component :is="visibleGroups[g] ? Eye : EyeOff" class="w-3.5 h-3.5" />
            {{ g }}
          </button>
        </div>

        <table class="min-w-max text-xs border-collapse">
          <thead class="sticky top-0 z-10 bg-background/95 backdrop-blur-sm">
            <!-- Group header row -->
            <tr v-if="groupInfo.length" class="border-b border-foreground/[0.12]">
              <th v-for="g in groupInfo" :key="g.label" :colspan="g.colspan"
                class="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-widest text-foreground/30 text-left">
                {{ g.label }}
              </th>
            </tr>
            <!-- Column header row -->
            <tr class="border-b border-foreground/[0.08]">
              <th v-for="col in visibleColumns" :key="col.key" @click="toggleSort(col.key)"
                class="px-2 py-2 font-medium text-foreground/50 whitespace-nowrap cursor-pointer hover:text-foreground select-none transition-colors text-[11px]"
                :class="sortColumn === col.key ? 'text-primary' : ''">
                {{ col.label }}<span v-if="sortColumn === col.key" class="ml-0.5">{{ sortDir === 'asc' ? '▲' : '▼' }}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(t, i) in sortedTracks" :key="t.id"
              class="border-b border-foreground/[0.03] transition-colors"
              :class="[rowCls(t), i % 2 === 0 ? '' : 'bg-foreground/[0.01]', 'hover:bg-foreground/[0.03]']">
              <td v-for="col in visibleColumns" :key="col.key"
                class="px-2 py-1.5"
                :class="col.cls || ''">
                <span v-if="col.badge" class="inline-block px-2 py-0.5 rounded-full text-[10px] font-medium"
                  :class="col.badge(val(t, col.key), t)?.cls || ''">
                  {{ col.badge(val(t, col.key), t)?.text }}
                </span>
                <span v-else-if="col.fmt" :title="String(val(t, col.key) ?? '')">
                  {{ col.fmt(val(t, col.key), t) }}
                </span>
                <span v-else :title="String(val(t, col.key) ?? '')" class="whitespace-nowrap">
                  {{ val(t, col.key) ?? '—' }}
                </span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- Genres Table -->
      <div v-else-if="activeTab === 'genres' && sortedGenres.length">
        <table class="w-full text-xs border-collapse">
          <thead class="sticky top-0 z-10 bg-background/95 backdrop-blur-sm">
            <tr class="border-b border-foreground/[0.08]">
              <th v-for="col in genreColumns" :key="col.key" @click="toggleSort(col.key)"
                class="text-left px-4 py-2 font-medium text-foreground/50 whitespace-nowrap cursor-pointer hover:text-foreground select-none text-[11px]"
                :class="sortColumn === col.key ? 'text-primary' : ''">
                {{ col.label }}<span v-if="sortColumn === col.key" class="ml-0.5">{{ sortDir === 'asc' ? '▲' : '▼' }}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(g, i) in sortedGenres" :key="g.id"
              class="border-b border-foreground/[0.03] transition-colors"
              :class="[i % 2 === 0 ? '' : 'bg-foreground/[0.01]', 'hover:bg-foreground/[0.03]']">
              <td v-for="col in genreColumns" :key="col.key" :class="col.cls || ''" class="px-4 py-1.5">
                {{ g[col.key as keyof typeof g] ?? '—' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- Lyrics Table -->
      <div v-else-if="activeTab === 'lyrics' && sortedLyrics.length">
        <table class="w-full text-xs border-collapse">
          <thead class="sticky top-0 z-10 bg-background/95 backdrop-blur-sm">
            <tr class="border-b border-foreground/[0.08]">
              <th v-for="col in lyricColumns" :key="col.key" @click="toggleSort(col.key)"
                class="text-left px-4 py-2 font-medium text-foreground/50 whitespace-nowrap cursor-pointer hover:text-foreground select-none text-[11px]"
                :class="sortColumn === col.key ? 'text-primary' : ''">
                {{ col.label }}<span v-if="sortColumn === col.key" class="ml-0.5">{{ sortDir === 'asc' ? '▲' : '▼' }}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(l, i) in sortedLyrics" :key="l.track_id"
              class="border-b border-foreground/[0.03] transition-colors"
              :class="[i % 2 === 0 ? '' : 'bg-foreground/[0.01]', 'hover:bg-foreground/[0.03]']">
              <td v-for="col in lyricColumns" :key="col.key" :class="col.cls || ''" class="px-4 py-1.5">
                {{ l[col.key as keyof LyricRow] ?? '—' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- Health & Forensics Tab -->
      <div v-else-if="activeTab === 'health'" class="p-6 space-y-6 overflow-y-auto">
        <!-- Overview summary cards -->
        <div v-if="healthSummary" class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3">
          <div class="p-3.5 rounded-xl bg-foreground/[0.03] border border-foreground/[0.08]">
            <div class="text-[11px] font-medium text-foreground/50 uppercase tracking-wider">Analyzed</div>
            <div class="text-xl font-bold mt-1 text-foreground tabular-nums">{{ healthSummary.total_analyzed }}</div>
          </div>
          <div class="p-3.5 rounded-xl bg-green-500/10 border border-green-500/20">
            <div class="text-[11px] font-medium text-green-400 uppercase tracking-wider">Good Quality</div>
            <div class="text-xl font-bold mt-1 text-green-400 tabular-nums">{{ healthSummary.good_count }}</div>
          </div>
          <div class="p-3.5 rounded-xl bg-amber-500/10 border border-amber-500/20">
            <div class="text-[11px] font-medium text-amber-400 uppercase tracking-wider">Fake 24-bit</div>
            <div class="text-xl font-bold mt-1 text-amber-400 tabular-nums">{{ healthSummary.fake_24_count }}</div>
          </div>
          <div class="p-3.5 rounded-xl bg-yellow-500/10 border border-yellow-500/20">
            <div class="text-[11px] font-medium text-yellow-400 uppercase tracking-wider">Upsampled / Lossy</div>
            <div class="text-xl font-bold mt-1 text-yellow-400 tabular-nums">{{ healthSummary.upsampled_count + healthSummary.lossy_count }}</div>
          </div>
          <div class="p-3.5 rounded-xl bg-red-500/10 border border-red-500/20">
            <div class="text-[11px] font-medium text-red-400 uppercase tracking-wider">Damaged / FLAC MD5</div>
            <div class="text-xl font-bold mt-1 text-red-400 tabular-nums">{{ healthSummary.damaged_count }}</div>
          </div>
          <div class="p-3.5 rounded-xl bg-purple-500/10 border border-purple-500/20">
            <div class="text-[11px] font-medium text-purple-400 uppercase tracking-wider">Duplicates</div>
            <div class="text-xl font-bold mt-1 text-purple-400 tabular-nums">{{ healthSummary.duplicate_groups_count }}</div>
          </div>
        </div>

        <!-- Damaged Tracks (FLAC checksum errors, truncated files) -->
        <div v-if="filteredDamagedTracks.length" class="space-y-3">
          <div class="flex items-center gap-2 text-sm font-semibold text-red-400">
            <ShieldAlert class="w-4 h-4" />
            <span>Damaged Tracks (Bitrot & Checksum Mismatch)</span>
          </div>
          <div class="border border-red-500/30 rounded-xl overflow-hidden bg-red-500/5">
            <table class="w-full text-xs border-collapse">
              <thead>
                <tr class="border-b border-red-500/20 bg-red-500/10 text-foreground/60 text-left">
                  <th class="px-4 py-2 font-medium">Title</th>
                  <th class="px-4 py-2 font-medium">Artist</th>
                  <th class="px-4 py-2 font-medium">Format</th>
                  <th class="px-4 py-2 font-medium">Error Details</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="t in filteredDamagedTracks" :key="t.id" class="border-b border-red-500/10 hover:bg-red-500/10">
                  <td class="px-4 py-2 font-medium text-foreground">{{ t.title }}</td>
                  <td class="px-4 py-2 text-foreground/70">{{ t.artist }}</td>
                  <td class="px-4 py-2 text-foreground/60 uppercase">{{ t.file_format }}</td>
                  <td class="px-4 py-2 text-red-400 font-mono text-[11px]">{{ t.error }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <!-- Suspect & Fake 24-bit Tracks -->
        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 text-sm font-semibold text-foreground">
              <AlertTriangle class="w-4 h-4 text-amber-400" />
              <span>Suspect & Fake Hi-Res Tracks</span>
            </div>
          </div>
          <div v-if="filteredSuspectTracks.length" class="border border-foreground/10 rounded-xl overflow-hidden bg-foreground/[0.02]">
            <table class="w-full text-xs border-collapse">
              <thead>
                <tr class="border-b border-foreground/10 bg-foreground/[0.04] text-foreground/60 text-left">
                  <th class="px-4 py-2 font-medium">Title</th>
                  <th class="px-4 py-2 font-medium">Artist</th>
                  <th class="px-4 py-2 font-medium">Format</th>
                  <th class="px-4 py-2 font-medium">Audio Specs</th>
                  <th class="px-4 py-2 font-medium">Cutoff Frequency</th>
                  <th class="px-4 py-2 font-medium">Forensic Verdict</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="t in filteredSuspectTracks" :key="t.id" class="border-b border-foreground/[0.04] hover:bg-foreground/[0.03]">
                  <td class="px-4 py-2 font-medium text-foreground">{{ t.title }}</td>
                  <td class="px-4 py-2 text-foreground/70">{{ t.artist }}</td>
                  <td class="px-4 py-2 text-foreground/60 uppercase">{{ t.file_format }}</td>
                  <td class="px-4 py-2 tabular-nums">{{ (t.sample_rate / 1000).toFixed(1) }} kHz / {{ t.bit_depth }}-bit</td>
                  <td class="px-4 py-2 tabular-nums">{{ t.cutoff_hz ? `${(t.cutoff_hz / 1000).toFixed(1)} kHz` : '—' }}</td>
                  <td class="px-4 py-2">
                    <span v-if="t.real_bits" class="inline-block px-2 py-0.5 rounded-full text-[10px] font-medium bg-amber-500/20 text-amber-400 border border-amber-500/30">
                      Fake {{ t.bit_depth }}-bit (Actual {{ t.real_bits }}-bit)
                    </span>
                    <span v-else-if="t.verdict === 'lossy'" class="inline-block px-2 py-0.5 rounded-full text-[10px] font-medium bg-red-500/20 text-red-400 border border-red-500/30">
                      Lossy Cutoff
                    </span>
                    <span v-else-if="t.verdict === 'upsampled'" class="inline-block px-2 py-0.5 rounded-full text-[10px] font-medium bg-yellow-500/20 text-yellow-400 border border-yellow-500/30">
                      Upsampled CD Master
                    </span>
                    <span v-else class="inline-block px-2 py-0.5 rounded-full text-[10px] font-medium bg-foreground/10 text-foreground/60">
                      {{ t.verdict }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else class="p-8 text-center text-foreground/40 text-xs border border-dashed border-foreground/10 rounded-xl">
            No suspect tracks detected in your analyzed library.
          </div>
        </div>
      </div>

      <!-- Empty -->
      <div v-else class="flex flex-col items-center justify-center h-32 text-foreground/40">
        <p>{{ isLoading ? '' : 'No data found.' }}</p>
      </div>
    </div>
  </div>
</template>
