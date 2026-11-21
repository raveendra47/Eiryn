import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useAnalysisStore = defineStore('analysis', () => {
  const analyzing = ref(false)
  const analysisCurrent = ref(0)
  const analysisTotal = ref(0)
  const analysisFile = ref('')

  const fetchingLyrics = ref(false)
  const lyricsCurrent = ref(0)
  const lyricsTotal = ref(0)

  const fetchingArtwork = ref(false)
  const artworkCurrent = ref(0)
  const artworkTotal = ref(0)
  const artworkArtist = ref('')

  const fetchingGenre = ref(false)
  const genreCurrent = ref(0)
  const genreTotal = ref(0)
  const genreTrack = ref('')
  const genreArtist = ref('')

  return {
    analyzing, analysisCurrent, analysisTotal, analysisFile,
    fetchingLyrics, lyricsCurrent, lyricsTotal,
    fetchingArtwork, artworkCurrent, artworkTotal, artworkArtist,
    fetchingGenre, genreCurrent, genreTotal, genreTrack, genreArtist,
  }
})
