<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { EQ_BANDS, response, autoHeadroom, formatHz, formatFreq, formatFilterKind, formatGain } from '../lib/dsp'
import type { EqSettings, Filter } from '../lib/dsp'

const props = withDefaults(
  defineProps<{
    mode?: 'graphic' | 'parametric'
    gains?: number[]
    filters?: Filter[]
    preamp?: number
    enabled?: boolean
    selectedIndex?: number | null
  }>(),
  {
    mode: 'parametric',
    gains: () => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    filters: () => [],
    preamp: 0,
    enabled: true,
    selectedIndex: null,
  }
)

const emit = defineEmits<{
  'update:gain': [index: number, gain: number]
  'update:filter': [index: number, updated: Filter]
  'select:filter': [index: number | null]
  'add:filter': [filter: Filter]
  'delete:filter': [index: number]
}>()

const H = 240
const RANGE = 15
const PAD = 22
const W = ref(800)
const svgRef = ref<SVGSVGElement | null>(null)
const dragIndex = ref<number | null>(null)
const hoveredIndex = ref<number | null>(null)

const GRID_FREQS = [50, 100, 200, 500, 1000, 2000, 5000, 10000]
const DB_TICKS = [12, 6, 0, -6, -12]

// Logarithmic frequency to X coordinate (20 Hz - 20 kHz)
const x = (f: number) => {
  const clamped = Math.max(20, Math.min(20000, f))
  return (Math.log10(clamped / 20) / Math.log10(1000)) * W.value
}

// X coordinate to frequency
const freqAt = (px: number) => {
  const norm = Math.max(0, Math.min(1, px / W.value))
  const f = 20 * Math.pow(1000, norm)
  if (f < 100) return Math.round(f)
  if (f < 1000) return Math.round(f / 5) * 5
  if (f < 5000) return Math.round(f / 20) * 20
  return Math.round(f / 50) * 50
}

// dB gain to Y coordinate
const y = (g: number) => {
  const clamped = Math.max(-RANGE, Math.min(RANGE, g))
  return H / 2 - (clamped / RANGE) * (H / 2 - PAD)
}

// Y coordinate to dB gain
const gainAt = (py: number) => {
  const g = ((H / 2 - py) / (H / 2 - PAD)) * RANGE
  return Math.round(Math.max(-RANGE, Math.min(RANGE, g)) * 10) / 10
}

const isGraphic = computed(() => props.mode === 'graphic' || props.mode === 'presets')

const eqSettings = computed<EqSettings>(() => ({
  enabled: props.enabled,
  mode: isGraphic.value ? 'graphic' : 'parametric',
  preamp: props.preamp,
  gains: props.gains,
  filters: props.filters,
}))

// High-resolution response curve points
const curvePoints = computed(() => {
  const numSteps = Math.max(160, Math.round(W.value / 3))
  const pts = response(eqSettings.value, numSteps)
  return pts.map((p) => {
    const px = Number.isFinite(p.freq) ? x(p.freq) : 0
    const py = Number.isFinite(p.db) ? y(p.db) : H / 2
    return `${px.toFixed(1)},${py.toFixed(1)}`
  }).join(' ')
})

const areaPolygon = computed(() => {
  return `0,${H} ${curvePoints.value} ${W.value},${H}`
})

const headroomDb = computed(() => autoHeadroom(eqSettings.value))

// Interactive nodes (parametric filters or graphic sliders)
const nodes = computed(() => {
  if (isGraphic.value) {
    return EQ_BANDS.map((f, i) => {
      const g = props.gains[i] ?? 0
      return {
        index: i,
        freq: f,
        gain: g,
        q: 1.41,
        kind: 'peak' as const,
        px: x(f),
        py: y(g),
        label: `${formatHz(f)}: ${formatGain(g)}`,
        sub: 'Graphic Band',
      }
    })
  }

  return (props.filters || []).map((f, i) => ({
    index: i,
    freq: f.freq,
    gain: f.gain,
    q: f.q,
    kind: f.kind,
    px: x(f.freq),
    py: y(f.gain),
    label: `${formatFilterKind(f.kind)} ${formatFreq(f.freq)}, ${formatGain(f.gain)}`,
    sub: `Q: ${f.q.toFixed(2)}`,
  }))
})

const activeNode = computed(() => {
  if (dragIndex.value !== null) return nodes.value[dragIndex.value] || null
  if (props.selectedIndex !== null && props.selectedIndex !== undefined) return nodes.value[props.selectedIndex] || null
  if (hoveredIndex.value !== null) return nodes.value[hoveredIndex.value] || null
  return null
})

