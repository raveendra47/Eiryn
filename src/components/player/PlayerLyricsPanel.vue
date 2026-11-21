<script setup lang="ts">
import PlayerLyrics from '../PlayerLyrics.vue'

defineProps<{
  lyrics?: string
  loading: boolean
  position: number
}>()

const emit = defineEmits<{
  seek: [time: number]
  'search-lyrics': []
}>()
</script>

<template>
  <div class="h-full w-full flex flex-col overflow-hidden">
    <PlayerLyrics
      :lyrics="lyrics"
      :is-loading="loading"
      :current-position="position"
      variant="ambient"
      class="dark"
      @seek="(time) => emit('seek', time)"
      @search-lyrics="emit('search-lyrics')"
    />
  </div>
</template>
