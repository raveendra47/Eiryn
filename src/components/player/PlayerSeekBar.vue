<script setup lang="ts">
import { ref, computed } from 'vue'

const props = withDefaults(defineProps<{
  progressPercent: number
  position: number
  duration: number
  variant?: 'default' | 'ambient'
}>(), { variant: 'default' })

const emit = defineEmits<{
  seek: [value: number]
}>()

const isSeeking = ref(false)
const seekValue = ref(0)

const displayPosition = computed(() =>
  isSeeking.value ? (seekValue.value / 100) * props.duration : props.position,
)

const shownPercent = computed(() =>
  isSeeking.value ? seekValue.value : props.progressPercent,
)

const remaining = computed(() => {
  const rem = props.duration - displayPosition.value
  return rem > 0 ? rem : 0
})

function formatTime(secs: number): string {
  if (!secs || !isFinite(secs)) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${s.toString().padStart(2, '0')}`
}

function onSeekStart() {
  isSeeking.value = true
  seekValue.value = props.progressPercent
}

function onSeekEnd() {
  emit('seek', (seekValue.value / 100) * props.duration)
  isSeeking.value = false
}
</script>

<template>
  <div v-if="variant === 'ambient'" class="w-full space-y-1.5">
    <input
      type="range"
      :value="shownPercent"
      min="0"
      max="100"
      step="0.1"
      class="w-full seek-ambient"
      :style="{ '--pct': shownPercent + '%' }"
      @input="(v) => (seekValue = parseFloat((v.target as HTMLInputElement).value))"
      @mousedown="onSeekStart"
      @mouseup="onSeekEnd"
      @touchstart="onSeekStart"
      @touchend="onSeekEnd"
    />
    <div class="flex justify-between text-xs text-white/60 tabular-nums">
      <span>{{ formatTime(displayPosition) }}</span>
      <span>-{{ formatTime(remaining) }}</span>
    </div>
  </div>

  <div v-else class="w-full max-w-sm space-y-1.5">
    <input
      type="range"
      :value="isSeeking ? seekValue : progressPercent"
      min="0"
      max="100"
      step="0.1"
      class="w-full accent-primary"
      @input="(v) => (seekValue = parseFloat((v.target as HTMLInputElement).value))"
      @mousedown="onSeekStart"
      @mouseup="onSeekEnd"
      @touchstart="onSeekStart"
      @touchend="onSeekEnd"
    />
    <div class="flex justify-between text-[10.5px] text-white/60 tabular-nums">
      <span>{{ formatTime(displayPosition) }}</span>
      <span>{{ formatTime(duration) }}</span>
    </div>
  </div>
</template>

<style scoped>
.seek-ambient {
  -webkit-appearance: none;
  appearance: none;
  height: 5px;
  border-radius: 999px;
  outline: none;
  cursor: pointer;
  background: linear-gradient(to right, #ffffff var(--pct), rgba(255, 255, 255, 0.18) var(--pct));
}
.seek-ambient::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: #ffffff;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  opacity: 0;
  transition: opacity 0.15s ease;
}
.seek-ambient:hover::-webkit-slider-thumb,
.seek-ambient:active::-webkit-slider-thumb,
.seek-ambient:focus-visible::-webkit-slider-thumb {
  opacity: 1;
}
.seek-ambient::-moz-range-thumb {
  width: 13px;
  height: 13px;
  border: none;
  border-radius: 50%;
  background: #ffffff;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  opacity: 0;
  transition: opacity 0.15s ease;
}
.seek-ambient:hover::-moz-range-thumb,
.seek-ambient:active::-moz-range-thumb,
.seek-ambient:focus-visible::-moz-range-thumb {
  opacity: 1;
}
</style>