let ro: ResizeObserver | null = null

onMounted(() => {
  if (svgRef.value?.parentElement) {
    ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.contentRect.width > 100) {
          W.value = entry.contentRect.width
        }
      }
    })
    ro.observe(svgRef.value.parentElement)
    W.value = svgRef.value.parentElement.clientWidth || 800
  }
  window.addEventListener('keydown', onKeyDown)
})

onUnmounted(() => {
  if (ro) ro.disconnect()
  window.removeEventListener('keydown', onKeyDown)
})

function onPointerDown(e: PointerEvent, idx: number) {
  e.preventDefault()
  e.stopPropagation()
  dragIndex.value = idx
  emit('select:filter', idx)
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
}

function onPointerMove(e: PointerEvent) {
  if (dragIndex.value === null || !svgRef.value) return
  const rect = svgRef.value.getBoundingClientRect()
  const px = ((e.clientX - rect.left) / rect.width) * W.value
  const py = ((e.clientY - rect.top) / rect.height) * H

  if (isGraphic.value) {
    const newGain = gainAt(py)
    emit('update:gain', dragIndex.value, newGain)
  } else {
    const filter = props.filters[dragIndex.value]
    if (!filter) return
    const newFreq = freqAt(px)
    const newGain = gainAt(py)
    emit('update:filter', dragIndex.value, {
      ...filter,
      freq: newFreq,
      gain: newGain,
    })
  }
}

function onPointerUp(e: PointerEvent) {
  if (dragIndex.value !== null) {
    try {
      ;(e.currentTarget as Element)?.releasePointerCapture(e.pointerId)
    } catch {
      // ignore
    }
    dragIndex.value = null
  }
}

function onWheel(e: WheelEvent, idx: number) {
  e.preventDefault()
  e.stopPropagation()
  if (isGraphic.value) return
  const filter = props.filters[idx]
  if (!filter) return

  // Wheel up narrows (higher Q), wheel down widens (lower Q)
  const factor = e.deltaY < 0 ? 1.08 : 0.92
  const newQ = Math.round(Math.max(0.1, Math.min(16.0, filter.q * factor)) * 100) / 100
  emit('update:filter', idx, {
    ...filter,
    q: newQ,
  })
}

function onSvgDoubleClick(e: MouseEvent) {
  if (isGraphic.value || !svgRef.value) return
  const rect = svgRef.value.getBoundingClientRect()
  const px = ((e.clientX - rect.left) / rect.width) * W.value
  const py = ((e.clientY - rect.top) / rect.height) * H
  const freq = freqAt(px)
  const gain = gainAt(py)

  emit('add:filter', {
    kind: 'peak',
    freq,
    gain,
    q: 1.41,
    enabled: true,
  })
}

function onKeyDown(e: KeyboardEvent) {
  if (isGraphic.value) return
  if (props.selectedIndex === null || props.selectedIndex === undefined) return
  const idx = props.selectedIndex
  const filter = props.filters[idx]
  if (!filter) return

  if (e.key === 'Delete' || e.key === 'Backspace') {
    e.preventDefault()
    emit('delete:filter', idx)
    return
  }

  if (e.key === 'ArrowUp') {
    e.preventDefault()
    const gain = Math.min(RANGE, Math.round((filter.gain + 0.5) * 10) / 10)
    emit('update:filter', idx, { ...filter, gain })
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    const gain = Math.max(-RANGE, Math.round((filter.gain - 0.5) * 10) / 10)
    emit('update:filter', idx, { ...filter, gain })
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault()
    const freq = Math.max(20, Math.round(filter.freq * 0.96))
    emit('update:filter', idx, { ...filter, freq })
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    const freq = Math.min(20000, Math.round(filter.freq * 1.04))
    emit('update:filter', idx, { ...filter, freq })
  }
}
</script>

