import { computed } from 'vue'
import { useAppStore } from '../../stores/app'
import { hexToRgba } from '../../lib/artwork'

function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return v || fallback
}

/** Shared echarts theme: runtime accent + glass tooltip surface per theme. */
export function useChartTheme() {
  const appStore = useAppStore()

  const primary = computed(() => {
    void appStore.theme
    return (
      appStore.primaryColor ?? cssVar('--primary', '#E11D48')
    )
  })

  const tooltipSurface = computed(() => {
    void appStore.theme
    const root = document.documentElement
    if (root.classList.contains('black'))
      return { background: 'rgba(25, 25, 25, 0.96)', border: 'rgba(255, 255, 255, 0.12)', text: '#ffffff' }
    if (root.classList.contains('dark'))
      return { background: 'rgba(55, 55, 60, 0.96)', border: 'rgba(255, 255, 255, 0.12)', text: '#ffffff' }
    return { background: 'rgba(255, 255, 255, 0.96)', border: 'rgba(0, 0, 0, 0.12)', text: '#0a0a0a' }
  })

  const tooltipBase = computed(() => ({
    backgroundColor: tooltipSurface.value.background,
    borderColor: tooltipSurface.value.border,
    borderWidth: 1,
    extraCssText:
      'border-radius: 12px; backdrop-filter: blur(20px); -webkit-backdrop-filter: blur(20px); box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.35);',
    padding: [8, 10] as unknown as number,
    textStyle: { color: tooltipSurface.value.text, fontSize: 12 },
  }))

  return { primary, tooltipSurface, tooltipBase, hexToRgba }
}
