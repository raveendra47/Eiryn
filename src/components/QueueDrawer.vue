<script setup lang="ts">
import { ref, watch } from 'vue'
import { Music, X, ListMusic, GripVertical, MoreVertical, ArrowDownToLine } from '@lucide/vue'
import { formatTime } from '../lib/utils'
import { usePlayerStore } from '../stores/player'
import type { Track } from '../lib/invoke'
import { primeArtworkCache } from '../composables/useArtworkUrl'
import TrackContextMenu from './TrackContextMenu.vue'

const store = usePlayerStore()
const trackContextMenu = ref<InstanceType<typeof TrackContextMenu> | null>(null)
const listEl = ref<any>(null)
const artworkCache = ref<Record<string, string>>({})

// Windowed prefetch around the current track: a full-library queue must not
// materialize thousands of artworks. Shared md cache, bounded concurrency.
const QUEUE_ARTWORK_RADIUS = 25
let queueArtGen = 0

watch([() => store.queue, () => store.queueIndex], async ([tracks, center]) => {
  const my = ++queueArtGen
  const c = typeof center === 'number' ? center : 0
  const win = (tracks as Track[]).slice(Math.max(0, c - QUEUE_ARTWORK_RADIUS), c + QUEUE_ARTWORK_RADIUS + 1)
  const urls = await primeArtworkCache(win.filter((t) => t.has_artwork).map((t) => t.id))
  if (my !== queueArtGen) return
  const next: Record<string, string> = {}
  for (const [id, url] of urls) next[id] = url
  artworkCache.value = next
}, { immediate: true })

function onContextMenu(e: MouseEvent, track: Track) {
  trackContextMenu.value?.open(e, track, { showRemoveFromQueue: true })
}

function scrollToCurrent() {
  if (!listEl.value || !store.currentTrack) return
  listEl.value.scrollToKey(store.currentTrack.id, 'center')
}
</script>

<template>
  <div class="h-full w-full bg-background/80 backdrop-blur-2xl flex flex-col">
    <div class="flex items-center justify-between px-4 py-3 border-b border-foreground/[0.06]">
      <div class="flex items-center gap-2 font-semibold">
        <ListMusic class="w-4 h-4 text-primary" />
        <span>Queue</span>
        <span class="text-xs text-foreground opacity-50 font-normal ml-1">({{ store.queue.length }})</span>
      </div>
      <div class="flex items-center gap-1">
        <button @click="scrollToCurrent"
          class="p-1.5 rounded-full hover:bg-foreground/10 transition-colors text-foreground opacity-60 hover:text-foreground"
          title="Scroll to current">
          <ArrowDownToLine class="w-4 h-4" />
        </button>
        <button @click="store.toggleQueue()"
          class="p-1.5 rounded-full hover:bg-foreground/10 transition-colors text-foreground opacity-60 hover:text-foreground"
          title="Close">
          <X class="w-4 h-4" />
        </button>
      </div>
    </div>

    <VirtualList
      ref="listEl"
      :model-value="store.queue"
      @update:model-value="store.reorderQueue"
      data-key="id"
      :size="64"
      handle=".dnd-handle"
      :animation="150"
      chosen-class="drag-chosen"
      :ghost-style="{ display: 'none' }"
      class="flex-1 custom-scrollbar">
      <template v-slot:item="{ record, index }">
        <div class="flex items-center gap-2 px-4 py-2 hover:bg-foreground/[0.04] transition-colors cursor-pointer group select-none h-14 border-b border-foreground/[0.04]"
          :class="{ 'bg-primary/10 border-l-2 border-l-primary': store.currentTrack?.id === (record as Track).id }"
          :data-current="store.currentTrack?.id === (record as Track).id || null"
          @dblclick="store.playTracks(store.queue, index)"
          @contextmenu.prevent="onContextMenu($event, record as Track)">
          <div class="dnd-handle opacity-20 group-hover:opacity-60 transition-opacity cursor-grab text-foreground flex-shrink-0">
            <GripVertical class="w-4 h-4" />
          </div>
          <div class="w-10 h-10 bg-foreground/5 rounded-md flex-shrink-0 overflow-hidden flex items-center justify-center">
            <img v-if="artworkCache[(record as Track).id]" :src="artworkCache[(record as Track).id]" :alt="(record as Track).title" class="w-full h-full object-cover" />
            <Music v-else class="w-4 h-4 text-foreground opacity-40" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-sm truncate font-medium" :class="{ 'text-primary': store.currentTrack?.id === (record as Track).id }">
              {{ (record as Track).title || 'Unknown' }}
            </div>
            <div class="text-xs text-foreground opacity-50 truncate">{{ (record as Track).artist || 'Unknown Artist' }}</div>
          </div>
          <span class="text-xs text-foreground opacity-40 tabular-nums flex-shrink-0">{{ formatTime((record as Track).duration) }}</span>
          <button @click.stop="onContextMenu($event, record as Track)"
            class="p-1 opacity-0 group-hover:opacity-100 hover:bg-foreground/[0.08] rounded-full transition-all ml-1">
            <MoreVertical class="w-3.5 h-3.5 text-foreground opacity-50" />
          </button>
        </div>
      </template>
    </VirtualList>
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
