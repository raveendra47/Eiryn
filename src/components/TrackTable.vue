<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from 'vue'
import { Music } from '@lucide/vue'
import { RecycleScroller } from 'vue-virtual-scroller'
import { usePlayerStore } from '../stores/player'
import { useTrackTableSettings, COLUMN_MAP } from '../composables/useTrackTableSettings'
import type { ColumnKey } from '../composables/useTrackTableSettings'
import type { Track } from '../lib/invoke'
import TrackTableHeader from './TrackTableHeader.vue'
import TrackTableRow from './TrackTableRow.vue'
import TrackContextMenu from './TrackContextMenu.vue'

const SIMPLE_COLUMNS: ColumnKey[] = ['index', 'title', 'artist', 'duration', 'context_menu']

const props = withDefaults(defineProps<{
  tracks: Track[]
  isLoading?: boolean
  playlistId?: string
  variant?: 'default' | 'glass'
  scrollToCurrent?: boolean
  hideHeader?: boolean
  allowDnd?: boolean
  simpleMode?: boolean
  contextMenuOptions?: { showRemoveFromQueue?: boolean; playlistId?: string }
}>(), {
  variant: 'default',
  scrollToCurrent: false,
  hideHeader: false,
  allowDnd: false,
  simpleMode: false,
})

const emit = defineEmits<{
  playTrack: [track: Track, index: number]
  reorder: [tracks: Track[]]
  navigateAlbum: [name: string, artist: string]
  navigateArtist: [name: string]
}>()

const playerStore = usePlayerStore()
const settings = useTrackTableSettings()
const trackContextMenu = ref<InstanceType<typeof TrackContextMenu> | null>(null)
const trackTableEl = ref<HTMLElement | null>(null)
const selectedIds = ref<Set<string>>(new Set())
const lastSelectedIndex = ref<number | null>(null)
const selectionAnchorIndex = ref<number | null>(null)
const sortColumn = ref<string | null>(null)
const sortDir = ref<'asc' | 'desc'>('asc')

const orderedVisibleColumns = computed(() => {
  if (props.simpleMode) {
    return SIMPLE_COLUMNS.map((k) => COLUMN_MAP[k])
  }
  const order = settings.columnOrder.value
  const visible = settings.visibleColumns.value
  return order.filter((k) => visible.includes(k)).map((k) => COLUMN_MAP[k])
})

const gridTemplateColumns = computed(() =>
  orderedVisibleColumns.value.map((c) =>
    props.simpleMode ? c.gridWidth : settings.effectiveGridWidth(c)
  ).join(' '),
)

const totalMinWidth = computed(() => {
  const sum = orderedVisibleColumns.value.reduce((acc, c) => acc + c.minWidthPx, 0)
  return sum + 'px'
})

const headerContainerRef = ref<HTMLElement | null>(null)

// Windowed rendering: only ~visible rows are mounted (row heights are uniform:
// 56px normal, 36px collapsed — titles are single-line truncated).
const ROW_HEIGHT_DEFAULT = 56
const ROW_HEIGHT_COLLAPSED = 36
interface ScrollerHandle {
  $el: HTMLElement
  scrollToItem: (index: number) => void
}
const scrollerRef = ref<ScrollerHandle | null>(null)
const scrollerItemSize = ref(ROW_HEIGHT_DEFAULT)
watch(() => settings.collapsedMode.value, (c) => {
  scrollerItemSize.value = c ? ROW_HEIGHT_COLLAPSED : ROW_HEIGHT_DEFAULT
})

function attachScrollerScroll() {
  const el = scrollerRef.value?.$el as HTMLElement | null
  if (!el || el.dataset.scrollBound === '1') return
  el.dataset.scrollBound = '1'
  el.addEventListener('scroll', onTableScroll, { passive: true })
}

function onTableScroll(e: Event) {
  const target = e.target as HTMLElement
  if (headerContainerRef.value) {
    headerContainerRef.value.scrollLeft = target.scrollLeft
  }
}

onUnmounted(() => {
  const el = scrollerRef.value?.$el as HTMLElement | null
  el?.removeEventListener('scroll', onTableScroll)
})

const sortedTracks = computed(() => {
  if (!sortColumn.value) return props.tracks
  const col = COLUMN_MAP[sortColumn.value as ColumnKey]
  if (!col?.sortFn) return props.tracks
  return [...props.tracks].sort((a, b) => {
    return sortDir.value === 'asc' ? col.sortFn!(a, b) : -col.sortFn!(a, b)
  })
})

function handleSort(col: string) {
  if (sortColumn.value === col) {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortColumn.value = col
    sortDir.value = 'asc'
  }
}

function handleReorderColumn(from: ColumnKey, to: ColumnKey) {
  settings.reorderColumns(from, to)
}

function handleResizeColumn(key: ColumnKey, width: number) {
  settings.setColumnWidth(key, width)
}

function handleTrackClick(e: MouseEvent, track: Track, index: number) {
  const cmdOrCtrl = e.metaKey || e.ctrlKey
  const shift = e.shiftKey
  if (shift && selectionAnchorIndex.value !== null) {
    const start = Math.min(selectionAnchorIndex.value, index)
    const end = Math.max(selectionAnchorIndex.value, index)
    const newSelected = new Set(cmdOrCtrl ? selectedIds.value : [])
    for (let i = start; i <= end; i++) newSelected.add(sortedTracks.value[i].id)
    selectedIds.value = newSelected
    lastSelectedIndex.value = index
  } else if (cmdOrCtrl) {
    const s = new Set(selectedIds.value)
    if (s.has(track.id)) s.delete(track.id); else s.add(track.id)
    selectedIds.value = s
    lastSelectedIndex.value = index
    selectionAnchorIndex.value = index
  } else {
    selectedIds.value = new Set([track.id])
    lastSelectedIndex.value = index
    selectionAnchorIndex.value = index
  }
}

