use crate::library::cue;
use crate::library::db;
use crate::library::track::{extract_artwork_bytes, extract_lyrics_from_file, normalize_genre_name, split_genre_string, Genre, Track};
use crate::library::track_artwork::TrackArtworkCache;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use rayon::prelude::*;

#[derive(Serialize)]
pub struct ScanResult {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub track_ids: Vec<String>,
}

/// Checks whether a folder exists, is a directory, and can be read.
/// Guards against mass deletion / dropped tracks when an external USB drive
/// or network share is disconnected or unmounted.
pub fn is_folder_readable(folder: &Path) -> bool {
    if !folder.exists() || !folder.is_dir() {
        return false;
    }
    std::fs::read_dir(folder).is_ok()
}

/// aria2 or qBittorrent companion control file ("<file or folder>.aria2", ".!qb").
pub fn is_download_mark(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("aria2") || e.eq_ignore_ascii_case("!qb"))
}

/// Checks if a file or its parent directories indicate an active download in progress.
/// Uses in-memory marks from companion files (.aria2, .!qb) to avoid extra disk stat syscalls.
pub fn is_downloading(path: &Path, marks: &HashSet<PathBuf>) -> bool {
    let partial = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["part", "partial", "crdownload", "download", "!qb", "aria2__temp", "tmp"]
            .contains(&e.to_ascii_lowercase().as_str())
    });
    if partial {
        return true;
    }

    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let name_lower = name.to_ascii_lowercase();
        if name_lower.ends_with(".part")
            || name_lower.ends_with(".crdownload")
            || name_lower.ends_with("!qb")
            || name_lower.ends_with(".aria2")
        {
            return true;
        }
    }

    // Check if the file itself or any parent directory is within an active download mark
    path.ancestors().any(|a| marks.contains(a))
}

/// Check if a single path is part of an active download (for real-time filesystem events).
pub fn downloading_now(path: &Path, root: &Path) -> bool {
    if is_downloading(path, &HashSet::new()) {
        return true;
    }
    path.ancestors()
        .take_while(|a| a.starts_with(root) && *a != root)
        .any(|a| {
            let mut aria = a.as_os_str().to_owned();
            aria.push(".aria2");
            let mut qb = a.as_os_str().to_owned();
            qb.push(".!qb");
            Path::new(&aria).exists() || Path::new(&qb).exists()
        })
}

/// Lightweight FLAC metadata header chain validation.
/// Inspects the 4-byte FLAC magic identifier and follows the metadata block chain.
/// Ensures block 0 is STREAMINFO and no subsequent block is STREAMINFO, which immediately
/// catches zero-padded truncated downloads on the first hop without reading empty blocks.
pub fn flac_metadata_ok(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("flac")) {
        return true;
    }

    let Ok(mut file) = std::fs::File::open(path) else { return false };

    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() || &magic != b"fLaC" {
        return true; // Not standard raw FLAC (e.g. ID3 header) — let lofty handle it
    }

    for i in 0..256 {
        let mut header = [0u8; 4];
        if file.read_exact(&mut header).is_err() {
            return false;
        }

        let is_last = (header[0] & 0x80) != 0;
        let block_type = header[0] & 0x7F;
        let block_len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as u64;

        // In FLAC spec, exactly one STREAMINFO block exists and it must be block 0.
        // A truncated or zero-padded file has 0u8 for headers, causing block_type == 0 for i > 0.
        // This immediately detects zero-padded corrupted downloads on the first pass!
        if (block_type == 0) != (i == 0) {
            return false;
        }

        if is_last {
            return true;
        }

        if file.seek(SeekFrom::Current(block_len as i64)).is_err() {
            return false;
        }
    }

    false
}

/// Tracks per insert transaction. Bounds the WAL lock and retry cost of a
/// folder-wide scan (Airmedy batches Bleve+SQLite writes the same way).
const COMMIT_CHUNK: usize = 200;

