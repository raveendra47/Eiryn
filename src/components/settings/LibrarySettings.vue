<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, reactive } from 'vue'
import { RotateCcw, Plus, Trash2, Folder, FolderOpen, RefreshCw, FileText, Loader2, Activity, Image, Radio, CheckCircle, XCircle, Minus, Download, Upload, AlertTriangle, Archive } from '@lucide/vue'
import { useLibraryStore } from '../../stores/library'
import { useAppStore } from '../../stores/app'
import { useAnalysisStore } from '../../stores/analysis'
import { listen } from '@tauri-apps/api/event'
import * as api from '../../lib/invoke'
import ConfirmDialog from '../ConfirmDialog.vue'

const library = useLibraryStore()
const appStore = useAppStore()
const analysis = useAnalysisStore()

const folders = ref<string[]>([])
const scanning = ref(false)
const scanResult = ref<string | null>(null)
const folderToDelete = ref<string | null>(null)
const isLoading = ref(true)

const batchFetching = ref(false)
const fetchingAll = ref(false)
const activeFolder = ref<string | null>(null)
const batchCurrent = ref(0)
const batchTotal = ref(0)
const folderResults = reactive<Record<string, string>>({})
const addingFolders = ref(false)
const folderScanStatus = reactive<Record<string, string>>({})
const scanCurrent = ref(0)
const scanTotal = ref(0)

const artistResults = reactive<Record<string, string>>({})
const analysisFileStatus = reactive<Record<string, string>>({})
const genreResults = reactive<Record<string, string>>({})
const analysisResult = ref<string | null>(null)
const artworkResult = ref<string | null>(null)
const genreResult = ref<string | null>(null)
const lyricsSummary = ref<string | null>(null)

const backupLoading = ref(false)
const backupStatus = ref<string | null>(null)
const backupSuccess = ref(true)

async function exportBackup() {
  try {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const path = await save({
      filters: [{ name: 'Eiryn Backup Archive', extensions: ['zip'] }],
      defaultPath: `eiryn-backup-${new Date().toISOString().slice(0, 10)}.zip`,
    })
    if (!path) return
    backupLoading.value = true
    backupStatus.value = 'Creating library backup archive...'
    const summary = await api.exportLibraryBackup(path)
    backupSuccess.value = true
    backupStatus.value = `Exported ${summary.playlist_count} playlists, ${summary.favorite_count} favorites, ${summary.eq_profile_count} EQ profiles (${(summary.file_size_bytes / 1024).toFixed(1)} KB)`
  } catch (e: any) {
    backupSuccess.value = false
    backupStatus.value = `Backup failed: ${e}`
  } finally {
    backupLoading.value = false
  }
}

async function restoreBackup() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Eiryn Backup Archive', extensions: ['zip'] }],
    })
    if (!selected || typeof selected !== 'string') return
    backupLoading.value = true
    backupStatus.value = 'Restoring library backup...'
    const summary = await api.restoreLibraryBackup(selected)
    backupSuccess.value = true
    backupStatus.value = `Restored ${summary.restored_playlists} playlists, ${summary.restored_favorites} favorites, ${summary.restored_lyrics} lyrics (relinked ${summary.relinked_tracks} tracks)`
  } catch (e: any) {
    backupSuccess.value = false
    backupStatus.value = `Restore failed: ${e}`
  } finally {
    backupLoading.value = false
  }
}

const unanalyzedCount = computed(() => library.tracks.filter(t => t.bpm === 0 && t.duration >= 30 && !t.cue_parent_id && !t.analysis_failed_reason).length)

const scanProgress = computed(() =>
  scanTotal.value > 0 ? Math.round((scanCurrent.value / scanTotal.value) * 100) : 0
)
let unlistenProgress: (() => void) | null = null
let unlistenFolderDone: (() => void) | null = null
let unlistenAllDone: (() => void) | null = null
let unlistenAnalysisProgress: (() => void) | null = null
let unlistenAnalysisDone: (() => void) | null = null
let unlistenArtworkProgress: (() => void) | null = null
let unlistenArtworkDone: (() => void) | null = null
let unlistenGenreProgress: (() => void) | null = null
let unlistenGenreDone: (() => void) | null = null
let unlistenFolderScan: (() => void) | null = null
let unlistenFullAnalysisProgress: (() => void) | null = null
let unlistenFullAnalysisDone: (() => void) | null = null

const fullAnalyzing = ref(false)
const fullDone = ref(0)
const fullFailed = ref(0)
const fullResult = ref<string | null>(null)

const batchProgress = computed(() =>
  batchTotal.value > 0 ? Math.round((batchCurrent.value / batchTotal.value) * 100) : 0
)

