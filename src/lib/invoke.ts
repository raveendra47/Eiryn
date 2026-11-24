import { invoke } from '@tauri-apps/api/core'

export interface Track {
  id: string
  file_path: string
  title: string
  artist: string
  album: string
  album_artist: string
  album_id: string
  composer: string
  track_number: number
  disc_number: number
  year: number
  genre: string
  raw_genre_names: string
  duration: number
  sample_rate: number
  bit_depth: number
  channels: number
  bitrate: number
  file_size: number
  file_format: string
  has_artwork: boolean
  replaygain_track_gain: number
  replaygain_album_gain: number
  cue_parent_id: string | null
  cue_offset: number
  artwork_key: string | null
  bpm: number
  musical_key: string
  energy: number
  analysis_failed_reason: string | null
  file_modified: number
  matched_lyric?: string
}

export interface Playlist {
  id: string
  name: string
  description: string
  is_smart: boolean
  smart_rules: string | null
  last_updated: string | null
  created_at: string
  is_folder_playlist: boolean
  folder_path: string | null
}

export interface ScanResult {
  added: number
  updated: number
  removed: number
}

export interface Genre {
  id: string
  name: string
  normalization_key: string
  track_count: number
}

export interface Album {
  id: string
  title: string
  artist: string
  year: number
  track_count: number
  duration: number
  artwork_key: string | null
  first_track_id: string
  is_hi_res: boolean
}

export interface SearchAllResults {
  tracks: Track[]
  albums: { name: string; artist: string; year: number; track_count: number; first_track_id: string }[]
  artists: string[]
  genres: string[]
  composers: string[]
}

export interface LyricsResult {
  title: string
  artist: string
  album: string
  lyrics: string
  source: string
}

export interface LyricRow {
  track_id: string
  content: string
  source: string
  meta_content: string
  meta_source: string
}

export const saveLyrics = (trackId: string, content: string, source: string) => invoke<void>('save_lyrics', { trackId, content, source })

export const getTrackLyrics = (trackId: string) => invoke<LyricRow | null>('get_track_lyrics', { trackId })
export const getAllLyrics = () => invoke<LyricRow[]>('get_all_lyrics')

export const batchFetchLyrics = (folderPath?: string, retryMissing = false) => invoke<string>('batch_fetch_lyrics', { folderPath, retryMissing })

export const getTracks = () => invoke<Track[]>('get_tracks')
export const getAlbums = () => invoke<Album[]>('get_albums')
export const getAlbumTracks = (albumId: string) => invoke<Track[]>('get_album_tracks', { albumId })
export const getTrack = (id: string) => invoke<Track | null>('get_track', { id })
export const searchTracks = (query: string) => invoke<Track[]>('search_tracks', { query })
export const searchAll = (query: string) => invoke<SearchAllResults>('search_all', { query })
export const scanFolder = (path: string) => invoke<ScanResult>('scan_folder', { path })
export const getArtwork = (id: string) => invoke<string>('get_artwork', { id })
export const seek = (posSecs: number) => invoke<void>('seek', { posSecs })
export const playTrack = (id: string) => invoke<void>('play_track', { id })
export const togglePlay = () => invoke<boolean>('toggle_play')
export const stopPlayback = () => invoke<void>('stop_playback')
export const setVolume = (vol: number) => invoke<void>('set_volume', { vol })
export const setEqBand = (index: number, gainDb: number) => invoke<void>('set_eq_band', { index, gainDb })
export const setEqEnabled = (enabled: boolean) => invoke<void>('set_eq_enabled', { enabled })
export const setKaraoke = (enabled: boolean) => invoke<void>('set_karaoke', { enabled })
export const getKaraoke = () => invoke<boolean>('get_karaoke')
export const setEqPreamp = (gainDb: number) => invoke<void>('set_eq_preamp', { gainDb })

export interface EqState {
  enabled: boolean
  preamp_db: number
}

