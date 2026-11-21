//! Single-file ZIP Library Backup & Restore for Eiryn.
//!
//! Backs up user data that cannot be rebuilt from rescanning files:
//! - Playlists & playlist tracks (with LexoRank and fallback track matching metadata)
//! - Favorites
//! - Listening history
//! - Custom EQ profiles and 10-band / parametric presets
//! - User-saved lyrics
//! - User preferences and settings
//!
//! Restores with automatic track relinking:
//! If the music directory moved on a new machine, songs are seamlessly matched by (title, artist, album).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

pub const BACKUP_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: u32,
    pub app: String,
    pub created_at: String,
    pub playlist_count: usize,
    pub favorite_count: usize,
    pub history_count: usize,
    pub eq_profile_count: usize,
    pub lyric_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackRefBackup {
    pub track_id: String,
    pub file_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub position: i32,
    pub rank: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistBackup {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_smart: bool,
    pub smart_rules: Option<String>,
    pub tracks: Vec<TrackRefBackup>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteBackup {
    pub track_id: String,
    pub file_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryBackup {
    pub track_id: String,
    pub file_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub played_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EqProfileBackup {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_active: bool,
    pub preset_key: String,
    pub bands: Vec<(i32, f32)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LyricBackup {
    pub track_id: String,
    pub file_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub content: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupSummary {
    pub zip_path: String,
    pub file_size_bytes: u64,
    pub playlist_count: usize,
    pub favorite_count: usize,
    pub history_count: usize,
    pub eq_profile_count: usize,
    pub lyric_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RestoreSummary {
    pub restored_playlists: usize,
    pub restored_favorites: usize,
    pub restored_history: usize,
    pub restored_eq_profiles: usize,
    pub restored_lyrics: usize,
    pub relinked_tracks: usize,
}

pub async fn export_backup(pool: &SqlitePool, dest_zip_path: &Path) -> Result<BackupSummary, String> {
    // 1. Collect Playlists & tracks
    let pl_rows = sqlx::query("SELECT id, name, description, is_smart, smart_rules FROM playlists")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch playlists: {e}"))?;

    let mut playlists_backup = Vec::new();
    for pl in pl_rows {
        let pl_id: String = pl.get("id");
        let name: String = pl.get("name");
        let desc: String = pl.get("description");
        let is_smart: bool = pl.get::<i64, _>("is_smart") != 0;
        let rules: Option<String> = pl.get("smart_rules");

        let pt_rows = sqlx::query(
            "SELECT pt.track_id, pt.position, pt.rank, t.file_path, t.title, t.artist, t.album
             FROM playlist_tracks pt
             JOIN tracks t ON pt.track_id = t.id
             WHERE pt.playlist_id = ?
             ORDER BY CASE WHEN pt.rank != '' THEN pt.rank ELSE printf('%09d', pt.position) END ASC"
        )
        .bind(&pl_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch playlist tracks: {e}"))?;

        let mut tracks = Vec::new();
        for pt in pt_rows {
            tracks.push(TrackRefBackup {
                track_id: pt.get("track_id"),
                file_path: pt.get("file_path"),
                title: pt.get("title"),
                artist: pt.get("artist"),
                album: pt.get("album"),
                position: pt.get::<i32, _>("position"),
                rank: pt.get("rank"),
            });
        }

        playlists_backup.push(PlaylistBackup {
            id: pl_id,
            name,
            description: desc,
            is_smart,
            smart_rules: rules,
            tracks,
        });
    }

    // 2. Collect Favorites
    let fav_rows = sqlx::query(
        "SELECT f.track_id, f.created_at, t.file_path, t.title, t.artist, t.album
         FROM favorites f
         JOIN tracks t ON f.track_id = t.id"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch favorites: {e}"))?;

    let mut favorites_backup = Vec::new();
    for f in fav_rows {
        favorites_backup.push(FavoriteBackup {
            track_id: f.get("track_id"),
            file_path: f.get("file_path"),
            title: f.get("title"),
            artist: f.get("artist"),
            album: f.get("album"),
            created_at: f.get("created_at"),
        });
    }

    // 3. Collect History (last 10,000 plays)
    let hist_rows = sqlx::query(
        "SELECT h.track_id, h.played_at, t.file_path, t.title, t.artist, t.album
         FROM play_history h
         JOIN tracks t ON h.track_id = t.id
         ORDER BY h.played_at DESC
         LIMIT 10000"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch history: {e}"))?;

    let mut history_backup = Vec::new();
    for h in hist_rows {
        history_backup.push(HistoryBackup {
            track_id: h.get("track_id"),
            file_path: h.get("file_path"),
            title: h.get("title"),
            artist: h.get("artist"),
            album: h.get("album"),
            played_at: h.get("played_at"),
        });
    }

    // 4. Collect EQ Profiles
    let eq_rows = sqlx::query("SELECT id, name, is_default, is_active, preset_key FROM eq_profiles")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    let mut eq_backup = Vec::new();
    for eq in eq_rows {
        let profile_id: String = eq.get("id");
        let band_rows = sqlx::query("SELECT band_index, gain FROM eq_bands WHERE profile_id = ? ORDER BY band_index ASC")
            .bind(&profile_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default();

        let bands = band_rows
            .into_iter()
            .map(|b| (b.get::<i32, _>("band_index"), b.get::<f64, _>("gain") as f32))
            .collect();

        eq_backup.push(EqProfileBackup {
            id: profile_id,
            name: eq.get("name"),
            is_default: eq.get::<i64, _>("is_default") != 0,
            is_active: eq.get::<i64, _>("is_active") != 0,
            preset_key: eq.get("preset_key"),
            bands,
        });
    }

    // 5. Collect Lyrics
    let lyric_rows = sqlx::query(
        "SELECT l.track_id, l.content, l.source, t.file_path, t.title, t.artist, t.album
         FROM lyrics l
         JOIN tracks t ON l.track_id = t.id"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut lyrics_backup = Vec::new();
    for l in lyric_rows {
        lyrics_backup.push(LyricBackup {
            track_id: l.get("track_id"),
            file_path: l.get("file_path"),
            title: l.get("title"),
            artist: l.get("artist"),
            album: l.get("album"),
            content: l.get("content"),
            source: l.get("source"),
        });
    }

    // 6. Collect Settings
    let setting_rows = sqlx::query("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut settings_map = HashMap::new();
    for s in setting_rows {
        let k: String = s.get("key");
        let v: String = s.get("value");
        settings_map.insert(k, v);
    }

    let manifest = BackupManifest {
        version: BACKUP_VERSION,
        app: "Eiryn".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        playlist_count: playlists_backup.len(),
        favorite_count: favorites_backup.len(),
        history_count: history_backup.len(),
        eq_profile_count: eq_backup.len(),
        lyric_count: lyrics_backup.len(),
    };

    // Write into ZIP archive
    let file = File::create(dest_zip_path)
        .map_err(|e| format!("Failed to create backup zip: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Write manifest.json
    zip.start_file("manifest.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write playlists.json
    zip.start_file("playlists.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&playlists_backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write favorites.json
    zip.start_file("favorites.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&favorites_backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write history.json
    zip.start_file("history.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&history_backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write eq_profiles.json
    zip.start_file("eq_profiles.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&eq_backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write lyrics.json
    zip.start_file("lyrics.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&lyrics_backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // Write settings.json
    zip.start_file("settings.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(&serde_json::to_vec_pretty(&settings_map).map_err(|e| e.to_string())?)
        .map_err(|e| format!("ZIP write error: {e}"))?;

    zip.finish().map_err(|e| format!("Failed to finalize zip: {e}"))?;

    let file_size_bytes = std::fs::metadata(dest_zip_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(BackupSummary {
        zip_path: dest_zip_path.display().to_string(),
        file_size_bytes,
        playlist_count: manifest.playlist_count,
        favorite_count: manifest.favorite_count,
        history_count: manifest.history_count,
        eq_profile_count: manifest.eq_profile_count,
        lyric_count: manifest.lyric_count,
    })
}

pub async fn restore_backup(pool: &SqlitePool, src_zip_path: &Path) -> Result<RestoreSummary, String> {
    let file = File::open(src_zip_path)
        .map_err(|e| format!("Failed to open backup zip: {e}"))?;
    let mut zip = ZipArchive::new(file)
        .map_err(|e| format!("Invalid backup zip format: {e}"))?;

    // 1. Read manifest.json
    let mut manifest_str = String::new();
    {
        let mut entry = zip.by_name("manifest.json")
            .map_err(|_| "Backup archive is missing manifest.json".to_string())?;
        entry.read_to_string(&mut manifest_str)
            .map_err(|e| format!("Failed to read manifest.json: {e}"))?;
    }
    let manifest: BackupManifest = serde_json::from_str(&manifest_str)
        .map_err(|e| format!("Invalid manifest format: {e}"))?;

    if manifest.version > BACKUP_VERSION {
        return Err(format!("Backup version {} is newer than supported version {}", manifest.version, BACKUP_VERSION));
    }

    // 2. Build local track relinking index
    let local_tracks = sqlx::query("SELECT id, file_path, title, artist, album FROM tracks")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch local tracks: {e}"))?;

    let mut by_path: HashMap<String, String> = HashMap::new();
    let mut by_tags: HashMap<(String, String, String), String> = HashMap::new();

    for t in local_tracks {
        let tid: String = t.get("id");
        let path: String = t.get("file_path");
        let title: String = t.get("title");
        let artist: String = t.get("artist");
        let album: String = t.get("album");

        by_path.insert(path.to_lowercase(), tid.clone());
        let tag_key = (
            title.trim().to_lowercase(),
            artist.trim().to_lowercase(),
            album.trim().to_lowercase(),
        );
        if !tag_key.0.is_empty() {
            by_tags.insert(tag_key, tid);
        }
    }

    let relink = |path: &str, title: &str, artist: &str, album: &str| -> Option<String> {
        if let Some(id) = by_path.get(&path.to_lowercase()) {
            return Some(id.clone());
        }
        let tag_key = (
            title.trim().to_lowercase(),
            artist.trim().to_lowercase(),
            album.trim().to_lowercase(),
        );
        by_tags.get(&tag_key).cloned()
    };

    let mut relinked_tracks = 0usize;
    let mut restored_playlists = 0usize;
    let mut restored_favorites = 0usize;
    let mut restored_history = 0usize;
    let mut restored_eq_profiles = 0usize;
    let mut restored_lyrics = 0usize;

    // 3. Restore Playlists
    if let Ok(mut entry) = zip.by_name("playlists.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(playlists) = serde_json::from_str::<Vec<PlaylistBackup>>(&data) {
                for pl in playlists {
                    let _ = sqlx::query(
                        "INSERT INTO playlists (id, name, description, is_smart, smart_rules)
                         VALUES (?, ?, ?, ?, ?)
                         ON CONFLICT(id) DO UPDATE SET
                            name = excluded.name,
                            description = excluded.description,
                            is_smart = excluded.is_smart,
                            smart_rules = excluded.smart_rules"
                    )
                    .bind(&pl.id)
                    .bind(&pl.name)
                    .bind(&pl.description)
                    .bind(pl.is_smart)
                    .bind(&pl.smart_rules)
                    .execute(pool)
                    .await;

                    for tr in pl.tracks {
                        if let Some(local_id) = relink(&tr.file_path, &tr.title, &tr.artist, &tr.album) {
                            if local_id != tr.track_id {
                                relinked_tracks += 1;
                            }
                            let _ = sqlx::query(
                                "INSERT INTO playlist_tracks (playlist_id, track_id, position, rank)
                                 VALUES (?, ?, ?, ?)
                                 ON CONFLICT(playlist_id, track_id) DO UPDATE SET
                                    position = excluded.position,
                                    rank = excluded.rank"
                            )
                            .bind(&pl.id)
                            .bind(&local_id)
                            .bind(tr.position)
                            .bind(&tr.rank)
                            .execute(pool)
                            .await;
                        }
                    }
                    restored_playlists += 1;
                }
            }
        }
    }

    // 4. Restore Favorites
    if let Ok(mut entry) = zip.by_name("favorites.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(favs) = serde_json::from_str::<Vec<FavoriteBackup>>(&data) {
                for f in favs {
                    if let Some(local_id) = relink(&f.file_path, &f.title, &f.artist, &f.album) {
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO favorites (track_id, created_at) VALUES (?, ?)"
                        )
                        .bind(&local_id)
                        .bind(&f.created_at)
                        .execute(pool)
                        .await;
                        restored_favorites += 1;
                    }
                }
            }
        }
    }

    // 5. Restore History
    if let Ok(mut entry) = zip.by_name("history.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(hist) = serde_json::from_str::<Vec<HistoryBackup>>(&data) {
                for h in hist {
                    if let Some(local_id) = relink(&h.file_path, &h.title, &h.artist, &h.album) {
                        let _ = sqlx::query(
                            "INSERT INTO play_history (track_id, played_at) VALUES (?, ?)"
                        )
                        .bind(&local_id)
                        .bind(&h.played_at)
                        .execute(pool)
                        .await;
                        restored_history += 1;
                    }
                }
            }
        }
    }

    // 6. Restore EQ Profiles
    if let Ok(mut entry) = zip.by_name("eq_profiles.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(eq_profiles) = serde_json::from_str::<Vec<EqProfileBackup>>(&data) {
                for eq in eq_profiles {
                    let _ = sqlx::query(
                        "INSERT INTO eq_profiles (id, name, is_default, is_active, preset_key)
                         VALUES (?, ?, ?, ?, ?)
                         ON CONFLICT(id) DO UPDATE SET
                            name = excluded.name,
                            is_default = excluded.is_default,
                            is_active = excluded.is_active,
                            preset_key = excluded.preset_key"
                    )
                    .bind(&eq.id)
                    .bind(&eq.name)
                    .bind(eq.is_default)
                    .bind(eq.is_active)
                    .bind(&eq.preset_key)
                    .execute(pool)
                    .await;

                    for (idx, gain) in eq.bands {
                        let _ = sqlx::query(
                            "INSERT INTO eq_bands (profile_id, band_index, gain)
                             VALUES (?, ?, ?)
                             ON CONFLICT(profile_id, band_index) DO UPDATE SET
                                gain = excluded.gain"
                        )
                        .bind(&eq.id)
                        .bind(idx)
                        .bind(gain as f64)
                        .execute(pool)
                        .await;
                    }
                    restored_eq_profiles += 1;
                }
            }
        }
    }

    // 7. Restore Lyrics
    if let Ok(mut entry) = zip.by_name("lyrics.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(lyrics) = serde_json::from_str::<Vec<LyricBackup>>(&data) {
                for l in lyrics {
                    if let Some(local_id) = relink(&l.file_path, &l.title, &l.artist, &l.album) {
                        let _ = sqlx::query(
                            "INSERT INTO lyrics (track_id, content, source)
                             VALUES (?, ?, ?)
                             ON CONFLICT(track_id) DO UPDATE SET
                                content = excluded.content,
                                source = excluded.source"
                        )
                        .bind(&local_id)
                        .bind(&l.content)
                        .bind(&l.source)
                        .execute(pool)
                        .await;
                        restored_lyrics += 1;
                    }
                }
            }
        }
    }

    // 8. Restore Settings
    if let Ok(mut entry) = zip.by_name("settings.json") {
        let mut data = String::new();
        if entry.read_to_string(&mut data).is_ok() {
            if let Ok(settings) = serde_json::from_str::<HashMap<String, String>>(&data) {
                for (k, v) in settings {
                    let _ = sqlx::query(
                        "INSERT INTO settings (key, value) VALUES (?, ?)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
                    )
                    .bind(&k)
                    .bind(&v)
                    .execute(pool)
                    .await;
                }
            }
        }
    }

    Ok(RestoreSummary {
        restored_playlists,
        restored_favorites,
        restored_history,
        restored_eq_profiles,
        restored_lyrics,
        relinked_tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_and_restore_manifest_roundtrip() {
        let manifest = BackupManifest {
            version: BACKUP_VERSION,
            app: "Eiryn".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            playlist_count: 3,
            favorite_count: 10,
            history_count: 50,
            eq_profile_count: 2,
            lyric_count: 8,
        };
        let serialized = serde_json::to_vec_pretty(&manifest).unwrap();
        let parsed: BackupManifest = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(parsed.version, BACKUP_VERSION);
        assert_eq!(parsed.playlist_count, 3);
        assert_eq!(parsed.favorite_count, 10);
        assert_eq!(parsed.history_count, 50);
        assert_eq!(parsed.eq_profile_count, 2);
        assert_eq!(parsed.lyric_count, 8);
    }
}
