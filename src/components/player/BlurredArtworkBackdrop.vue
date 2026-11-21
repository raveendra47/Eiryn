<script setup lang="ts">
defineProps<{
  artworkUrl?: string | null
  opacity?: number
}>()
</script>

<template>
  <div class="absolute inset-0 overflow-hidden pointer-events-none select-none">
    <div
      v-if="artworkUrl"
      class="absolute inset-[-15%] bg-cover bg-center transition-all duration-700 ease-out"
      :style="{
        backgroundImage: `url(${artworkUrl})`,
        filter: 'blur(90px) saturate(1.6) brightness(0.7)',
        transform: 'scale(1.18)',
      }"
    />
    <div
      class="absolute inset-0 bg-black/50 transition-opacity duration-500"
      :style="{ opacity: opacity ?? 0.5 }"
    />
  </div>
</template>

<style scoped>
</style>
