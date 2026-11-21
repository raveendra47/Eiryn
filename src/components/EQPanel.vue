<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import {
  Headphones,
  FolderOpen,
  ClipboardPaste,
  Plus,
  Trash2,
  X,
  SlidersHorizontal,
  ChevronDown,
  Check,
} from '@lucide/vue'
import { useAppStore } from '../stores/app'
import * as api from '../lib/invoke'
import type { ParametricFilterItem, ParametricEqState, EQProfile } from '../lib/invoke'
import {
  EQ_BANDS,
  autoHeadroom,
  formatHz,
  formatFreq,
  formatFilterKind,
  formatGain,
} from '../lib/dsp'
import type { EqSettings, Filter } from '../lib/dsp'
import Slider from './Slider.vue'
import EqGraph from './EqGraph.vue'

const app = useAppStore()

// Tabs: 'parametric' (Headphone correction) or 'presets' (Presets)
const activeTab = ref<'parametric' | 'presets'>('parametric')

// Graphic EQ State
const profiles = ref<EQProfile[]>([])
const activeProfile = ref<EQProfile | null>(null)
const graphicGains = ref<number[]>([0, 0, 0, 0, 0, 0, 0, 0, 0, 0])

// Parametric EQ (Headphone Correction) State
const parametricFilters = ref<ParametricFilterItem[]>([
  { kind: 'lowshelf', freq: 105, gain: -2.8, q: 0.71, enabled: true },
  { kind: 'peak', freq: 169, gain: -2.6, q: 1.41, enabled: true },
  { kind: 'peak', freq: 719, gain: 3.6, q: 1.41, enabled: true },
  { kind: 'peak', freq: 2400, gain: -4.1, q: 2.1, enabled: true },
  { kind: 'peak', freq: 4600, gain: -4.9, q: 2.8, enabled: true },
  { kind: 'peak', freq: 6600, gain: 4.4, q: 3.0, enabled: true },
  { kind: 'peak', freq: 5200, gain: -2.4, q: 2.5, enabled: true },
  { kind: 'highshelf', freq: 10000, gain: -4.4, q: 0.71, enabled: true },
])
const parametricProfileName = ref('Pasted profile')
const preampDb = ref(0)
const selectedFilterIndex = ref<number | null>(null)

// Modals
const showPasteModal = ref(false)
const pasteText = ref('')
const pasteError = ref<string | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)

// Current EQ settings for curve calculation
const currentEqSettings = computed<EqSettings>(() => {
  if (activeTab.value === 'parametric') {
    return {
      enabled: app.eqEnabled,
      mode: 'parametric',
      preamp: Number.isFinite(preampDb.value) ? preampDb.value : 0,
      gains: graphicGains.value,
      filters: parametricFilters.value as Filter[],
    }
  }
  return {
    enabled: app.eqEnabled,
    mode: 'graphic',
    preamp: 0,
    gains: [...graphicGains.value],
    filters: [],
  }
})

// Auto Headroom / Preamp readout
const calculatedHeadroom = computed(() => {
  return autoHeadroom(currentEqSettings.value)
})

const levelChangeText = computed(() => {
  if (!app.eqEnabled) return 'Equalizer Off'
  if (preampDb.value < -0.05) {
    return `${preampDb.value.toFixed(1)} dB preamp`
  }
  if (calculatedHeadroom.value < -0.05) {
    return `${calculatedHeadroom.value.toFixed(1)} dB preamp`
  }
  return 'No level change'
})

