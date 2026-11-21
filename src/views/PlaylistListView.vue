<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { ListMusic, Play, Plus, Trash2, Download, Upload, Heart, Wand, Folder } from '@lucide/vue'
import { useLibraryStore } from '../stores/library'
import { usePlayerStore } from '../stores/player'
import { usePlaylistsStore } from '../stores/playlists'
import { useContextMenu } from '../composables/useContextMenu'
import ContextMenu from '../components/ContextMenu.vue'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import * as api from '../lib/invoke'

const router = useRouter()
const library = useLibraryStore()
const playerStore = usePlayerStore()
const playlistStore = usePlaylistsStore()
const menu = useContextMenu()
const showNewPlaylist = ref(false)
const newName = ref('')
const showNewSmartPlaylist = ref(false)
const showNewFolderPlaylist = ref(false)
const showDeleteConfirm = ref(false)
const pendingDeleteId = ref('')
const pendingDeleteName = ref('')

const trackCounts = ref<Record<string, number>>({})

onMounted(async () => {
  if (library.playlists.length === 0) await library.loadPlaylists()
  for (const p of library.playlists) {
    try {
      trackCounts.value[p.id] = await api.getPlaylistTrackCount(p.id)
    } catch {
      trackCounts.value[p.id] = 0
    }
  }
})

function navigate(id: string) {
  router.push({ name: 'playlist-detail', params: { id } })
}

function navigateFavorites() {
  router.push('/playlists/favorites')
}

function playFavorites() {
  api.getFavoriteIds().then(ids => {
    const idSet = new Set(ids)
    const favTracks = library.tracks.filter(t => idSet.has(t.id))
    if (favTracks.length > 0) playerStore.playTracks(favTracks, 0)
  })
}

function playPlaylist(id: string) {
  api.getPlaylistTracks(id).then(tracks => {
    if (tracks.length > 0) playerStore.playTracks(tracks, 0)
  })
}

async function createPlaylist() {
  if (!newName.value.trim()) return
  await api.createPlaylist(newName.value.trim())
  newName.value = ''
  showNewPlaylist.value = false
  await library.loadPlaylists()
}

async function createSmartPlaylist() {
  if (!newName.value.trim()) return
  await api.createPlaylist(newName.value.trim(), true, JSON.stringify({ operator: 'and', rules: [{ field: 'date_added', operator: 'in_last', value: 30 }] }))
  newName.value = ''
  showNewSmartPlaylist.value = false
  await library.loadPlaylists()
  await playlistStore.refresh(library.playlists[library.playlists.length - 1].id)
}

const pendingFolderPath = ref('')

async function pickFolderPlaylist() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: false })
    if (!selected) return
    const folderPath = Array.isArray(selected) ? selected[0] : selected
    pendingFolderPath.value = folderPath
    const folderName = folderPath.split('\\').pop() || folderPath.split('/').pop() || 'Folder'
    newName.value = folderName
    showNewFolderPlaylist.value = true
  } catch (e) { console.error('Folder picker failed', e) }
}

async function confirmFolderPlaylist() {
  if (!newName.value.trim() || !pendingFolderPath.value) return
  await api.createFolderPlaylist(newName.value.trim(), pendingFolderPath.value)
  newName.value = ''
  pendingFolderPath.value = ''
  showNewFolderPlaylist.value = false
  await library.loadPlaylists()
}

function promptDelete(id: string, name: string) {
  pendingDeleteId.value = id
  pendingDeleteName.value = name
  showDeleteConfirm.value = true
}

async function doDelete() {
  if (pendingDeleteId.value) {
    await api.deletePlaylist(pendingDeleteId.value)
    await library.loadPlaylists()
  }
}

async function promptExport(id: string) {
  try {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const path = await save({
      filters: [{ name: 'M3U Playlist', extensions: ['m3u'] }],
      defaultPath: `${library.playlists.find(p => p.id === id)?.name || 'playlist'}.m3u`,
    })
    if (path) {
      await api.exportPlaylistM3U(id, path)
    }
  } catch (e) {
    console.error('Export failed', e)
  }
}

async function promptImport() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      title: 'Choose a playlist to import',
      multiple: false,
      filters: [{ name: 'M3U / M3U8 Playlist', extensions: ['m3u', 'm3u8'] }],
    })
    if (selected && typeof selected === 'string') {
      const fileName = selected.split(/[\\/]/).pop()?.replace(/\.m3u8?$/i, '') || 'Imported Playlist'
      await api.importPlaylistM3U(selected, fileName)
      await library.loadPlaylists()
    }
  } catch (e) {
    console.error('Import failed', e)
  }
}