const analysisProgress = computed(() =>
  analysis.analysisTotal > 0 ? Math.round((analysis.analysisCurrent / analysis.analysisTotal) * 100) : 0
)

onMounted(async () => {
  try {
    unlistenProgress = await listen<{ current: number; total: number; track: string; status: string; folder: string }>('batch-lyrics-progress', (event) => {
      const p = event.payload
      batchCurrent.value = p.current
      batchTotal.value = p.total
      activeFolder.value = p.folder
      analysis.fetchingLyrics = true
      analysis.lyricsCurrent = p.current
      analysis.lyricsTotal = p.total
    })
    unlistenFolderDone = await listen<{ folder: string; fetched: number; skipped_already_have: number; skipped_not_found: number; total: number }>('batch-lyrics-folder-done', (event) => {
      const p = event.payload
      if (p.fetched > 0) {
        let msg = `Fetched ${p.fetched} lyrics`
        if (p.skipped_not_found > 0 || p.skipped_already_have > 0) {
          msg += ` (${p.skipped_already_have} had, ${p.skipped_not_found} not found)`
        }
        folderResults[p.folder] = msg
      } else if (p.skipped_already_have === p.total) {
        folderResults[p.folder] = 'All lyrics already fetched'
      } else if (p.skipped_not_found > 0 && p.skipped_already_have > 0) {
        folderResults[p.folder] = `No lyrics found for ${p.skipped_not_found} tracks (${p.skipped_already_have} already have)`
      } else if (p.skipped_not_found > 0) {
        folderResults[p.folder] = `No lyrics found for ${p.skipped_not_found} tracks`
      } else {
        folderResults[p.folder] = 'No new lyrics found'
      }
    })
    unlistenAllDone = await listen('batch-lyrics-all-done', () => {
      batchFetching.value = false
      fetchingAll.value = false
      activeFolder.value = null
      batchCurrent.value = 0
      batchTotal.value = 0
      analysis.fetchingLyrics = false
      const fetched = Object.values(folderResults).filter(r => r.startsWith('Fetched')).length
      lyricsSummary.value = fetched > 0 ? `Lyrics fetched for all folders` : 'No new lyrics found'
    })
    unlistenFolderScan = await listen<{ path: string; status: string; added?: number; error?: string }>('folder-scan-progress', (event) => {
      const p = event.payload
      if (p.status === 'scanning') {
        folderScanStatus[p.path] = 'scanning'
      } else if (p.status === 'done') {
        folderScanStatus[p.path] = `done:${p.added ?? 0}`
      } else if (p.status === 'error') {
        folderScanStatus[p.path] = `error:${p.error ?? 'unknown'}`
      }
    })
    unlistenAnalysisProgress = await listen<{ processed: number; total: number; current: string; status?: string }>('audio-analysis-progress', (event) => {
      const p = event.payload
      analysis.analyzing = true
      analysis.analysisCurrent = p.processed
      analysis.analysisTotal = p.total
      analysis.analysisFile = p.current
      if (p.current) {
        analysisFileStatus[p.current] = p.status ?? 'analyzed'
      }
    })
    unlistenAnalysisDone = await listen<{ analyzed: number }>('audio-analysis-done', async (event) => {
      analysis.analyzing = false
      const a = event.payload.analyzed
      if (a > 0) analysisResult.value = `Analyzed ${a} tracks`
      await library.loadAll()
    })
    unlistenFullAnalysisProgress = await listen<{ done: number; failed: number }>('full-analysis-progress', (event) => {
      fullAnalyzing.value = true
      fullDone.value = event.payload.done
      fullFailed.value = event.payload.failed
    })
    unlistenFullAnalysisDone = await listen<{ done: number; failed: number }>('full-analysis-done', async (event) => {
      fullAnalyzing.value = false
      const { done, failed } = event.payload
      fullResult.value = failed > 0
        ? `Spectrum analysis: ${done} done, ${failed} failed`
        : `Spectrum analysis: ${done} tracks`
      await library.loadAll()
    })
    unlistenArtworkProgress = await listen<{ artist?: string; status?: string; processed?: number; total?: number }>('artist-artwork-progress', (event) => {
      const p = event.payload
      analysis.fetchingArtwork = true
      if (p.artist && p.status) {
        artistResults[p.artist] = p.status
      }
      if (p.processed !== undefined) {
        analysis.artworkCurrent = p.processed
        analysis.artworkTotal = p.total ?? 0
      }
    })
    unlistenArtworkDone = await listen<{ processed: number }>('artist-artwork-done', async (event) => {
      analysis.fetchingArtwork = false
      const p = event.payload.processed
      if (p > 0) artworkResult.value = `Fetched artwork for ${p} artists`
      await library.loadAll()
    })
    unlistenGenreProgress = await listen<{ current: number; total: number; track: string; artist: string; status: string; genre?: string }>('batch-genre-progress', (event) => {
      const p = event.payload
      analysis.fetchingGenre = true
      analysis.genreCurrent = p.current
      analysis.genreTotal = p.total
      analysis.genreTrack = p.track
      analysis.genreArtist = p.artist
      const key = `${p.track} — ${p.artist}`
      if (p.status === 'found') genreResults[key] = `✓ ${p.genre}`
      else if (p.status === 'not_found') genreResults[key] = 'Not found'
      else genreResults[key] = 'Searching...'
    })
    unlistenGenreDone = await listen<{ fetched: number; skipped: number; error?: string }>('batch-genre-done', async (event) => {
      analysis.fetchingGenre = false
      const p = event.payload
      if (p.error === 'no_api_key') {
        genreResult.value = 'Set a Last.fm API key to enable genre enrichment'
        await library.loadAll()
        return
      }
      const f = p.fetched
      const s = p.skipped
      if (f > 0) genreResult.value = `Fetched ${f} genres${s > 0 ? ` (${s} not found)` : ''}`
      await library.loadAll()
    })
  } catch {}
})

