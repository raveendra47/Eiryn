<script setup lang="ts">
import { Play, Library, Monitor } from '@lucide/vue'

const categories = [
  {
    id: 'playback',
    icon: Play,
    title: 'Playback Control',
    shortcuts: [
      { keys: 'Space', action: 'Play / Pause' },
      { keys: 'Ctrl + →  /  Ctrl + N', action: 'Next track' },
      { keys: 'Ctrl + ←  /  Ctrl + P', action: 'Previous track' },
      { keys: '→', action: 'Seek forward 5 seconds' },
      { keys: '←', action: 'Seek backward 5 seconds' },
      { keys: 'Shift + →', action: 'Seek forward 30 seconds' },
      { keys: 'Shift + ←', action: 'Seek backward 30 seconds' },
      { keys: 'Ctrl + ↑', action: 'Volume up' },
      { keys: 'Ctrl + ↓', action: 'Volume down' },
      { keys: 'M  /  Ctrl + M', action: 'Mute / Unmute' },
      { keys: 'Ctrl + S  /  Alt + S', action: 'Toggle shuffle' },
      { keys: 'Ctrl + R', action: 'Cycle repeat mode' },
      { keys: 'Ctrl + L', action: 'Toggle live lyrics / living artwork' },
      { keys: 'Ctrl + U', action: 'Toggle queue / Up Next' },
    ],
  },
  {
    id: 'navigation',
    icon: Library,
    title: 'Library & Navigation',
    shortcuts: [
      { keys: 'Ctrl + K', action: 'Command Palette (search & quick actions)' },
      { keys: 'Ctrl + F  /  /', action: 'Search library' },
      { keys: 'Ctrl + D', action: 'Add current track to queue' },
      { keys: 'Ctrl + Shift + L', action: 'Like / favorite current track' },
      { keys: 'Ctrl + Alt + M', action: 'Toggle mini-player window' },
      { keys: 'F11  /  Ctrl + Shift + F', action: 'Toggle living artwork / fullscreen' },
      { keys: 'Esc', action: 'Exit fullscreen / close panels & dialogs' },
    ],
  },
  {
    id: 'window',
    icon: Monitor,
    title: 'Window & App',
    shortcuts: [
      { keys: 'Ctrl + Q', action: 'Quit application' },
      { keys: 'Ctrl + ,', action: 'Open settings' },
      { keys: 'Ctrl + Shift + I', action: 'Open dev tools (debug builds)' },
    ],
  },
]
</script>

<template>
  <div class="space-y-10 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <section v-for="cat in categories" :key="cat.id">
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <component :is="cat.icon" class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">{{ cat.title }}</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div v-for="(s, i) in cat.shortcuts" :key="i"
          class="p-3.5 flex items-center justify-between gap-x-3">
          <kbd class="px-2.5 py-1 bg-foreground/10 rounded-md text-xs font-mono leading-none text-foreground/80 whitespace-nowrap">
            {{ s.keys }}
          </kbd>
          <p class="text-sm text-foreground/70 text-right leading-snug">{{ s.action }}</p>
        </div>
      </div>
    </section>
  </div>
</template>
