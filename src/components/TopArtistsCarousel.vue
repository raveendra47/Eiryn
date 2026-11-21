<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ChevronLeft, ChevronRight, User, Users } from '@lucide/vue'
import * as api from '../lib/invoke'

const props = defineProps<{
  artists: { name: string; seconds: number; plays: number }[]
}>()

const router = useRouter()
const page = ref(0)
const PAGE_SIZE = 5
const artworkUrls = ref(new Map<string, string>())

const pageCount = computed(() => Math.max(1, Math.ceil(props.artists.length / PAGE_SIZE)))
const visibleArtists = computed(() =>
  props.artists.slice(page.value * PAGE_SIZE, page.value * PAGE_SIZE + PAGE_SIZE),
)

function formatTime(secs: number): string {
  const m = Math.round(secs / 60)
  const h = Math.floor(m / 60)
  if (h > 0) return `${h}h ${m % 60}m`
  return `${m}m`
}

async function loadArtwork(name: string) {
  if (artworkUrls.value.has(name)) return
  try {
    const bytes = await api.getArtistArtwork(name, 'md')
    if (bytes && bytes.length > 0) {
      const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' }))
      artworkUrls.value.set(name, url)
    }
  } catch { /* icon fallback */ }
}

function previous() {
  page.value = (page.value - 1 + pageCount.value) % pageCount.value
}

function next() {
  page.value = (page.value + 1) % pageCount.value
}

function openArtist(name: string) {
  router.push(`/artists/${encodeURIComponent(name)}`)
}

watch(() => props.artists, (list) => {
  page.value = 0
  for (const a of list.slice(0, PAGE_SIZE * 2)) void loadArtwork(a.name)
}, { immediate: true })

onMounted(() => {
  for (const a of props.artists.slice(0, PAGE_SIZE * 2)) void loadArtwork(a.name)
})

onUnmounted(() => {
  for (const url of artworkUrls.value.values()) URL.revokeObjectURL(url)
  artworkUrls.value.clear()
})
</script>

<template>
  <article class="flex min-h-[17rem] flex-col rounded-xl border border-[var(--border-glass)] bg-[var(--bg-glass)] p-5 backdrop-blur-[30px]">
    <div class="mb-4 flex items-center gap-2 text-xs font-medium text-[color:var(--text-muted)]">
      <Users class="h-4 w-4" />
      <h2>Top artists</h2>
      <div class="ml-auto flex items-center gap-1">
        <button type="button" :disabled="pageCount <= 1 || page === 0"
          class="flex h-7 w-7 items-center justify-center rounded-full text-[color:var(--text-muted)] transition-all hover:bg-white/[0.04] disabled:opacity-30"
          @click="previous">
          <ChevronLeft class="h-4 w-4" />
        </button>
        <button type="button" :disabled="pageCount <= 1 || page === pageCount - 1"
          class="flex h-7 w-7 items-center justify-center rounded-full text-[color:var(--text-muted)] transition-all hover:bg-white/[0.04] disabled:opacity-30"
          @click="next">
          <ChevronRight class="h-4 w-4" />
        </button>
      </div>
    </div>
    <div v-if="artists.length === 0" class="flex flex-1 flex-col items-center justify-center text-center text-[color:var(--text-muted)]">
      <Users class="mb-2 h-6 w-6 opacity-60" />
      <p class="text-sm">No artist data yet</p>
    </div>
    <div v-else class="grid flex-1 grid-cols-2 gap-4 sm:grid-cols-5">
      <button v-for="(a, i) in visibleArtists" :key="a.name" @click="openArtist(a.name)"
        class="group min-w-0 cursor-pointer text-center">
        <div class="aspect-square overflow-hidden rounded-full bg-foreground/5 ring-1 ring-foreground/[0.06] relative mb-3 flex items-center justify-center">
          <img v-if="artworkUrls.get(a.name)" :src="artworkUrls.get(a.name)" :alt="a.name"
            class="h-full w-full object-cover" loading="lazy" decoding="async" />
          <User v-else class="h-1/2 w-1/2 text-foreground opacity-40" />
          <span class="absolute left-2 top-2 rounded-full bg-black/50 px-2 py-0.5 text-[10px] tabular-nums text-white">
            {{ page * PAGE_SIZE + i + 1 }}
          </span>
        </div>
        <p class="truncate text-sm font-medium group-hover:text-foreground">{{ a.name || 'Unknown Artist' }}</p>
        <p class="mt-0.5 text-xs tabular-nums text-[color:var(--text-muted)]">{{ formatTime(a.seconds) }} · {{ a.plays }}×</p>
      </button>
    </div>
  </article>
</template>