function onContextMenu(e: MouseEvent, playlist: { id: string; name: string; is_smart: boolean; is_folder_playlist: boolean }) {
  menu.open(e, [
    { label: 'Play', icon: Play, action: () => playPlaylist(playlist.id) },
    { separator: true },
    {
      label: 'Export as M3U',
      icon: Download,
      action: () => promptExport(playlist.id),
    },
    { separator: true },
    {
      label: 'Delete',
      icon: Trash2,
      danger: true,
      action: () => promptDelete(playlist.id, playlist.name),
    },
  ])
}
</script>

<template>
  <div class="h-full flex flex-col overflow-hidden">
    <div class="flex items-center justify-between px-6 py-4 border-b border-foreground/[0.06]">
      <h1 class="text-xl font-bold">Playlists</h1>
      <div class="flex gap-2">
        <button @click="promptImport"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg border border-foreground/20 text-foreground/80 hover:bg-foreground/5 hover:text-foreground transition-colors"
          title="Import an existing M3U or M3U8 playlist file">
          <Upload class="w-4 h-4" />
          Import
        </button>
        <button @click="showNewSmartPlaylist = true"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg border border-primary/30 text-primary hover:bg-primary/10 transition-colors">
          <Wand class="w-4 h-4" />
          Smart
        </button>
        <button @click="pickFolderPlaylist"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg border border-amber-500/30 text-amber-500 hover:bg-amber-500/10 transition-colors">
          <Folder class="w-4 h-4" />
          Folder
        </button>
        <button @click="showNewPlaylist = true"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:opacity-90 transition-opacity">
          <Plus class="w-4 h-4" />
          New Playlist
        </button>
      </div>
    </div>

    <div class="flex-1 overflow-y-auto custom-scrollbar p-6">
      <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
        <div class="group cursor-pointer" @click="navigateFavorites">
          <div class="aspect-square bg-rose-500/10 rounded-lg ring-1 ring-rose-500/20 overflow-hidden relative flex items-center justify-center mb-3 transition-all group-hover:ring-rose-500/40">
            <Heart class="w-1/3 h-1/3 text-rose-500 opacity-80 group-hover:opacity-100 transition-opacity" />
            <div class="absolute inset-0 bg-background/20 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
              <button @click.stop="playFavorites"
                class="w-12 h-12 bg-foreground text-background rounded-full shadow-xl flex items-center justify-center transform translate-y-4 group-hover:translate-y-0 transition-all duration-300">
                <Play class="w-6 h-6 fill-current ml-1" />
              </button>
            </div>
          </div>
          <div class="px-1">
            <h3 class="font-medium text-sm truncate">Favorites</h3>
            <p class="text-xs text-foreground opacity-50 truncate mt-0.5">Liked tracks</p>
          </div>
        </div>

        <div v-for="p in library.playlists" :key="p.id"
          class="group cursor-pointer" @click="navigate(p.id)"
          @contextmenu.prevent="onContextMenu($event, p)">
          <div class="aspect-square bg-foreground/5 rounded-lg ring-1 ring-foreground/[0.06] overflow-hidden relative flex items-center justify-center mb-3 transition-all group-hover:ring-primary/30"
            :class="p.is_folder_playlist ? 'ring-amber-500/20' : p.is_smart ? 'ring-primary/20' : ''">
            <Folder v-if="p.is_folder_playlist" class="w-1/3 h-1/3 text-amber-500 opacity-50 group-hover:opacity-70 transition-opacity" />
            <Wand v-else-if="p.is_smart" class="w-1/3 h-1/3 text-primary opacity-50 group-hover:opacity-70 transition-opacity" />
            <ListMusic v-else class="w-1/3 h-1/3 text-foreground opacity-40 group-hover:opacity-60 transition-opacity" />
            <div class="absolute inset-0 bg-background/20 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
              <button @click.stop="playPlaylist(p.id)"
                class="w-12 h-12 bg-foreground text-background rounded-full shadow-xl flex items-center justify-center transform translate-y-4 group-hover:translate-y-0 transition-all duration-300">
                <Play class="w-6 h-6 fill-current ml-1" />
              </button>
            </div>
          </div>
          <div class="px-1">
            <div class="flex items-center gap-1.5">
              <h3 class="font-medium text-sm truncate">{{ p.name }}</h3>
              <span v-if="p.is_folder_playlist" class="text-[10px] px-1.5 py-0.5 rounded-full bg-amber-500/10 text-amber-500 font-medium flex-shrink-0">Folder</span>
              <span v-else-if="p.is_smart" class="text-[10px] px-1.5 py-0.5 rounded-full bg-primary/10 text-primary font-medium flex-shrink-0">Smart</span>
            </div>
            <p v-if="p.is_folder_playlist" class="text-xs text-foreground opacity-50 truncate mt-0.5" :title="p.folder_path || ''">{{ p.folder_path || '' }}</p>
            <p v-else class="text-xs text-foreground opacity-50 truncate mt-0.5">{{ trackCounts[p.id] || 0 }} tracks</p>
          </div>
        </div>
      </div>
    </div>

    <!-- New Playlist Dialog -->
    <div v-if="showNewPlaylist" class="fixed inset-0 z-50 flex items-center justify-center bg-background/60 backdrop-blur-sm"
      @click.self="showNewPlaylist = false">
      <div class="bg-background border border-foreground/[0.1] rounded-xl p-6 shadow-2xl w-80" @click.stop>
        <h2 class="text-lg font-bold mb-4">New Playlist</h2>
        <input v-model="newName" placeholder="Playlist name" autofocus
          class="w-full px-3 py-2 rounded-lg bg-foreground/5 border border-foreground/[0.1] text-sm outline-none focus:border-primary/50 transition-colors"
          @keydown.enter="createPlaylist" />
        <div class="flex justify-end gap-2 mt-4">
          <button @click="showNewPlaylist = false"
            class="px-3 py-1.5 text-sm rounded-lg hover:bg-foreground/10 transition-colors">Cancel</button>
          <button @click="createPlaylist" :disabled="!newName.trim()"
            class="px-3 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:opacity-90 transition-opacity disabled:opacity-40">Create</button>
        </div>
      </div>
    </div>

    <!-- New Smart Playlist Dialog -->
    <div v-if="showNewSmartPlaylist" class="fixed inset-0 z-50 flex items-center justify-center bg-background/60 backdrop-blur-sm"
      @click.self="showNewSmartPlaylist = false">
      <div class="bg-background border border-foreground/[0.1] rounded-xl p-6 shadow-2xl w-80" @click.stop>
        <h2 class="text-lg font-bold mb-4">New Smart Playlist</h2>
        <p class="text-xs text-foreground/60 mb-3">Auto-updating playlist based on rules. You can configure rules after creation.</p>
        <input v-model="newName" placeholder="Smart playlist name" autofocus
          class="w-full px-3 py-2 rounded-lg bg-foreground/5 border border-foreground/[0.1] text-sm outline-none focus:border-primary/50 transition-colors"
          @keydown.enter="createSmartPlaylist" />
        <div class="flex justify-end gap-2 mt-4">
          <button @click="showNewSmartPlaylist = false"
            class="px-3 py-1.5 text-sm rounded-lg hover:bg-foreground/10 transition-colors">Cancel</button>
          <button @click="createSmartPlaylist" :disabled="!newName.trim()"
            class="px-3 py-1.5 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:opacity-90 transition-opacity disabled:opacity-40">Create</button>
        </div>
      </div>
    </div>

    <!-- New Folder Playlist Dialog -->
    <div v-if="showNewFolderPlaylist" class="fixed inset-0 z-50 flex items-center justify-center bg-background/60 backdrop-blur-sm"
      @click.self="showNewFolderPlaylist = false">
      <div class="bg-background border border-foreground/[0.1] rounded-xl p-6 shadow-2xl w-96" @click.stop>
        <h2 class="text-lg font-bold mb-4">New Folder Playlist</h2>
        <p class="text-xs text-foreground/60 mb-2 truncate">Folder: <span class="text-foreground/80 font-mono">{{ pendingFolderPath }}</span></p>
        <p class="text-xs text-foreground/60 mb-3">Auto-updating playlist with all tracks from this folder.</p>
        <input v-model="newName" placeholder="Playlist name" autofocus
          class="w-full px-3 py-2 rounded-lg bg-foreground/5 border border-foreground/[0.1] text-sm outline-none focus:border-primary/50 transition-colors"
          @keydown.enter="confirmFolderPlaylist" />
        <div class="flex justify-end gap-2 mt-4">
          <button @click="showNewFolderPlaylist = false; pendingFolderPath = ''"
            class="px-3 py-1.5 text-sm rounded-lg hover:bg-foreground/10 transition-colors">Cancel</button>
          <button @click="confirmFolderPlaylist" :disabled="!newName.trim()"
            class="px-3 py-1.5 text-sm font-medium bg-amber-500 text-white rounded-lg hover:opacity-90 transition-opacity disabled:opacity-40">Create</button>
        </div>
      </div>
    </div>

    <ContextMenu :visible="menu.visible.value" :x="menu.x.value" :y="menu.y.value"
      :items="menu.items.value" @close="menu.close()" />

    <ConfirmDialog :open="showDeleteConfirm" title="Delete Playlist"
      :message="`Delete &quot;${pendingDeleteName}&quot;? Tracks won't be removed from your library.`"
      confirmLabel="Delete" :danger="true"
      @update:open="showDeleteConfirm = $event"
      @confirm="doDelete()" @cancel="showDeleteConfirm = false" />
  </div>
</template>
