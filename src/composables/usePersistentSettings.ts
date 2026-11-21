export async function getSetting<T>(key: string, fallback: T): Promise<T> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json', { autoSave: true })
    const val = await store.get<T>(key)
    if (val !== null && val !== undefined) return val
  } catch {}
  try {
    const raw = localStorage.getItem(key)
    if (raw !== null) return JSON.parse(raw) as T
  } catch {}
  return fallback
}

export async function setSetting<T>(key: string, value: T): Promise<void> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json', { autoSave: true })
    await store.set(key, value)
  } catch {}
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {}
}
