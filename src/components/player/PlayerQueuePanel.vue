<script setup lang="ts">
import { ref, watch } from 'vue'
import { ListMusic, X, Goal, Music, GripVertical, MoreVertical } from '@lucide/vue'
import { usePlayerStore } from '../../stores/player'
import { formatTime } from '../../lib/utils'
import TrackContextMenu from '../TrackContextMenu.vue'
import type { Track } from '../../lib/invoke'
import { primeArtworkCache } from '../../composables/useArtworkUrl'

defineProps<{
  queue: Track[]
}>()

const emit = defineEmits<{
  close: []
  playTrack: [index: number]
}>()

const playerStore = usePlayerStore()
const trackContextMenu = ref<InstanceType<typeof TrackContextMenu> | null>(null)
const listEl = ref<any>(null)

const artworkCache = ref<Record<string, string>>({})

// Windowed prefetch around the current track (see QueueDrawer): a
// full-library queue must not materialize thousands of artworks.
const QUEUE_ARTWORK_RADIUS = 25
let queueArtGen = 0

watch([() => playerStore.queue, () => playerStore.queueIndex], async ([tracks, center]) => {
  const my = ++queueArtGen
  const c = typeof center === 'number' ? center : 0
  const win = (tracks as Track[]).slice(Math.max(0, c - QUEUE_ARTWORK_RADIUS), c + QUEUE_ARTWORK_RADIUS + 1)
  const urls = await primeArtworkCache(win.filter((t) => t.has_artwork).map((t) => t.id))
  if (my !== queueArtGen) return
  const next: Record<string, string> = {}
  for (const [id, url] of urls) next[id] = url
  artworkCache.value = next
}, { immediate: true })

function scrollToCurrent() {
  if (!listEl.value || !playerStore.currentTrack) return
  listEl.value.scrollToKey(playerStore.currentTrack.id, 'center')
}

function onContextMenu(e: MouseEvent, track: Track) {
  trackContextMenu.value?.open(e, track, { showRemoveFromQueue: true })
}
</script>

<template>
  <div class="h-full w-full bg-black/25 backdrop-blur-xl border border-white/10 rounded-3xl flex flex-col overflow-hidden shadow-2xl">
    <div class="flex-1 flex flex-col h-full">
      <div class="flex items-center justify-between px-6 py-4 border-b border-white/5">
        <div class="flex items-center gap-2 text-white/80">
          <ListMusic class="w-4 h-4" />
          <span class="text-sm font-semibold uppercase tracking-wider">Up Next</span>
        </div>
        <div class="flex items-center gap-1">
          <button @click="scrollToCurrent"
            class="text-white/40 hover:text-white transition-colors p-1 hover:bg-white/5 rounded-full">
            <Goal class="w-4 h-4" />
          </button>
          <button @click="emit('close')"
            class="text-white/40 hover:text-white transition-colors p-1 hover:bg-white/5 rounded-full">
            <X class="w-4 h-4" />
          </button>
        </div>
      </div>

      <div v-if="queue.length === 0" class="flex-1 flex flex-col items-center justify-center text-white/40 gap-3">
        <ListMusic class="w-10 h-10 opacity-20" />
        <p class="text-sm">Queue is empty</p>
      </div>
      <VirtualList
        v-else
        ref="listEl"
        :model-value="queue"
        @update:model-value="playerStore.reorderQueue"
        data-key="id"
        :size="64"
        handle=".dnd-handle"
        :animation="150"
        chosen-class="drag-chosen"
        :ghost-style="{ display: 'none' }"
        class="flex-1 custom-scrollbar px-2 py-2">
        <template v-slot:item="{ record, index }">
          <div class="flex items-center gap-3 px-3 py-2 rounded-xl hover:bg-white/5 transition-colors cursor-pointer group select-none"
            :class="{ 'bg-white/10 border-l-2 border-l-primary': playerStore.currentTrack?.id === (record as Track).id }"
            :data-current="playerStore.currentTrack?.id === (record as Track).id || null"
            @dblclick="emit('playTrack', index)"
            @contextmenu.prevent="onContextMenu($event, record as Track)">
            <div class="dnd-handle opacity-20 group-hover:opacity-60 transition-opacity cursor-grab text-white flex-shrink-0">
              <GripVertical class="w-4 h-4" />
            </div>
            <div class="w-10 h-10 bg-white/5 rounded-md flex-shrink-0 overflow-hidden flex items-center justify-center">
              <img v-if="artworkCache[(record as Track).id]" :src="artworkCache[(record as Track).id]" :alt="(record as Track).title" class="w-full h-full object-cover" />
              <Music v-else class="w-4 h-4 text-white/40" />
            </div>
            <div class="flex-1 min-w-0">
              <div class="text-sm truncate font-medium text-white"
                :class="{ 'text-primary': playerStore.currentTrack?.id === (record as Track).id }">
                {{ (record as Track).title || 'Unknown' }}
              </div>
              <div class="text-xs text-white/50 truncate">{{ (record as Track).artist || 'Unknown Artist' }}</div>
            </div>
            <span class="text-xs text-white/40 tabular-nums flex-shrink-0">{{ formatTime((record as Track).duration) }}</span>
            <button @click.stop="onContextMenu($event, record as Track)"
              class="p-1 opacity-0 group-hover:opacity-100 hover:bg-white/10 rounded-full transition-all ml-1">
              <MoreVertical class="w-3.5 h-3.5 text-white/50" />
            </button>
          </div>
        </template>
      </VirtualList>
    </div>
  </div>
  <TrackContextMenu ref="trackContextMenu" />
</template>

<style scoped>
.drag-chosen {
  opacity: 0.9;
  z-index: 50;
  box-shadow: 0 8px 16px rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}
</style>
