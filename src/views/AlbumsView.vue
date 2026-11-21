<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLibraryStore } from '../stores/library'
import { usePlayerStore } from '../stores/player'
import ViewHeader from '../components/ViewHeader.vue'
import AlbumGrid from '../components/AlbumGrid.vue'
import AlbumListView from '../components/AlbumListView.vue'
import AlbumViewFilter from '../components/AlbumViewFilter.vue'

const router = useRouter()
const library = useLibraryStore()
const playerStore = usePlayerStore()

onMounted(() => { if (library.albums.length === 0 || library.tracks.length === 0) library.loadAll() })

const searchQuery = ref('')
const viewMode = ref<'grid' | 'list'>(
  (localStorage.getItem('albums-view-mode') as 'grid' | 'list') || 'grid'
)
watch(viewMode, v => localStorage.setItem('albums-view-mode', v))

type SortCol = 'title' | 'artist' | 'year'
type SortDir = 'asc' | 'desc'
const sortColumn = ref<SortCol>(
  (localStorage.getItem('albums-sort-col') as SortCol) || 'title'
)
const sortDir = ref<SortDir>(
  (localStorage.getItem('albums-sort-dir') as SortDir) || 'asc'
)
watch(sortColumn, v => localStorage.setItem('albums-sort-col', v))
watch(sortDir, v => localStorage.setItem('albums-sort-dir', v))

const albums = computed(() => {
  return library.albums.map(a => ({
    id: a.id,
    name: a.title,
    artist: a.artist,
    year: a.year,
    trackCount: a.track_count,
    firstTrackId: a.first_track_id,
    duration: a.duration,
    isHiRes: a.is_hi_res,
    artworkKey: a.artwork_key,
  }))
})

function foldUnicode(s: string): string {
  return s.toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '')
}

const processedAlbums = computed(() => {
  let result = albums.value
  if (searchQuery.value) {
    const q = foldUnicode(searchQuery.value)
    result = result.filter(a => foldUnicode(a.name).includes(q) || foldUnicode(a.artist).includes(q))
  }
  return [...result].sort((a, b) => {
    let cmp = 0
    if (sortColumn.value === 'title') cmp = a.name.localeCompare(b.name)
    else if (sortColumn.value === 'artist') cmp = a.artist.localeCompare(b.artist)
    else if (sortColumn.value === 'year') cmp = a.year - b.year
    return sortDir.value === 'asc' ? cmp : -cmp
  })
})

function navigateToAlbum(name: string, artist: string) {
  router.push({ path: `/albums/${encodeURIComponent(name)}`, query: { artist: artist || undefined } })
}

function playAlbum(name: string) {
  const albumTracks = library.tracks.filter(t => t.album === name)
  if (albumTracks.length > 0) playerStore.playTracks(albumTracks, 0)
}

function navigateToArtist(name: string) {
  router.push(`/artists/${encodeURIComponent(name)}`)
}
</script>

<template>
  <div class="h-full flex flex-col overflow-hidden bg-background">
    <ViewHeader v-model="searchQuery" title="Albums" search-placeholder="Search albums...">
      <template #actions>
        <AlbumViewFilter
          :view-mode="viewMode"
          :sort-column="sortColumn"
          :sort-dir="sortDir"
          @update:view-mode="(v) => viewMode = v"
          @update:sort-column="(c) => sortColumn = c"
          @update:sort-dir="(d) => sortDir = d" />
      </template>
    </ViewHeader>

    <div class="flex-1 overflow-y-auto custom-scrollbar" :class="viewMode === 'grid' ? 'px-6 py-8' : ''">
      <div v-if="processedAlbums.length === 0" class="h-full flex items-center justify-center text-foreground opacity-50 text-sm">
        No albums found
      </div>
      <AlbumGrid v-else-if="viewMode === 'grid'" :albums="processedAlbums" @click="navigateToAlbum" @play="playAlbum"
        @artist-click="navigateToArtist" />
      <AlbumListView v-else :albums="processedAlbums" :sort-column="sortColumn" :sort-dir="sortDir"
        @click="navigateToAlbum" @play="playAlbum" @artist-click="navigateToArtist"
        @update:sort-column="(c: SortCol) => sortColumn = c"
        @update:sort-dir="(d: SortDir) => sortDir = d" />
    </div>
  </div>
</template>
