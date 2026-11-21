<script setup lang="ts">
import { AppWindow, Sun, Circle, Trash2, RotateCcw } from '@lucide/vue'
import { useAppStore } from '../../stores/app'
import * as api from '../../lib/invoke'
import { emit } from '@tauri-apps/api/event'
import ColorPicker from '../ColorPicker.vue'
import { PRIMARY_COLOR_PRESETS } from '../../lib/color'

const appStore = useAppStore()

async function onClearStats() {
  const confirmed = confirm('This will reset all play counts and listening time. This cannot be undone.')
  if (!confirmed) return
  await api.clearPlayHistory()
  emit('stats-cleared', {})
}

const themes = [
  { value: 'light' as const, icon: Sun, label: 'Light' },
  { value: 'black' as const, icon: Circle, label: 'Black (OLED)' },
]

</script>

<template>
  <div class="space-y-10 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <AppWindow class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Behavior</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Start at Login</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Launch automatically when you sign in</p>
          </div>
          <button @click="appStore.updateStartAtLogin(!appStore.startAtLogin)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.startAtLogin ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.startAtLogin ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Show Tray Icon</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Display icon in system tray (requires restart)</p>
          </div>
          <button @click="appStore.updateShowTrayIcon(!appStore.showTrayIcon)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.showTrayIcon ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.showTrayIcon ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <Sun class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Appearance</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5">
          <p class="text-sm font-semibold mb-3">Theme Mode</p>
          <div class="flex flex-wrap gap-2">
            <button v-for="t in themes" :key="t.value" @click="appStore.updateTheme(t.value)"
              :class="['flex items-center gap-2 px-4 py-2 rounded-xl border transition-all text-xs font-medium', appStore.theme === t.value ? 'bg-primary text-primary-foreground border-primary shadow-sm shadow-primary/20' : 'bg-foreground/[0.04] border-foreground/10 text-foreground/70 hover:text-foreground']">
              <component :is="t.icon" class="w-4 h-4" />
              {{ t.label }}
            </button>
          </div>
        </div>
        <div class="p-5">
          <p class="text-sm font-semibold mb-1">Accent color</p>
          <p class="text-xs text-foreground opacity-60 mb-3">Overrides the active palette's accent until reset</p>
          <div class="flex flex-wrap items-center gap-2">
            <button
              v-for="color in PRIMARY_COLOR_PRESETS"
              :key="color"
              type="button"
              class="flex h-7 w-7 items-center justify-center rounded-full border-2 transition-all hover:scale-110"
              :class="appStore.primaryColor === color ? 'border-primary' : 'border-transparent'"
              :aria-label="`Use ${color} as accent color`"
              :aria-pressed="appStore.primaryColor === color"
              @click="appStore.updatePrimaryColor(color)">
              <span class="h-5 w-5 rounded-full" :style="{ backgroundColor: color }" />
            </button>
            <ColorPicker
              :model-value="appStore.primaryColor"
              :presets="PRIMARY_COLOR_PRESETS"
              :ariaLabel="'Choose a custom accent color'"
              :hexLabel="'#RRGGBB'"
              @update:model-value="appStore.updatePrimaryColor" />
            <button
              v-if="appStore.primaryColor"
              type="button"
              @click="appStore.updatePrimaryColor(null)"
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-foreground/10 text-xs font-medium text-foreground/70 hover:text-foreground hover:border-foreground/30 transition-all">
              <RotateCcw class="w-3.5 h-3.5" />
              Palette default
            </button>
          </div>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <Trash2 class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Data</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between gap-x-2">
          <div>
            <p class="text-sm font-semibold">Clear Listening Stats</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Reset all play counts and listening time</p>
          </div>
          <button @click="onClearStats"
            class="px-4 py-2 rounded-xl bg-red-500/10 text-red-500 border border-red-500/20 text-xs font-semibold hover:bg-red-500/20 transition-colors whitespace-nowrap">
            Clear
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