<template>
  <div class="w-full relative select-none rounded-2xl bg-[#0f1013] border border-white/5 p-2 overflow-hidden shadow-2xl">
    <!-- SVG Area -->
    <svg
      ref="svgRef"
      :viewBox="`0 0 ${W} ${H}`"
      class="w-full h-[240px] overflow-visible cursor-crosshair"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @dblclick="onSvgDoubleClick"
    >
      <defs>
        <!-- Soft gradient glow underneath the curve matching the reference -->
        <linearGradient id="eqShadingGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#b8a18c" stop-opacity="0.22" />
          <stop offset="50%" stop-color="#8e96aa" stop-opacity="0.08" />
          <stop offset="100%" stop-color="#0f1013" stop-opacity="0.0" />
        </linearGradient>

        <filter id="nodeGlow" x="-50%" y="-50%" width="200%" height="200%">
          <feDropShadow dx="0" dy="0" stdDeviation="3.5" flood-color="#ffffff" flood-opacity="0.45" />
        </filter>
      </defs>

      <!-- Vertical Logarithmic Frequency Grid Lines -->
      <g>
        <g v-for="f in GRID_FREQS" :key="f">
          <line
            :x1="x(f)"
            :x2="x(f)"
            y1="12"
            :y2="H - 22"
            stroke="rgba(255, 255, 255, 0.05)"
            stroke-width="1"
            stroke-dasharray="3 4"
          />
          <text
            :x="x(f)"
            :y="H - 8"
            text-anchor="middle"
            class="text-[10px] fill-zinc-500 font-sans font-medium select-none"
          >
            {{ formatHz(f) }}
          </text>
        </g>
      </g>

      <!-- Horizontal dB Reference Lines -->
      <g>
        <g v-for="g in DB_TICKS" :key="g">
          <!-- 0 dB Axis line (distinct dashed line) -->
          <line
            v-if="g === 0"
            x1="32"
            :x2="W"
            :y1="y(0)"
            :y2="y(0)"
            stroke="rgba(255, 255, 255, 0.16)"
            stroke-width="1"
            stroke-dasharray="4 4"
          />
          <line
            v-else
            x1="32"
            :x2="W"
            :y1="y(g)"
            :y2="y(g)"
            stroke="rgba(255, 255, 255, 0.04)"
            stroke-width="1"
            stroke-dasharray="3 4"
          />
          <!-- dB label on the left -->
          <text
            x="8"
            :y="y(g) + 3.5"
            class="text-[10px] fill-zinc-500 font-sans select-none"
          >
            {{ g > 0 ? `+${g}` : g }}
          </text>
        </g>
      </g>

      <!-- Shaded Area Under the Curve -->
      <polygon :points="areaPolygon" fill="url(#eqShadingGrad)" />

      <!-- Smooth Response Curve Stroke -->
      <polyline
        :points="curvePoints"
        fill="none"
        stroke="#9ba3b4"
        stroke-width="2.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />

      <!-- Draggable Nodes (Points along curve) -->
      <g v-for="node in nodes" :key="node.index">
        <!-- Hit target area -->
        <circle
          :cx="node.px"
          :cy="node.py"
          r="16"
          fill="transparent"
          class="cursor-grab active:cursor-grabbing"
          @pointerdown="onPointerDown($event, node.index)"
          @mouseenter="hoveredIndex = node.index"
          @mouseleave="hoveredIndex = null"
          @wheel="onWheel($event, node.index)"
        />

        <!-- Selected ring halo -->
        <circle
          v-if="selectedIndex === node.index || dragIndex === node.index"
          :cx="node.px"
          :cy="node.py"
          r="9.5"
          fill="rgba(226, 187, 152, 0.2)"
          stroke="#e2bb98"
          stroke-width="1"
          filter="url(#nodeGlow)"
        />

        <!-- The Node Ring (Hollow with styled border matching screenshot) -->
        <circle
          :cx="node.px"
          :cy="node.py"
          :r="selectedIndex === node.index || dragIndex === node.index ? 7 : 6"
          :fill="selectedIndex === node.index || dragIndex === node.index ? '#272930' : '#141519'"
          :stroke="selectedIndex === node.index || dragIndex === node.index ? '#ffffff' : '#cbb59d'"
          :stroke-width="selectedIndex === node.index || dragIndex === node.index ? 2.5 : 2.2"
          class="pointer-events-none transition-transform"
        />
      </g>
    </svg>

    <!-- Floating Readout Tooltip when dragging/selecting a node -->
    <div
      v-if="activeNode"
      class="absolute top-3 left-10 pointer-events-none bg-zinc-900/90 border border-white/10 px-3 py-1 rounded-lg backdrop-blur-md shadow-lg flex items-center gap-2 text-xs"
    >
      <span class="text-white font-medium">{{ activeNode.label }}</span>
      <span v-if="mode === 'parametric'" class="text-zinc-400 font-mono text-[11px]">{{ activeNode.sub }}</span>
    </div>
  </div>
</template>
