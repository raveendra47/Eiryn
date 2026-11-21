import { computed, type Ref } from 'vue'

export interface LyricWord {
  time: number
  text: string
}

export interface LyricLine {
  time: number
  text: string
  secondary?: string
  words?: LyricWord[]
}

const TS_RE = /\[(\d+):(\d+(?:\.\d+)?)\]/
const LINE_TS_RE = /^(\[\d+:\d+(?:\.\d+)?\])+/
const WORD_TS_RE = /<(\d+):(\d+(?:\.\d+)?)>(.*?)(?=<|$)/g
const OFFSET_RE = /^\[offset:([+-]?\d+)\]/

function parseTimestamp(ts: string): number {
  const m = ts.match(TS_RE)
  if (!m) return 0
  return parseInt(m[1], 10) * 60 + parseFloat(m[2])
}

function parseText(raw: string): { primary: string; secondary?: string } {
  const sep = raw.includes('^') ? '^' : raw.includes(' / ') ? ' / ' : null
  if (sep) {
    const parts = raw.split(sep).map(s => s.trim())
    return { primary: parts[0], secondary: parts[1] }
  }
  return { primary: raw }
}

export function useLyrics(lyrics: Ref<string | undefined>) {
  const isSynced = computed(() => {
    if (!lyrics.value) return false
    return TS_RE.test(lyrics.value)
  })

  const syncedLines = computed<LyricLine[]>(() => {
    if (!lyrics.value) return []
    const lines: LyricLine[] = []

    let offsetMs = 0

    for (const raw of lyrics.value.split('\n')) {
      const trimmed = raw.trim()
      if (!trimmed) continue

      // Skip metadata tags (ti, ar, al, by, re, ve, etc.)
      if (/^\[[a-z]+:/.test(trimmed) && !trimmed.startsWith('[offset:')) continue

      // Parse global offset
      const offsetMatch = trimmed.match(OFFSET_RE)
      if (offsetMatch) {
        offsetMs = parseInt(offsetMatch[1], 10)
        continue
      }

      // Extract all [mm:ss.xx] line timestamps
      const tsMatch = trimmed.match(LINE_TS_RE)
      if (!tsMatch) continue

      const timestamps = trimmed.match(new RegExp(TS_RE.source, 'g'))
      if (!timestamps || timestamps.length === 0) continue

      // Text after all timestamps
      const textRaw = trimmed.slice(tsMatch[0].length).trim()

      // Parse word-level timestamps (Enhanced LRC A2)
      const words: LyricWord[] = []
      let wordlessText = textRaw
      let wordMatch: RegExpExecArray | null
      const wordRe = new RegExp(WORD_TS_RE.source, 'g')
      while ((wordMatch = wordRe.exec(textRaw)) !== null) {
        const wt = parseInt(wordMatch[1], 10) * 60 + parseFloat(wordMatch[2])
        words.push({ time: wt, text: wordMatch[3] })
      }
      if (words.length > 0) {
        wordlessText = textRaw.replace(WORD_TS_RE, '').trim()
      }

      const lineText = wordlessText || (words.length > 0 ? '' : textRaw)
      const { primary, secondary } = parseText(lineText)

      // Create a line for each timestamp (supports duplicate timestamps)
      for (const ts of timestamps) {
        const t = parseTimestamp(ts) + offsetMs / 1000
        if (words.length > 0) {
          lines.push({ time: t, text: primary, secondary, words })
        } else {
          lines.push({ time: t, text: primary, secondary })
        }
      }
    }

    return lines.sort((a, b) => a.time - b.time)
  })

  const plainLines = computed<{ primary: string; secondary?: string }[]>(() => {
    if (!lyrics.value) return []
    return lyrics.value
      .split('\n')
      .map(l => l.replace(/^\[(\d+:\d+(?:\.\d+)?)\]/, '').trim())
      .filter(l => l)
      .map(l => parseText(l.replace(WORD_TS_RE, '').trim()))
  })

  return { isSynced, syncedLines, plainLines }
}
