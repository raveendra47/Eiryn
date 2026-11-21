<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import {
  Search, Play, Pause, SkipForward, SkipBack, Shuffle, Repeat,
  Volume2, VolumeX, Maximize2, Minimize2, Music, Disc, User,
  Sliders, Activity, Settings, Heart, Radio, ListPlus, X, Command
} from '@lucide/vue'
import { usePlayerStore } from '../stores/player'
import { useLibraryStore } from '../stores/library'
import { useAppStore } from '../stores/app'
import type { Track, Album, Artist } from '../lib/invoke'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  'update:open': [val: boolean]
}>()

const router = useRouter()
const player = usePlayerStore()
const library = useLibraryStore()
const app = useAppStore()

const inputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLElement | null>(null)
const query = ref('')
const selectedIndex = ref(0)

interface CommandItem {
  id: string
  title: string
  subtitle?: string
  icon: any
  category: 'Actions' | 'Navigation' | 'Tracks' | 'Albums' | 'Artists'
  action: () => void
}

const staticActions: CommandItem[] = [
  {
    id: 'toggle-play',
    title: 'Play / Pause',
    subtitle: 'Toggle audio playback',
    icon: Play,
    category: 'Actions',
    action: () => player.togglePlayPause(),
  },
  {
    id: 'next-track',
    title: 'Next Track',
    subtitle: 'Skip to following song',
    icon: SkipForward,
    category: 'Actions',
    action: () => player.next(),
  },
  {
    id: 'prev-track',
    title: 'Previous Track',
    subtitle: 'Return to start / previous song',
    icon: SkipBack,
    category: 'Actions',
    action: () => player.previous(),
  },
  {
    id: 'toggle-shuffle',
    title: 'Toggle Shuffle',
    subtitle: 'Randomize playlist playback order',
    icon: Shuffle,
    category: 'Actions',
    action: () => player.setShuffle(!player.shuffle),
  },
  {
    id: 'cycle-repeat',
    title: 'Cycle Repeat Mode',
    subtitle: 'Off → Repeat All → Repeat One',
    icon: Repeat,
    category: 'Actions',
    action: () => player.cycleRepeat(),
  },
  {
    id: 'toggle-mute',
    title: 'Toggle Mute',
    subtitle: 'Silence or restore volume',
    icon: Volume2,
    category: 'Actions',
    action: () => player.toggleMute(),
  },
  {
    id: 'toggle-fullscreen',
    title: 'Toggle Living Artwork / Fullscreen',
    subtitle: 'Open immersive Now Playing view',
    icon: Maximize2,
    category: 'Actions',
    action: () => player.setMode(player.playerMode === 'fullscreen' ? 'sticky' : 'fullscreen'),
  },
  {
    id: 'toggle-mini',
    title: 'Toggle Mini Player',
    subtitle: 'Floating desktop widget',
    icon: Minimize2,
    category: 'Actions',
    action: () => player.toggleMiniWindow(),
  },
  {
    id: 'nav-home',
    title: 'Go to Home / Overview',
    subtitle: 'Recently played & highlights',
    icon: Music,
    category: 'Navigation',
    action: () => router.push('/'),
  },
  {
    id: 'nav-tracks',
    title: 'Go to Tracks',
    subtitle: 'All songs in library',
    icon: Music,
    category: 'Navigation',
    action: () => router.push('/tracks'),
  },
  {
    id: 'nav-albums',
    title: 'Go to Albums',
    subtitle: 'Browse album discography',
    icon: Disc,
    category: 'Navigation',
    action: () => router.push('/albums'),
  },
  {
    id: 'nav-artists',
    title: 'Go to Artists',
    subtitle: 'Browse library performers',
    icon: User,
    category: 'Navigation',
    action: () => router.push('/artists'),
  },
  {
    id: 'nav-sound',
    title: 'Sound & DSP (EQ / Resampling)',
    subtitle: 'Equalizer, Headroom & Audio Engine',
    icon: Sliders,
    category: 'Navigation',
    action: () => router.push('/sound'),
  },
  {
    id: 'nav-health',
    title: 'Library Health & Needs Attention',
    subtitle: 'Triage fake lossless, duplicates & missing tags',
    icon: Activity,
    category: 'Navigation',
    action: () => router.push('/library'),
  },
  {
    id: 'nav-settings',
    title: 'Open Settings',
    subtitle: 'Audio output, themes & preferences',
    icon: Settings,
    category: 'Navigation',
    action: () => router.push('/settings'),
  },
]

