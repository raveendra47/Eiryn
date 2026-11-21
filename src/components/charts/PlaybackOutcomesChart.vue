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

const props = defineProps<{ completed: number; skipped: number; stopped: number }>()

const appStore = useAppStore()
const { tooltipBase, hexToRgba } = useChartTheme()

const total = computed(() => props.completed + props.skipped + props.stopped)

function formatNum(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M'
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K'
  return String(n)
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
      return `${item.name}<br/><strong>${formatNum(Number(item.value))} · ${item.percent}%</strong>`
    },
  },
  series: [{
    type: 'pie',
    radius: ['62%', '82%'],
    center: ['50%', '50%'],
    startAngle: 90,
    clockwise: true,
    itemStyle: { borderColor: 'var(--bg-glass)', borderWidth: 3, borderRadius: 5 },
    label: { show: false },
    labelLine: { show: false },
    emphasis: { scale: true, scaleSize: 5, itemStyle: { shadowBlur: 16, shadowColor: hexToRgba(appStore.primaryColor ?? '#E11D48', 0.38) } },
    data: [
      { value: props.completed, name: 'Completed', itemStyle: { color: appStore.primaryColor ?? '#E11D48' } },
      { value: props.skipped, name: 'Skipped', itemStyle: { color: '#fbbf24' } },
      { value: props.stopped, name: 'Stopped', itemStyle: { color: '#94a3b8' } },
    ],
  }],
}))
</script>

<template>
  <div class="relative h-[clamp(9rem,18vw,12rem)] w-[clamp(9rem,18vw,12rem)] shrink-0">
    <VChart class="h-full w-full" :option="chartOption" :init-options="{ renderer: 'svg' }" autoresize />
    <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
      <span class="text-xl font-semibold tracking-tight">{{ formatNum(total) }}</span>
      <span class="text-xs text-[color:var(--text-muted)]">attempts</span>
    </div>
  </div>
</template>
