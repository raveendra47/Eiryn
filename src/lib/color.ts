// Accent-color utilities (ported from Airmedy's `frontend/src/lib/color.ts`).
// A custom accent overrides the active theme palette's `--primary` until reset.

export const DEFAULT_PRIMARY_COLOR = '#E11D48'

/** Airmedy accent preset swatches (GeneralSettings primary-color presets). */
export const PRIMARY_COLOR_PRESETS = [
  '#E11D48',
  '#2563EB',
  '#7E22CE',
  '#DB2777',
  '#EA580C',
  '#CA8A04',
  '#15803D',
] as const

const hexColorPattern = /^#[0-9a-fA-F]{6}$/

/** Strict `#RRGGBB` (uppercased) or null — anything else is rejected. */
export function normalizeHexColor(value: string | undefined | null): string | null {
  const color = value?.trim() ?? ''
  return hexColorPattern.test(color) ? color.toUpperCase() : null
}

/** `R G B` channel string for `rgb(var(--primary-rgb) / <alpha>)` usage. */
export function hexToRgbChannels(color: string): string {
  const normalized = normalizeHexColor(color) ?? DEFAULT_PRIMARY_COLOR
  return `${parseInt(normalized.slice(1, 3), 16)} ${parseInt(normalized.slice(3, 5), 16)} ${parseInt(normalized.slice(5, 7), 16)}`
}

function perceivedBrightness(color: string): number {
  const channels = hexToRgbChannels(color).split(' ').map(Number)
  const [red, green, blue] = channels
  return Math.sqrt(
    0.299 * red ** 2 +
    0.587 * green ** 2 +
    0.114 * blue ** 2,
  )
}

// Accent controls favor white text unless a color is visibly light (especially
// yellow or pastel). This keeps saturated colors such as orange visually cohesive.
const lightForegroundThreshold = 165

export function primaryForeground(color: string): '#FFFFFF' | '#18181B' {
  return perceivedBrightness(color) >= lightForegroundThreshold ? '#18181B' : '#FFFFFF'
}
