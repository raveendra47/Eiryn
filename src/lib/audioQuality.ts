import type { Track } from './invoke'

// Audio quality tiers (ported from Airmedy's `frontend/src/lib/audioQuality.ts`).
export type AudioQualityTier = 'LOSSY' | 'LOSSLESS' | 'HI-RES' | 'DSD' | 'UNKNOWN'

const LOSSY_EXTENSIONS = new Set(['mp3', 'aac', 'ogg', 'opus'])
const CONTAINER_EXTENSIONS = new Set(['m4a', 'mp4'])
const DSD_EXTENSIONS = new Set(['dsf', 'dff'])
const LOSSLESS_EXTENSIONS = new Set(['flac', 'wav', 'aiff', 'ape', 'wv', 'tta'])

// Eiryn's Track has no codec field (unlike Airmedy's TrackDTO), so m4a/mp4
// can't be told apart (AAC vs ALAC) — report UNKNOWN (badge hidden) rather
// than guess blind. Revisit if backend codec extraction is added.
export function determineAudioQuality(track: Pick<Track, 'file_format' | 'bit_depth' | 'sample_rate'>): AudioQualityTier {
  const ext = (track.file_format || '').toLowerCase()

  if (LOSSY_EXTENSIONS.has(ext)) return 'LOSSY'

  if (CONTAINER_EXTENSIONS.has(ext)) return 'UNKNOWN'

  if (DSD_EXTENSIONS.has(ext)) return 'DSD'

  if (LOSSLESS_EXTENSIONS.has(ext)) {
    const bitDepth = track.bit_depth || 0
    const isHiRes = (bitDepth > 0 && bitDepth > 16) || track.sample_rate > 48000
    if (isHiRes) return 'HI-RES'
    return 'LOSSLESS'
  }

  return 'UNKNOWN'
}