// Graphic Preset Definitions
const STANDARD_PRESETS = [
  { name: 'Flat', gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { name: 'Bass Boost', gains: [5.5, 4.5, 3.0, 1.5, 0, 0, 0, 0, 0, 0] },
  { name: 'Bass Reducer', gains: [-5.0, -4.0, -2.5, -1.0, 0, 0, 0, 0, 0, 0] },
  { name: 'Treble Boost', gains: [0, 0, 0, 0, 0, 1.0, 2.5, 4.0, 5.5, 6.0] },
  { name: 'Treble Reducer', gains: [0, 0, 0, 0, 0, -1.0, -2.5, -4.0, -5.5, -6.0] },
  { name: 'Rock', gains: [4.5, 3.0, 1.5, 0, -0.5, 1.5, 3.0, 4.0, 4.5, 5.0] },
  { name: 'Pop', gains: [-1.5, 1.0, 3.0, 3.5, 2.0, -1.0, -1.5, -1.0, 1.5, 2.5] },
  { name: 'Jazz', gains: [3.0, 2.0, 0.5, 1.5, -1.5, -1.5, 0, 1.5, 2.5, 3.5] },
  { name: 'Classical', gains: [4.0, 3.0, 2.5, 2.0, -1.0, -1.0, 0, 2.0, 3.0, 3.5] },
  { name: 'Electronic', gains: [5.0, 4.0, 1.5, 0, -2.0, 2.0, 1.0, 2.5, 4.5, 5.0] },
  { name: 'Vocal Boost', gains: [-2.0, -2.5, -1.0, 1.5, 3.5, 3.5, 2.5, 1.0, 0, -1.5] },
  { name: 'Hip-Hop', gains: [5.5, 4.5, 2.0, 1.0, -0.5, -0.5, 1.0, -1.0, 2.5, 3.0] },
  { name: 'Acoustic', gains: [3.5, 2.5, 1.0, 1.0, 1.5, 1.5, 2.5, 3.0, 3.5, 2.5] },
]

onMounted(async () => {
  try {
    // 1. Fetch Parametric EQ state
    const pState = await api.getParametricEqState()
    if (pState && pState.filters && pState.filters.length > 0) {
      parametricFilters.value = pState.filters
      preampDb.value = pState.preamp_db
      if (pState.profile_name) parametricProfileName.value = pState.profile_name
      if (pState.mode === 'graphic') {
        activeTab.value = 'presets'
      } else {
        activeTab.value = 'parametric'
      }
    }

    // 2. Fetch Graphic EQ profiles
    const [allProfiles, active] = await Promise.all([
      api.getEqProfiles(),
      api.getActiveEqProfile(),
    ])
    profiles.value = allProfiles
    if (active) {
      activeProfile.value = active
      graphicGains.value = [...active.bands]
    }
  } catch (e) {
    console.error('Failed to initialize EQ panel:', e)
  }
})

// Toggle Equalizer on/off
async function toggleEq() {
  const next = !app.eqEnabled
  await app.updateEqEnabled(next)
  await syncEqToBackend()
}

// Switch between Presets and Headphone Correction tabs
async function switchTab(tab: 'parametric' | 'presets') {
  activeTab.value = tab
  selectedFilterIndex.value = null
  await syncEqToBackend()
}

// Sync current EQ settings to backend DSP engine & SQLite database
async function syncEqToBackend() {
  if (activeTab.value === 'parametric') {
    await api.applyParametricEq(
      parametricFilters.value,
      preampDb.value,
      app.eqEnabled,
      parametricProfileName.value
    )
  } else {
    // Graphic mode
    await api.setEqMode('graphic')
    await api.setEqEnabled(app.eqEnabled)
    await api.setEqPreamp(preampDb.value)
    if (activeProfile.value) {
      for (let i = 0; i < graphicGains.value.length; i++) {
        await api.setEqBand(i, graphicGains.value[i])
      }
    }
  }
}

// Handle Node updates from EqGraph
async function onFilterUpdated(idx: number, updated: Filter) {
  if (idx >= 0 && idx < parametricFilters.value.length) {
    parametricFilters.value[idx] = {
      ...parametricFilters.value[idx],
      kind: updated.kind,
      freq: updated.freq,
      gain: updated.gain,
      q: updated.q,
      enabled: updated.enabled,
    }
    await syncEqToBackend()
  }
}

async function onGainUpdated(idx: number, gain: number) {
  if (idx >= 0 && idx < graphicGains.value.length) {
    graphicGains.value[idx] = gain
    if (activeProfile.value) {
      activeProfile.value.bands[idx] = gain
      await api.updateEqBand(activeProfile.value.id, idx, gain)
    }
    await api.setEqBand(idx, gain)
  }
}

function onFilterSelected(idx: number | null) {
  selectedFilterIndex.value = idx
}

async function onFilterAdded(filter: Filter) {
  parametricFilters.value.push({
    kind: filter.kind,
    freq: filter.freq,
    gain: filter.gain,
    q: filter.q,
    enabled: true,
  })
  selectedFilterIndex.value = parametricFilters.value.length - 1
  await syncEqToBackend()
}

async function deleteFilter(idx: number) {
  parametricFilters.value.splice(idx, 1)
  if (selectedFilterIndex.value === idx) {
    selectedFilterIndex.value = null
  } else if (selectedFilterIndex.value !== null && selectedFilterIndex.value > idx) {
    selectedFilterIndex.value--
  }
  await syncEqToBackend()
}

// Add Point Button clicked
async function addPoint() {
  // Insert a sensible new filter point around 1000 Hz or mid-range
  const newPoint: ParametricFilterItem = {
    kind: 'peak',
    freq: 1000,
    gain: 0.0,
    q: 1.41,
    enabled: true,
  }
  parametricFilters.value.push(newPoint)
  selectedFilterIndex.value = parametricFilters.value.length - 1
  await syncEqToBackend()
}

// Robust client-side AutoEq parser fallback
function parseAutoEqClient(text: string): { preamp: number; filters: ParametricFilterItem[] } {
  let preamp = 0
  const filters: ParametricFilterItem[] = []

  for (const rawLine of text.split('\n')) {
    const line = rawLine.trim()
    if (!line || line.startsWith('#')) continue

    // Preamp: -6.0 dB
    if (line.toLowerCase().startsWith('preamp')) {
      const match = line.match(/[-+]?\d*\.?\d+/)
      if (match) {
        preamp = parseFloat(match[0]) || 0
      }
      continue
    }

    const isEnabled = !line.toUpperCase().includes('OFF')

    // Find type
    let kind: ParametricFilterItem['kind'] = 'peak'
    if (/\b(LSC|LS|LSQ|LOWSHELF|LOW SHELF)\b/i.test(line)) {
      kind = 'lowshelf'
    } else if (/\b(HSC|HS|HSQ|HIGHSHELF|HIGH SHELF)\b/i.test(line)) {
      kind = 'highshelf'
    } else if (/\b(LP|LPQ|LOWPASS|LOW PASS)\b/i.test(line)) {
      kind = 'lowpass'
    } else if (/\b(HP|HPQ|HIGHPASS|HIGH PASS)\b/i.test(line)) {
      kind = 'highpass'
    } else if (/\b(PK|PEQ|BELL|PEAK)\b/i.test(line)) {
      kind = 'peak'
    }

    // Find frequency (Fc ... Hz)
    const fcMatch = line.match(/(?:Fc|Freq|Frequency)[\s:=]*([0-9.]+)/i)
    if (!fcMatch) continue
    const freq = parseFloat(fcMatch[1])
    if (!freq || isNaN(freq)) continue

    // Find gain (Gain ... dB)
    const gainMatch = line.match(/(?:Gain)[\s:=]*([-+]?[0-9.]+)/i)
    const gain = gainMatch ? parseFloat(gainMatch[1]) || 0 : 0

    // Find Q factor (Q ...)
    const qMatch = line.match(/\bQ[\s:=]*([0-9.]+)/i)
    const defaultQ = (kind === 'lowshelf' || kind === 'highshelf' || kind === 'lowpass' || kind === 'highpass') ? 0.707 : 1.41
    const q = qMatch ? parseFloat(qMatch[1]) || defaultQ : defaultQ

    filters.push({
      kind,
      freq: Math.round(freq),
      gain: Math.round(gain * 10) / 10,
      q: Math.round(q * 100) / 100,
      enabled: isEnabled,
    })
  }

  return { preamp, filters }
}

// Load file button clicked
function triggerLoadFile() {
  fileInputRef.value?.click()
}

async function onFileSelected(e: Event) {
  const target = e.target as HTMLInputElement
  const file = target.files?.[0]
  if (!file) return

  try {
    const text = await file.text()
    // 1. Client-side parse first for instant UI response
    const clientParsed = parseAutoEqClient(text)
    if (clientParsed.filters.length > 0) {
      parametricFilters.value = clientParsed.filters
      preampDb.value = clientParsed.preamp
      parametricProfileName.value = file.name.replace(/\.[^/.]+$/, '') || 'Loaded profile'
      selectedFilterIndex.value = null
      if (!app.eqEnabled) await app.updateEqEnabled(true)
      await syncEqToBackend()
      return
    }

    // 2. Fall back to backend parser if client parser found nothing
    const parsed = await api.parseAutoEqText(text)
    const preamp = typeof (parsed as any)?.preamp_db === 'number'
      ? (parsed as any).preamp_db
      : typeof (parsed as any)?.preamp === 'number'
        ? (parsed as any).preamp
        : 0

    if (parsed && parsed.filters && parsed.filters.length > 0) {
      parametricFilters.value = parsed.filters
      preampDb.value = preamp
      parametricProfileName.value = file.name.replace(/\.[^/.]+$/, '') || 'Loaded profile'
      selectedFilterIndex.value = null
      if (!app.eqEnabled) await app.updateEqEnabled(true)
      await syncEqToBackend()
    }
  } catch (err) {
    console.error('Failed to parse loaded file:', err)
  } finally {
    target.value = ''
  }
}

// Paste text modal apply
async function applyPastedText() {
  const text = pasteText.value.trim()
  if (!text) return
  pasteError.value = null

  try {
    // 1. Try robust client-side parser first
    const clientParsed = parseAutoEqClient(text)
    if (clientParsed.filters.length > 0) {
      parametricFilters.value = clientParsed.filters
      preampDb.value = clientParsed.preamp
      parametricProfileName.value = 'Pasted profile'
      selectedFilterIndex.value = null
      showPasteModal.value = false
      pasteText.value = ''
      if (!app.eqEnabled) await app.updateEqEnabled(true)
      await syncEqToBackend()
      return
    }

    // 2. Try backend parser
    const parsed = await api.parseAutoEqText(text)
    const preamp = typeof (parsed as any)?.preamp_db === 'number'
      ? (parsed as any).preamp_db
      : typeof (parsed as any)?.preamp === 'number'
        ? (parsed as any).preamp
        : 0

    if (!parsed || !parsed.filters || parsed.filters.length === 0) {
      pasteError.value = 'Could not find valid filter lines in the pasted text.'
      return
    }

    parametricFilters.value = parsed.filters
    preampDb.value = preamp
    parametricProfileName.value = (parsed as any).name || 'Pasted profile'
    selectedFilterIndex.value = null
    showPasteModal.value = false
    pasteText.value = ''
    if (!app.eqEnabled) await app.updateEqEnabled(true)
    await syncEqToBackend()
  } catch (err: any) {
    pasteError.value = typeof err === 'string' ? err : err.message || 'Failed to parse EQ text'
  }
}

// Select a preset in Presets tab
async function selectPreset(preset: typeof STANDARD_PRESETS[0]) {
  graphicGains.value = [...preset.gains]
  if (!app.eqEnabled) {
    await app.updateEqEnabled(true)
  }
  if (activeProfile.value) {
    activeProfile.value.bands = [...preset.gains]
    for (let i = 0; i < preset.gains.length; i++) {
      await api.updateEqBand(activeProfile.value.id, i, preset.gains[i])
      await api.setEqBand(i, preset.gains[i])
    }
  }
  await syncEqToBackend()
}

function resetGraphicBands() {
  selectPreset(STANDARD_PRESETS[0])
}
</script>

<template>
  <div class="w-full space-y-4 text-white">
    <!-- Hidden File Input for "Load file" -->
    <input
      ref="fileInputRef"
      type="file"
      accept=".txt,.csv"
      class="hidden"
      @change="onFileSelected"
    />

    <!-- Header matching reference screenshot -->
    <div class="flex items-start justify-between">
      <div>
        <h2 class="text-2xl font-semibold tracking-tight text-white">Equalizer</h2>
        <p class="text-xs text-zinc-400 mt-1 leading-normal">
          Drag the points to shape the sound. The level is lowered automatically so boosts can't distort.
        </p>
      </div>

      <!-- Main Toggle Switch -->
      <button
        role="switch"
        :aria-checked="app.eqEnabled"
        class="relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none"
        :class="app.eqEnabled ? 'bg-white' : 'bg-zinc-700'"
        @click="toggleEq"
      >
        <span
          class="pointer-events-none inline-block h-5 w-5 transform rounded-full shadow ring-0 transition duration-200 ease-in-out"
          :class="app.eqEnabled ? 'translate-x-5 bg-zinc-950' : 'translate-x-0 bg-zinc-400'"
        />
      </button>
    </div>

    <!-- Segmented Mode Pills & Headroom Readout -->
    <div class="flex items-center justify-between pt-1">
      <!-- Pill segmented buttons -->
      <div class="inline-flex items-center rounded-full bg-zinc-900/90 p-1 border border-white/5">
        <button
          class="px-4 py-1.5 rounded-full text-xs font-medium transition-all"
          :class="activeTab === 'presets'
            ? 'bg-white text-zinc-950 shadow-sm'
            : 'text-zinc-400 hover:text-white'"
          @click="switchTab('presets')"
        >
          Presets
        </button>
        <button
          class="px-4 py-1.5 rounded-full text-xs font-medium transition-all"
          :class="activeTab === 'parametric'
            ? 'bg-white text-zinc-950 shadow-sm'
            : 'text-zinc-400 hover:text-white'"
          @click="switchTab('parametric')"
        >
          Headphone correction
        </button>
      </div>

      <!-- Right Readout: "No level change" / "-X.X dB preamp" -->
      <div class="text-xs text-zinc-400 font-normal">
        {{ levelChangeText }}
      </div>
    </div>

    <!-- Live Interactive Frequency Response Graph -->
    <EqGraph
      :mode="activeTab === 'presets' ? 'graphic' : 'parametric'"
      :gains="graphicGains"
      :filters="parametricFilters"
      :preamp="activeTab === 'presets' ? 0 : preampDb"
      :enabled="app.eqEnabled"
      :selected-index="selectedFilterIndex"
      @update:gain="onGainUpdated"
      @update:filter="onFilterUpdated"
      @select:filter="onFilterSelected"
      @add:filter="onFilterAdded"
      @delete:filter="deleteFilter"
    />

    <!-- ==================== HEADPHONE CORRECTION SECTION ==================== -->
    <div v-if="activeTab === 'parametric'" class="space-y-3 pt-1">
      <!-- Action header row -->
      <div class="flex items-center justify-between gap-4 flex-wrap">
        <div class="flex items-start gap-2.5">
          <Headphones class="w-5 h-5 text-zinc-300 mt-0.5 flex-shrink-0" />
          <div>
            <h3 class="text-sm font-semibold text-white">{{ parametricProfileName }}</h3>
            <p class="text-[11px] text-zinc-400 mt-0.5">
              Get one for your headphones from <strong class="text-zinc-300 font-medium">AutoEq</strong> (find your model, then its ParametricEQ.txt). Equalizer APO files work too.
            </p>
          </div>
        </div>

        <!-- Action Buttons matching screenshot -->
        <div class="flex items-center gap-2">
          <!-- Load file button (warm beige tint from screenshot) -->
          <button
            class="px-3.5 py-1.5 rounded-full bg-[#f0caa3] hover:bg-[#e6be95] text-zinc-950 text-xs font-medium flex items-center gap-1.5 transition-colors shadow-sm"
            @click="triggerLoadFile"
          >
            <FolderOpen class="w-3.5 h-3.5" />
            <span>Load file</span>
          </button>

          <!-- Paste text button -->
          <button
            class="px-3.5 py-1.5 rounded-full bg-zinc-800 hover:bg-zinc-700 text-white text-xs font-medium flex items-center gap-1.5 transition-colors"
            @click="showPasteModal = true"
          >
            <span>Paste text</span>
          </button>

          <!-- Add point button -->
          <button
            class="px-3.5 py-1.5 rounded-full bg-zinc-800 hover:bg-zinc-700 text-white text-xs font-medium flex items-center gap-1.5 transition-colors"
            @click="addPoint"
          >
            <Plus class="w-3.5 h-3.5" />
            <span>Add point</span>
          </button>
        </div>
      </div>

      <!-- Hint text -->
      <p class="text-[11px] text-zinc-400">
        Select a point to fine-tune it. Scroll over a point to make it wider or narrower; use the arrow keys for small steps.
      </p>

      <!-- Pill Chips List of Active Filters matching reference image -->
      <div class="flex flex-wrap items-center gap-2 pt-1">
        <div
          v-for="(filter, i) in parametricFilters"
          :key="i"
          class="group inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium cursor-pointer transition-all border"
          :class="selectedFilterIndex === i
            ? 'bg-zinc-800 text-white border-[#f0caa3] ring-1 ring-[#f0caa3]/40 shadow'
            : 'bg-[#18191e] hover:bg-zinc-800 text-zinc-300 border-white/5 hover:border-white/10'"
          @click="selectedFilterIndex = selectedFilterIndex === i ? null : i"
        >
          <span>{{ formatFilterKind(filter.kind) }} {{ formatFreq(filter.freq) }}, {{ formatGain(filter.gain) }}</span>
          
          <!-- Delete button when hovered or selected -->
          <button
            class="opacity-0 group-hover:opacity-100 hover:text-red-400 p-0.5 rounded-full transition-opacity"
            :class="{ 'opacity-100 text-zinc-400': selectedFilterIndex === i }"
            title="Delete point"
            @click.stop="deleteFilter(i)"
          >
            <X class="w-3 h-3" />
          </button>
        </div>
      </div>

      <!-- Selected Filter Detail Editor Panel -->
      <div
        v-if="selectedFilterIndex !== null && parametricFilters[selectedFilterIndex]"
        class="bg-zinc-900/80 border border-white/10 rounded-xl p-3 flex items-center justify-between gap-4 flex-wrap text-xs animate-in fade-in-50 duration-150"
      >
        <div class="flex items-center gap-3">
          <span class="text-zinc-400 font-medium">Type:</span>
          <select
            v-model="parametricFilters[selectedFilterIndex].kind"
            class="bg-zinc-800 border border-white/10 rounded-lg px-2.5 py-1 text-xs text-white focus:outline-none"
            @change="syncEqToBackend"
          >
            <option value="peak">Bell (Peak)</option>
            <option value="lowshelf">Low shelf</option>
            <option value="highshelf">High shelf</option>
            <option value="lowpass">Low pass</option>
            <option value="highpass">High pass</option>
          </select>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-zinc-400 font-medium">Freq:</span>
          <input
            v-model.number="parametricFilters[selectedFilterIndex].freq"
            type="number"
            min="20"
            max="20000"
            class="w-20 bg-zinc-800 border border-white/10 rounded-lg px-2 py-1 text-xs text-white focus:outline-none"
            @change="syncEqToBackend"
          />
          <span class="text-zinc-500">Hz</span>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-zinc-400 font-medium">Gain:</span>
          <input
            v-model.number="parametricFilters[selectedFilterIndex].gain"
            type="number"
            step="0.1"
            min="-15"
            max="15"
            class="w-16 bg-zinc-800 border border-white/10 rounded-lg px-2 py-1 text-xs text-white focus:outline-none"
            @change="syncEqToBackend"
          />
          <span class="text-zinc-500">dB</span>
        </div>

        <div class="flex items-center gap-3">
          <span class="text-zinc-400 font-medium">Q:</span>
          <input
            v-model.number="parametricFilters[selectedFilterIndex].q"
            type="number"
            step="0.05"
            min="0.1"
            max="16"
            class="w-16 bg-zinc-800 border border-white/10 rounded-lg px-2 py-1 text-xs text-white focus:outline-none"
            @change="syncEqToBackend"
          />
        </div>

        <button
          class="text-red-400 hover:text-red-300 hover:bg-red-500/10 px-2 py-1 rounded-lg flex items-center gap-1 transition-colors"
          @click="deleteFilter(selectedFilterIndex)"
        >
          <Trash2 class="w-3.5 h-3.5" />
          <span>Remove</span>
        </button>
      </div>
    </div>

    <!-- ==================== PRESETS SECTION ==================== -->
    <div v-else class="space-y-4 pt-1">
      <div class="flex items-center justify-between gap-4">
        <!-- Preset Quick Selector -->
        <div class="flex items-center gap-2">
          <span class="text-xs text-zinc-400 font-medium">Preset:</span>
          <div class="flex flex-wrap gap-1.5">
            <button
              v-for="p in STANDARD_PRESETS.slice(0, 7)"
              :key="p.name"
              class="px-2.5 py-1 rounded-full text-xs font-medium border transition-colors"
              :class="JSON.stringify(graphicGains) === JSON.stringify(p.gains)
                ? 'bg-white text-black border-white'
                : 'bg-zinc-900 text-zinc-300 border-white/10 hover:border-white/20'"
              @click="selectPreset(p)"
            >
              {{ p.name }}
            </button>
          </div>
        </div>

        <button
          class="px-3 py-1 rounded-lg text-xs text-zinc-400 hover:text-white bg-zinc-900 border border-white/10 transition-colors"
          @click="resetGraphicBands"
        >
          Reset to Flat
        </button>
      </div>

      <!-- 10 Vertical Graphic Band Sliders -->
      <div class="flex items-end justify-between gap-2 h-40 px-2 pt-2 bg-[#0f1013] border border-white/5 rounded-2xl p-4">
        <div
          v-for="(f, i) in EQ_BANDS"
          :key="f"
          class="flex flex-col items-center flex-1 min-w-0 h-full"
        >
          <span class="text-[10px] text-zinc-400 mb-1 tabular-nums">
            {{ graphicGains[i] >= 0 ? '+' : '' }}{{ graphicGains[i]?.toFixed(1) }}
          </span>
          <div class="flex-1 flex items-center justify-center w-full">
            <div class="relative" style="width: 24px; height: 80px;">
              <div
                class="absolute inset-0 flex items-center justify-center"
                style="transform: rotate(-90deg); transform-origin: center; width: 80px; height: 24px; top: 50%; left: 50%; margin-top: -12px; margin-left: -40px;"
              >
                <Slider
                  :model-value="graphicGains[i]"
                  :min="-12"
                  :max="12"
                  :step="0.5"
                  class="w-full"
                  @update:model-value="(val: number) => onGainUpdated(i, val)"
                />
              </div>
            </div>
          </div>
          <span class="text-[10px] text-zinc-400 mt-1">{{ formatHz(f) }}</span>
        </div>
      </div>
    </div>

    <!-- Paste Text Modal -->
    <div
      v-if="showPasteModal"
      class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
      @click.self="showPasteModal = false"
    >
      <div class="w-full max-w-md bg-zinc-900 border border-white/10 rounded-2xl p-5 shadow-2xl space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-semibold text-white">Paste AutoEq / APO Filter Lines</h3>
          <button
            class="p-1 rounded-lg text-zinc-400 hover:text-white hover:bg-white/10 transition-colors"
            @click="showPasteModal = false"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <p class="text-xs text-zinc-400 leading-normal">
          Paste lines directly from AutoEq's ParametricEQ.txt or Equalizer APO configuration.
        </p>

        <textarea
          v-model="pasteText"
          rows="7"
          placeholder="Preamp: -6.0 dB&#10;Filter 1: ON PK Fc 28 Hz Gain 6.3 dB Q 1.41&#10;Filter 2: ON PK Fc 105 Hz Gain -2.1 dB Q 0.71&#10;Filter 3: ON PK Fc 719 Hz Gain 3.6 dB Q 1.41"
          class="w-full px-3 py-2 rounded-xl bg-zinc-950 border border-white/10 text-xs font-mono text-zinc-200 focus:outline-none focus:ring-1 focus:ring-[#f0caa3] resize-none"
        />

        <p
          v-if="pasteError"
          class="text-xs text-red-400 bg-red-400/10 px-3 py-1.5 rounded-lg border border-red-400/20"
        >
          {{ pasteError }}
        </p>

        <div class="flex justify-end gap-2 pt-2 border-t border-white/10">
          <button
            class="px-4 py-1.5 rounded-full text-xs font-medium text-zinc-400 hover:text-white hover:bg-white/5 transition-colors"
            @click="showPasteModal = false"
          >
            Cancel
          </button>
          <button
            :disabled="!pasteText.trim()"
            class="px-4 py-1.5 rounded-full text-xs font-medium bg-[#f0caa3] text-zinc-950 hover:bg-[#e6be95] disabled:opacity-40 transition-colors"
            @click="applyPastedText"
          >
            Apply Filters
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
