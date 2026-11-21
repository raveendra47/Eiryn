<script setup lang="ts">
import { computed } from 'vue'
import { Music } from '@lucide/vue'

const props = withDefaults(defineProps<{
  artworkUrl?: string | null
  trackTitle: string
  isPlaying: boolean
  size?: 'sm' | 'md' | 'lg' | 'full'
}>(), { size: 'lg' })

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm': return 'h-12 w-12'
    case 'md': return 'h-16 w-16'
    case 'lg': return 'w-[clamp(8rem,34vh,20rem)] h-[clamp(8rem,34vh,20rem)]'
    case 'full': return 'w-full h-full'
  }
})

const emit = defineEmits<{
  click: []
  contextmenu: []
}>()
</script>

<template>
  <div
    class="rounded-2xl shadow-[0_24px_60px_rgba(0,0,0,0.6)] overflow-hidden flex-shrink-0 ring-1 ring-white/8 transition-all duration-700 ease-[cubic-bezier(0.4,0,0.2,1)] aspect-square cursor-pointer"
    :class="[sizeClass, isPlaying ? 'scale-100' : 'scale-[0.92]']"
    @click="emit('click')" @contextmenu.prevent="emit('contextmenu')">
    <img v-if="artworkUrl" :src="artworkUrl" :alt="trackTitle"
      class="w-full h-full object-cover" />
    <div v-else class="w-full h-full bg-white/5 flex items-center justify-center">
      <Music class="w-20 h-20 text-white/15" />
    </div>
  </div>
</template>
