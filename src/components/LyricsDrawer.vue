<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Mic2, X, Search } from '@lucide/vue'
import { usePlayerStore } from '../stores/player'
import { useLyrics } from '../composables/useLyrics'
import FindLyricsDialog from './FindLyricsDialog.vue'

const store = usePlayerStore()

const { isSynced, syncedLines, plainLines } = useLyrics(computed(() => store.lyricsContent))

const isLoading = computed(() => store.isLoadingLyrics)
const findLyricsOpen = ref(false)

const activeIndex = computed(() => {
  if (!isSynced.value) return -1
  const pos = store.position
  const idx = [...syncedLines.value].reverse().findIndex(line => line.time <= pos)
  return idx !== -1 ? syncedLines.value.length - 1 - idx : -1
})

const scrollContainer = ref<HTMLElement | null>(null)
const lineRefs = ref<(HTMLElement | null)[]>([])

function setLineRef(el: any, i: number) {
  if (el) {
    lineRefs.value[i] = el as HTMLElement
  }
}

watch(syncedLines, () => {
  lineRefs.value = []
}, { flush: 'post' })

watch(activeIndex, (newIndex) => {
  if (newIndex === -1 || !lineRefs.value[newIndex] || !scrollContainer.value) return
  const el = lineRefs.value[newIndex]
  el?.scrollIntoView({ behavior: 'smooth', block: 'center' })
})
</script>

<template>
  <div class="h-full w-full bg-background/80 backdrop-blur-2xl flex flex-col">
    <div class="flex items-center justify-between px-4 py-3 border-b border-foreground/[0.06]">
      <div class="flex items-center gap-2 font-semibold">
        <Mic2 class="w-4 h-4 text-primary" />
        <span>Lyrics</span>
      </div>
      <div class="flex items-center gap-1">
        <button
          v-if="store.currentTrack"
          class="p-1.5 rounded-full hover:bg-foreground/10 transition-colors text-foreground opacity-60 hover:opacity-100"
          title="Search Lyrics Online"
          @click="findLyricsOpen = true">
          <Search class="w-4 h-4" />
        </button>
        <button
          class="p-1.5 rounded-full hover:bg-foreground/10 transition-colors text-foreground opacity-60 hover:opacity-100"
          @click="store.toggleLyrics()">
          <X class="w-4 h-4" />
        </button>
      </div>
    </div>

    <div ref="scrollContainer" class="flex-1 overflow-y-auto custom-scrollbar px-6 py-8">
      <div v-if="!store.currentTrack" class="h-full flex flex-col items-center justify-center text-foreground opacity-50 gap-3">
        <Mic2 class="w-10 h-10 opacity-20" />
        <p class="text-sm">No track playing</p>
      </div>

      <div v-else-if="isLoading" class="space-y-4">
        <div v-for="i in 8" :key="i" class="h-5 bg-foreground/5 rounded animate-pulse" :style="{ width: (60 + Math.random() * 30) + '%' }" />
      </div>

      <div v-else-if="isSynced" class="space-y-4">
        <div v-for="(line, i) in syncedLines" :key="i" :ref="(el: any) => setLineRef(el, i)"
          class="text-base transition-all duration-300 cursor-pointer"
          :class="i === activeIndex ? 'text-primary font-medium scale-105' : 'text-foreground opacity-50 hover:opacity-80'"
          @click="store.seek(line.time)">
          {{ line.text || '\u00A0' }}
        </div>
      </div>

      <div v-else-if="plainLines.length" class="space-y-3">
        <p v-for="(line, i) in plainLines" :key="i" class="text-base text-foreground opacity-70 leading-relaxed">
          {{ line.primary }}<span v-if="line.secondary" class="opacity-50"> / {{ line.secondary }}</span>
        </p>
      </div>

      <div v-else class="h-full flex flex-col items-center justify-center text-foreground opacity-50 gap-3 text-center px-4">
        <Mic2 class="w-10 h-10 opacity-20" />
        <p class="text-sm">No lyrics available</p>
        <button
          v-if="store.currentTrack"
          class="mt-2 text-xs text-primary hover:underline bg-primary/10 hover:bg-primary/20 px-3 py-1.5 rounded-full transition-colors"
          @click="findLyricsOpen = true"
        >
          Search lyrics online
        </button>
      </div>
    </div>

    <FindLyricsDialog v-model:open="findLyricsOpen" :track="store.currentTrack" />
  </div>
</template>
