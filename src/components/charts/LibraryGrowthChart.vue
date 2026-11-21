<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import type { EChartsOption } from 'echarts'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'
import { SVGRenderer } from 'echarts/renderers'
import { useChartTheme } from './useChartTheme'

use([LineChart, GridComponent, TooltipComponent, SVGRenderer])

const props = defineProps<{
  growth: { label: string; seconds: number }[]
}>()

const { primary, tooltipBase, hexToRgba } = useChartTheme()

function formatNum(n: number): string {
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M'
  if (n >= 1000) return (n / 1000).toFixed(1) + 'K'
  return String(n)
}

function parseLabel(label: string): Date {
  return new Date(label.length === 4 ? `${label}-01-01T12:00:00` : `${label}T12:00:00`)
}

function dayLabel(label: string): string {
  if (label.length === 4) return label
  const parsed = parseLabel(label)
  return props.growth.length <= 7
    ? new Intl.DateTimeFormat(undefined, { weekday: 'short' }).format(parsed)
    : new Intl.DateTimeFormat(undefined, { day: 'numeric' }).format(parsed)
}

function tooltipDate(label: string): string {
  return new Intl.DateTimeFormat(
    undefined,
    label.length === 4 ? { year: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' },
  ).format(parseLabel(label))
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
      return `${tooltipDate(props.growth[item.dataIndex].label)}<br/><strong>${formatNum(Number(item.data))}</strong>`
    },
  },
  xAxis: {
    type: 'category',
    boundaryGap: false,
    data: props.growth.map((point) => dayLabel(point.label)),
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
    type: 'line',
    data: props.growth.map((point) => point.seconds),
    smooth: true,
    showSymbol: props.growth.length === 1,
    symbolSize: props.growth.length === 1 ? 10 : 0,
    lineStyle: { color: primary.value, width: 2.5 },
    itemStyle: { color: primary.value, borderColor: primary.value, borderWidth: 0 },
    areaStyle: {
      color: {
        type: 'linear', x: 0, y: 0, x2: 0, y2: 1,
        colorStops: [
          { offset: 0, color: hexToRgba(primary.value, 0.42) },
          { offset: 1, color: hexToRgba(primary.value, 0) },
        ],
      },
    },
    emphasis: { itemStyle: { color: primary.value, borderColor: primary.value, borderWidth: 0 } },
  }],
}))
</script>

<template>
  <div class="h-full min-h-36 min-w-0">
    <VChart class="h-full w-full" :option="chartOption" :init-options="{ renderer: 'svg' }" autoresize />
  </div>
</template>
