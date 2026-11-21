<script setup lang="ts">
interface PlainLine {
  primary: string
  secondary?: string
}

withDefaults(defineProps<{
  lines: PlainLine[]
  variant?: 'drawer' | 'ambient'
}>(), { variant: 'drawer' })
</script>

<template>
  <div class="h-full overflow-y-auto px-8 py-16 scrollbar-hide" :class="{ 'lyrics-mask': variant === 'ambient' }">
    <div class="max-w-2xl mx-auto space-y-4">
      <div
        v-for="(line, index) in lines"
        :key="index"
        class="leading-relaxed select-text"
      >
        <p :class="variant === 'ambient'
          ? 'text-[clamp(1.25rem,2vw,1.75rem)] font-semibold text-white/50'
          : 'text-lg md:text-xl font-medium text-white/70'">{{ line.primary }}</p>
        <p v-if="line.secondary" :class="variant === 'ambient'
          ? 'text-base md:text-lg text-white/35 mt-1'
          : 'text-base md:text-lg font-medium text-white/40 mt-0.5'">{{ line.secondary }}</p>
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