async function startFullAnalysis() {
  fullResult.value = null
  fullDone.value = 0
  fullFailed.value = 0
  fullAnalyzing.value = true
  try {
    await api.runFullAnalysisBackfill()
  } catch (e: any) {
    fullAnalyzing.value = false
    fullResult.value = typeof e === 'string' ? e : 'Spectrum analysis failed to start'
    console.error('Full analysis failed:', e)
  }
}

async function startAnalysis() {
  analysisResult.value = null
  Object.keys(analysisFileStatus).forEach(k => delete analysisFileStatus[k])
  analysis.analyzing = true
  analysis.analysisCurrent = 0
  analysis.analysisTotal = unanalyzedCount.value
  try {
    const result = await api.analyzeAudio()
    if (result === 'nothing_to_do' || result === 'already_running') {
      analysis.analyzing = false
    }
  } catch (e: any) {
    analysis.analyzing = false
    console.error('Analysis failed:', e)
  }
}

async function startArtworkFetch(retry = false) {
  artworkResult.value = null
  Object.keys(artistResults).forEach(k => delete artistResults[k])
  analysis.fetchingArtwork = true
  try {
    await api.batchFetchArtistArtwork(retry)
  } catch (e: any) {
    analysis.fetchingArtwork = false
    console.error('Artwork fetch failed:', e)
  }
}

async function startGenreFetch(retry = false) {
  genreResult.value = null
  Object.keys(genreResults).forEach(k => delete genreResults[k])
  analysis.fetchingGenre = true
  try {
    await api.batchFetchGenre(retry)
  } catch (e: any) {
    analysis.fetchingGenre = false
    console.error('Genre fetch failed:', e)
  }
}

onUnmounted(() => {
  unlistenProgress?.()
  unlistenFolderDone?.()
  unlistenAllDone?.()
  unlistenFolderScan?.()
  unlistenAnalysisProgress?.()
  unlistenAnalysisDone?.()
  unlistenFullAnalysisProgress?.()
  unlistenFullAnalysisDone?.()
  unlistenArtworkProgress?.()
  unlistenArtworkDone?.()
  unlistenGenreProgress?.()
  unlistenGenreDone?.()
})

function folderStatusClass(result: string): string {
  if (!result || result === 'pending' || result === 'Fetching...') return 'text-foreground/60'
  if (result === 'already_running' || result.startsWith('Error')) return 'text-error'
  if (result.startsWith('Fetched') || result === 'All lyrics already fetched') return 'text-green-400'
  if (result.startsWith('No lyrics')) return 'text-amber-400'
  return 'text-foreground/60'
}

async function fetchForFolder(folder: string, retry = false) {
  if (batchFetching.value) {
    folderResults[folder] = 'already_running'
    return
  }
  batchFetching.value = true
  fetchingAll.value = false
  activeFolder.value = folder
  batchCurrent.value = 0
  batchTotal.value = 0
  folderResults[folder] = 'Fetching...'
  try {
    const result = await api.batchFetchLyrics(folder, retry)
    if (result === 'already_running') {
      folderResults[folder] = 'already_running'
      batchFetching.value = false
      activeFolder.value = null
    } else if (result === 'nothing_to_do') {
      folderResults[folder] = 'No tracks in this folder'
      batchFetching.value = false
      activeFolder.value = null
    } else if (result === 'all_done') {
      folderResults[folder] = 'All lyrics already fetched'
      batchFetching.value = false
      activeFolder.value = null
    }
    // "started" — background task handles completion events
  } catch (e: any) {
    folderResults[folder] = `Error: ${e}`
    batchFetching.value = false
    activeFolder.value = null
  }
}