export const getEqState = () => invoke<EqState>('get_eq_state')
export const pingAudio = () => invoke<void>('ping_audio')
export const isAudioReady = () => invoke<boolean>('is_audio_ready')

export const getPosition = () => invoke<number>('get_position')
export const getPlaylists = () => invoke<Playlist[]>('get_playlists')
export const getPlaylist = (id: string) => invoke<Playlist | null>('get_playlist', { id })
export const createPlaylist = (name: string, isSmart?: boolean, smartRules?: string) => invoke<Playlist>('create_playlist', { name, isSmart, smartRules })
export const deletePlaylist = (id: string) => invoke<void>('delete_playlist', { id })
export const updatePlaylistRules = (id: string, smartRules: string | null) => invoke<void>('update_playlist_rules', { id, smartRules })
export const refreshSmartPlaylist = (id: string) => invoke<number>('refresh_smart_playlist', { id })
export const addToPlaylist = (playlistId: string, trackId: string) => invoke<void>('add_to_playlist', { playlistId, trackId })
export const addTracksToPlaylist = (playlistId: string, trackIds: string[]) => invoke<void>('add_tracks_to_playlist', { playlistId, trackIds })
export const removeFromPlaylist = (playlistId: string, trackId: string) => invoke<void>('remove_from_playlist', { playlistId, trackId })
export const movePlaylistTrack = (playlistId: string, trackId: string, prevTrackId?: string | null, nextTrackId?: string | null) =>
  invoke<string>('move_playlist_track', { playlistId, trackId, prevTrackId: prevTrackId ?? null, nextTrackId: nextTrackId ?? null })
export const getPlaylistTracks = (playlistId: string) => invoke<Track[]>('get_playlist_tracks', { playlistId })
export const getFavoriteIds = () => invoke<string[]>('get_favorite_ids')
export const getLyrics = (filePath: string) => invoke<string | null>('get_lyrics', { filePath })
export const toggleFavorite = (id: string) => invoke<boolean>('toggle_favorite', { id })
export const getLibraryFolders = () => invoke<string[]>('get_library_folders')
export const addLibraryFolder = (path: string) => invoke<void>('add_library_folder', { path })
export const addLibraryFolders = (paths: string[]) => invoke<ScanResult>('add_library_folders', { paths })
export const removeLibraryFolder = (path: string) => invoke<void>('remove_library_folder', { path })

export interface BackupSummary {
  zip_path: string
  file_size_bytes: number
  playlist_count: number
  favorite_count: number
  history_count: number
  eq_profile_count: number
  lyric_count: number
}

export interface RestoreSummary {
  restored_playlists: number
  restored_favorites: number
  restored_history: number
  restored_eq_profiles: number
  restored_lyrics: number
  relinked_tracks: number
}

export const exportLibraryBackup = (destPath: string) => invoke<BackupSummary>('export_library_backup', { destPath })
export const restoreLibraryBackup = (srcPath: string) => invoke<RestoreSummary>('restore_library_backup', { srcPath })

export const analyzeAudio = () => invoke<string>('analyze_audio')
export const batchFetchArtistArtwork = (retryMissing = false) => invoke<string>('batch_fetch_artist_artwork', { retryMissing })
export const batchFetchGenre = (retryMissing = false) => invoke<string>('batch_fetch_genre', { retryMissing })
export const getGenreProgress = () =>
  invoke<{ tagged: number; total: number; missing: number; checked_missing: number; running: boolean }>('get_genre_progress')
export const buildGenreQueue = (trackId: string) => invoke<Track[]>('build_genre_queue', { trackId })
export const deleteTracks = (ids: string[], deleteFiles: boolean) => invoke<void>('delete_tracks', { ids, deleteFiles })
export const recordPlay = (id: string) => invoke<number>('record_play', { id })
export const finishPlay = (historyId: number, elapsedSecs: number, duration: number, endReason?: 'completed' | 'skipped' | 'stopped') =>
  invoke<void>('finish_play', { historyId, elapsedSecs, duration, endReason: endReason ?? null })

