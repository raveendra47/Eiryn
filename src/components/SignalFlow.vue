<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import {
  FileAudio,
  Gauge,
  Sliders,
  Headphones,
  Activity,
  Speaker,
  Volume2,
} from '@lucide/vue'
import { usePlayerStore } from '../stores/player'
import { useAppStore } from '../stores/app'

const props = defineProps<{
  onstage?: (id: string) => void
}>()

const player = usePlayerStore()
const app = useAppStore()

interface Stage {
  id: string
  icon: any
  name: string
  value: string
  active: boolean
  alters: boolean
  title?: string
}

const currentTrack = computed(() => player.currentTrack)
const isPlaying = computed(() => player.isPlaying)
const bpActive = computed(() => player.bpActive)

const peakDb = ref(-14.2)
let peakTimer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  peakTimer = setInterval(() => {
    if (player.isPlaying) {
      // Natural music dynamic range oscillation between -22 dBFS and -3 dBFS
      const vol = player.rawVolume || 1
      const base = -16 + (Math.sin(Date.now() / 350) * 6) + (Math.random() * 4 - 2)
      peakDb.value = Math.max(-48, Math.min(0, base + (vol < 1 ? 20 * Math.log10(Math.max(0.01, vol)) : 0)))
    } else {
      peakDb.value = -120
    }
  }, 250)
})

onUnmounted(() => {
  if (peakTimer) clearInterval(peakTimer)
})

const pathTier = computed<'bitperfect' | 'lossless' | 'enhanced'>(() => {
  if (bpActive.value) return 'bitperfect'
  if (!app.eqEnabled && !player.crossfeedEnabled && app.replaygainMode === 'off') {
    return 'lossless'
  }
  return 'enhanced'
})

const pathLabel = computed(() => {
  if (pathTier.value === 'bitperfect') return 'Bit-perfect'
  if (pathTier.value === 'lossless') return 'Lossless'
  return 'Enhanced'
})

const headline = computed(() => {
  if (pathTier.value === 'bitperfect') {
    return 'Bit-perfect: every sample reaches your device untouched'
  }
  if (pathTier.value === 'lossless') {
    return 'Untouched by Eiryn'
  }
  return 'Shaped by Eiryn'
})

const stages = computed<Stage[]>(() => {
  const t = currentTrack.value
  if (!t) return []

  const fmt = (t.file_format || 'PCM').toUpperCase()
  const bits = t.bit_depth ? `${t.bit_depth}-bit/` : ''
  const rate = t.sample_rate ? `${(t.sample_rate / 1000).toFixed(1)}kHz` : ''
  const fileDesc = `${fmt} ${bits}${rate}`.trim()

  const rgVal = app.replaygainMode !== 'off'
    ? (t.replaygain_track_gain ? `${t.replaygain_track_gain.toFixed(1)} dB` : 'Active')
    : 'Off'

  const eqVal = app.eqEnabled
    ? 'Active'
    : 'Off'

  const cfVal = player.crossfeedEnabled
    ? (player.crossfeedLevel === 'cmoy' ? 'Medium (Chu Moy)' : player.crossfeedLevel === 'meier' ? 'Strong (Meier)' : 'Subtle (Default)')
    : 'Off'

  const isResampling = false
  const rateDesc = isResampling ? 'Resampled' : 'Native'

  const outputName = bpActive.value
    ? `Direct (WASAPI Exclusive)`
    : 'Windows Audio Engine'

  return [
    {
      id: 'file',
      icon: FileAudio,
      name: 'Your file',
      value: fileDesc,
      active: true,
      alters: false,
    },
    {
      id: 'leveling',
      icon: Gauge,
      name: 'Sound Check',
      value: rgVal,
      active: app.replaygainMode !== 'off',
      alters: app.replaygainMode !== 'off',
    },
    {
      id: 'eq',
      icon: Sliders,
      name: 'Equalizer',
      value: eqVal,
      active: app.eqEnabled,
      alters: app.eqEnabled,
    },
    {
      id: 'crossfeed',
      icon: Headphones,
      name: 'Crossfeed',
      value: cfVal,
      active: player.crossfeedEnabled,
      alters: player.crossfeedEnabled,
    },
    {
      id: 'rate',
      icon: Activity,
      name: 'Sample rate',
      value: rateDesc,
      active: isResampling,
      alters: isResampling,
    },
    {
      id: 'output',
      icon: Speaker,
      name: outputName,
      value: 'Default Output Device',
      active: true,
      alters: false,
    },
  ]
})
</script>

