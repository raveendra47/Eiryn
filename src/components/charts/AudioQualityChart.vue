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
  quality: { kind: string; count: number }[]
}>()

const appStore = useAppStore()
const { tooltipBase, hexToRgba } = useChartTheme()

function formatNum(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M'
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K'
  return String(n)
}

const QUALITY_LABELS: Record<string, string> = {
  lossy: 'Lossy',
  lossless: 'Lossless',
  hi_res: 'Hi-Res',
  dsd: 'DSD',
  unknown: 'Unknown',
}

const total = computed(() => props.quality.reduce((sum, item) => sum + item.count, 0))

const colors = computed<Record<string, string>>(() => ({
  lossless: appStore.primaryColor ?? '#E11D48',
  hi_res: '#a5b4fc',
  dsd: '#fda4af',
  lossy: '#94a3b8',
  unknown: '#52525b',
}))

const chartOption = computed<EChartsOption>(() => ({
  animationDuration: 650,
  animationDurationUpdate: 400,
  animationEasing: 'cubicOut',
  tooltip: {
    ...tooltipBase.value,
    trigger: 'item',
    formatter: (params) => {
      const item = Array.isArray(params) ? params[0] : params
      return `${item.name}<br/><strong>${formatNum(Number(item.value))} · ${item.percent}%</strong>`
    },
  },
  series: [{
    type: 'pie',
    radius: ['62%', '82%'],
    center: ['50%', '50%'],
    startAngle: 90,
    clockwise: true,
    avoidLabelOverlap: true,
    itemStyle: { borderColor: 'var(--bg-glass)', borderWidth: 3, borderRadius: 5 },
    label: { show: false },
    labelLine: { show: false },
    emphasis: { scale: true, scaleSize: 5, itemStyle: { shadowBlur: 16, shadowColor: hexToRgba(appStore.primaryColor ?? '#E11D48', 0.38) } },
    data: props.quality.map((item) => ({
      value: item.count,
      name: QUALITY_LABELS[item.kind] ?? item.kind,
      itemStyle: { color: colors.value[item.kind] || colors.value.unknown },
    })),
  }],
}))
</script>

<template>
  <div class="relative h-[clamp(9rem,18vw,12rem)] w-[clamp(9rem,18vw,12rem)] shrink-0">
    <VChart class="h-full w-full" :option="chartOption" :init-options="{ renderer: 'svg' }" autoresize />
    <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
      <span class="text-xl font-semibold tracking-tight">{{ formatNum(total) }}</span>
    </div>
  </div>
</template>
