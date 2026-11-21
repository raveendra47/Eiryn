<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import type { EChartsOption } from 'echarts'
import { BarChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'
import { SVGRenderer } from 'echarts/renderers'
import { useChartTheme } from './useChartTheme'

use([BarChart, GridComponent, TooltipComponent, SVGRenderer])

const props = defineProps<{
  activity: { label: string; seconds: number }[]
}>()

const { primary, tooltipBase, hexToRgba } = useChartTheme()

function formatDuration(secs: number): string {
  const m = Math.round(secs / 60)
  const h = Math.floor(m / 60)
  if (h > 0) return `${h}h ${m % 60}m`
  return `${m}m`
}

function dayLabel(label: string): string {
  // Monthly buckets (YYYY-MM) vs daily buckets (YYYY-MM-DD).
  const parsed = new Date(label.length === 7 ? `${label}-01T12:00:00` : `${label}T12:00:00`)
  if (label.length === 7)
    return new Intl.DateTimeFormat(undefined, { month: 'short' }).format(parsed)
  if (props.activity.length <= 7)
    return new Intl.DateTimeFormat(undefined, { weekday: 'short' }).format(parsed)
  return new Intl.DateTimeFormat(undefined, { day: 'numeric' }).format(parsed)
}

function tooltipDate(label: string): string {
  const parsed = new Date(label.length === 7 ? `${label}-01T12:00:00` : `${label}T12:00:00`)
  return new Intl.DateTimeFormat(
    undefined,
    label.length === 7
      ? { month: 'long', year: 'numeric' }
      : { month: 'short', day: 'numeric', year: 'numeric' },
  ).format(parsed)
}

const chartOption = computed<EChartsOption>(() => ({
  animationDuration: 700,
  animationDurationUpdate: 450,
  animationEasing: 'cubicOut',
  grid: { top: 14, right: 8, bottom: 26, left: 8 },
  tooltip: {
    ...tooltipBase.value,
    trigger: 'axis',
    formatter: (params) => {
      const item = Array.isArray(params) ? params[0] : params
      return `${tooltipDate(props.activity[item.dataIndex].label)}<br/><strong>${formatDuration(Number(item.data))}</strong>`
    },
  },
  xAxis: {
    type: 'category',
    boundaryGap: true,
    data: props.activity.map((point) => dayLabel(point.label)),
    axisLine: { show: false },
    axisTick: { show: false },
    axisLabel: { color: 'var(--text-muted)', fontSize: 10, margin: 12 },
  },
  yAxis: {
    type: 'value',
    show: false,
    min: 0,
    splitNumber: 3,
    splitLine: { show: true, lineStyle: { color: 'var(--border-glass)', type: 'dashed', opacity: 0.45 } },
  },
  series: [{
    type: 'bar',
    data: props.activity.map((point) => point.seconds),
    barMaxWidth: 24,
    barWidth: '54%',
    itemStyle: {
      color: {
        type: 'linear', x: 0, y: 0, x2: 0, y2: 1,
        colorStops: [
          { offset: 0, color: primary.value },
          { offset: 1, color: hexToRgba(primary.value, 0.22) },
        ],
      },
      borderRadius: [5, 5, 1, 1],
    },
    emphasis: { itemStyle: { shadowBlur: 14, shadowColor: hexToRgba(primary.value, 0.38) } },
  }],
}))
</script>

<template>
  <div class="h-full min-h-36 min-w-0">
    <VChart class="h-full w-full" :option="chartOption" :init-options="{ renderer: 'svg' }" autoresize />
  </div>
</template>
