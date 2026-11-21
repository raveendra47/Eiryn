import { defineStore } from 'pinia'
import { ref } from 'vue'
import { hexToRgbChannels, normalizeHexColor, primaryForeground } from '../lib/color'

export const useAppStore = defineStore('app', () => {
  const theme = ref<'light' | 'black'>('light')
  const startAtLogin = ref(false)
  const showTrayIcon = ref(true)
  const lastfmUsername = ref('')
  const lastfmSessionKey = ref('')
  const lastfmEnabled = ref(false)
  const lastfmConnected = ref(false)
  const lastfmApiKey = ref('')
  const lastfmApiSecret = ref('')
  const replaygainMode = ref<'off' | 'track' | 'album'>(
    (localStorage.getItem('resona-replaygain') as 'off' | 'track' | 'album') || 'track'
  )
  const eqEnabled = ref(true)
  const enableLrclib = ref(true)
  const enableKugou = ref(true)
  const preferLocalLyrics = ref(true)
  const autoFetchLyrics = ref(false)
  const autoAnalyzeAudio = ref(false)
  const autoFetchArtistArtwork = ref(false)
  const autoFetchGenre = ref(false)
  const useOnlineArtistArtwork = ref(true)
  const preventSleepWhilePlaying = ref(false)
  const showPlayerIndicator = ref(true)
  const remoteServerEnabled = ref(false)
  const remoteServerPort = ref(8888)
  const remoteServerPassword = ref('0000')
  const sidebarWidth = ref(parseInt(localStorage.getItem('resona-sidebar-width') || '260', 10))

  const activeSettingsTab = ref<'general' | 'library' | 'integrations' | 'playback' | 'remote' | 'about' | 'keyboard'>('general')

  type Theme = 'light' | 'black'

  function applyTheme(newTheme: Theme) {
    const root = document.documentElement
    root.classList.remove('light', 'dark', 'black')
    if (newTheme === 'black') {
      // Black = .dark + .black, .black only overrides bg vars.
      root.classList.add('dark', 'black')
    } else {
      root.classList.add('light')
    }
    theme.value = newTheme as any
    localStorage.setItem('resona-theme', newTheme)
    // Re-apply a stored custom accent on top of the theme defaults.
    if (primaryColor.value) applyPrimaryColor(primaryColor.value)
  }

  function loadSavedTheme() {
    const saved = localStorage.getItem('resona-theme')
    if (saved === 'light' || saved === 'black') {
      applyTheme(saved)
    } else {
      // Retired themes (system/dark/palettes) fall back to Light.
      applyTheme('light')
    }
  }

  function updateSidebarWidth(width: number) {
    sidebarWidth.value = Math.max(180, Math.min(400, width))
    localStorage.setItem('resona-sidebar-width', String(sidebarWidth.value))
  }

  function setActiveSettingsTab(tab: typeof activeSettingsTab.value) {
    activeSettingsTab.value = tab
  }

  function updateTheme(newTheme: typeof theme.value) {
    applyTheme(newTheme)
    persistSetting('theme', newTheme).catch((e) => console.error('Theme save failed', e))
  }

  /** Custom accent override (null = active palette's default). */
  const primaryColor = ref<string | null>(null)

  function applyPrimaryColor(color: string | null) {
    const root = document.documentElement
    if (!color) {
      root.style.removeProperty('--primary')
      root.style.removeProperty('--primary-rgb')
      root.style.removeProperty('--primary-foreground')
      primaryColor.value = null
      return
    }
    const normalized = normalizeHexColor(color)
    if (!normalized) return
    root.style.setProperty('--primary', normalized)
    root.style.setProperty('--primary-rgb', hexToRgbChannels(normalized))
    root.style.setProperty('--primary-foreground', primaryForeground(normalized))
    primaryColor.value = normalized
  }

  async function persistSetting(key: string, value: string) {
    try {
      const { setSettingValue } = await import('../lib/invoke')
      await setSettingValue(key, value)
    } catch (e) {
      throw e
    }
  }

  async function updatePrimaryColor(color: string | null) {
    if (color !== null && !normalizeHexColor(color)) return
    applyPrimaryColor(color)
    localStorage.setItem('resona-accent', color ?? '')
    try {
      await persistSetting('primary_color', color ?? '')
    } catch (e) {
      console.error('Accent save failed', e)
    }
  }

  /** Backend wins over localStorage once set (Airmedy parity). */
  async function loadAppearance() {
    loadSavedTheme()
    const localAccent = localStorage.getItem('resona-accent')
    applyPrimaryColor(normalizeHexColor(localAccent ?? ''))
    try {
      const { getSettingValue } = await import('../lib/invoke')
      const [savedTheme, savedAccent] = await Promise.all([
        getSettingValue('theme'),
        getSettingValue('primary_color'),
      ])
      if (savedTheme === 'light' || savedTheme === 'black') applyTheme(savedTheme)
      if (savedAccent !== null && savedAccent !== undefined) {
        applyPrimaryColor(normalizeHexColor(savedAccent))
      }
    } catch {}
  }
  async function updateStartAtLogin(val: boolean) {
    startAtLogin.value = val
    try {
      if (val) {
        const { enable } = await import('@tauri-apps/plugin-autostart')
        await enable()
      } else {
        const { disable } = await import('@tauri-apps/plugin-autostart')
        await disable()
      }
    } catch (e) {
      console.error('Autostart toggle failed', e)
    }
  }
  async function updateShowTrayIcon(val: boolean) {
    showTrayIcon.value = val
    try {
      await persistSetting('show_tray_icon', val ? 'true' : 'false')
    } catch (e) {
      console.error('Failed to save show_tray_icon', e)
    }
  }
  async function loadLastfmConfig() {
    try {
      const { getLastfmConfig } = await import('../lib/invoke')
      const cfg = await getLastfmConfig()
      lastfmUsername.value = cfg.username
      lastfmSessionKey.value = cfg.session_key
      lastfmEnabled.value = cfg.enabled
      lastfmConnected.value = !!cfg.session_key
      lastfmApiKey.value = cfg.api_key
      lastfmApiSecret.value = cfg.api_secret
    } catch {}
  }
  async function setLastfmSession(token: string) {
    try {
      const { setLastfmSession } = await import('../lib/invoke')
      const cfg = await setLastfmSession(token)
      lastfmUsername.value = cfg.username
      lastfmSessionKey.value = cfg.session_key
      lastfmEnabled.value = true
      lastfmConnected.value = true
    } catch (e) {
      console.error('Last.fm auth failed', e)
      throw e
    }
  }
  async function updateLastfmEnabled(val: boolean) {
    lastfmEnabled.value = val
    try {
      const { setLastfmEnabled } = await import('../lib/invoke')
      await setLastfmEnabled(val)
    } catch (e) {
      console.error('Last.fm toggle failed', e)
    }
  }
  async function disconnectLastfm() {
    try {
      const { disconnectLastfm } = await import('../lib/invoke')
      await disconnectLastfm()
      lastfmUsername.value = ''
      lastfmSessionKey.value = ''
      lastfmEnabled.value = false
      lastfmConnected.value = false
    } catch (e) {
      console.error('Last.fm disconnect failed', e)
    }
  }

  async function updateLastfmApiKey(apiKey: string, apiSecret: string) {
    lastfmApiKey.value = apiKey
    lastfmApiSecret.value = apiSecret
    try {
      const { setLastfmApiKey } = await import('../lib/invoke')
      await setLastfmApiKey(apiKey, apiSecret)
    } catch (e) {
      console.error('Failed to save Last.fm API key', e)
    }
  }

  async function loadSavedSettings() {
    try {
      const { getSettingValue } = await import('../lib/invoke')
      const trayVal = await getSettingValue('show_tray_icon')
      if (trayVal !== null && trayVal !== undefined) {
        showTrayIcon.value = trayVal === 'true'
      }
    } catch {}
  }
  function updateReplaygainMode(val: 'off' | 'track' | 'album') {
    replaygainMode.value = val
    localStorage.setItem('resona-replaygain', val)
  }
  function updateEqEnabled(val: boolean) { eqEnabled.value = val }
  function updateEnableLrclib(val: boolean) { enableLrclib.value = val }
  function updateEnableKugou(val: boolean) { enableKugou.value = val }
  function updatePreferLocalLyrics(val: boolean) { preferLocalLyrics.value = val }
  function updateAutoFetchLyrics(val: boolean) { autoFetchLyrics.value = val }
  function updateAutoAnalyzeAudio(val: boolean) { autoAnalyzeAudio.value = val }
  function updateAutoFetchArtistArtwork(val: boolean) { autoFetchArtistArtwork.value = val }
  function updateAutoFetchGenre(val: boolean) { autoFetchGenre.value = val }
  function updateUseOnlineArtistArtwork(val: boolean) { useOnlineArtistArtwork.value = val }
  async function updatePreventSleepWhilePlaying(val: boolean) {
    preventSleepWhilePlaying.value = val
    try {
      if (val) {
        const { preventSleep } = await import('../lib/invoke')
        await preventSleep()
      } else {
        const { allowSleep } = await import('../lib/invoke')
        await allowSleep()
      }
    } catch (e) {
      console.error('Sleep toggle failed', e)
    }
  }
  function updateShowPlayerIndicator(val: boolean) { showPlayerIndicator.value = val }
  async function updateRemoteServerEnabled(val: boolean) {
    remoteServerEnabled.value = val
    try {
      if (val) {
        const { remoteStart } = await import('../lib/invoke')
        await remoteStart(remoteServerPort.value, remoteServerPassword.value)
      } else {
        const { remoteStop } = await import('../lib/invoke')
        await remoteStop()
      }
    } catch (e) {
      console.error('Remote server toggle failed', e)
    }
  }
  const toneLoudness = ref<boolean>(localStorage.getItem('eiryn-tone-loudness') !== 'false')
  const toneSmart = ref<boolean>(localStorage.getItem('eiryn-tone-smart') !== 'false')

  function updateToneLoudness(val: boolean) {
    toneLoudness.value = val
    localStorage.setItem('eiryn-tone-loudness', String(val))
  }

  function updateToneSmart(val: boolean) {
    toneSmart.value = val
    localStorage.setItem('eiryn-tone-smart', String(val))
  }

  function updateRemoteServerPort(val: number) { remoteServerPort.value = val }
  function updateRemoteServerPassword(val: string) { remoteServerPassword.value = val }

  return {
    theme, startAtLogin, showTrayIcon,
    lastfmUsername, lastfmSessionKey, lastfmEnabled, lastfmConnected, lastfmApiKey, lastfmApiSecret,
    replaygainMode, eqEnabled, toneLoudness, toneSmart, enableLrclib, enableKugou, preferLocalLyrics, autoFetchLyrics,
    autoAnalyzeAudio, autoFetchArtistArtwork, autoFetchGenre, useOnlineArtistArtwork, preventSleepWhilePlaying, showPlayerIndicator,
    remoteServerEnabled, remoteServerPort, remoteServerPassword,
    sidebarWidth, activeSettingsTab,
    primaryColor,
    applyTheme, loadSavedTheme, loadSavedSettings, loadAppearance, updateSidebarWidth, setActiveSettingsTab,
    updateTheme, updatePrimaryColor, updateStartAtLogin, updateShowTrayIcon,
    loadLastfmConfig, setLastfmSession, updateLastfmEnabled, disconnectLastfm, updateLastfmApiKey,
    updateReplaygainMode, updateEqEnabled, updateToneLoudness, updateToneSmart, updateEnableLrclib, updateEnableKugou,
    updatePreferLocalLyrics, updateAutoFetchLyrics, updateAutoAnalyzeAudio, updateAutoFetchArtistArtwork, updateAutoFetchGenre, updateUseOnlineArtistArtwork,
    updatePreventSleepWhilePlaying, updateShowPlayerIndicator,
    updateRemoteServerEnabled, updateRemoteServerPort, updateRemoteServerPassword,
  }
})
