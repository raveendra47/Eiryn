<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import type { EChartsOption } from 'echarts'
import { PieChart } from 'echarts/charts'
import { TooltipComponent } from 'echarts/components'
import { SVGRenderer } from 'echarts/renderers'
import { useChartTheme } from './useChartTheme'
import { useAppStore } from '../../stores/app'

use([PieChart, TooltipComponent, SVGRenderer])

const props = defineProps<{
  genres: { name: string; seconds: number; is_other: boolean }[]
}>()

const appStore = useAppStore()
const { tooltipBase, hexToRgba } = useChartTheme()

const total = computed(() => props.genres.reduce((sum, genre) => sum + genre.seconds, 0))

const palette = computed(() => [
  appStore.primaryColor ?? '#E11D48',
  '#a5b4fc',
  '#67e8f9',
  '#fda4af',
  '#fcd34d',
  '#94a3b8',
])

function formatDuration(secs: number): string {
  const m = Math.round(secs / 60)
  const h = Math.floor(m / 60)
  if (h > 0) return `${h}h ${m % 60}m`
  return `${m}m`
}

const chartOption = computed<EChartsOption>(() => ({
  animationDuration: 650,
  animationDurationUpdate: 400,
  animationEasing: 'cubicOut',
  tooltip: {
    ...tooltipBase.value,
    trigger: 'item',
    formatter: (params) => {
      const item = Array.isArray(params) ? params[0] : params
      return `${item.name}<br/><strong>${formatDuration(Number(item.value))} · ${item.percent}%</strong>`
    },
  },
  series: [{
    type: 'pie',
    radius: ['62%', '82%'],
    center: ['50%', '50%'],
    startAngle: 90,
    itemStyle: { borderColor: 'var(--bg-glass)', borderWidth: 3, borderRadius: 5 },
    label: { show: false },
    labelLine: { show: false },
    emphasis: { scale: true, scaleSize: 5, itemStyle: { shadowBlur: 16, shadowColor: hexToRgba(appStore.primaryColor ?? '#E11D48', 0.38) } },
    data: props.genres.map((genre, index) => ({
      value: genre.seconds,
      name: genre.is_other ? 'Other' : genre.name,
      itemStyle: { color: palette.value[index] ?? palette.value[palette.value.length - 1] },
    })),
  }],
}))
</script>

<template>
  <div class="relative h-[clamp(9rem,18vw,12rem)] w-[clamp(9rem,18vw,12rem)] shrink-0">
    <VChart class="h-full w-full" :option="chartOption" :init-options="{ renderer: 'svg' }" autoresize />
    <div class="pointer-events-none absolute inset-0 flex items-center justify-center">
      <span class="text-xl font-semibold tracking-tight">{{ formatDuration(total) }}</span>
    </div>
  </div>
</template>