export interface ListeningInsights {
  plays: number
  seconds: number
  attempts: number
  completed: number
  skipped: number
  stopped: number
  completion_rate: number
  skip_rate: number
  avg_session_secs: number
  streak_days: number
  change_percent: number | null
  activity: { label: string; seconds: number }[]
  genres: { name: string; seconds: number; is_other: boolean }[]
  top_artists: { name: string; seconds: number; plays: number }[]
  top_tracks: { track: Track; plays: number; seconds: number }[]
}

export interface LibraryInsights {
  tracks: number
  albums: number
  artists: number
  playlists: number
  total_bytes: number
  growth: { label: string; seconds: number }[]
  quality: { kind: string; count: number }[]
}

export const getListeningInsights = (period: string) =>
  invoke<ListeningInsights>('get_listening_insights', { period })

export const getLibraryInsights = (period: string) =>
  invoke<LibraryInsights>('get_library_insights', { period })
export const clearPlayHistory = () => invoke<void>('clear_play_history')
export const quitApp = () => invoke<void>('quit_app')
export const toggleDevTools = () => invoke<void>('toggle_devtools')
export const getRecentlyPlayedTracks = (limit: number) => invoke<Track[]>('get_recently_played_tracks', { limit })
export const getMostListenedTracks = (limit: number) => invoke<Track[]>('get_most_listened_tracks', { limit })
export const getLeastListenedTracks = (limit: number) => invoke<Track[]>('get_least_listened_tracks', { limit })

export interface ListeningStats {
  total_plays: number
  total_listening_minutes: number
}
export const getListeningStats = () => invoke<ListeningStats>('get_listening_stats')
export const getAllGenres = () => invoke<Genre[]>('get_all_genres')
export const getTracksByGenreId = (genreId: string) => invoke<Track[]>('get_tracks_by_genre_id', { genreId })
export const showInExplorer = (id: string) => invoke<void>('show_in_explorer', { id })
export const findLyrics = (title: string, artist: string, album: string, save?: string) => invoke<LyricsResult[]>('find_lyrics', { title, artist, album, save })
export const updateTrackMetadata = (input: { id: string; title: string; artist: string; album: string; album_artist: string; genre: string; year: number; track_number: number; disc_number: number }) => invoke<void>('update_track_metadata', { input })
export const getPlaylistsForTrack = (trackId: string) => invoke<string[]>('get_playlists_for_track', { trackId })
export const renamePlaylist = (id: string, name: string) => invoke<void>('rename_playlist', { id, name })
export const exportPlaylistM3U = (id: string, outputPath: string) => invoke<void>('export_playlist_m3u', { id, outputPath })
export const importPlaylistM3U = (filePath: string, name: string) => invoke<Playlist>('import_playlist_m3u', { filePath, name })
export const getPlaylistTrackCount = (id: string) => invoke<number>('get_playlist_track_count', { id })
export const reorderPlaylistTracks = (playlistId: string, trackIds: string[]) => invoke<void>('reorder_playlist_tracks', { playlistId, trackIds })
export const createFolderPlaylist = (name: string, folderPath: string) => invoke<Playlist>('create_folder_playlist', { name, folderPath })
export const refreshFolderPlaylists = () => invoke<void>('refresh_folder_playlists')
export const getArtistArtwork = (artist: string, size?: string) => invoke<number[] | null>('get_artist_artwork', { artist, size })
export const getTrackArtwork = (id: string, size?: string) => invoke<number[] | null>('get_track_artwork', { id, size })
export const getFileArtwork = (filePath: string) => invoke<number[] | null>('get_file_artwork', { filePath })
export const setArtwork = (id: string, imageB64: string) => invoke<void>('set_artwork', { id, imageB64 })
export const remoteStart = (port: number, password: string) => invoke<void>('remote_start', { port, password })
export const remoteStop = () => invoke<void>('remote_stop')
export const remoteStatus = () => invoke<boolean>('remote_status')