const filteredItems = computed<CommandItem[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) {
    return staticActions
  }

  // Filter actions & navigation
  const matchingStatic = staticActions.filter(item =>
    item.title.toLowerCase().includes(q) || (item.subtitle && item.subtitle.toLowerCase().includes(q))
  )

  // Search tracks in library
  const trackMatches: CommandItem[] = library.tracks
    .filter(t => t.title.toLowerCase().includes(q) || (t.artist && t.artist.toLowerCase().includes(q)))
    .slice(0, 8)
    .map(t => ({
      id: `track-${t.id}`,
      title: t.title,
      subtitle: `${t.artist || 'Unknown Artist'} · ${t.album || ''}`,
      icon: Music,
      category: 'Tracks',
      action: () => {
        player.playTracks([t], 0)
      },
    }))

  // Search albums in library
  const albumMatches: CommandItem[] = library.albums
    .filter(a => a.name.toLowerCase().includes(q) || (a.artist && a.artist.toLowerCase().includes(q)))
    .slice(0, 5)
    .map(a => ({
      id: `album-${a.id}`,
      title: a.name,
      subtitle: `${a.artist || 'Unknown Artist'} · ${a.track_count || 0} songs`,
      icon: Disc,
      category: 'Albums',
      action: () => router.push(`/albums/${a.id}`),
    }))

  // Search artists in library
  const artistMatches: CommandItem[] = library.artists
    .filter(ar => ar.name.toLowerCase().includes(q))
    .slice(0, 4)
    .map(ar => ({
      id: `artist-${ar.name}`,
      title: ar.name,
      subtitle: `${ar.track_count || 0} songs`,
      icon: User,
      category: 'Artists',
      action: () => router.push(`/artists/${encodeURIComponent(ar.name)}`),
    }))

  return [...matchingStatic, ...trackMatches, ...albumMatches, ...artistMatches]
})

function close() {
  emit('update:open', false)
  query.value = ''
  selectedIndex.value = 0
}

function execute(item: CommandItem) {
  close()
  item.action()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (filteredItems.value.length > 0) {
      selectedIndex.value = (selectedIndex.value + 1) % filteredItems.value.length
      scrollToSelected()
    }
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    if (filteredItems.value.length > 0) {
      selectedIndex.value = (selectedIndex.value - 1 + filteredItems.value.length) % filteredItems.value.length
      scrollToSelected()
    }
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const item = filteredItems.value[selectedIndex.value]
    if (item) execute(item)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    close()
  }
}

function scrollToSelected() {
  nextTick(() => {
    const el = listRef.value?.querySelector(`[data-index="${selectedIndex.value}"]`) as HTMLElement
    el?.scrollIntoView({ block: 'nearest' })
  })
}

watch(() => props.open, (isOpen) => {
  if (isOpen) {
    selectedIndex.value = 0
    nextTick(() => {
      inputRef.value?.focus()
      inputRef.value?.select()
    })
  }
})
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="opacity-0 scale-98"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-98"
    >
      <div
        v-if="open"
        class="fixed inset-0 z-[9999] flex items-start justify-center pt-[12vh] px-4 bg-black/60 backdrop-blur-md select-none"
        @click.self="close"
        @keydown="onKeydown"
      >
        <div class="w-full max-w-xl bg-zinc-900/95 border border-white/10 rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[70vh] backdrop-filter">
          <!-- Search Header -->
          <div class="flex items-center px-4 py-3.5 border-b border-white/10 gap-3">
            <Search class="w-5 h-5 text-white/40 shrink-0" />
            <input
              ref="inputRef"
              v-model="query"
              type="text"
              placeholder="Type a command, page, track, or artist..."
              class="flex-1 bg-transparent border-0 text-white placeholder-white/40 text-base focus:outline-none"
            />
            <kbd class="px-2 py-0.5 bg-white/10 rounded text-[11px] text-white/50 font-mono">Esc</kbd>
          </div>

          <!-- Results List -->
          <div
            ref="listRef"
            class="flex-1 overflow-y-auto p-2 space-y-1 custom-scrollbar min-h-[140px]"
          >
            <div
              v-if="filteredItems.length === 0"
              class="py-10 text-center text-white/40 text-sm"
            >
              No matching commands or music found for "{{ query }}"
            </div>

            <div
              v-for="(item, idx) in filteredItems"
              :key="item.id"
              :data-index="idx"
              class="flex items-center gap-3 px-3 py-2.5 rounded-xl cursor-pointer transition-colors duration-100 group"
              :class="idx === selectedIndex ? 'bg-primary text-white shadow-lg' : 'hover:bg-white/5 text-white/80'"
              @mouseenter="selectedIndex = idx"
              @click="execute(item)"
            >
              <div
                class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                :class="idx === selectedIndex ? 'bg-white/20 text-white' : 'bg-white/5 text-white/60'"
              >
                <component :is="item.icon" class="w-4 h-4" />
              </div>

              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium truncate" :class="idx === selectedIndex ? 'text-white' : 'text-white/90'">
                  {{ item.title }}
                </div>
                <div
                  v-if="item.subtitle"
                  class="text-xs truncate"
                  :class="idx === selectedIndex ? 'text-white/80' : 'text-white/50'"
                >
                  {{ item.subtitle }}
                </div>
              </div>

              <span
                class="text-[11px] px-2 py-0.5 rounded-full uppercase tracking-wider font-semibold shrink-0"
                :class="idx === selectedIndex ? 'bg-white/20 text-white' : 'bg-white/5 text-white/40'"
              >
                {{ item.category }}
              </span>
            </div>
          </div>

          <!-- Footer Shortcut Hints -->
          <div class="px-4 py-2 border-t border-white/5 bg-black/20 flex items-center justify-between text-[11px] text-white/40">
            <div class="flex items-center gap-3">
              <span><kbd class="px-1.5 py-0.5 bg-white/10 rounded font-mono">↑</kbd> <kbd class="px-1.5 py-0.5 bg-white/10 rounded font-mono">↓</kbd> Navigate</span>
              <span><kbd class="px-1.5 py-0.5 bg-white/10 rounded font-mono">↵</kbd> Select</span>
            </div>
            <span><kbd class="px-1.5 py-0.5 bg-white/10 rounded font-mono">Ctrl+K</kbd> to toggle</span>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
