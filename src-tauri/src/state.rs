#[cfg(windows)]
use crate::gpu::GpuArtworkEngine;
use crate::audio::player::Player;
use crate::audio::lufs::LufsCache;
use crate::library::artist_artwork::ArtistArtworkCache;
use crate::library::artist_artwork_worker::ArtistArtworkWorker;
use crate::library::track_artwork::TrackArtworkCache;
use crate::queue::QueueState;
use crate::remote::RemoteControl;
use crate::search::tantivy_index::TantivyIndex;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use serde_json;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub player: Arc<Player>,
    pub db: SqlitePool,
    pub search: Arc<Mutex<TantivyIndex>>,
    pub queue_state: Arc<Mutex<QueueState>>,
    pub volume: Arc<AtomicU32>,
    pub media: Arc<Mutex<Option<std::sync::mpsc::Sender<crate::media::Update>>>>,
    pub remote: RemoteControl,
    pub artwork_cache: Arc<Mutex<HashMap<String, String>>>,
    pub artist_artwork_cache: Arc<ArtistArtworkCache>,
    pub artist_artwork_worker: Arc<ArtistArtworkWorker>,
    pub track_artwork_cache: Arc<TrackArtworkCache>,
    pub palette_cache: Arc<Mutex<HashMap<String, crate::library::palette::ThemeColors>>>,
    pub is_scanning: Arc<AtomicBool>,
    pub is_analyzing: Arc<AtomicBool>,
    pub is_loudness_running: Arc<AtomicBool>,
    pub is_fetching: Arc<AtomicBool>,
    pub is_fetching_genre: Arc<AtomicBool>,
    pub current_track_id: Arc<Mutex<Option<String>>>,
    pub cache_root: Arc<Mutex<PathBuf>>,
    pub lufs_cache: Option<Arc<Mutex<LufsCache>>>,
    #[cfg(windows)]
    pub gpu_engine: Option<Arc<GpuArtworkEngine>>,
    pub tray_menu: Arc<std::sync::Mutex<Option<crate::tray::TrayMenu>>>,
    pub was_crash: std::sync::atomic::AtomicBool,
    pub pending_external_file: Arc<Mutex<Option<serde_json::Value>>>,
    pub file_player_counter: AtomicU32,
}