export const preventSleep = () => invoke<void>('prevent_sleep')
export const allowSleep = () => invoke<void>('allow_sleep')
export const nowPlaying = (artist: string, track: string, album: string, duration: number) =>
  invoke<void>('now_playing', { artist, track, album, duration })
export const scrobble = (artist: string, track: string, album: string) =>
  invoke<void>('scrobble', { artist, track, album })

export interface LastfmConfig {
  username: string
  session_key: string
  enabled: boolean
  api_key: string
  api_secret: string
}
export const getLastfmAuthUrl = () => invoke<string>('get_lastfm_auth_url')
export const getLastfmConfig = () => invoke<LastfmConfig>('get_lastfm_config')
export const setLastfmSession = (token: string) => invoke<LastfmConfig>('set_lastfm_session', { token })
export const setLastfmEnabled = (enabled: boolean) => invoke<void>('set_lastfm_enabled', { enabled })
export const disconnectLastfm = () => invoke<void>('disconnect_lastfm')
export const setLastfmApiKey = (apiKey: string, apiSecret: string) => invoke<void>('set_lastfm_api_key', { apiKey, apiSecret })
export const validateLastfmApiKey = (apiKey: string) => invoke<{ valid: boolean; message: string }>('validate_lastfm_api_key', { apiKey })

export interface RadioStation {
  id: string
  name: string
  url: string
  genre: string
  logo_url: string
  country: string
  language: string
  bitrate: number
}

export const getRadioStations = () => invoke<RadioStation[]>('get_radio_stations')
export const addRadioStation = (station: RadioStation) => invoke<void>('add_radio_station', { station })
export const deleteRadioStation = (id: string) => invoke<void>('delete_radio_station', { id })
export const updateRadioStation = (station: RadioStation) => invoke<void>('update_radio_station', { station })
export const playRadio = (url: string) => invoke<void>('play_radio', { url })
export const stopRadio = () => invoke<void>('stop_radio')
export const radioStatus = () => invoke<boolean>('radio_status')
// --- EQ Profiles ---
export interface EQProfile {
  id: string
  name: string
  is_default: boolean
  is_active: boolean
  bands: number[]
}

export const getEqProfiles = () => invoke<EQProfile[]>('get_eq_profiles')
export const getActiveEqProfile = () => invoke<EQProfile | null>('get_active_eq_profile')
export const createEqProfile = (name: string) => invoke<EQProfile>('create_eq_profile', { name })
export const renameEqProfile = (id: string, name: string) => invoke<void>('rename_eq_profile', { id, name })
export const deleteEqProfile = (id: string) => invoke<void>('delete_eq_profile', { id })
export const applyEqProfile = (id: string) => invoke<void>('apply_eq_profile', { id })
export const updateEqBand = (profileId: string, index: number, gainDb: number) => invoke<void>('update_eq_band', { profileId, index, gainDb })

export interface ThemeColors {
  vibrant: string
  muted: string
  dominant: string
}

export const getTrackPalette = (id: string) => invoke<ThemeColors | null>('get_track_palette', { id })

export const processArtworkGpu = (trackId: string, targetSize?: number) =>
  invoke<number[] | null>('process_artwork_gpu', { trackId, targetSize })

export interface PlayerState {
  track_id: string | null
  is_playing: boolean
  position: number
  volume: number
  repeat_mode: string
  shuffle: boolean
  queue_index: number
  queue_len: number
  gapless: boolean
  crossfade: number
}

export const getPlayerState = () => invoke<PlayerState>('get_player_state')

export interface TrackRef {
  id: string
  title: string
  artist: string
  duration: number
  file_path: string
  cue_parent_id: string | null
  cue_offset: number
  bpm: number
  energy: number
}

