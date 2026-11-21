<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLibraryStore } from '../stores/library'
import { usePlayerStore } from '../stores/player'
import TrackTable from '../components/TrackTable.vue'
import AlbumDetailHero from '../components/AlbumDetailHero.vue'
import AlbumDetailInfo from '../components/AlbumDetailInfo.vue'
import ContextMenu from '../components/ContextMenu.vue'
import AppendToQueueDialog from '../components/AppendToQueueDialog.vue'
import AddToPlaylistDialog from '../components/AddToPlaylistDialog.vue'
import { useAlbumContextMenu } from '../composables/useAlbumContextMenu'
import { useContextMenu } from '../composables/useContextMenu'
import * as api from '../lib/invoke'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()
const playerStore = usePlayerStore()

const directTracks = ref<api.Track[]>([])
const loading = ref(false)

const albumParam = computed(() => decodeURIComponent(route.params.id as string))
const artistFilter = computed(() => (route.query.artist as string) || '')
const tracks = computed(() => {
  if (directTracks.value.length > 0) return directTracks.value
  let result = library.tracks.filter(t => (t.album_id && t.album_id === albumParam.value) || (t.album || '') === albumParam.value)
  if (artistFilter.value) {
    result = result.filter(t => (t.album_artist || t.artist || '') === artistFilter.value)
  }
  return result
})

watch(albumParam, async (val) => {
  if (!val) return
  try {
    loading.value = true
    const res = await api.getAlbumTracks(val)
    if (res && res.length > 0) {
      directTracks.value = res
    }
  } catch (err) {
    console.warn('Direct album fetch failed, falling back to library store:', err)
  } finally {
    loading.value = false
  }
}, { immediate: true })

const albumTitle = computed(() => tracks.value[0]?.album || albumParam.value)
const artist = computed(() => tracks.value[0]?.album_artist || tracks.value[0]?.artist || artistFilter.value || 'Unknown Artist')
const artworkSrc = ref<string | null>(null)
const dominantColor = ref<string | null>(null)

watch(() => tracks.value[0]?.id, async (firstId) => {
  if (!firstId) return
  const first = tracks.value[0]
  try {
    const palette = await api.getTrackPalette(firstId)
    if (palette?.dominant) dominantColor.value = palette.dominant
  } catch {}
  if (first?.has_artwork) {
    try {
      const b64 = await api.getArtwork(firstId)
      if (b64) artworkSrc.value = `data:image/jpeg;base64,${b64}`
    } catch {}
  }
}, { immediate: true })

const contextMenu = useContextMenu()
const { buildMenuItems } = useAlbumContextMenu()
function openContextMenu(e: MouseEvent) {
  const items = buildMenuItems(tracks.value)
  contextMenu.open(e, items)
}

const queueDialogOpen = ref(false)
const playlistDialogOpen = ref(false)

function playAll() {
  if (tracks.value.length > 0) playerStore.playTracks(tracks.value, 0)
}

function shufflePlay() {
  const shuffled = [...tracks.value]
  for (let i = shuffled.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]]
  }
  if (shuffled.length > 0) playerStore.playTracks(shuffled, 0)
}

async function addToQueue(position: 'next' | 'end') {
  if (playerStore.queueBusy) return
  const q = [...playerStore.queue]
  if (position === 'next') {
    q.splice(playerStore.queueIndex + 1, 0, ...tracks.value)
  } else {
    q.push(...tracks.value)
  }
  playerStore.queue = q
  queueDialogOpen.value = false
  try {
    if (position === 'next') {
      await api.insertQueueTracks(tracks.value)
    } else {
      await api.appendQueueTracks(tracks.value)
    }
  } catch (e) { console.error('Queue mutation failed:', e) }
  await playerStore.syncQueueFromBackend()
}

async function addToPlaylist(playlistId: string) {
  const trackIds = tracks.value.map(t => t.id)
  await api.addTracksToPlaylist(playlistId, trackIds)
  playlistDialogOpen.value = false
}

async function handleCreatePlaylist(name: string) {
  const pl = await library.createPlaylist(name)
  if (pl) {
    const trackIds = tracks.value.map(t => t.id)
    await api.addTracksToPlaylist(pl.id, trackIds)
  }
  playlistDialogOpen.value = false
}
</script>

<template>
  <div class="h-full flex flex-col bg-background overflow-hidden">
    <div class="flex-1 overflow-y-auto custom-scrollbar">
      <AlbumDetailHero
        :title="albumTitle"
        :artist="artist"
        :artwork-src="artworkSrc"
        :dominant-color="dominantColor"
        :track-count="tracks.length"
        @play="playAll"
        @shuffle="shufflePlay"
        @contextmenu="openContextMenu"
        @artist-click="(name) => router.push(`/artists/${encodeURIComponent(name)}`)">
        <template #metadata>
          <AlbumDetailInfo :tracks="tracks" />
        </template>
      </AlbumDetailHero>

      <div class="p-6 pb-12">
        <TrackTable
          :tracks="tracks"
          :show-artwork="false"
          :simple-mode="true"
          @navigate-album="(name, artist) => router.push({ path: `/albums/${encodeURIComponent(name)}`, query: { artist: artist || undefined } })"
          @navigate-artist="(name) => router.push(`/artists/${encodeURIComponent(name)}`)" />
      </div>
    </div>

    <ContextMenu :visible="contextMenu.visible.value" :x="contextMenu.x.value" :y="contextMenu.y.value"
      :items="contextMenu.items.value" @close="contextMenu.close()" />

    <AppendToQueueDialog :open="queueDialogOpen" :track-count="tracks.length"
      @confirm="addToQueue" @close="queueDialogOpen = false" />

    <AddToPlaylistDialog :open="playlistDialogOpen" :track-count="tracks.length"
      :playlists="library.playlists"
      @confirm="addToPlaylist" @create-playlist="handleCreatePlaylist" @close="playlistDialogOpen = false" />
  </div>
</template>