async function fetchAll(retry = false) {
  if (batchFetching.value) return
  batchFetching.value = true
  fetchingAll.value = true
  activeFolder.value = null
  batchCurrent.value = 0
  batchTotal.value = 0
  for (const f of folders.value) folderResults[f] = 'pending'
  try {
    const result = await api.batchFetchLyrics(undefined, retry)
    if (result === 'already_running' || result === 'nothing_to_do' || result === 'all_done') {
      for (const f of folders.value) folderResults[f] = result === 'all_done' ? 'All lyrics already fetched' : result
      batchFetching.value = false
      fetchingAll.value = false
    }
    // "started" — events handle per-folder results and final reset
  } catch (e: any) {
    const current = activeFolder.value
    if (current) folderResults[current] = `Error: ${e}`
    batchFetching.value = false
    fetchingAll.value = false
  }
}

async function loadFolders(showLoader = true) {
  if (showLoader) isLoading.value = true
  try {
    folders.value = await api.getLibraryFolders()
    for (const f of folders.value) {
      if (!(f in folderResults)) folderResults[f] = ''
    }
    // Clean up folderScanStatus for paths no longer in the list
    for (const key of Object.keys(folderScanStatus)) {
      if (!folders.value.includes(key)) {
        delete folderScanStatus[key]
      }
    }
  } catch {}
  if (showLoader) isLoading.value = false
}

onMounted(() => { loadFolders() })

async function pickAndAddFolder() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: true })
    if (!selected) return
    const paths = Array.isArray(selected) ? selected : [selected]
    addingFolders.value = true
    for (const p of paths) folderScanStatus[p] = 'waiting'
    const result = await api.addLibraryFolders(paths)
    addingFolders.value = false
    scanResult.value = `Added: ${result.added}, Removed: ${result.removed}`
    await loadFolders(false)
    await library.loadAll()
  } catch (e: any) {
    addingFolders.value = false
    scanResult.value = `Error: ${e}`
  }
}

async function removeFolder() {
  if (!folderToDelete.value) return
  const path = folderToDelete.value
  folderToDelete.value = null
  delete folderResults[path]
  try {
    await api.removeLibraryFolder(path)
    await library.loadAll()
    await loadFolders(false)
  } catch {}
}

async function syncAllFolders() {
  scanning.value = true
  scanResult.value = null
  scanCurrent.value = 0
  scanTotal.value = folders.value.length
  let totalAdded = 0
  try {
    for (const folder of folders.value) {
      const result = await api.scanFolder(folder)
      totalAdded += result.added
      scanCurrent.value++
    }
    scanResult.value = `Library synced. ${totalAdded} tracks added/updated.`
    await library.loadAll()
    await api.refreshFolderPlaylists()
  } catch (e: any) { scanResult.value = `Error: ${e}` }
  scanning.value = false
  scanCurrent.value = 0
  scanTotal.value = 0
}

async function scanFolder(path: string) {
  scanning.value = true
  scanCurrent.value = 0
  scanTotal.value = 1
  try {
    const result = await api.scanFolder(path)
    scanCurrent.value = 1
    scanResult.value = `Added: ${result.added}, Updated: ${result.updated}, Removed: ${result.removed}`
    await library.loadAll()
  } catch (e: any) { scanResult.value = `Error: ${e}` }
  scanning.value = false
  scanCurrent.value = 0
  scanTotal.value = 0
}

</script>