export interface QueueState {
  queue: TrackRef[]
  queue_index: number
  repeat_mode: string
  shuffle: boolean
}

export const setQueue = (tracks: Track[], startIndex: number) => invoke<void>('set_queue', { tracks, startIndex })
export const nextTrack = () => invoke<TrackRef | null>('next_track')
export const prevTrack = () => invoke<TrackRef | null>('prev_track')
export const setRepeatMode = (mode: string) => invoke<void>('set_repeat_mode', { mode })
export const setShuffle_ = (enabled: boolean) => invoke<void>('set_shuffle', { enabled })
export const getQueueState = () => invoke<QueueState>('get_queue_state')
export const removeQueueTrack = (index: number) => invoke<void>('remove_queue_track', { index })
export const insertQueueTracks = (tracks: Track[]) => invoke<void>('insert_queue_tracks', { tracks })
export const reorderQueue = (tracks: Track[]) => invoke<void>('reorder_queue', { tracks })
export const appendQueueTracks = (tracks: Track[]) => invoke<void>('append_queue_tracks', { tracks })
// --- Gapless / Crossfade (Phase 4) ---
export const setGapless = (enabled: boolean) => invoke<void>('set_gapless', { enabled })
export const setCrossfade = (seconds: number) => invoke<void>('set_crossfade', { seconds })
export const prepareNextTrack = (trackId: string) => invoke<void>('prepare_next_track', { trackId })
export const advanceQueue = () => invoke<TrackRef | null>('advance_queue')

// --- Bit-Perfect Output ---
// format: 0=none, 1=S32, 2=S24, 3=S16 (exclusive integer formats)
export interface OutputState {
  bit_perfect_requested: boolean
  bit_perfect_active: boolean
  sample_rate: number
  format: number
  note: string | null
}

export const setBitPerfect = (enabled: boolean) => invoke<void>('set_bit_perfect', { enabled })
export const getOutputState = () => invoke<OutputState>('get_output_state')

// --- Session / Crash Recovery ---
export interface SessionData {
  track_id: string | null
  position: number
  volume: number
  repeat_mode: string
  shuffle: boolean
  queue_index: number
  gapless: boolean
  crossfade: number
}

export interface RestoreSessionResult {
  session: SessionData | null
  was_crash: boolean
  crash_log: string | null
}

export const saveSession = (
  trackId: string | null,
  position: number,
  volume: number,
  repeatMode: string,
  shuffle: boolean,
  queueIndex: number,
  gapless: boolean,
  crossfade: number,
) => invoke<void>('save_session', {
  trackId, position, volume, repeatMode, shuffle, queueIndex, gapless, crossfade,
})

export const restoreSession = () => invoke<RestoreSessionResult>('restore_session')
export const clearSession = () => invoke<void>('clear_session')
export const dismissCrashReport = () => invoke<void>('dismiss_crash_report')
export const consumePendingExternalFile = () => invoke<Record<string, unknown> | null>('consume_pending_external_file')
export const playExternalFile = (filePath: string, durationSecs?: number) =>
  invoke<Record<string, unknown>>('play_external_file', { filePath, durationSecs: durationSecs ?? null })
export const isExternalPlayback = () => invoke<boolean>('is_external_playback')

// --- Smart Mix ---
export const generateSmartMix = (seedTrackId: string, count: number) =>
  invoke<Track[]>('generate_smart_mix', { seedTrackId, count })

// --- File Associations / Default Player ---

// --- Loudness Normalization (FFmpeg ebur128) ---
export interface LoudnessInfo {
  track_id: string
  loudness_lufs: number
  loudness_range: number
  true_peak: number
  analyzed_version: number
}

export interface NormalizationSettings {
  mode: string
  target_lufs: number
  prevent_clip: boolean
}

export interface LoudnessProgress {
  analyzed: number
  total: number
  ffmpeg_available: boolean
}

