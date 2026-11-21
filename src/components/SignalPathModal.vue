<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { X, CheckCircle2, Zap, Sliders, Headphones, Cpu, Disc3 } from '@lucide/vue'
import * as api from '../lib/invoke'

const emit = defineEmits<{
  (e: 'close'): void
}>()

const loading = ref(true)
const error = ref<string | null>(null)
const info = ref<api.SignalPathInfo | null>(null)

async function loadSignalPath() {
  loading.value = true
  error.value = null
  try {
    info.value = await api.getSignalPath()
  } catch (e) {
    error.value = typeof e === 'string' ? e : (e as Error)?.message || 'Failed to load signal path'
  } finally {
    loading.value = false
  }
}

function onKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    emit('close')
  }
}

onMounted(() => {
  loadSignalPath()
  window.addEventListener('keydown', onKeyDown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown)
})
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-fade-in select-none"
       @click.self="emit('close')">
    <div class="relative w-full max-w-md bg-[#13141c]/95 border border-white/10 rounded-2xl shadow-2xl overflow-hidden p-6 text-white backdrop-blur-xl">
      <!-- Header -->
      <div class="flex items-center justify-between pb-4 border-b border-white/10">
        <div class="flex items-center gap-2.5">
          <div class="w-3 h-3 rounded-full animate-pulse"
               :class="info?.is_bit_perfect ? 'bg-emerald-400 shadow-[0_0_10px_rgba(52,211,153,0.8)]' : 'bg-primary shadow-[0_0_10px_rgba(96,165,250,0.8)]'" />
          <h2 class="text-base font-semibold tracking-tight">Signal Path</h2>
        </div>
        <button class="w-7 h-7 flex items-center justify-center rounded-lg text-white/50 hover:text-white hover:bg-white/10 transition-colors"
                @click="emit('close')">
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Content -->
      <div v-if="loading" class="py-12 flex flex-col items-center justify-center gap-3 text-white/50">
        <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin" />
        <span class="text-xs">Reading signal pipeline...</span>
      </div>

      <div v-else-if="error" class="py-10 text-center text-red-400 text-sm">
        {{ error }}
      </div>

      <div v-else-if="info" class="mt-5 space-y-4 max-h-[70vh] overflow-y-auto pr-1">
        <!-- 1. Source Node -->
        <div class="p-3.5 rounded-xl bg-white/5 border border-white/10 flex items-start gap-3.5">
          <div class="w-8 h-8 rounded-lg bg-emerald-500/20 text-emerald-400 flex items-center justify-center shrink-0 mt-0.5">
            <Disc3 class="w-4 h-4" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-[11px] font-semibold uppercase tracking-wider text-emerald-400">Source</div>
            <div class="text-sm font-semibold truncate text-white mt-0.5">{{ info.title }}</div>
            <div class="text-xs text-white/60 truncate">{{ info.artist }}</div>
            <div class="flex items-center gap-2 mt-2 text-[11px] font-mono text-white/70">
              <span class="px-1.5 py-0.5 rounded bg-white/10 border border-white/10 font-bold">{{ info.file_format }}</span>
              <span>{{ info.bit_depth ? `${info.bit_depth}-bit / ` : '' }}{{ (info.sample_rate / 1000).toFixed(1) }} kHz</span>
              <span v-if="info.bitrate">· {{ Math.round(info.bitrate / 1000) }} kbps</span>
            </div>
          </div>
        </div>

        <!-- Connector Arrow -->
        <div class="flex justify-center -my-2 text-white/20 font-bold">↓</div>

        <!-- 2. Volume Normalization / Gain -->
        <div class="p-3 rounded-xl bg-white/[0.03] border border-white/5 flex items-center gap-3.5">
          <div class="w-7 h-7 rounded-lg bg-amber-500/20 text-amber-400 flex items-center justify-center shrink-0">
            <Zap class="w-3.5 h-3.5" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-[10px] uppercase tracking-wider text-white/50">Loudness Adjustment</div>
            <div class="text-xs font-medium text-white/90 mt-0.5">
              {{ info.lufs_gain_db !== 0 ? `LUFS Target Gain: ${info.lufs_gain_db > 0 ? '+' : ''}${info.lufs_gain_db.toFixed(1)} dB` : 'Unity Gain (0.0 dB)' }}
            </div>
          </div>
        </div>

        <!-- Connector Arrow -->
        <div class="flex justify-center -my-2 text-white/20 font-bold">↓</div>

        <!-- 3. Equalizer Stage -->
        <div class="p-3 rounded-xl bg-white/[0.03] border border-white/5 flex items-center gap-3.5">
          <div class="w-7 h-7 rounded-lg bg-purple-500/20 text-purple-400 flex items-center justify-center shrink-0">
            <Sliders class="w-3.5 h-3.5" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-[10px] uppercase tracking-wider text-white/50">Parametric / 10-Band EQ</div>
            <div class="text-xs font-medium text-white/90 mt-0.5 flex items-center gap-1.5">
              <span>{{ info.eq_enabled ? `Profile: ${info.eq_profile_name}` : 'Bypassed (Bit-Perfect Direct)' }}</span>
            </div>
          </div>
        </div>

        <!-- Connector Arrow -->
        <div class="flex justify-center -my-2 text-white/20 font-bold">↓</div>

        <!-- 4. Headphone Crossfeed bs2b -->
        <div class="p-3 rounded-xl bg-white/[0.03] border border-white/5 flex items-center gap-3.5">
          <div class="w-7 h-7 rounded-lg bg-cyan-500/20 text-cyan-400 flex items-center justify-center shrink-0">
            <Headphones class="w-3.5 h-3.5" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-[10px] uppercase tracking-wider text-white/50">Headphone Crossfeed (bs2b)</div>
            <div class="text-xs font-medium text-white/90 mt-0.5">
              {{ info.crossfeed_enabled ? `Active (${info.crossfeed_level.toUpperCase()})` : 'Bypassed' }}
            </div>
          </div>
        </div>

        <!-- Connector Arrow -->
        <div class="flex justify-center -my-2 text-white/20 font-bold">↓</div>

        <!-- 5. Output Stage -->
        <div class="p-3.5 rounded-xl border flex items-start gap-3.5"
             :class="info.is_bit_perfect ? 'bg-emerald-500/10 border-emerald-500/30' : 'bg-primary/10 border-primary/25'">
          <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0 mt-0.5"
               :class="info.is_bit_perfect ? 'bg-emerald-500/25 text-emerald-400' : 'bg-primary/25 text-primary'">
            <Cpu class="w-4 h-4" />
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-[11px] font-semibold uppercase tracking-wider"
                 :class="info.is_bit_perfect ? 'text-emerald-400' : 'text-primary'">
              {{ info.is_bit_perfect ? 'Bit-Perfect Output' : 'Audio Endpoint' }}
            </div>
            <div class="text-sm font-semibold text-white mt-0.5">{{ info.output_device }}</div>
            <div class="text-xs text-white/70 mt-1 font-mono">
              {{ info.output_format }} · {{ (info.output_sample_rate / 1000).toFixed(1) }} kHz
            </div>
            <div v-if="info.is_bit_perfect" class="flex items-center gap-1.5 mt-2 text-[11px] text-emerald-400">
              <CheckCircle2 class="w-3.5 h-3.5" />
              <span>Direct WASAPI Exclusive Mode (No Windows Resampling)</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@keyframes fadeIn {
  from { opacity: 0; transform: scale(0.97); }
  to { opacity: 1; transform: scale(1); }
}
.animate-fade-in {
  animation: fadeIn 0.15s ease-out;
}
</style>
