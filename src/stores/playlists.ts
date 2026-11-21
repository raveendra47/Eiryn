import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../lib/invoke'

export const usePlaylistsStore = defineStore('playlists', () => {
  const playlists = ref<api.Playlist[]>([])

  async function load() {
    try {
      playlists.value = await api.getPlaylists()
    } catch (e) {
      console.error('Failed to load playlists', e)
    }
  }

  async function create(name: string, isSmart?: boolean, smartRules?: string): Promise<api.Playlist | null> {
    try {
      const p = await api.createPlaylist(name, isSmart, smartRules)
      await load()
      return p
    } catch (e) {
      console.error('Failed to create playlist', e)
      return null
    }
  }

  async function remove(id: string) {
    try {
      await api.deletePlaylist(id)
      await load()
    } catch (e) {
      console.error('Failed to delete playlist', e)
    }
  }

  async function rename(id: string, name: string) {
    try {
      await api.renamePlaylist(id, name)
      await load()
    } catch (e) {
      console.error('Failed to rename playlist', e)
    }
  }

  async function updateSmartRules(id: string, rules: string | null) {
    try {
      await api.updatePlaylistRules(id, rules)
      await load()
    } catch (e) {
      console.error('Failed to update smart rules', e)
    }
  }

  async function refresh(id: string): Promise<number> {
    try {
      const count = await api.refreshSmartPlaylist(id)
      return count
    } catch (e) {
      console.error('Failed to refresh smart playlist', e)
      return 0
    }
  }

  return { playlists, load, create, remove, rename, updateSmartRules, refresh }
})
