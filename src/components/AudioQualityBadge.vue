<script setup lang="ts">
import { computed } from 'vue'
import { Music, AudioLines, Zap, Crown } from '@lucide/vue'
import type { Track } from '../lib/invoke'
import { determineAudioQuality } from '../lib/audioQuality'

const props = defineProps<{
  track: Pick<Track, 'file_format' | 'bit_depth' | 'sample_rate'> | null | undefined
}>()

const tier = computed(() => (props.track ? determineAudioQuality(props.track) : 'UNKNOWN'))

const badge = computed(() => {
  switch (tier.value) {
    case 'LOSSY':
      return { icon: Music, label: 'Lossy', cls: 'bg-foreground/8 text-foreground/50 border-foreground/15' }
    case 'HI-RES':
      return { icon: Zap, label: 'Hi-Res', cls: 'bg-amber-500/10 text-amber-500 border-amber-500/30' }
    case 'DSD':
      return { icon: Crown, label: 'DSD', cls: 'bg-fuchsia-500/10 text-fuchsia-400 border-fuchsia-400/30' }
    case 'LOSSLESS':
      return { icon: AudioLines, label: 'Lossless', cls: 'bg-primary/10 text-primary border-primary/20' }
    default:
      return null
  }
})
</script>

<template>
  <span
    v-if="badge"
    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full border select-none"
    :class="badge.cls">
    <component :is="badge.icon" class="w-3 h-3" />
    <span class="text-[9px] font-bold uppercase tracking-wider">{{ badge.label }}</span>
  </span>
</template>
