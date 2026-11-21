<script setup lang="ts">
import { ref, computed } from 'vue'
import { Moon } from '@lucide/vue'
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()
const isOpen = ref(false)

const isActive = computed(() => playerStore.sleepTimerMinutes > 0)
const remainingText = computed(() => {
  const rem = playerStore.sleepTimerRemaining
  if (rem <= 0) return ''
  const m = Math.floor(rem / 60)
  const s = rem % 60
  return `${m}:${s < 10 ? '0' : ''}${s}`
})

const options = [
  { label: 'Off', minutes: 0 },
  { label: '15 minutes', minutes: 15 },
  { label: '30 minutes', minutes: 30 },
  { label: '45 minutes', minutes: 45 },
  { label: '1 hour', minutes: 60 },
  { label: '2 hours', minutes: 120 },
]

function selectMinutes(mins: number) {
  if (mins === 0) {
    playerStore.cancelSleepTimer()
  } else {
    playerStore.setSleepTimer(mins)
  }
  isOpen.value = false
}
</script>

<template>
  <div class="relative inline-block text-left">
    <button
      type="button"
      class="flex items-center gap-1.5 p-2 rounded-lg text-foreground/70 hover:text-foreground hover:bg-white/5 transition-colors relative"
      :class="{ 'text-primary': isActive }"
      :title="isActive ? `Sleep Timer: ${remainingText} remaining` : 'Sleep Timer'"
      @click="isOpen = !isOpen"
    >
      <Moon class="w-4 h-4" />
      <span v-if="isActive" class="text-xs font-mono font-medium text-primary">
        {{ remainingText }}
      </span>
    </button>

    <!-- Backdrop -->
    <div
      v-if="isOpen"
      class="fixed inset-0 z-40"
      @click="isOpen = false"
    />

    <!-- Dropdown menu -->
    <div
      v-if="isOpen"
      class="absolute bottom-full right-0 mb-2 w-44 rounded-xl border border-white/10 bg-background/95 backdrop-blur-md shadow-2xl py-1 z-50 animate-in fade-in zoom-in-95 duration-100"
    >
      <div class="px-3 py-1.5 text-[11px] font-semibold text-muted-foreground uppercase tracking-wider border-b border-white/5">
        Sleep Timer
      </div>
      <button
        v-for="opt in options"
        :key="opt.minutes"
        type="button"
        class="w-full text-left px-3 py-1.5 text-xs text-foreground/80 hover:text-foreground hover:bg-white/10 flex items-center justify-between transition-colors"
        :class="{ 'text-primary font-bold bg-primary/10': playerStore.sleepTimerMinutes === opt.minutes }"
        @click="selectMinutes(opt.minutes)"
      >
        <span>{{ opt.label }}</span>
        <span v-if="playerStore.sleepTimerMinutes === opt.minutes" class="text-primary text-[10px]">●</span>
      </button>
    </div>
  </div>
</template>