export const getTrackLoudness = (trackId: string) =>
  invoke<LoudnessInfo | null>('get_track_loudness', { trackId })

export const calculateTrackLufs = (trackId: string) =>
  invoke<LoudnessInfo>('calculate_track_lufs', { trackId })

export const runLoudnessBackfill = (limit?: number) =>
  invoke<{ started: boolean }>('run_loudness_backfill', { limit: limit ?? null })

export const getNormalizationSettings = () =>
  invoke<NormalizationSettings>('get_normalization_settings')

export const setNormalizationSettings = (mode: string, targetLufs: number, preventClip: boolean) =>
  invoke<void>('set_normalization_settings', { mode, targetLufs, preventClip })

export const getLoudnessProgress = () =>
  invoke<LoudnessProgress>('get_loudness_progress')

export const runFullAnalysisBackfill = (limit?: number) =>
  invoke<{ started: boolean }>('run_full_analysis_backfill', { limit: limit ?? null })

export const getAnalysisProgress = () =>
  invoke<{ analyzed: number; total: number }>('get_analysis_progress')

export const recomputeMoods = () =>
  invoke<{ features: number; tracks: number }>('recompute_moods')

export const getTrackAnalysis = (trackId: string) =>
  invoke<Record<string, unknown> | null>('get_track_analysis', { trackId })

// --- Appearance (theme + custom accent) ---
export const getSettingValue = (key: string) =>
  invoke<string | null>('get_setting_value', { key })

export const setSettingValue = (key: string, value: string) =>
  invoke<void>('set_setting_value', { key, value })

// --- Signal Path & Audiophile DSP ---
export interface SignalPathInfo {
  title: string
  artist: string
  file_path: string
  file_format: string
  sample_rate: number
  bit_depth: number
  channels: number
  bitrate: number
  is_bit_perfect: boolean
  replaygain_db: number
  lufs_gain_db: number
  crossfeed_enabled: boolean
  crossfeed_level: string
  eq_enabled: boolean
  eq_profile_name: string
  output_sample_rate: number
  output_format: string
  output_device: string
}

export const getSignalPath = () => invoke<SignalPathInfo>('get_signal_path')

export interface CrossfeedPayload {
  enabled: boolean
  level: string
}

export const getCrossfeedSettings = () => invoke<CrossfeedPayload>('get_crossfeed_settings')
export const setCrossfeedSettings = (settings: CrossfeedPayload) => invoke<void>('set_crossfeed_settings', { settings })

export const importAutoEqProfile = (name: string, text: string) =>
  invoke<EQProfile>('import_autoeq_profile', { name, text })

// --- Library Health & Forensics ---
export interface HealthSummary {
  total_analyzed: number
  good_count: number
  suspect_count: number
  lossy_count: number
  upsampled_count: number
  fake_24_count: number
  duplicate_groups_count: number
  damaged_count: number
}

export interface DamagedTrackItem {
  id: string
  title: string
  artist: string
  album: string
  file_path: string
  file_format: string
  error: string
}

export interface SuspectTrackItem {
  id: string
  title: string
  artist: string
  file_path: string
  file_format: string
  sample_rate: number
  bit_depth: number
  cutoff_hz: number | null
  verdict: string
  real_bits?: number | null
}

export interface DuplicateGroup {
  title: string
  artist: string
  tracks: SuspectTrackItem[]
}

export interface QualityResult {
  cutoff_hz: number | null
  verdict: 'good' | 'suspect' | 'lossy' | 'upsampled'
  real_bits: number | null
}

export const getLibraryHealthSummary = () => invoke<HealthSummary>('get_library_health_summary')
export const getSuspectTracks = (limit?: number) => invoke<SuspectTrackItem[]>('get_suspect_tracks', { limit: limit ?? null })
export const getDuplicateTracks = () => invoke<DuplicateGroup[]>('get_duplicate_tracks')
export const getDamagedTracks = () => invoke<DamagedTrackItem[]>('get_damaged_tracks')
export const verifyTrackIntegrity = (trackId: string) => invoke<DamagedTrackItem | null>('verify_track_integrity', { trackId })
export const deleteTrackToTrash = (trackId: string) => invoke<void>('delete_track_to_trash', { trackId })
export const analyzeTrackQuality = (trackId: string) => invoke<QualityResult>('analyze_track_quality', { trackId })

