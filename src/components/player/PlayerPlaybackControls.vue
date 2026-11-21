<script setup lang="ts">
import { computed } from 'vue'
import {
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
} from '@lucide/vue'
import PlayerControlButton from './PlayerControlButton.vue'

const props = defineProps<{
  isPlaying: boolean
  shuffle: boolean
  repeatMode: 'off' | 'all' | 'one'
  showIndicator: boolean
  spread?: boolean
}>()

const emit = defineEmits<{
  togglePlay: []
  next: []
  previous: []
  toggleShuffle: []
  cycleRepeat: []
}>()

const repeatIcon = computed(() =>
  props.repeatMode === 'one' ? Repeat1 : Repeat,
)
const repeatActive = computed(
  () => props.repeatMode === 'one' || props.repeatMode === 'all',
)
const repeatTitle = computed(() => {
  switch (props.repeatMode) {
    case 'one':
      return 'Repeat One'
    case 'all':
      return 'Repeat All'
    default:
      return 'Repeat Off'
  }
})
</script>

<template>
  <div :class="spread ? 'flex items-center justify-between w-full' : 'flex items-center gap-7'">
    <PlayerControlButton
      :class="shuffle ? 'text-white/80' : 'text-white/30 hover:text-white/80'"
      class="transition-all duration-100 active:scale-90"
      :active="shuffle"
      :show-indicator="showIndicator"
      dot-class="bg-white"
      @click="emit('toggleShuffle')"
    >
      <Shuffle class="w-5 h-5" />
    </PlayerControlButton>
    <button
      class="text-white/80 hover:text-white transition-all duration-100 active:scale-90"
      @click="emit('previous')">
      <SkipBack class="w-7 h-7 fill-current" />
    </button>
    <button
      :class="spread ? 'w-[74px] h-[74px]' : 'w-14 h-14'"
      class="bg-white rounded-full flex items-center justify-center hover:scale-105 active:scale-95 transition-transform shadow-xl"
      @click="emit('togglePlay')">
      <Pause v-if="isPlaying" :class="spread ? 'w-9 h-9' : 'w-6 h-6'" class="fill-current text-[#0A0A0A]" />
      <Play v-else :class="spread ? 'w-9 h-9' : 'w-6 h-6'" class="fill-current text-[#0A0A0A] ml-0.5" />
    </button>
    <button
      class="text-white/80 hover:text-white transition-all duration-100 active:scale-90"
      @click="emit('next')">
      <SkipForward class="w-7 h-7 fill-current" />
    </button>
    <PlayerControlButton
      :class="repeatActive ? 'text-white/80' : 'text-white/30 hover:text-white/80'"
      class="transition-all duration-100 active:scale-90"
      :active="repeatActive"
      :show-indicator="showIndicator"
      dot-class="bg-white"
      :title="repeatTitle"
      @click="emit('cycleRepeat')"
    >
      <component :is="repeatIcon" class="w-5 h-5" />
    </PlayerControlButton>
  </div>
</template>