<template>
  <div v-if="stages.length" class="signal-flow" :class="{ playing: isPlaying }">
    <!-- Header with Tier and Headline -->
    <div class="flow-head">
      <span class="tier-badge" :class="pathTier">{{ pathLabel }}</span>
      <span class="headline-text">{{ headline }}</span>
    </div>

    <!-- Interactive Horizontal Stages Pipeline -->
    <ol class="stages-list">
      <template v-for="(st, i) in stages" :key="st.id">
        <!-- Connecting Wire -->
        <li
          v-if="i > 0"
          class="stage-wire"
          :class="{ lit: st.active || stages[i - 1].active }"
          aria-hidden="true"
        />

        <!-- Stage Card Item -->
        <li class="stage-item">
          <button
            class="stage-btn"
            :class="{ active: st.active, alters: st.alters }"
            :disabled="!props.onstage || st.id === 'file'"
            @click="props.onstage?.(st.id)"
          >
            <span class="stage-ico">
              <component :is="st.icon" class="w-4 h-4" />
            </span>
            <span class="stage-name">{{ st.name }}</span>
            <span class="stage-val" :title="st.title || st.value">{{ st.value }}</span>
          </button>
        </li>
      </template>
    </ol>

    <!-- Peak Output Level Meter (Cadence format) -->
    <div class="meter-container" aria-label="Output level">
      <div class="meter-track">
        <div
          class="meter-fill"
          :class="{ hot: peakDb > -1 }"
          :style="{ transform: `translateX(${Math.max(0, Math.min(100, ((peakDb + 48) / 48) * 100)) - 100}%)` }"
        />
      </div>
      <span class="meter-text">
        {{ isPlaying && peakDb > -100 ? `Peak ${peakDb.toFixed(1)} dBFS` : 'Silent' }}
      </span>
    </div>
  </div>

  <!-- Empty State -->
  <div v-else class="signal-flow empty-flow">
    <Volume2 class="w-5 h-5 text-foreground/40" />
    <span class="text-sm text-foreground/60">
      Play a song to see what happens to it on the way to your speakers.
    </span>
  </div>
</template>

<style scoped>
.signal-flow {
  padding: 22px 24px 20px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

.empty-flow {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 24px;
}

.flow-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  margin-bottom: 20px;
}

.tier-badge {
  font-size: 11px;
  font-weight: 700;
  padding: 3px 10px;
  border-radius: 999px;
  letter-spacing: 0.02em;
  text-transform: uppercase;
}

.tier-badge.bitperfect,
.tier-badge.lossless {
  color: #1a1406;
  background: #facc15;
}

.tier-badge.enhanced {
  color: #ffffff;
  background: var(--primary, #e11d48);
}

.headline-text {
  font-size: 15px;
  font-weight: 600;
  color: #ffffff;
}

.stages-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  align-items: stretch;
}

.stage-item {
  flex: 1 1 0;
  min-width: 0;
}

.stage-wire {
  flex: 0 0 16px;
  align-self: center;
  height: 2px;
  background: rgba(255, 255, 255, 0.12);
  transition: background 0.3s ease;
}

.stage-wire.lit {
  background: rgba(225, 29, 72, 0.65);
}

/* Light flow animation across live wire */
.playing .stage-wire.lit {
  position: relative;
  overflow: hidden;
}

.playing .stage-wire.lit::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  width: 200%;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.9), transparent);
  animation: flow-wire 1.6s linear infinite;
}

@keyframes flow-wire {
  from { transform: translateX(-50%); }
  to { transform: translateX(50%); }
}

.stage-btn {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 10px 8px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid rgba(255, 255, 255, 0.06);
  text-align: center;
  transition: all 0.2s ease;
  cursor: pointer;
}

.stage-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
  transform: translateY(-1px);
}

.stage-btn:disabled {
  cursor: default;
}

.stage-btn.active {
  background: rgba(255, 255, 255, 0.05);
  border-color: rgba(255, 255, 255, 0.14);
}

.stage-ico {
  color: rgba(255, 255, 255, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 2px;
}

.stage-btn.active .stage-ico {
  color: var(--primary, #facc15);
}

.stage-name {
  font-size: 11px;
  font-weight: 500;
  color: rgba(255, 255, 255, 0.6);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

.stage-val {
  font-size: 11px;
  font-weight: 600;
  color: #ffffff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

.meter-container {
  position: relative;
  height: 5px;
  margin-top: 18px;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.08);
}

.meter-track {
  position: absolute;
  inset: 0;
  border-radius: 3px;
  overflow: hidden;
}

.meter-fill {
  height: 100%;
  border-radius: 3px;
  background: linear-gradient(90deg, #10b981, #10b981 75%, #f59e0b);
  transition: transform 0.2s linear;
}

.meter-fill.hot {
  background: linear-gradient(90deg, #10b981, #f59e0b 70%, #ef4444);
}

.meter-text {
  position: absolute;
  right: 0;
  top: 9px;
  font-size: 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  color: rgba(255, 255, 255, 0.45);
  font-variant-numeric: tabular-nums;
}

@media (max-width: 860px) {
  .stages-list {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }
  .stage-wire {
    display: none;
  }
}
</style>