export const toggleMiniWindow = () => invoke<void>('toggle_mini_window')
export const showMainWindow = () => invoke<void>('show_main_window')
export const removeWindowBorder = (label: string) => invoke<void>('remove_window_border', { label })

export interface SplitAlbumVariant {
  album_artist: string
  track_count: number
  track_ids: string[]
}

export interface SplitAlbumItem {
  album_name: string
  total_tracks: number
  variants: SplitAlbumVariant[]
}

export interface IncompleteAlbumItem {
  album: string
  artist: string
  existing_tracks: number[]
  missing_tracks: number[]
  total_expected: number
}

export interface DualScriptArtistPair {
  name_a: string
  name_b: string
  track_count_a: number
  track_count_b: number
}

export const getSplitAlbums = () => invoke<SplitAlbumItem[]>('get_split_albums')
export const mergeSplitAlbum = (trackIds: string[], targetAlbumArtist: string) =>
  invoke<number>('merge_split_album', { trackIds, targetAlbumArtist })
export const getIncompleteAlbums = () => invoke<IncompleteAlbumItem[]>('get_incomplete_albums')
export const getDualScriptArtists = () => invoke<DualScriptArtistPair[]>('get_dual_script_artists')
export const mergeArtistNames = (fromArtist: string, toArtist: string) =>
  invoke<number>('merge_artist_names', { fromArtist, toArtist })

// --- Audio Output Devices & Per-Device Sound Settings ---
export interface AudioDevicesResponse {
  devices: string[]
  current: string
  is_default: boolean
}

export interface DeviceSoundSettings {
  device_name: string
  eq_enabled: boolean
  eq_mode: 'graphic' | 'parametric'
  eq_profile_id?: string
  eq_bands?: number[]
  eq_preamp?: number
  eq_parametric_name?: string
  eq_parametric_filters?: ParametricFilterItem[]
  bit_perfect?: boolean
  crossfeed_enabled?: boolean
  crossfeed_level?: string
  volume?: number
}

export interface ParametricFilterItem {
  kind: 'peak' | 'lowshelf' | 'highshelf' | 'lowpass' | 'highpass'
  freq: number
  gain: number
  q: number
  enabled: boolean
}

export interface ParametricEqState {
  filters: ParametricFilterItem[]
  preamp_db: number
  enabled: boolean
  mode: string
  profile_name: string
}

export interface AutoEqParseResult {
  name: string
  preamp: number
  filters: ParametricFilterItem[]
}

export const getAudioDevices = () => invoke<AudioDevicesResponse>('get_audio_devices')
export const setAudioDevice = (device?: string | null) => invoke<void>('set_audio_device', { device: device ?? null })
export const getDeviceSoundSettings = (device: string) => invoke<DeviceSoundSettings | null>('get_device_sound_settings', { device })
export const saveDeviceSoundSettings = (device: string, settings: DeviceSoundSettings) =>
  invoke<void>('save_device_sound_settings', { device, settings })
export const setEqMode = (mode: 'graphic' | 'parametric') => invoke<void>('set_eq_mode', { mode })
export const applyParametricEq = (
  filters: ParametricFilterItem[],
  preampDb: number,
  enabled: boolean,
  profileName?: string
) => invoke<void>('apply_parametric_eq', { filters, preampDb, enabled, profileName: profileName ?? null })
export const getParametricEqState = () => invoke<ParametricEqState>('get_parametric_eq_state')
export const parseAutoEqText = (text: string) => invoke<AutoEqParseResult>('parse_autoeq_text', { text })



