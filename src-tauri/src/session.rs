use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionData {
    pub track_id: Option<String>,
    pub position: f64,
    pub volume: f32,
    pub repeat_mode: String,
    pub shuffle: bool,
    pub queue_index: usize,
    pub gapless: bool,
    pub crossfade: f64,
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            track_id: None,
            position: 0.0,
            volume: 0.8,
            repeat_mode: "off".to_string(),
            shuffle: false,
            queue_index: 0,
            gapless: true,
            crossfade: 2.0,
        }
    }
}

pub fn save_session(app_handle: &AppHandle, data: &SessionData) {
    let store = match app_handle.store("session.json") {
        Ok(s) => s,
        Err(_) => return,
    };
    store.set("track_id", json!(data.track_id));
    store.set("position", json!(data.position));
    store.set("volume", json!(data.volume));
    store.set("repeat_mode", json!(data.repeat_mode));
    store.set("shuffle", json!(data.shuffle));
    store.set("queue_index", json!(data.queue_index));
    store.set("gapless", json!(data.gapless));
    store.set("crossfade", json!(data.crossfade));
}

pub fn load_session(app_handle: &AppHandle) -> Option<SessionData> {
    let store = app_handle.store("session.json").ok()?;
    store.get("track_id")?;
    Some(SessionData {
        track_id: store.get("track_id")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        position: store.get("position").and_then(|v| v.as_f64()).unwrap_or(0.0),
        volume: store.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.8) as f32,
        repeat_mode: store.get("repeat_mode")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "off".to_string()),
        shuffle: store.get("shuffle").and_then(|v| v.as_bool()).unwrap_or(false),
        queue_index: store.get("queue_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        gapless: store.get("gapless").and_then(|v| v.as_bool()).unwrap_or(true),
        crossfade: store.get("crossfade").and_then(|v| v.as_f64()).unwrap_or(2.0),
    })
}

pub fn clear_session(app_handle: &AppHandle) {
    if let Ok(store) = app_handle.store("session.json") {
        store.delete("track_id");
        store.delete("position");
        store.delete("volume");
        store.delete("repeat_mode");
        store.delete("shuffle");
        store.delete("queue_index");
        store.delete("gapless");
        store.delete("crossfade");
    }
}

pub fn save_clean_exit(app_handle: &AppHandle) {
    if let Ok(store) = app_handle.store("session.json") {
        store.set("clean_exit", json!(true));
    }
}

pub fn was_crash(app_handle: &AppHandle) -> bool {
    let store = match app_handle.store("session.json") {
        Ok(s) => s,
        Err(_) => return false,
    };
    let has_session = store.get("track_id").is_some();
    let clean = store.get("clean_exit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    has_session && !clean
}

pub fn mark_session_started(app_handle: &AppHandle) {
    if let Ok(store) = app_handle.store("session.json") {
        store.set("clean_exit", json!(false));
    }
}

pub fn get_last_crash_log(data_dir: &std::path::Path) -> Option<String> {
    let crash_dir = data_dir.join("crashes");
    if !crash_dir.exists() {
        return None;
    }
    let mut entries: Vec<_> = std::fs::read_dir(&crash_dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
        .collect();
    entries.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    entries.last().and_then(|e| std::fs::read_to_string(e.path()).ok())
}
