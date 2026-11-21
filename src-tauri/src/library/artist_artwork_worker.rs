use crate::library::artist_artwork::ArtistArtworkCache;
use crate::library::db;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::Emitter;

pub struct ArtistArtworkWorker {
    queue: Arc<Mutex<Vec<String>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
    enabled: Arc<Mutex<bool>>,
}

impl ArtistArtworkWorker {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            enabled: Arc::new(Mutex::new(true)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        *self.enabled.blocking_lock() = enabled;
    }

    pub async fn enqueue(&self, artist: String) {
        let mut in_flight = self.in_flight.lock().await;
        if in_flight.contains(&artist) {
            return;
        }
        in_flight.insert(artist.clone());
        drop(in_flight);
        let mut queue = self.queue.lock().await;
        queue.push(artist);
    }

    pub async fn in_flight_count(&self) -> usize {
        self.in_flight.lock().await.len()
    }

    pub fn start(
        self: Arc<Self>,
        db: SqlitePool,
        cache: Arc<ArtistArtworkCache>,
        app_handle: tauri::AppHandle,
    ) {
        tauri::async_runtime::spawn(async move {
            Self::reset_stale_not_found_marks(&db).await;
            loop {
                let artist = {
                    let mut queue = self.queue.lock().await;
                    if queue.is_empty() {
                        None
                    } else {
                        Some(queue.remove(0))
                    }
                };
                if let Some(artist_name) = artist {
                    let enabled = *self.enabled.lock().await;
                    if enabled {
                        Self::process(&db, &cache, &app_handle, &artist_name).await;
                    }
                    let mut in_flight = self.in_flight.lock().await;
                    in_flight.remove(&artist_name);
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });
    }

    /// One-time migration: all `artwork_checked` marks created by the old Google
    /// scraper were false negatives. Clear them once (guarded by a settings flag)
    /// so the Deezer fetch re-tries every artist.
    async fn reset_stale_not_found_marks(db: &SqlitePool) {
        if db::get_setting(db, "artist_artwork_v2_reset")
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        let _ = db::reset_artist_artwork_not_found(db).await;
        let _ = db::set_setting(db, "artist_artwork_v2_reset", "1").await;
    }

    async fn try_fetch_from_deezer(
        client: &reqwest::Client,
        artist_name: &str,
    ) -> Option<Vec<u8>> {
        let url = format!(
            "https://api.deezer.com/search/artist?q={}",
            urlencoding(artist_name)
        );
        let json: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
        let picture = json["data"][0]["picture_xl"].as_str()?;
        // d41d8cd9... is Deezer's default/placeholder artist image — treat as no result.
        if picture.contains("d41d8cd98f00b204e9800998ecf8427e") {
            return None;
        }
        let bytes = client
            .get(picture)
            .send()
            .await
            .ok()?
            .bytes()
            .await
            .ok()?
            .to_vec();
        if bytes.len() > 1024 && image::load_from_memory(&bytes).is_ok() {
            Some(bytes)
        } else {
            None
        }
    }

    async fn process(
        db: &SqlitePool,
        cache: &ArtistArtworkCache,
        app_handle: &tauri::AppHandle,
        artist_name: &str,
    ) {
        let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
            "artist": artist_name,
            "status": "searching",
        }));

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .ok()
        {
            Some(c) => c,
            None => {
                let _ = db::mark_artist_artwork_checked(db, artist_name).await;
                let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
                    "artist": artist_name,
                    "status": "not_found",
                }));
                return;
            }
        };

        let img_bytes = Self::try_fetch_from_deezer(&client, artist_name).await;
        let img_bytes = match img_bytes {
            Some(b) => b,
            None => {
                let _ = db::mark_artist_artwork_checked(db, artist_name).await;
                let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
                    "artist": artist_name,
                    "status": "not_found",
                }));
                return;
            }
        };

        let key = match cache.save(&img_bytes) {
            Ok(k) => k,
            Err(_) => {
                let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
                    "artist": artist_name,
                    "status": "not_found",
                }));
                return;
            }
        };

        let _ = db::set_artist_artwork_key(db, artist_name, &key).await;

        let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
            "artist": artist_name,
            "status": "found",
        }));
        let _ = app_handle.emit("artist-artwork-updated", serde_json::json!({
            "artist": artist_name,
            "key": key,
        }));
    }
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