<template>
  <div class="space-y-8 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <div class="flex items-center justify-between mb-4 select-none">
      <h2 class="text-xl font-bold">Library</h2>
      <div class="flex gap-3">
        <button @click="syncAllFolders" :disabled="scanning || folders.length === 0"
          class="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-xl hover:opacity-90 transition-all disabled:opacity-50 text-sm font-bold shadow-lg shadow-primary/20">
          <RotateCcw class="w-4 h-4" :class="{ 'animate-spin': scanning }" />
          {{ scanning ? 'Syncing...' : 'Sync Library' }}
        </button>
      </div>
    </div>

    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
      <div class="flex items-center justify-between mb-6">
        <div>
          <h3 class="text-lg font-bold mb-1">Music Folders</h3>
          <p class="text-sm text-foreground opacity-60">Manage your music library folders</p>
        </div>
        <button @click="pickAndAddFolder" :disabled="scanning || addingFolders"
          class="flex items-center gap-2 px-4 py-2 bg-foreground/[0.04] text-foreground rounded-xl hover:bg-foreground/[0.08] transition-all text-sm font-bold disabled:opacity-50">
          <Loader2 v-if="addingFolders" class="w-4 h-4 animate-spin" />
          <Plus v-else class="w-4 h-4" />
          {{ addingFolders ? 'Adding...' : 'Add Folder' }}
        </button>
      </div>

      <div v-if="isLoading" class="py-12 flex justify-center">
        <RotateCcw class="w-8 h-8 animate-spin text-foreground opacity-40" />
      </div>

      <div v-else-if="folders.length === 0"
        class="py-12 text-center border-2 border-dashed border-foreground/[0.06] rounded-2xl">
        <Folder class="w-12 h-12 mx-auto text-foreground opacity-30 mb-4" />
        <p class="text-foreground opacity-60 text-sm font-medium">No folders added yet</p>
      </div>

      <ul v-else class="space-y-2">
        <li v-for="folder in folders" :key="folder"
          class="flex items-center justify-between p-4 bg-foreground/[0.02] border border-foreground/[0.04] rounded-xl group transition-all hover:bg-foreground/[0.04]">
          <div class="flex items-center gap-4 overflow-hidden min-w-0">
            <div class="p-2 bg-background rounded-lg shadow-sm">
              <Loader2 v-if="folderScanStatus[folder] === 'scanning'" class="w-4 h-4 animate-spin text-primary" />
              <FolderOpen v-else class="w-4 h-4 text-foreground opacity-60" />
            </div>
            <div class="min-w-0">
              <span class="text-sm font-bold truncate" :title="folder">{{ folder }}</span>
              <p v-if="folderScanStatus[folder]" class="text-xs mt-0.5"
                :class="folderScanStatus[folder] === 'scanning' ? 'text-primary' : folderScanStatus[folder]?.startsWith('done') ? 'text-success' : folderScanStatus[folder]?.startsWith('error') ? 'text-error' : 'text-foreground/60'">
                <template v-if="folderScanStatus[folder] === 'waiting'">Waiting...</template>
                <template v-else-if="folderScanStatus[folder] === 'scanning'">Scanning...</template>
                <template v-else-if="folderScanStatus[folder]?.startsWith('done')">{{ folderScanStatus[folder]?.replace('done:', '') }} tracks added</template>
                <template v-else-if="folderScanStatus[folder]?.startsWith('error')">{{ folderScanStatus[folder]?.replace('error:', 'Error: ') }}</template>
              </p>
            </div>
          </div>
          <div class="flex items-center gap-1">
            <button @click="scanFolder(folder)" :disabled="scanning || addingFolders"
              class="p-2 text-foreground/50 hover:text-foreground rounded-lg transition-colors disabled:opacity-40" title="Refresh">
              <RefreshCw class="w-3.5 h-3.5" :class="{ 'animate-spin': scanning }" />
            </button>
            <button @click="folderToDelete = folder" :disabled="scanning || addingFolders"
              class="p-2 text-error hover:text-error hover:bg-error/10 rounded-lg transition-all disabled:opacity-50">
              <Trash2 class="w-3.5 h-3.5" />
            </button>
          </div>
        </li>
      </ul>

      <div v-if="scanning && scanTotal > 0" class="mb-4 space-y-2">
        <div class="w-full h-2 bg-foreground/[0.06] rounded-full overflow-hidden">
          <div class="h-full bg-primary rounded-full transition-all duration-300 ease-out"
            :style="{ width: scanProgress + '%' }" />
        </div>
        <p class="text-xs text-foreground/60">
          Scanning folder {{ scanCurrent }} / {{ scanTotal }}
        </p>
      </div>

      <p v-if="scanResult" class="flex items-center gap-2 mt-4 text-sm" :class="scanResult.startsWith('Error') ? 'text-error' : 'text-success'">
        <CheckCircle v-if="!scanResult.startsWith('Error')" class="w-4 h-4 shrink-0" />
        <XCircle v-else class="w-4 h-4 shrink-0" />
        {{ scanResult }}
      </p>
      <p class="mt-2 text-sm text-foreground/50">{{ library.tracks.length }} tracks, {{ library.playlists.length }} playlists</p>
    </section>

    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
      <div class="flex items-center justify-between mb-4">
        <div>
          <h3 class="text-lg font-bold mb-1">Lyrics Fetching</h3>
          <p class="text-sm text-foreground opacity-60">Fetch missing lyrics from online sources for each folder</p>
        </div>
      <div class="flex items-center gap-2">
        <button @click="fetchAll(true)" :disabled="batchFetching || folders.length === 0"
          class="flex items-center gap-2 px-3 py-2 rounded-xl transition-all text-sm font-semibold disabled:opacity-50 border border-foreground/10 text-foreground/70 hover:text-foreground hover:border-foreground/25"
          title="Re-search tracks previously marked as not found">
          <RotateCcw class="w-4 h-4" />
          Retry Missing
        </button>
        <button @click="fetchAll()" :disabled="batchFetching || folders.length === 0"
          class="flex items-center gap-2 px-4 py-2 rounded-xl transition-all text-sm font-bold disabled:opacity-50"
          :class="batchFetching && fetchingAll ? 'bg-primary/20 text-primary' : 'bg-primary text-primary-foreground hover:opacity-90'">
          <Loader2 v-if="batchFetching && fetchingAll" class="w-4 h-4 animate-spin" />
          <FileText v-else class="w-4 h-4" />
          {{ batchFetching && fetchingAll ? 'Fetching...' : 'Fetch All' }}
        </button>
      </div>
      </div>

      <!-- Global progress bar -->
      <div v-if="batchFetching" class="mb-4 space-y-2">
        <div class="w-full h-2 bg-foreground/[0.06] rounded-full overflow-hidden">
          <div class="h-full bg-primary rounded-full transition-all duration-300 ease-out"
            :style="{ width: batchProgress + '%' }" />
        </div>
        <p class="text-xs text-foreground/60">
          <template v-if="activeFolder">{{ activeFolder }} — </template>
          {{ batchCurrent }} / {{ batchTotal }} tracks
        </p>
      </div>

      <div v-if="folders.length === 0"
        class="py-8 text-center border-2 border-dashed border-foreground/[0.06] rounded-2xl">
        <FileText class="w-10 h-10 mx-auto text-foreground opacity-30 mb-3" />
        <p class="text-foreground opacity-60 text-sm">Add a music folder to start fetching lyrics</p>
      </div>

      <ul v-else class="space-y-2">
        <li v-for="folder in folders" :key="folder"
          class="flex items-center justify-between p-4 bg-foreground/[0.02] border border-foreground/[0.04] rounded-xl group transition-all hover:bg-foreground/[0.04]">
          <div class="flex items-center gap-4 overflow-hidden min-w-0">
            <div class="p-2 bg-background rounded-lg shadow-sm">
              <FolderOpen class="w-4 h-4 text-foreground opacity-60" />
            </div>
            <div class="min-w-0">
              <p class="text-sm font-bold truncate" :title="folder">{{ folder }}</p>
              <p v-if="folderResults[folder]" class="text-xs mt-0.5"
                :class="folderStatusClass(folderResults[folder])">
                {{ folderResults[folder] === 'pending' ? 'Waiting...' : folderResults[folder] }}
              </p>
            </div>
          </div>
          <div class="flex items-center gap-1 flex-shrink-0">
            <button @click="fetchForFolder(folder, true)" :disabled="batchFetching"
              class="p-2 text-foreground/50 hover:text-foreground rounded-lg transition-colors disabled:opacity-40"
              :title="batchFetching && activeFolder === folder ? 'Fetching...' : 'Fetch Lyrics (re-search not found)'">
              <Loader2 v-if="batchFetching && activeFolder === folder" class="w-3.5 h-3.5 animate-spin" />
              <RefreshCw v-else class="w-3.5 h-3.5" />
            </button>
          </div>
        </li>
      </ul>

      <div class="flex items-center gap-2 mt-4">
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" :checked="appStore.autoFetchLyrics"
            @change="appStore.updateAutoFetchLyrics(($event.target as HTMLInputElement).checked)"
            class="sr-only peer">
          <div class="w-9 h-5 bg-foreground/20 rounded-full peer peer-checked:bg-primary peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-0.5 after:start-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all"></div>
        </label>
        <span class="text-xs text-foreground/60">Auto-fetch after scan</span>
      </div>

      <p v-if="!batchFetching && lyricsSummary" class="flex items-center gap-2 mt-4 text-sm text-success">
        <CheckCircle class="w-4 h-4" />
        {{ lyricsSummary }}
      </p>
    </section>

    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
      <div class="flex items-center justify-between mb-4">
        <div>
          <h3 class="text-lg font-bold mb-1">Audio Analysis</h3>
          <p class="text-sm text-foreground opacity-60">Detect BPM, musical key, and energy for smart features</p>
        </div>
        <button @click="startAnalysis" :disabled="analysis.analyzing || unanalyzedCount === 0"
          class="flex items-center gap-2 px-4 py-2 rounded-xl transition-all text-sm font-bold disabled:opacity-50"
          :class="analysis.analyzing ? 'bg-primary/20 text-primary' : 'bg-primary text-primary-foreground hover:opacity-90'">
          <Loader2 v-if="analysis.analyzing" class="w-4 h-4 animate-spin" />
          <Activity v-else class="w-4 h-4" />
          {{ analysis.analyzing ? 'Analyzing...' : `Analyze Audio (${unanalyzedCount})` }}
        </button>
      </div>

      <div class="flex items-center gap-2 mb-4">
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" :checked="appStore.autoAnalyzeAudio"
            @change="appStore.updateAutoAnalyzeAudio(($event.target as HTMLInputElement).checked)"
            class="sr-only peer">
          <div class="w-9 h-5 bg-foreground/20 rounded-full peer peer-checked:bg-primary peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-0.5 after:start-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all"></div>
        </label>
        <span class="text-xs text-foreground/60">Auto-analyze after scan</span>
      </div>

      <div v-if="analysis.analyzing" class="mb-4 space-y-2">
        <div class="w-full h-2 bg-foreground/[0.06] rounded-full overflow-hidden">
          <div class="h-full bg-primary rounded-full transition-all duration-300 ease-out"
            :style="{ width: analysisProgress + '%' }" />
        </div>
        <p class="text-xs text-foreground/60">
          {{ analysis.analysisCurrent }} / {{ analysis.analysisTotal }} — {{ analysis.analysisFile }}
        </p>
      </div>

      <ul v-if="Object.keys(analysisFileStatus).length > 0" class="mb-4 space-y-1 max-h-48 overflow-y-auto">
        <li v-for="(status, file) in analysisFileStatus" :key="file"
          class="flex items-center gap-2 text-xs truncate"
          :class="status === 'analyzed' ? 'text-success' : status === 'skipped' ? 'text-foreground/50' : 'text-primary'">
          <Loader2 v-if="status === 'analyzing'" class="w-3 h-3 shrink-0 animate-spin" />
          <CheckCircle v-else-if="status === 'analyzed'" class="w-3 h-3 shrink-0" />
          <Minus v-else class="w-3 h-3 shrink-0" />
          <span class="truncate">{{ file }}</span>
        </li>
      </ul>

      <p v-if="!analysis.analyzing && analysisResult" class="flex items-center gap-2 text-sm text-success">
        <CheckCircle class="w-4 h-4" />
        {{ analysisResult }}
      </p>

      <p v-else-if="!analysis.analyzing && unanalyzedCount === 0" class="text-sm text-foreground/60">
        All tracks analyzed ✓
      </p>

      <div class="flex items-center justify-between mt-4 pt-4 border-t border-foreground/[0.06]">
        <div>
          <p class="text-sm font-semibold">Full spectrum + mood</p>
          <p class="text-xs text-foreground opacity-60 mt-1">Loudness, spectral features and mood via FFmpeg (~15s per track)</p>
        </div>
        <button @click="startFullAnalysis" :disabled="fullAnalyzing"
          class="flex items-center gap-2 px-4 py-2 rounded-xl transition-all text-sm font-bold disabled:opacity-50"
          :class="fullAnalyzing ? 'bg-primary/20 text-primary' : 'bg-primary text-primary-foreground hover:opacity-90'">
          <Loader2 v-if="fullAnalyzing" class="w-4 h-4 animate-spin" />
          <Activity v-else class="w-4 h-4" />
          {{ fullAnalyzing ? `Spectrum ${fullDone}` : 'Analyze Spectrum' }}
        </button>
      </div>
      <p v-if="fullAnalyzing" class="text-xs text-foreground/60 mt-2">
        {{ fullDone }} done<template v-if="fullFailed > 0">, {{ fullFailed }} failed</template> — pauses while music plays
      </p>
      <p v-if="!fullAnalyzing && fullResult" class="flex items-center gap-2 text-sm text-success mt-2">
        <CheckCircle class="w-4 h-4" />
        {{ fullResult }}
      </p>
    </section>

    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
      <div class="flex items-center justify-between mb-4">
        <div>
          <h3 class="text-lg font-bold mb-1">Artist Artwork</h3>
          <p class="text-sm text-foreground opacity-60">Fetch artist images from Last.fm</p>
        </div>
        <div class="flex items-center gap-2">
          <button @click="startArtworkFetch(true)" :disabled="analysis.fetchingArtwork"
            class="flex items-center gap-2 px-3 py-2 rounded-xl transition-all text-sm font-semibold disabled:opacity-50 border border-foreground/10 text-foreground/70 hover:text-foreground hover:border-foreground/25"
            title="Re-search artists previously marked as not found">
            <RotateCcw class="w-4 h-4" />
            Retry Missing
          </button>
          <button @click="startArtworkFetch()" :disabled="analysis.fetchingArtwork"
            class="flex items-center gap-2 px-4 py-2 rounded-xl transition-all text-sm font-bold disabled:opacity-50"
            :class="analysis.fetchingArtwork ? 'bg-primary/20 text-primary' : 'bg-primary text-primary-foreground hover:opacity-90'">
            <Loader2 v-if="analysis.fetchingArtwork" class="w-4 h-4 animate-spin" />
            <Image v-else class="w-4 h-4" />
            {{ analysis.fetchingArtwork ? 'Fetching...' : 'Fetch Artist Artwork' }}
          </button>
        </div>
      </div>

      <div class="flex items-center gap-2 mb-4">
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" :checked="appStore.autoFetchArtistArtwork"
            @change="appStore.updateAutoFetchArtistArtwork(($event.target as HTMLInputElement).checked)"
            class="sr-only peer">
          <div class="w-9 h-5 bg-foreground/20 rounded-full peer peer-checked:bg-primary peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-0.5 after:start-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all"></div>
        </label>
        <span class="text-xs text-foreground/60">Auto-fetch after scan</span>
      </div>

      <div v-if="analysis.fetchingArtwork" class="mb-4 space-y-2">
        <div class="w-full h-2 bg-foreground/[0.06] rounded-full overflow-hidden">
          <div class="h-full bg-primary rounded-full transition-all duration-300 ease-out"
            :style="{ width: analysis.artworkTotal > 0 ? Math.round((analysis.artworkCurrent / analysis.artworkTotal) * 100) + '%' : '0%' }" />
        </div>
        <p class="text-xs text-foreground/60">
          {{ analysis.artworkCurrent }} / {{ analysis.artworkTotal }} — {{ analysis.artworkArtist }}
        </p>
      </div>

      <ul v-if="Object.keys(artistResults).length > 0" class="mb-4 space-y-2">
        <li v-for="(status, artist) in artistResults" :key="artist"
          class="flex items-center gap-3 p-2 bg-foreground/[0.02] rounded-xl">
          <Image class="w-4 h-4 text-foreground/60 shrink-0" />
          <div class="min-w-0">
            <p class="text-sm font-medium truncate">{{ artist }}</p>
            <p class="text-xs mt-0.5"
              :class="status === 'found' ? 'text-success' : status === 'not_found' ? 'text-foreground/50' : 'text-primary'">
              {{ status === 'searching' ? 'Searching...' : status === 'found' ? '✓ Fetched' : 'Not found' }}
            </p>
          </div>
        </li>
      </ul>

      <p v-if="!analysis.fetchingArtwork && artworkResult" class="flex items-center gap-2 text-sm text-success">
        <CheckCircle class="w-4 h-4" />
        {{ artworkResult }}
      </p>
    </section>

    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6">
      <div class="flex items-center justify-between">
        <div>
          <h3 class="text-lg font-bold mb-1">Audio Analysis & Genre Enrichment</h3>
          <p class="text-sm text-foreground opacity-60">
            Audio analysis, Camelot harmonic key profiling, and multi-source genre enrichment are unified in the Sound suite.
          </p>
        </div>
        <router-link
          to="/sound"
          class="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-bold bg-primary text-primary-foreground hover:opacity-90 transition-all shrink-0 ml-4"
        >
          <Sparkles class="w-4 h-4" />
          Open Sound Suite
        </router-link>
      </div>
    </section>

    <!-- Library Backup & Restore -->
    <section class="bg-card rounded-2xl border border-foreground/[0.06] p-6 space-y-4">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div class="flex items-center gap-2">
            <Archive class="w-5 h-5 text-primary" />
            <h3 class="text-lg font-bold">Single-File Library Backup & Restore</h3>
          </div>
          <p class="text-sm text-foreground opacity-60 mt-1">
            Package your playlists, smart rules, favorites, play history, custom EQ presets, and saved lyrics into a single <code class="px-1.5 py-0.5 rounded bg-foreground/10 text-xs">.zip</code> archive, with 1-click restore and automatic track relinking.
          </p>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-medium bg-foreground/10 hover:bg-foreground/15 text-foreground transition-all disabled:opacity-50"
            :disabled="backupLoading"
            @click="exportBackup"
          >
            <Download class="w-4 h-4" />
            <span>Export Backup</span>
          </button>
          <button
            class="flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-bold bg-primary text-primary-foreground hover:opacity-90 transition-all disabled:opacity-50"
            :disabled="backupLoading"
            @click="restoreBackup"
          >
            <Upload class="w-4 h-4" />
            <span>Restore Backup</span>
          </button>
        </div>
      </div>
      <p v-if="backupStatus" class="text-xs text-foreground/70 flex items-center gap-2">
        <CheckCircle v-if="backupSuccess" class="w-4 h-4 text-green-400 shrink-0" />
        <AlertTriangle v-else class="w-4 h-4 text-amber-400 shrink-0" />
        <span>{{ backupStatus }}</span>
      </p>
    </section>

    <ConfirmDialog
      :open="!!folderToDelete"
      @cancel="folderToDelete = null"
      title="Remove Folder"
      message="Remove this folder from your library? Tracks won't be deleted from disk."
      confirm-label="Remove"
      danger
      @confirm="removeFolder"
    />
  </div>
</template>