pub async fn scan_folder(pool: &SqlitePool, folder: &Path, artwork_cache: &TrackArtworkCache) -> Result<ScanResult, String> {
    // Disconnection / accessibility safeguard
    if !is_folder_readable(folder) {
        return Err(format!("Folder is not accessible or drive is disconnected: {}", folder.display()));
    }

    // Must cover everything the file opener accepts (main.rs associations);
    // unscannable formats fail singly in Track::from_path with a warning.
    let exts = ["mp3", "flac", "wav", "ogg", "m4a", "aac", "wma", "opus",
        "wv", "aiff", "aif", "dsf", "dff", "ape", "mpc", "tta"];

    // Collect all file paths in the folder
    let raw_files: Vec<PathBuf> = WalkDir::new(folder)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();

    // Partition download control marks (e.g. .aria2, .!qb) from potential audio tracks
    let (mark_files, candidate_files): (Vec<PathBuf>, Vec<PathBuf>) = raw_files
        .into_iter()
        .partition(|p| is_download_mark(p));

    let marks: HashSet<PathBuf> = mark_files
        .into_iter()
        .map(|m| m.with_extension(""))
        .collect();

    let entries: Vec<PathBuf> = candidate_files
        .into_par_iter()
        .filter(|path| {
            !is_downloading(path, &marks) &&
            path.extension()
                .and_then(|e| e.to_str())
                .map(|e| exts.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();

    // Fetch existing track stamps for incremental scan
    let existing_stamps = db::get_track_stamps_for_folder(pool, &folder.to_string_lossy()).await?;

    let mut added = 0;
    let mut updated = 0;
    let mut track_ids: Vec<String> = Vec::new();

    // Process tracks in parallel with batched inserts.
    // The parse phase is CPU-heavy (metadata, artwork, lyrics, CUE sheets), so it
    // runs on the blocking pool instead of occupying a Tokio worker thread.
    let stamps_for_parse = existing_stamps.clone();
    let artwork_cache = artwork_cache.clone();
    let processed_results: Vec<(Track, Option<String>, Option<(String, String)>, Option<Vec<Track>>)> = tokio::task::spawn_blocking(move || {
        entries
        .par_iter()
        .filter_map(|path| {
            let path_str = path.to_string_lossy().to_string();

            // Incremental check: skip only if BOTH mtime and size match, so
            // same-second rewrites and mtime-preserving tag edits re-parse.
            if let Some((stored_mtime, stored_size)) = stamps_for_parse.get(&path_str) {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(modified) = meta.modified() {
                        let current_mtime = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        if current_mtime == *stored_mtime && meta.len() as i64 == *stored_size {
                            return None;
                        }
                    }
                }
            }

            let is_flac = path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("flac"))
                .unwrap_or(false);
            if is_flac && !flac_metadata_ok(path) {
                log::warn!("Skipping corrupted FLAC header chain: {:?}", path);
                return None;
            }

            match Track::from_path(path) {
                Ok(mut track) => {
                    // Extract artwork
                    let artwork_key = if track.has_artwork && track.artwork_key.is_none() {
                        extract_artwork_bytes(path).and_then(|bytes| artwork_cache.save(&bytes).ok())
                    } else {
                        None
                    };
                    
                    if let Some(key) = &artwork_key {
                        track.artwork_key = Some(key.clone());
                    }
                    
                    // Extract lyrics
                    let lyrics = extract_lyrics_from_file(path);
                    
                    // Parse CUE sheets
                    let cue_tracks = cue::find_cue_for_audio(path)
                        .map(|cue_content| {
                            let cue_tracks = cue::parse_cue(&cue_content);
                            let parent_id = track.id.clone();
                            cue_tracks.into_iter().enumerate().map(|(_, ct)| {
                                let mut virtual_track = track.clone();
                                virtual_track.id = format!("{}-cue-{:02}", parent_id, ct.track_number);
                                virtual_track.title = ct.title.clone();
                                if !ct.artist.is_empty() {
                                    virtual_track.artist = ct.artist.clone();
                                }
                                virtual_track.track_number = ct.track_number;
                                virtual_track.cue_parent_id = Some(parent_id.clone());
                                virtual_track.cue_offset = ct.start_secs;
                                virtual_track
                            }).collect::<Vec<_>>()
                        });
                    
                    Some((track, artwork_key, lyrics, cue_tracks))
                }
                Err(e) => {
                    log::warn!("Failed to read metadata for {:?}: {e}", path);
                    None
                }
            }
        })
        .collect()
    })
    .await
    .map_err(|e| format!("Scan parse phase panicked: {e}"))?;

    // Insert tracks in chunks: one transaction per COMMIT_CHUNK tracks bounds
    // the WAL lock and retry cost (a giant folder-wide tx is all-or-nothing).
    // A failed chunk records the first error but later chunks still commit.
    let mut pending = processed_results;
    let mut first_err: Option<String> = None;
    while !pending.is_empty() {
        let n = COMMIT_CHUNK.min(pending.len());
        let batch: Vec<_> = pending.drain(..n).collect();
        let mut tx = pool.begin().await.map_err(|e| format!("Failed to begin transaction: {e}"))?;
        for (mut track, _artwork_key, lyrics, cue_tracks) in batch {
            // Files we already knew about (with a stamp on record) are updates;
            // everything else is a new addition.
            let is_update = existing_stamps.contains_key(&track.file_path);
            // The effective id is the stored one (stable across rescans) — never
            // the fresh UUID parsed from disk, which would orphan CUE children.
            match db::upsert_track(&mut *tx, &mut track).await {
                Ok(parent_id) => {
                    if is_update {
                        updated += 1;
                    } else {
                        added += 1;
                    }
                    track_ids.push(track.id.clone());

                    // Process genres
                    if !track.raw_genre_names.is_empty() {
                        let genre_names = split_genre_string(&track.raw_genre_names);
                        let mut genre_ids: Vec<String> = Vec::new();
                        for name in &genre_names {
                            let nk = normalize_genre_name(name);
                            let gid = format!("genre-{}", nk);
                            let genre = Genre {
                                id: gid.clone(),
                                name: name.clone(),
                                normalization_key: nk,
                                track_count: 0,
                            };
                            if let Err(e) = db::upsert_genre(&mut *tx, &genre).await {
                                log::warn!("Failed to upsert genre {name}: {e}");
                            }
                            genre_ids.push(gid);
                        }
                        if let Err(e) = db::set_track_genres(&mut *tx, &track.id, &genre_ids).await {
                            log::warn!("Failed to set genres for {}: {e}", track.file_path);
                        }
                    }

                    // Insert lyrics
                    if let Some((meta_lyrics, meta_source)) = lyrics {
                        if let Err(e) = db::upsert_lyrics(&mut *tx, &track.id, "", "", &meta_lyrics, &meta_source).await {
                            log::warn!("Failed to upsert lyrics for {}: {e}", track.file_path);
                        }
                    }

                    // Insert CUE child tracks
                    if let Some(cue_track_list) = cue_tracks {
                        for mut ct in cue_track_list {
                            // Children are keyed by the parent's EFFECTIVE id so their
                            // ids and cue_parent_id survive rescans intact.
                            ct.id = format!("{}-cue-{:02}", parent_id, ct.track_number);
                            ct.cue_parent_id = Some(parent_id.clone());
                            if let Err(e) = db::upsert_track(&mut *tx, &mut ct).await {
                                log::warn!("Failed to upsert CUE child {}: {e}", ct.id);
                                continue;
                            }
                            added += 1;

                            // Inherit parent's genres
                            if !ct.raw_genre_names.is_empty() {
                                let genre_names = split_genre_string(&ct.raw_genre_names);
                                let mut genre_ids: Vec<String> = Vec::new();
                                for name in &genre_names {
                                    let nk = normalize_genre_name(name);
                                    let gid = format!("genre-{}", nk);
                                    let genre = Genre {
                                        id: gid.clone(),
                                        name: name.clone(),
                                        normalization_key: nk,
                                        track_count: 0,
                                    };
                                    if let Err(e) = db::upsert_genre(&mut *tx, &genre).await {
                                        log::warn!("Failed to upsert genre {name}: {e}");
                                    }
                                    genre_ids.push(gid);
                                }
                                if let Err(e) = db::set_track_genres(&mut *tx, &ct.id, &genre_ids).await {
                                    log::warn!("Failed to set genres for CUE child {}: {e}", ct.id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to upsert track {}: {e}", track.file_path);
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        if let Err(e) = tx.commit().await {
            log::warn!("Failed to commit scan chunk: {e}");
            if first_err.is_none() {
                first_err = Some(format!("Failed to commit transaction: {e}"));
            }
        }
    }

    // Post-pass (artwork propagation + orphan cleanup) in its own transaction.
    let mut tx = pool.begin().await.map_err(|e| format!("Failed to begin transaction: {e}"))?;

    // Propagate artwork_key to tracks in the same album that have none
    let _ = sqlx::query(
        "UPDATE tracks SET artwork_key = (
            SELECT t2.artwork_key FROM tracks t2
            WHERE t2.album = tracks.album
              AND t2.artwork_key IS NOT NULL AND t2.artwork_key != ''
            LIMIT 1
        ) WHERE artwork_key IS NULL AND album IN (
            SELECT album FROM tracks
            WHERE artwork_key IS NOT NULL AND artwork_key != '' AND album != ''
        )"
    )
    .execute(&mut *tx)
    .await;

    // Guard against mid-scan disconnection of external or network drives
    if !is_folder_readable(folder) {
        log::warn!("Aborting orphan cleanup: folder {:?} is no longer readable (drive disconnected?)", folder);
        tx.commit().await.map_err(|e| format!("Failed to commit transaction: {e}"))?;
        if let Some(e) = first_err {
            return Err(e);
        }
        return Ok(ScanResult {
            added,
            updated,
            removed: 0,
            track_ids,
        });
    }

    // Remove tracks whose files no longer exist on disk in this folder
    let sep = std::path::MAIN_SEPARATOR;
    let folder_str = folder.to_string_lossy();
    let with_sep = format!("{}{}", folder_str, sep);
    let escaped = with_sep.replace('%', "^%").replace('_', "^_");
    let like_pattern = format!("{}%", escaped);
    let orphan_ids: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, file_path FROM tracks WHERE file_path = ? OR file_path LIKE ? ESCAPE '^'"
    )
    .bind(folder_str.as_ref())
    .bind(&like_pattern)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| format!("Failed to fetch orphan tracks: {e}"))?;

    let mut missing_ids: Vec<String> = Vec::new();
    for (id, file_path) in &orphan_ids {
        if !std::path::Path::new(file_path).exists() {
            missing_ids.push(id.clone());
        }
    }

    // Safety guard: if ALL tracks in a folder that previously had tracks are suddenly
    // missing, verify whether the missing files' parent directories exist.
    // If an external drive, USB stick, or network mount was unplugged/disconnected,
    // do not wipe the tracks, play counts, favorites, and playlists!
    if !orphan_ids.is_empty() && missing_ids.len() == orphan_ids.len() && orphan_ids.len() > 3 {
        if let Some((_, first_missing_path)) = orphan_ids.first() {
            let p = Path::new(first_missing_path);
            if let Some(parent) = p.parent() {
                if !parent.exists() || std::fs::metadata(parent).is_err() {
                    log::warn!(
                        "Drive disconnect safety guard triggered: all {} tracks in {:?} appear missing, but parent {:?} is inaccessible. Preserving tracks.",
                        orphan_ids.len(), folder, parent
                    );
                    missing_ids.clear();
                }
            }
        }
    }

    // Also find CUE children of missing tracks
    if !missing_ids.is_empty() {
        let params: Vec<String> = missing_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let cue_sql = format!("SELECT id FROM tracks WHERE cue_parent_id IN ({})", params.join(","));
        let mut q = sqlx::query_scalar::<_, String>(&cue_sql);
        for id in &missing_ids {
            q = q.bind(id);
        }
        if let Ok(children) = q.fetch_all(&mut *tx).await {
            missing_ids.extend(children);
        }
    }

    let mut actually_removed = 0;
    if !missing_ids.is_empty() {
        let params: Vec<String> = missing_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let placeholders = params.join(",");

        let del_hist = format!("DELETE FROM play_history WHERE track_id IN ({})", placeholders);
        let mut q = sqlx::query(&del_hist);
        for id in &missing_ids { q = q.bind(id); }
        let _ = q.execute(&mut *tx).await;

        let del_fav = format!("DELETE FROM favorites WHERE track_id IN ({})", placeholders);
        let mut q = sqlx::query(&del_fav);
        for id in &missing_ids { q = q.bind(id); }
        let _ = q.execute(&mut *tx).await;

        let del_pl = format!("DELETE FROM playlist_tracks WHERE track_id IN ({})", placeholders);
        let mut q = sqlx::query(&del_pl);
        for id in &missing_ids { q = q.bind(id); }
        let _ = q.execute(&mut *tx).await;

        let del_trk = format!("DELETE FROM tracks WHERE id IN ({})", placeholders);
        let mut q = sqlx::query(&del_trk);
        for id in &missing_ids { q = q.bind(id); }
        q.execute(&mut *tx).await.map_err(|e| format!("Failed to delete orphan tracks: {e}"))?;

        actually_removed = missing_ids.len();
    }

    tx.commit().await.map_err(|e| format!("Failed to commit transaction: {e}"))?;

    if let Some(e) = first_err {
        return Err(e);
    }

    Ok(ScanResult {
        added,
        updated,
        removed: actually_removed,
        track_ids,
    })
}

// Windows-specific file watcher will be implemented in Phase 1
// For now, keep the notify-based watcher as a placeholder
#[cfg(not(windows))]
pub fn start_watcher(
    pool: SqlitePool,
    folder: std::path::PathBuf,
) -> Result<(), String> {
    log::warn!("File watcher not yet implemented for this platform");
    Ok(())
}

#[cfg(windows)]
pub fn start_watcher(
    _pool: SqlitePool,
    _folder: std::path::PathBuf,
) -> Result<(), String> {
    log::info!("Windows file watcher placeholder - will use ReadDirectoryChangesW");
    Ok(())
}
