<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { LyricWord } from '../composables/useLyrics'

interface LyricLine {
  time: number
  text: string
  secondary?: string
  words?: LyricWord[]
}

const props = withDefaults(defineProps<{
  lines: LyricLine[]
  currentPosition: number
  variant?: 'drawer' | 'ambient'
}>(), { variant: 'drawer' })

const emit = defineEmits<{
  seek: [time: number]
}>()

const activeIndex = computed(() => {
  // Cadence-style look-ahead: highlight a hair early so lines land with the
  // singing instead of after it.
  const t = props.currentPosition + 0.25
  const idx = [...props.lines].reverse().findIndex(
    line => line.time <= t
  )
  return idx !== -1 ? props.lines.length - 1 - idx : -1
})

const activeLine = computed(() =>
  activeIndex.value >= 0 ? props.lines[activeIndex.value] : null
)

const activeWordIndex = computed(() => {
  const line = activeLine.value
  if (!line?.words?.length) return -1
  const idx = [...line.words].reverse().findIndex(
    w => w.time <= props.currentPosition
  )
  return idx !== -1 ? line.words.length - 1 - idx : -1
})

const activeWordProgress = computed(() => {
  const line = activeLine.value
  if (!line?.words?.length || activeWordIndex.value < 0) return 0
  const words = line.words
  const word = words[activeWordIndex.value]
  const nextWord = words[activeWordIndex.value + 1]
  const wordEnd = nextWord ? nextWord.time : line.time + 3
  const wordLen = wordEnd - word.time
  if (wordLen <= 0) return 1
  return Math.min(1, Math.max(0, (props.currentPosition - word.time) / wordLen))
})

function wordStyle(wordIdx: number): Record<string, string> {
  const style: Record<string, string> = {}
  const ambient = props.variant === 'ambient'
  if (wordIdx < activeWordIndex.value) {
    style.opacity = '1'
    style.color = 'inherit'
  } else if (wordIdx === activeWordIndex.value) {
    const pct = (1 - activeWordProgress.value) * 100
    style.clipPath = `inset(0 ${pct}% 0 0)`
    style.opacity = '1'
    style.color = 'inherit'
  } else if (ambient) {
    style.color = 'rgba(255, 255, 255, 0.38)'
    style.opacity = '1'
  } else {
    style.opacity = '0.35'
  }
  return style
}

function lineClass(index: number): string {
  if (props.variant === 'ambient') {
    if (index === activeIndex.value) return 'text-white scale-[1.02] blur-none opacity-100'
    if (index < activeIndex.value) return 'text-white/45 blur-[0.6px] opacity-100'
    return 'text-white/32 blur-[0.6px] opacity-100 hover:text-white/50'
  }
  return index === activeIndex.value
    ? 'text-white scale-105 blur-none opacity-100'
    : index < activeIndex.value
      ? 'text-white/20 blur-[0.5px] opacity-60 hover:text-white/40'
      : 'text-white/30 blur-[1px] opacity-40 hover:text-white/60 hover:blur-none'
}

const scrollContainer = ref<HTMLElement | null>(null)
const lineRefs = ref<(HTMLElement | null)[]>([])

function setLineRef(el: any, index: number) {
  if (el) {
    lineRefs.value[index] = el as HTMLElement
  }
}

watch(() => props.lines, () => {
  lineRefs.value = []
}, { flush: 'post' })

// Auto-scroll pauses for 4 s after the user scrolls (Cadence parity) so it
// doesn't yank the view back mid-read.
let userScrolledAt = 0
function onUserScroll() {
  userScrolledAt = Date.now()
}

watch(activeIndex, (newIndex) => {
  if (newIndex === -1 || !lineRefs.value[newIndex] || !scrollContainer.value) return
  if (Date.now() - userScrolledAt < 4000) return
  const el = lineRefs.value[newIndex]
  el?.scrollIntoView({ behavior: 'smooth', block: 'center' })
})
</script>

<template>
  <div ref="scrollContainer" @wheel.passive="onUserScroll" @touchstart.passive="onUserScroll"
    class="h-full overflow-y-auto px-8 py-48 scrollbar-hide scroll-smooth"
    :class="{ 'lyrics-mask': variant === 'ambient' }">
    <div class="max-w-2xl mx-auto space-y-10">
      <div
        v-for="(line, index) in lines"
        :key="index"
        :ref="(el: any) => setLineRef(el, index)"
        class="transition-all duration-100 cursor-pointer select-none origin-left py-2"
        :class="[
          variant === 'ambient'
            ? 'text-[clamp(1.375rem,2.4vw,2.125rem)] font-bold tracking-tight leading-snug'
            : 'text-2xl md:text-4xl font-extrabold',
          lineClass(index),
        ]"
        @click="emit('seek', line.time)"
      >
        <!-- Render with word-level karaoke if available -->
        <div v-if="index === activeIndex && line.words?.length">
          <span
            v-for="(w, wi) in line.words"
            :key="wi"
            :style="wordStyle(wi)"
            class="transition-all duration-75"
          >{{ w.text }}</span>
        </div>
        <div v-else>{{ line.text }}</div>
        <div v-if="line.secondary" class="text-lg md:text-2xl font-bold mt-1 opacity-80">{{ line.secondary }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lyrics-mask {
  mask-image: linear-gradient(to bottom, transparent, black 12%, black 88%, transparent);
  -webkit-mask-image: linear-gradient(to bottom, transparent, black 12%, black 88%, transparent);
}
</style>
