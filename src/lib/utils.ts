/**
 * Utility functions for Eiryn player
 */

/**
 * Convert hex color to rgba string
 * @param hex - Hex color code (e.g., '#FF0000')
 * @param alpha - Alpha value (0-1)
 * @returns rgba string
 */
export function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

export function darken(hex: string, factor: number): string {
  const r = Math.round(parseInt(hex.slice(1, 3), 16) * factor)
  const g = Math.round(parseInt(hex.slice(3, 5), 16) * factor)
  const b = Math.round(parseInt(hex.slice(5, 7), 16) * factor)
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
}

/**
 * Get display title for a track
 * @param track - Track object
 * @returns Display title
 */
export function getTrackDisplayTitle(track: any): string {
  return track?.title || 'Unknown Track'
}

/**
 * Format milliseconds to MM:SS format
 * @param ms - Milliseconds
 * @returns Formatted time string
 */
export function formatTime(ms: number): string {
  if (!isFinite(ms)) return '0:00'
  const s = Math.floor(ms)
  const m = Math.floor(s / 60)
  const sec = s % 60
  return `${m}:${sec.toString().padStart(2, '0')}`
}

/**
 * Clamp a number between min and max
 * @param num - Number to clamp
 * @param min - Minimum value
 * @param max - Maximum value
 * @returns Clamped number
 */
export function clamp(num: number, min: number, max: number): number {
  return Math.min(Math.max(num, min), max)
}

/**
 * Sanitize untrusted HTML string to prevent XSS.
 * Removes executable tags (script, iframe, object, embed) and dangerous attributes (onclick, onload, javascript:).
 */
export function sanitizeHtml(html: string): string {
  if (!html) return ''
  try {
    const parser = new DOMParser()
    const doc = parser.parseFromString(html, 'text/html')

    const dangerousTags = ['script', 'style', 'iframe', 'object', 'embed', 'link', 'base', 'form', 'frame', 'applet']
    for (const tag of dangerousTags) {
      const elements = doc.querySelectorAll(tag)
      elements.forEach((el) => el.remove())
    }

    const allElements = doc.querySelectorAll('*')
    allElements.forEach((el) => {
      const attrs = Array.from(el.attributes)
      for (const attr of attrs) {
        const name = attr.name.toLowerCase()
        const val = attr.value.toLowerCase().trim()
        if (
          name.startsWith('on') ||
          val.startsWith('javascript:') ||
          val.startsWith('data:text/html') ||
          val.startsWith('vbscript:')
        ) {
          el.removeAttribute(attr.name)
        }
      }
    })

    return doc.body.innerHTML
  } catch {
    return ''
  }
}


export function trackTelemetry(t: { file_format: string; bit_depth: number; sample_rate: number; channels: number }): string {
  const fmt = (t.file_format || '').toUpperCase()
  const depth = t.bit_depth ? `${t.bit_depth}-bit` : ''
  const rate = t.sample_rate ? `${(t.sample_rate / 1000).toFixed(1)}kHz` : ''
  const ch = t.channels ? `${t.channels}ch` : ''
  const bps = t.sample_rate && t.bit_depth && t.channels
    ? `${((t.sample_rate * t.bit_depth * t.channels) / 1000).toFixed(0)} kbps` : ''
  return [fmt, depth, rate, ch, bps].filter(Boolean).join(' · ')
}
