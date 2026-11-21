import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { currentMonitor, primaryMonitor, PhysicalPosition, LogicalSize } from '@tauri-apps/api/window'
import type { WindowOptions } from '@tauri-apps/api/window'

const LABEL = 'mini-player'
const MINI_WIDTH = 420
const MINI_HEIGHT = 140

async function getWin() {
  return await WebviewWindow.getByLabel(LABEL)
}

export function getMiniWindow() {
  return getWin()
}

export async function openMiniWindow(): Promise<boolean> {
  const existing = await getWin()
  if (existing) {
    try {
      await existing.show()
      await existing.setFocus()
      return true
    } catch {}
  }

  const baseUrl = window.location.origin + window.location.pathname.replace(/\/$/, '')
  const savedOnTop = localStorage.getItem('resona-mini-always-on-top') !== 'false'

  let x = 100
  let y = 100
  try {
    const monitor = (await currentMonitor()) || (await primaryMonitor())
    if (monitor) {
      const scale = monitor.scaleFactor || 1
      const monX = monitor.position.x
      const monY = monitor.position.y
      const monW = monitor.size.width
      const monH = monitor.size.height
      // Bottom-right placement with 20px margin right and 60px bottom (above taskbar)
      x = Math.round(monX + monW - (MINI_WIDTH + 20) * scale)
      y = Math.round(monY + monH - (MINI_HEIGHT + 60) * scale)
    }
  } catch (e) {
    console.warn('[miniWindow] failed to calculate monitor work area:', e)
  }

  const winOptions: WindowOptions = {
    title: 'Eiryn Mini Player',
    width: MINI_WIDTH,
    height: MINI_HEIGHT,
    minWidth: MINI_WIDTH,
    minHeight: MINI_HEIGHT,
    maxWidth: MINI_WIDTH,
    maxHeight: MINI_HEIGHT,
    resizable: false,
    decorations: false,
    alwaysOnTop: savedOnTop,
    skipTaskbar: true,
    transparent: true,
    shadow: false,
    visible: false,
  }

  try {
    const mainWin = getCurrentWebviewWindow()

    const miniWin = new WebviewWindow(LABEL, {
      url: baseUrl + '/#/mini-player',
      x,
      y,
      ...winOptions,
    })

    await new Promise<void>((resolve, reject) => {
      miniWin.once('tauri://created', () => resolve())
      miniWin.once('tauri://error', (e) => {
        reject(new Error(typeof e === 'string' ? e : (e as any)?.payload ?? 'window creation failed'))
      })
    })

    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('remove_window_border', { label: LABEL })
    } catch {}

    try {
      await miniWin.setSize(new LogicalSize(MINI_WIDTH, MINI_HEIGHT))
      await miniWin.setPosition(new PhysicalPosition(x, y))
    } catch {}

    await mainWin.hide()
    await miniWin.show()
    await miniWin.setFocus()
    return true
  } catch (e) {
    console.error('[miniWindow] failed to create', e)
    return false
  }
}

export async function closeMiniWindow() {
  const win = await getWin()
  if (win) {
    try {
      await win.close()
    } catch {}
  }
  try {
    const mainWin = await WebviewWindow.getByLabel('main')
    if (mainWin) {
      await mainWin.show()
      await mainWin.setFocus()
    }
  } catch {}
}

export async function toggleMiniWindow() {
  const exists = !!(await getWin())
  if (exists) {
    await closeMiniWindow()
  } else {
    await openMiniWindow()
  }
}