function playTrack(track: Track, index: number) {
  playerStore.playTracks(sortedTracks.value, index)
  emit('playTrack', track, index)
}

function onTrackContextMenu(e: MouseEvent, track: Track, index: number) {
  if (selectedIds.value.size > 1 && selectedIds.value.has(track.id)) {
    const selectedTracks = sortedTracks.value.filter(t => selectedIds.value.has(t.id))
    trackContextMenu.value?.openMulti(e, selectedTracks, { playlistId: props.playlistId })
  } else {
    selectedIds.value = new Set([track.id])
    lastSelectedIndex.value = index
    selectionAnchorIndex.value = index
    trackContextMenu.value?.open(e, track, {
      excludePlayNext: false,
      showRemoveFromQueue: props.contextMenuOptions?.showRemoveFromQueue,
      playlistId: props.playlistId,
      onNavigateAlbum: (name: string, artist: string) => emit('navigateAlbum', name, artist),
      onNavigateArtist: (name: string) => emit('navigateArtist', name),
    })
  }
}

function scrollToCurrentTrack() {
  if (!scrollerRef.value) return
  scrollerRef.value.scrollToItem(playerStore.queueIndex)
}

const dragOverIndex = ref<number | null>(null)

function onDragOver(e: DragEvent) {
  if (!props.allowDnd) return
  e.preventDefault()
  const rows = trackTableEl.value?.querySelectorAll('[data-track-index]')
  if (!rows) return
  let closest = -1
  let closestOffset = Number.NEGATIVE_INFINITY
  rows.forEach((row) => {
    const rect = row.getBoundingClientRect()
    const offset = e.clientY - rect.top - rect.height / 2
    if (offset < 0 && offset > closestOffset) {
      closestOffset = offset
      closest = Number((row as HTMLElement).dataset.trackIndex)
    }
  })
  dragOverIndex.value = closest
}

function onDrop(e: DragEvent) {
  if (!props.allowDnd) return
  e.preventDefault()
  dragOverIndex.value = null
  try {
    const data = JSON.parse(e.dataTransfer?.getData('text/plain') || '{}')
    const fromIndex = data.index as number
    if (fromIndex === undefined) return
    const newTracks = [...sortedTracks.value]
    const [moved] = newTracks.splice(fromIndex, 1)
    newTracks.splice(fromIndex >= newTracks.length ? newTracks.length : fromIndex, 0, moved)
    emit('reorder', newTracks)
  } catch {}
}

function onDragLeave() { dragOverIndex.value = null }

defineExpose({ scrollToCurrentTrack })
</script>

<template>
  <div ref="trackTableEl"
    class="flex flex-col h-full overflow-hidden"
    :class="variant === 'glass' ? 'dark' : ''"
    @dragover="onDragOver" @drop="onDrop" @dragleave="onDragLeave">

    <div ref="headerContainerRef" v-if="!hideHeader" class="overflow-hidden flex-shrink-0">
      <div :style="{ minWidth: totalMinWidth }">
        <TrackTableHeader
          :grid-template-columns="gridTemplateColumns"
          :ordered-visible-columns="orderedVisibleColumns"
          :sort-column="sortColumn"
          :sort-dir="sortDir"
          :variant="variant"
          @sort="handleSort"
          @reorder-column="handleReorderColumn"
          @resize-column="handleResizeColumn" />
      </div>
    </div>

    <div v-if="isLoading" class="flex items-center justify-center py-12 text-foreground opacity-50 text-sm">Loading...</div>
    <div v-else-if="sortedTracks.length === 0"
      class="flex flex-col items-center justify-center py-16 gap-2 text-foreground/40">
      <Music class="w-8 h-8" />
      <span class="text-sm">No tracks found</span>
    </div>

    <div v-else class="flex flex-col flex-1 min-h-0">
      <RecycleScroller ref="scrollerRef" :items="sortedTracks" :item-size="scrollerItemSize" key-field="id"
        :buffer="10" :prerender="8" flow-mode class="flex-1 custom-scrollbar"
        @vue:mounted="attachScrollerScroll">
        <template #before>
          <div v-if="scrollToCurrent" class="flex items-center justify-center py-2">
            <button @click="scrollToCurrentTrack" class="text-xs text-foreground/40 hover:text-foreground transition-colors flex items-center gap-1">
              Scroll to current track
            </button>
          </div>
        </template>
        <template #default="{ item, index }">
          <TrackTableRow :track="item" :index="index"
            :is-selected="selectedIds.has(item.id)"
            :ordered-visible-columns="orderedVisibleColumns"
            :grid-template-columns="gridTemplateColumns"
            :draggable="allowDnd"
            :variant="variant"
            :collapsed="settings.collapsedMode.value"
            @play-track="playTrack"
            @click="(e) => handleTrackClick(e, item, index)"
            @contextmenu="(e) => onTrackContextMenu(e, item, index)"
            @navigate-album="(name, artist) => emit('navigateAlbum', name, artist)"
            @navigate-artist="(name) => emit('navigateArtist', name)" />
        </template>
      </RecycleScroller>
    </div>
  </div>
  <TrackContextMenu ref="trackContextMenu" />
</template>
