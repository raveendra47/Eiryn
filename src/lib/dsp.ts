// RBJ Biquad Frequency Response calculations & Auto Headroom (Cadence parity)
export const EQ_BANDS = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
const GRAPHIC_Q = 1.41

export interface Filter {
  kind: 'peak' | 'lowshelf' | 'highshelf' | 'lowpass' | 'highpass' | string
  freq: number
  gain: number
  q: number
  enabled: boolean
}

export interface EqSettings {
  enabled: boolean
  mode?: 'graphic' | 'parametric' | 'presets' | string
  preamp: number
  gains: number[]
  filters: Filter[]
}

interface Coeffs {
  b0: number
  b1: number
  b2: number
  a1: number
  a2: number
}

export function normalizeFilterKind(kind?: string): 'peak' | 'lowshelf' | 'highshelf' | 'lowpass' | 'highpass' {
  const k = (kind || '').trim().toLowerCase()
  if (k === 'peak' || k === 'pk' || k === 'peq' || k === 'bell') return 'peak'
  if (k === 'lowshelf' || k === 'ls' || k === 'lsc' || k === 'lsq' || k === 'low shelf') return 'lowshelf'
  if (k === 'highshelf' || k === 'hs' || k === 'hsc' || k === 'hsq' || k === 'high shelf') return 'highshelf'
  if (k === 'lowpass' || k === 'lp' || k === 'lpq' || k === 'low pass') return 'lowpass'
  if (k === 'highpass' || k === 'hp' || k === 'hpq' || k === 'high pass') return 'highpass'
  return 'peak'
}

export function activeFilters(eq: EqSettings): Filter[] {
  const mode = (eq.mode || '').toLowerCase()
  if (mode === 'graphic' || mode === 'presets' || !mode) {
    return EQ_BANDS.map((freq, i) => ({
      kind: 'peak' as const,
      freq,
      gain: eq.gains?.[i] ?? 0,
      q: GRAPHIC_Q,
      enabled: true,
    })).filter((f) => f.gain !== 0)
  }
  return (eq.filters || []).map((f) => ({
    ...f,
    kind: normalizeFilterKind(f.kind),
    freq: Number(f.freq) || 1000,
    gain: Number(f.gain) || 0,
    q: Number(f.q) || 1.41,
    enabled: f.enabled !== false,
  })).filter((f) => f.enabled)
}

function design(f: Filter, rate: number): Coeffs {
  const gain = Number.isFinite(f.gain) ? f.gain : 0
  const freq = Number.isFinite(f.freq) ? f.freq : 1000
  const qVal = Math.max(Number.isFinite(f.q) ? f.q : 1.41, 0.01)

  const A = Math.pow(10, gain / 40)
  const w0 = (2 * Math.PI * freq) / rate
  const sin = Math.sin(w0)
  const cos = Math.cos(w0)
  const alpha = sin / (2 * qVal)
  let b0 = 1, b1 = 0, b2 = 0, a0 = 1, a1 = 0, a2 = 0

  const kind = normalizeFilterKind(f.kind)
  switch (kind) {
    case 'peak':
      b0 = 1 + alpha * A
      b1 = -2 * cos
      b2 = 1 - alpha * A
      a0 = 1 + alpha / A
      a1 = -2 * cos
      a2 = 1 - alpha / A
      break
    case 'lowshelf': {
      const s = 2 * Math.sqrt(A) * alpha
      b0 = A * (A + 1 - (A - 1) * cos + s)
      b1 = 2 * A * (A - 1 - (A + 1) * cos)
      b2 = A * (A + 1 - (A - 1) * cos - s)
      a0 = A + 1 + (A - 1) * cos + s
      a1 = -2 * (A - 1 + (A + 1) * cos)
      a2 = A + 1 + (A - 1) * cos - s
      break
    }
    case 'highshelf': {
      const s = 2 * Math.sqrt(A) * alpha
      b0 = A * (A + 1 + (A - 1) * cos + s)
      b1 = -2 * A * (A - 1 + (A + 1) * cos)
      b2 = A * (A + 1 - (A - 1) * cos - s)
      a0 = A + 1 - (A - 1) * cos + s
      a1 = 2 * (A - 1 - (A + 1) * cos)
      a2 = A + 1 - (A - 1) * cos - s
      break
    }
    case 'lowpass':
      b0 = (1 - cos) / 2
      b1 = 1 - cos
      b2 = (1 - cos) / 2
      a0 = 1 + alpha
      a1 = -2 * cos
      a2 = 1 - alpha
      break
    case 'highpass':
      b0 = (1 + cos) / 2
      b1 = -(1 + cos)
      b2 = (1 + cos) / 2
      a0 = 1 + alpha
      a1 = -2 * cos
      a2 = 1 - alpha
      break
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

function magnitudeDb(c: Coeffs, freq: number, rate: number): number {
  const w = (2 * Math.PI * freq) / rate
  const [c1, s1, c2, s2] = [Math.cos(w), -Math.sin(w), Math.cos(2 * w), -Math.sin(2 * w)]
  const nr = c.b0 + c.b1 * c1 + c.b2 * c2
  const ni = c.b1 * s1 + c.b2 * s2
  const dr = 1 + c.a1 * c1 + c.a2 * c2
  const di = c.a1 * s1 + c.a2 * s2
  const mag = (nr * nr + ni * ni) / Math.max(1e-12, dr * dr + di * di)
  return 10 * Math.log10(Math.max(1e-12, mag))
}

/** Combined response (dB, including manual preamp) across logarithmic frequency sweep */
export function response(eq: EqSettings, points = 240, rate = 48000): { freq: number; db: number }[] {
  const filters = activeFilters(eq)
    .filter((f) => f.freq > 0 && f.freq < rate * 0.45)
    .map((f) => design(f, rate))
  const preamp = Number.isFinite(eq.preamp) ? eq.preamp : 0
  return Array.from({ length: points + 1 }, (_, i) => {
    const freq = 20 * Math.pow(1000, i / points)
    const db = filters.reduce((sum, c) => sum + magnitudeDb(c, freq, rate), preamp)
    return { freq, db: Number.isFinite(db) ? db : 0 }
  })
}

/** Compute the automatic digital headroom (dB, <= 0) to prevent clipping */
export function autoHeadroom(eq: EqSettings): number {
  const resp = response(eq, 300)
  const top = Math.max(...resp.map((p) => p.db))
  return top > 0 ? -Math.round(top * 10) / 10 : 0
}

export const formatHz = (f: number) => (f >= 1000 ? `${+(f / 1000).toFixed(f % 1000 === 0 ? 0 : 1)}k` : `${Math.round(f)}`)
export const formatFreq = (f: number) => (f >= 1000 ? `${+(f / 1000).toFixed(f % 1000 === 0 ? 0 : 1)} kHz` : `${Math.round(f)} Hz`)

export function formatFilterKind(kind?: Filter['kind'] | string): string {
  const k = normalizeFilterKind(kind)
  switch (k) {
    case 'peak':
      return 'Bell'
    case 'lowshelf':
      return 'Low shelf'
    case 'highshelf':
      return 'High shelf'
    case 'lowpass':
      return 'Low pass'
    case 'highpass':
      return 'High pass'
    default:
      return 'Bell'
  }
}

export function formatGain(gain: number): string {
  const val = Number.isFinite(gain) ? gain : 0
  const sign = val > 0 ? '+' : ''
  return `${sign}${val.toFixed(1)} dB`
}
