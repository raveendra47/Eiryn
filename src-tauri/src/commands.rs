use crate::audio::lufs::{DEFAULT_TARGET_LUFS, NormalizationMode, apply_clip_guard, gain_for_target};
use crate::library::db;
use crate::library::loudness;
use crate::library::palette;
use crate::library::playlist;
use crate::library::scanner;
use crate::library::track::Track;
use crate::queue::{RepeatMode, TrackRef};
use crate::queue_persistence;
use crate::session::SessionData;
use crate::state::AppState;
use lofty::file::TaggedFileExt;
use std::path::Path;
use std::fs;
use tauri::Emitter;
use tauri::Manager;
use tauri::State;
use serde::{Deserialize, Serialize};


#[tauri::command]
pub async fn get_tracks(state: State<'_, AppState>) -> Result<Vec<Track>, String> {
    db::get_all_tracks(&state.db).await
}

#[tauri::command]
pub async fn get_albums(state: State<'_, AppState>) -> Result<Vec<db::Album>, String> {
    db::get_albums(&state.db).await
}

#[tauri::command]
pub async fn get_album_tracks(state: State<'_, AppState>, album_id: String) -> Result<Vec<Track>, String> {
    db::get_album_tracks(&state.db, &album_id).await
}

#[tauri::command]
pub async fn get_track(state: State<'_, AppState>, id: String) -> Result<Option<Track>, String> {
    db::get_track(&state.db, &id).await
}

#[tauri::command]
pub async fn search_tracks(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Track>, String> {
    let hits = {
        let search = state.search.lock().map_err(|e| e.to_string())?;
        search.search(&query, 50)
    };
    let mut results = Vec::new();
    for hit in hits {
        if let Ok(Some(mut t)) = db::get_track(&state.db, &hit.track_id).await {
            t.matched_lyric = hit.matched_lyric;
            results.push(t);
        }
    }
    Ok(results)
}

#[tauri::command]
pub async fn get_all_genres(state: State<'_, AppState>) -> Result<Vec<crate::library::track::Genre>, String> {
    db::get_all_genres(&state.db).await
}

#[tauri::command]
pub async fn get_tracks_by_genre_id(state: State<'_, AppState>, genre_id: String) -> Result<Vec<Track>, String> {
    db::get_tracks_by_genre_id(&state.db, &genre_id).await
}

pub async fn rebuild_search_with_lyrics(
    db: &sqlx::SqlitePool,
    search_mutex: &std::sync::Mutex<crate::search::tantivy_index::TantivyIndex>,
    tracks: &[Track],
) {
    let lyrics_rows = db::get_all_lyrics(db).await.unwrap_or_default();
    let mut lyrics_map = std::collections::HashMap::with_capacity(lyrics_rows.len());
    for row in lyrics_rows {
        let text = if !row.content.is_empty() { row.content } else { row.meta_content };
        if !text.is_empty() { lyrics_map.insert(row.track_id, text); }
    }
    if let Ok(mut search) = search_mutex.lock() {
        let _ = search.rebuild_with_lyrics(tracks, &lyrics_map);
    }
}

#[tauri::command]
pub async fn scan_folder(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<scanner::ScanResult, String> {
    if state.is_scanning.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(scanner::ScanResult { added: 0, updated: 0, removed: 0, track_ids: Vec::new() });
    }
    let scan_result = scanner::scan_folder(&state.db, Path::new(&path), &state.track_artwork_cache).await;
    state.is_scanning.store(false, std::sync::atomic::Ordering::SeqCst);
    let result = scan_result?;
    let tracks = db::get_all_tracks(&state.db).await?;
    rebuild_search_with_lyrics(&state.db, &state.search, &tracks).await;
    if let Ok(mut cache) = state.artwork_cache.lock() {
        cache.clear();
    }
    let _ = crate::library::smart_playlist::refresh_all_smart_playlists(&state.db).await;
    let _ = app_handle.emit("library:sync-finished", &result);
    let _ = run_genre_enrichment(app_handle.clone(), state.db.clone(), state.is_fetching_genre.clone(), Some(100)).await;
    let _ = run_audio_analysis(app_handle.clone(), state.db.clone(), state.is_analyzing.clone()).await;
    Ok(result)
}

fn extract_artwork_base64(file_path: &str) -> Result<String, String> {
    let tagged_file = lofty::read_from_path(Path::new(file_path))
        .map_err(|e| format!("Failed to read file: {e}"))?;
    let picture = tagged_file
        .tags()
        .iter()
        .filter_map(|tag| tag.pictures().first())
        .next()
        .ok_or("No artwork")?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(picture.data()))
}

#[tauri::command]
pub async fn get_artwork(state: State<'_, AppState>, id: String) -> Result<String, String> {
    {
        let cache = state.artwork_cache.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = cache.get(&id) {
            return Ok(cached.clone());
        }
    }
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;
    // lofty read + base64 on the blocking pool: never stall async workers.
    let file_path = track.file_path.clone();
    let artwork = tokio::task::spawn_blocking(move || extract_artwork_base64(&file_path))
        .await
        .map_err(|e| format!("Artwork task failed: {e}"))??;
    let mut cache = state.artwork_cache.lock().map_err(|e| e.to_string())?;
    cache.insert(id, artwork.clone());
    Ok(artwork)
}

fn invalidate_artwork_cache(state: &AppState, ids: &[String]) {
    if let Ok(mut cache) = state.artwork_cache.lock() {
        for id in ids {
            cache.remove(id);
        }
    }
}

#[tauri::command]
pub async fn set_artwork(app_handle: tauri::AppHandle, state: State<'_, AppState>, id: String, image_b64: String) -> Result<(), String> {
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;
    let path = std::path::Path::new(&track.file_path);

    use base64::Engine;
    let image_data = base64::engine::general_purpose::STANDARD
        .decode(&image_b64)
        .map_err(|e| format!("Invalid base64: {e}"))?;

    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::{BoundTaggedFile, TaggedFileExt};
    use lofty::picture::{Picture, PictureType, MimeType};
    use std::fs::OpenOptions;

    let file = OpenOptions::new().read(true).write(true).open(path)
        .map_err(|e| format!("Failed to open file: {e}"))?;
    let mut bound = BoundTaggedFile::read_from(file, ParseOptions::new())
        .map_err(|e| format!("Failed to read tags: {e}"))?;

    // Remove existing front cover pictures
    if let Some(tag) = bound.primary_tag_mut() {
        tag.remove_picture_type(PictureType::CoverFront);
    }

    // Add new picture
    let picture = Picture::new_unchecked(
        PictureType::CoverFront,
        Some(MimeType::Jpeg),
        None,
        image_data,
    );
    if let Some(tag) = bound.primary_tag_mut() {
        tag.push_picture(picture);
    }

    // Save back to file
    bound.save(WriteOptions::default())
        .map_err(|e| format!("Failed to write artwork: {e}"))?;

    // Update DB flag
    sqlx::query("UPDATE tracks SET has_artwork = 1 WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    invalidate_artwork_cache(&state, &[id.clone()]);
                let _ = app_handle.emit("library:track-updated", &id);
    Ok(())
}

#[tauri::command]
pub async fn get_track_artwork(
    state: State<'_, AppState>,
    id: String,
    size: Option<String>,
) -> Result<Option<Vec<u8>>, String> {
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;
    let variant = size.as_deref();
    if let Some(ref k) = track.artwork_key {
        if state.track_artwork_cache.exists(k, variant) {
            return state.track_artwork_cache.read(k, variant).map(Some);
        }
        if variant.is_some() {
            if let Ok(data) = state.track_artwork_cache.read(k, None) {
                return Ok(Some(data));
            }
        }
    }
    // Fallback: extract artwork directly from the audio file and cache it
    if let Some((_, new_key)) = extract_and_cache_artwork(&state.db, &state.track_artwork_cache, &track).await {
        if state.track_artwork_cache.exists(&new_key, variant) {
            return state.track_artwork_cache.read(&new_key, variant).map(Some);
        }
        return state.track_artwork_cache.read(&new_key, None).map(Some);
    }
    Ok(None)
}

#[tauri::command]
pub async fn get_file_artwork(file_path: String) -> Result<Option<Vec<u8>>, String> {
    Ok(crate::library::track::extract_artwork_bytes(std::path::Path::new(&file_path)))
}

/// Blocking extract + thumbnail save; DB update stays async with the caller.
/// Returns the raw bytes plus the new cache key.
async fn extract_and_cache_artwork(
    pool: &sqlx::SqlitePool,
    cache: &std::sync::Arc<crate::library::track_artwork::TrackArtworkCache>,
    track: &Track,
) -> Option<(Vec<u8>, String)> {
    let path = std::path::PathBuf::from(&track.file_path);
    let cache = cache.clone();
    let (bytes, key) = tokio::task::spawn_blocking(move || {
        let bytes = crate::library::track::extract_artwork_bytes(&path)?;
        let key = cache.save(&bytes).ok()?;
        Some((bytes, key))
    })
    .await
    .ok()??;
    let _ = sqlx::query("UPDATE tracks SET artwork_key = ?, has_artwork = 1 WHERE id = ?")
        .bind(&key)
        .bind(&track.id)
        .execute(pool)
        .await;
    Some((bytes, key))
}

/// Palette input: prefers the md thumbnail (500px) over the full-size
/// original. Same dominant color within rounding, ~35x faster k-means input
/// (measured 2.29s vs 66ms on a 2MB cover in debug builds).
async fn get_artwork_bytes(state: &AppState, track: &Track) -> Option<Vec<u8>> {
    if let Some(ref k) = track.artwork_key {
        if let Ok(data) = state.track_artwork_cache.read(k, Some("md")) {
            return Some(data);
        }
        if let Ok(data) = state.track_artwork_cache.read(k, None) {
            return Some(data);
        }
    }
    extract_and_cache_artwork(&state.db, &state.track_artwork_cache, track)
        .await
        .and_then(|(_, key)| {
            state
                .track_artwork_cache
                .read(&key, Some("md"))
                .or_else(|_| state.track_artwork_cache.read(&key, None))
                .ok()
        })
}

#[tauri::command]
pub async fn get_track_palette(state: State<'_, AppState>, id: String) -> Result<Option<palette::ThemeColors>, String> {
    // Palette cache: k-means over thumbnails is pure CPU, don't redo it.
    if let Ok(cache) = state.palette_cache.lock() {
        if let Some(colors) = cache.get(&id) {
            return Ok(Some(colors.clone()));
        }
    }
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;
    let bytes = get_artwork_bytes(&state, &track).await;
    match bytes {
        Some(b) => {
            let colors = tokio::task::spawn_blocking(move || palette::extract_palette(&b))
                .await
                .map_err(|e| format!("Palette task failed: {e}"))?;
            if let Some(ref c) = colors {
                if let Ok(mut cache) = state.palette_cache.lock() {
                    if cache.len() >= 100 {
                        cache.clear();
                    }
                    cache.insert(id, c.clone());
                }
            }
            Ok(colors)
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn play_track(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;

    // Determine the actual file to play (parent file for CUE tracks)
    let play_path = if let Some(parent_id) = &track.cue_parent_id {
        let parent = db::get_track(&state.db, parent_id)
            .await?
            .ok_or("CUE parent track not found")?;
        parent.file_path.clone()
    } else {
        track.file_path.clone()
    };

    if !std::path::Path::new(&play_path).exists() {
        return Err(format!("File not found: {play_path}"));
    }

    // Determine ReplayGain: prefer metadata, fall back to computed LUFS gain
    let rg_gain = get_playback_gain(&state, &track, &id).await;

    state.is_external_playback.store(false, std::sync::atomic::Ordering::Release);
    state.player.clear_next();
    state.player.play_track(std::path::PathBuf::from(&play_path), app_handle.clone(), rg_gain, track.sample_rate, Some(id.clone()))?;

    // If this is a CUE virtual track, seek to the track offset
    if track.cue_offset > 0.0 {
        state.player.seek(track.cue_offset as f32);
    }

    // Store current track ID for cross-window state sync
    if let Ok(mut tid) = state.current_track_id.lock() {
        *tid = Some(id.clone());
    }
    // Emit event so frontend knows a new track started
    let _ = app_handle.emit("track-started", &id);
    let _ = app_handle.emit("player:status", serde_json::json!({"playing": true, "track_id": id}));

    update_smtc_track(&state, &track);

    prime_next_track(&state, &app_handle).await;

    Ok(())
}

pub fn track_from_ref(r: &crate::queue::TrackRef, sample_rate: u32) -> Track {
    Track {
        id: r.id.clone(),
        file_path: r.file_path.clone(),
        title: r.title.clone(),
        artist: r.artist.clone(),
        album: r.album.clone(),
        album_artist: r.artist.clone(),
        album_id: String::new(),
        composer: String::new(),
        track_number: 0,
        disc_number: 0,
        year: 0,
        genre: String::new(),
        raw_genre_names: String::new(),
        duration: r.duration,
        sample_rate,
        bit_depth: 0,
        channels: 2,
        bitrate: 0,
        file_size: 0,
        file_format: String::new(),
        has_artwork: false,
        replaygain_track_gain: 0.0,
        replaygain_album_gain: 0.0,
        cue_parent_id: r.cue_parent_id.clone(),
        cue_offset: r.cue_offset,
        artwork_key: None,
        bpm: r.bpm,
        musical_key: String::new(),
        energy: 0.0,
        analysis_failed_reason: None,
        file_modified: 0,
        matched_lyric: None,
    }
}

pub fn update_smtc_track(state: &AppState, track: &Track) {
    if let Ok(mut at) = state.active_track.lock() {
        *at = Some(track.clone());
    }
    if let Ok(mut tid) = state.current_track_id.lock() {
        *tid = Some(track.id.clone());
    }

    let mut title = track.title.trim().to_string();
    if title.is_empty() {
        if let Some(stem) = std::path::Path::new(&track.file_path).file_stem().and_then(|s| s.to_str()) {
            title = stem.to_string();
        } else {
            title = "Unknown Track".to_string();
        }
    }
    let artist = if track.artist.trim().is_empty() {
        "Unknown Artist".to_string()
    } else {
        track.artist.clone()
    };
    let album = if track.album.trim().is_empty() {
        "Unknown Album".to_string()
    } else {
        track.album.clone()
    };

    // Update tray tooltip & label immediately
    let tooltip_text = format!("{title} — {artist}");
    if let Ok(tray_guard) = state.tray_menu.lock() {
        if let Some(ref tm) = *tray_guard {
            tm.set_tooltip(Some(&tooltip_text));
            tm.set_track_info(&title, &artist);
            tm.set_playing(true);
        }
    }

    if let Ok(guard) = state.media.lock() {
        if let Some(ref tx) = *guard {
            let cover = crate::hub::resolve_artwork_file_url(state, track);

            let _ = tx.send(crate::media::Update::Track(Some(crate::media::NowPlaying {
                title,
                artist,
                album,
                cover,
                duration: track.duration,
            })));

            let _ = tx.send(crate::media::Update::Playback {
                playing: true,
                position: track.cue_offset,
            });
        }
    }
}

pub fn update_smtc_playback(state: &AppState, playing: bool, position: f64) {
    if let Ok(guard) = state.media.lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.send(crate::media::Update::Playback {
                playing,
                position,
            });
        }
    }
}

async fn get_playback_gain(state: &AppState, track: &Track, _id: &str) -> f32 {
    let mode = db::get_setting(&state.db, "normalization_mode")
        .await
        .ok()
        .flatten()
        .map(|s| NormalizationMode::from_str(&s))
        .unwrap_or(NormalizationMode::Track);
    if mode == NormalizationMode::Off {
        return replaygain_fallback(track);
    }
    let target: f32 = db::get_setting(&state.db, "normalization_target_lufs")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TARGET_LUFS);
    let prevent_clip: bool = db::get_setting(&state.db, "normalization_prevent_clip")
        .await
        .ok()
        .flatten()
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or(true);

    // LUFS first (FFmpeg ebur128 measurement), ReplayGain tags as fallback.
    // Note: the frontend *also* scales volume by the track's ReplayGain tags
    // (player.ts:computeEffectiveVolume); that path only sees tag values, so
    // a LUFS-measured track with RG tags gets backend LUFS gain there. Both
    // paths multiply, matching the pre-existing double-apply behavior for RG.
    let lufs = db::get_track_loudness(&state.db, &track.id).await.ok().flatten();
    let (integrated, peak) = match lufs {
        Some(l) => (l.loudness_lufs as f32, l.true_peak as f32),
        None => return replaygain_fallback(track),
    };
    let (ref_lufs, ref_peak) = match mode {
        NormalizationMode::Album => {
            match db::get_album_loudness_stats(&state.db, &track.album, &track.album_artist, &track.id)
                .await
                .ok()
                .flatten()
            {
                Some((sibling_avg, sibling_peak, n)) => (
                    album_reference(integrated, sibling_avg as f32, n),
                    (sibling_peak as f32).min(peak),
                ),
                None => (integrated, peak),
            }
        }
        _ => (integrated, peak),
    };
    let mut gain = gain_for_target(ref_lufs, target);
    if prevent_clip {
        gain = apply_clip_guard(gain, ref_peak);
    }
    gain
}

/// Album-mode reference: true mean of analyzed siblings + current track, so
/// one shared gain preserves intra-album dynamics (Airmedy parity).
fn album_reference(current: f32, sibling_avg: f32, sibling_count: i64) -> f32 {
    (sibling_avg * sibling_count as f32 + current) / (sibling_count as f32 + 1.0)
}

fn replaygain_fallback(track: &Track) -> f32 {
    // If track has ReplayGain metadata, use it
    if track.replaygain_track_gain.abs() > 0.01 {
        return track.replaygain_track_gain as f32;
    }

    0.0
}

#[tauri::command]
pub async fn get_listening_insights(
    state: State<'_, AppState>,
    period: String,
) -> Result<crate::library::insights::ListeningInsights, String> {
    crate::library::insights::get_listening_insights(&state.db, &period).await
}

#[tauri::command]
pub async fn get_library_insights(
    state: State<'_, AppState>,
    period: String,
) -> Result<crate::library::insights::LibraryInsights, String> {
    crate::library::insights::get_library_insights(&state.db, &period).await
}

#[tauri::command]
pub async fn get_setting_value(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    db::get_setting(&state.db, &key).await
}

#[tauri::command]
pub async fn set_setting_value(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    // Allowlist: only frontend-owned appearance keys may use the generic path.
    match key.as_str() {
        "primary_color" | "theme" | "bit_perfect" => db::set_setting(&state.db, &key, &value).await,
        _ => Err("Setting is not user-configurable".to_string()),
    }
}

#[derive(Debug, Serialize)]
pub struct LoudnessInfo {
    pub track_id: String,
    pub loudness_lufs: f64,
    pub loudness_range: f64,
    pub true_peak: f64,
    pub analyzed_version: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NormalizationSettings {
    pub mode: String,
    pub target_lufs: f32,
    pub prevent_clip: bool,
}

#[tauri::command]
pub async fn run_full_analysis_backfill(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.clone();
    let player = state.player.clone();
    let cache = state.lufs_cache.clone();
    let flag = state.is_analyzing.clone();
    let limit = limit.unwrap_or(crate::library::analysis::FULL_BACKFILL_BATCH).clamp(1, 5000);
    tokio::spawn(async move {
        crate::library::analysis::run_full_analysis(pool, player, cache, app_handle, flag, limit).await;
    });
    Ok(serde_json::json!({"started": true}))
}

#[tauri::command]
pub async fn get_analysis_progress(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NULL")
        .fetch_one(&state.db)
        .await
        .map_err(|e| format!("Failed to count tracks: {e}"))?;
    let done: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT t.id) FROM tracks t
         LEFT JOIN track_analysis a ON a.track_id = t.id AND a.analyzed_version >= ?
         WHERE t.cue_parent_id IS NULL AND (t.bpm > 0 OR (t.musical_key IS NOT NULL AND t.musical_key != '') OR t.analysis_failed_reason IS NOT NULL OR a.track_id IS NOT NULL)",
    )
    .bind(crate::audio::spectrum::SPECTRAL_VERSION)
    .fetch_one(&state.db)
    .await
    .map_err(|e| format!("Failed to count analyzed tracks: {e}"))?;
    let is_running = state.is_analyzing.load(std::sync::atomic::Ordering::Relaxed);
    Ok(serde_json::json!({
        "analyzed": done.0,
        "total": total.0,
        "running": is_running,
    }))
}

#[tauri::command]
pub async fn recompute_moods(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let features = crate::audio::mood::recompute_percentiles(&state.db).await?;
    let tracks = crate::audio::mood::recompute_all_moods(&state.db).await?;
    Ok(serde_json::json!({"features": features, "tracks": tracks}))
}

#[tauri::command]
pub async fn get_track_analysis(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<Option<db::TrackAnalysis>, String> {
    db::get_track_analysis(&state.db, &track_id).await
}

#[tauri::command]
pub async fn get_track_loudness(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<Option<LoudnessInfo>, String> {
    Ok(db::get_track_loudness(&state.db, &track_id)
        .await?
        .map(|l| LoudnessInfo {
            track_id: l.track_id,
            loudness_lufs: l.loudness_lufs,
            loudness_range: l.loudness_range,
            true_peak: l.true_peak,
            analyzed_version: l.analyzed_version,
        }))
}

#[tauri::command]
pub async fn calculate_track_lufs(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<LoudnessInfo, String> {
    let track = db::get_track(&state.db, &track_id)
        .await?
        .ok_or("Track not found")?;
    // CUE children share the parent file; measure the parent, attribute to child.
    let measure_id;
    let file_path;
    if let Some(ref parent_id) = track.cue_parent_id {
        let parent = db::get_track(&state.db, parent_id)
            .await?
            .ok_or("CUE parent track not found")?;
        measure_id = parent.id.clone();
        file_path = parent.file_path.clone();
    } else {
        measure_id = track.id.clone();
        file_path = track.file_path.clone();
    }
    let (i, r, p) = loudness::measure_and_store(&state.db, &state.lufs_cache, &measure_id, &file_path).await?;
    Ok(LoudnessInfo {
        track_id: measure_id,
        loudness_lufs: i as f64,
        loudness_range: r as f64,
        true_peak: p as f64,
        analyzed_version: crate::audio::lufs::ANALYZER_VERSION,
    })
}

#[tauri::command]
pub async fn run_loudness_backfill(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    if !loudness::try_claim_worker(&state.is_loudness_running) {
        return Err("Loudness analysis already running".to_string());
    }
    let pool = state.db.clone();
    let player = state.player.clone();
    let cache = state.lufs_cache.clone();
    let flag = state.is_loudness_running.clone();
    let limit = limit.unwrap_or(loudness::BACKFILL_BATCH).clamp(1, 5000);
    tokio::spawn(async move {
        loudness::run_backfill(pool, player, cache, app_handle, limit).await;
        loudness::release_worker(&flag);
    });
    Ok(serde_json::json!({"started": true}))
}

#[tauri::command]
pub async fn get_normalization_settings(
    state: State<'_, AppState>,
) -> Result<NormalizationSettings, String> {
    let mode = db::get_setting(&state.db, "normalization_mode")
        .await?
        .unwrap_or_else(|| "track".to_string());
    let target_lufs: f32 = db::get_setting(&state.db, "normalization_target_lufs")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TARGET_LUFS);
    let prevent_clip = db::get_setting(&state.db, "normalization_prevent_clip")
        .await?
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or(true);
    Ok(NormalizationSettings { mode, target_lufs, prevent_clip })
}

#[tauri::command]
pub async fn set_normalization_settings(
    state: State<'_, AppState>,
    mode: String,
    target_lufs: f32,
    prevent_clip: bool,
) -> Result<(), String> {
    let mode = NormalizationMode::from_str(&mode);
    let target_lufs = target_lufs.clamp(-30.0, -5.0);
    db::set_setting(&state.db, "normalization_mode", mode.as_str()).await?;
    db::set_setting(&state.db, "normalization_target_lufs", &target_lufs.to_string()).await?;
    db::set_setting(&state.db, "normalization_prevent_clip", if prevent_clip { "1" } else { "0" }).await?;
    Ok(())
}

#[tauri::command]
pub async fn get_loudness_progress(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let (analyzed, total) =
        db::get_loudness_progress(&state.db, crate::audio::lufs::ANALYZER_VERSION).await?;
    let ffmpeg_available = crate::audio::lufs::find_ffmpeg().is_some();
    Ok(serde_json::json!({"analyzed": analyzed, "total": total, "ffmpeg_available": ffmpeg_available}))
}

fn track_to_ref(track: &Track) -> TrackRef {
    TrackRef {
        id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        duration: track.duration,
        file_path: track.file_path.clone(),
        cue_parent_id: track.cue_parent_id.clone(),
        cue_offset: track.cue_offset,
        bpm: track.bpm,
        energy: track.energy,
        last_played: 0,
    }
}

async fn play_track_ref(state: &AppState, app_handle: &tauri::AppHandle, track: &TrackRef) -> Result<(), String> {
    // Look up ReplayGain/gain from the full track metadata if available.
    // Non-CUE tracks use the same row for the native sample rate (CUE
    // children are clones of the parent, so their own sample_rate is valid).
    let (rg_gain, noncue_rate) = {
        let full_track = db::get_track(&state.db, &track.id).await
            .ok()
            .flatten();
        let rate = full_track.as_ref().map(|t| t.sample_rate).unwrap_or(0);
        if let Some(t) = full_track {
            if t.replaygain_track_gain.abs() > 0.01 {
                (t.replaygain_track_gain as f32, rate)
            } else {                (0.0, rate)
            }
        } else {
            (0.0, 0)
        }
    };

    let (play_path, play_rate) = if let Some(parent_id) = track.cue_parent_id.as_deref() {
        let parent = db::get_track(&state.db, parent_id).await
            .map_err(|e| e.to_string())?
            .ok_or("CUE parent track not found")?;
        (std::path::PathBuf::from(parent.file_path), parent.sample_rate)
    } else {
        (std::path::PathBuf::from(&track.file_path), noncue_rate)
    };

    if !play_path.exists() {
        return Err(format!("File not found: {}", play_path.display()));
    }

    state.is_external_playback.store(false, std::sync::atomic::Ordering::Release);
    state.player.clear_next();
    state.player.play_track(play_path, app_handle.clone(), rg_gain, play_rate, Some(track.id.clone()))?;

    if track.cue_offset > 0.0 {
        state.player.seek(track.cue_offset as f32);
    }

    if let Ok(mut tid) = state.current_track_id.lock() {
        *tid = Some(track.id.clone());
    }

    let _ = app_handle.emit("track-started", &track.id);
    let _ = app_handle.emit("player:status", serde_json::json!({"playing": true, "track_id": track.id}));

    let full_track = match db::get_track(&state.db, &track.id).await {
        Ok(Some(t)) => t,
        _ => track_from_ref(track, play_rate),
    };
    update_smtc_track(state, &full_track);

    // Gapless: pre-decode the next track if enabled
    prime_next_track(state, app_handle).await;

    Ok(())
}

async fn prime_next_track(state: &AppState, app_handle: &tauri::AppHandle) {
    if !state.player.is_gapless() {
        return;
    }
    if state.is_external_playback.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let next = state.queue_state.lock().ok().and_then(|q| q.peek_next().cloned());
    if let Some(n) = next {
        let next_path = if let Some(pid) = n.cue_parent_id.as_deref() {
            if let Ok(Some(p)) = db::get_track(&state.db, pid).await {
                std::path::PathBuf::from(p.file_path)
            } else {
                std::path::PathBuf::from(&n.file_path)
            }
        } else {
            std::path::PathBuf::from(&n.file_path)
        };
        let (next_gain, next_rate) = {
            let full = db::get_track(&state.db, &n.id).await.ok().flatten();
            let rate = full.as_ref().map(|t| t.sample_rate).unwrap_or(0);
            let gain = full
                .filter(|t| t.replaygain_track_gain.abs() > 0.01)
                .map(|t| t.replaygain_track_gain as f32)
                .unwrap_or(0.0);
            (gain, rate)
        };
        state.player.prepare_next(next_path, app_handle.clone(), next_gain, n.id.clone(), next_rate);
    } else {
        state.player.clear_next();
    }
}

#[tauri::command]
pub async fn set_gapless(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.player.set_gapless(enabled);
    Ok(())
}

#[tauri::command]
pub async fn set_crossfade(
    state: State<'_, AppState>,
    seconds: f64,
) -> Result<(), String> {
    state.player.set_crossfade(seconds as f32);
    Ok(())
}

#[tauri::command]
pub async fn set_bit_perfect(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.player.set_bit_perfect(enabled);
    // Persist so a saved preference resumes on the next launch (restored in
    // main.rs setup next to the EQ state). State changes are reported to the
    // UI via the audio thread's `output-updated` events.
    let saved = if enabled { "1" } else { "0" };
    crate::library::db::set_setting(&state.db, "bit_perfect", saved).await?;
    Ok(())
}

#[tauri::command]
pub async fn get_output_state(state: State<'_, AppState>) -> Result<crate::audio::player::OutputState, String> {
    Ok(state.player.output_state())
}

#[tauri::command]
pub async fn prepare_next_track(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    track_id: String,
) -> Result<(), String> {
    if state.is_external_playback.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(());
    }
    let track = db::get_track(&state.db, &track_id)
        .await?
        .ok_or("Track not found")?;
    let play_path = if let Some(parent_id) = &track.cue_parent_id {
        let parent = db::get_track(&state.db, parent_id)
            .await?
            .ok_or("CUE parent track not found")?;
        std::path::PathBuf::from(parent.file_path)
    } else {
        std::path::PathBuf::from(&track.file_path)
    };
    if !play_path.exists() {
        return Err(format!("File not found: {}", play_path.display()));
    }
    let rg_gain = get_playback_gain(&state, &track, &track_id).await;
    state.player.prepare_next(play_path, app_handle.clone(), rg_gain, track_id, track.sample_rate);
    Ok(())
}

#[tauri::command]
pub async fn advance_queue(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<TrackRef>, String> {
    // Only advance if a gapless transition is in progress; skip duplicate calls
    if !state.player.try_claim_transition() {
        return Ok(None);
    }
    let track = {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.next_track_auto().cloned()
    };
    if let Some(ref t) = track {
        if let Ok(mut tid) = state.current_track_id.lock() {
            *tid = Some(t.id.clone());
        }
        let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
        save_queue_state(&state, &queue_path);
        let full_track = match db::get_track(&state.db, &t.id).await {
            Ok(Some(ft)) => ft,
            _ => track_from_ref(t, 0),
        };
        update_smtc_track(&state, &full_track);
        Ok(Some(t.clone()))
    } else {
        Ok(None)
    }
}

fn save_queue_state(state: &AppState, queue_path: &std::path::PathBuf) {
    if let Ok(q) = state.queue_state.lock() {
        queue_persistence::save_queue(&q, queue_path);
    }
}

#[tauri::command]
pub async fn set_queue(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tracks: Vec<Track>,
    start_index: usize,
) -> Result<(), String> {
    let refs: Vec<TrackRef> = tracks.iter().map(track_to_ref).collect();
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.set_tracks(refs, start_index);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[tauri::command]
pub async fn next_track(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<TrackRef>, String> {
    // Claim any in-flight gapless transition so advance_queue won't double-advance
    let _ = state.player.try_claim_transition();
    let track = {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.next_track_manual().cloned()
    };
    if let Some(ref t) = track {
        play_track_ref(&state, &app_handle, t).await?;
        let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
        save_queue_state(&state, &queue_path);
        Ok(Some(t.clone()))
    } else {
        let _ = app_handle.emit("player:status", serde_json::json!({"playing": false, "track_id": null}));
        Ok(None)
    }
}

#[tauri::command]
pub async fn prev_track(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<TrackRef>, String> {
    let cur_pos = state.player.position() as f64;
    if cur_pos > 3.0 {
        state.player.seek(0.0);
        let _ = app_handle.emit("player:seek", 0.0);
        update_smtc_playback(&state, state.player.is_playing(), 0.0);
        let q = state.queue_state.lock().map_err(|e| e.to_string())?;
        return Ok(q.current_track().cloned());
    }
    let track = {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.prev_track().cloned()
    };
    if let Some(ref t) = track {
        play_track_ref(&state, &app_handle, t).await?;
        let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
        save_queue_state(&state, &queue_path);
        Ok(Some(t.clone()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn set_repeat_mode(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    mode: String,
) -> Result<(), String> {
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.set_repeat_mode(RepeatMode::from_str(&mode));
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    prime_next_track(&state, &app_handle).await;
    Ok(())
}

#[tauri::command]
pub async fn set_shuffle(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.set_shuffle(enabled);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[derive(Serialize)]
pub struct QueueStateResponse {
    pub queue: Vec<TrackRef>,
    pub queue_index: usize,
    pub repeat_mode: String,
    pub shuffle: bool,
}

#[tauri::command]
pub async fn get_queue_state(
    state: State<'_, AppState>,
) -> Result<QueueStateResponse, String> {
    let q = state.queue_state.lock().map_err(|e| e.to_string())?;
    Ok(QueueStateResponse {
        queue: q.original_list.clone(),
        queue_index: q.current_index,
        repeat_mode: q.repeat_mode.as_str().to_string(),
        shuffle: q.shuffle,
    })
}

#[tauri::command]
pub async fn remove_queue_track(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    index: usize,
) -> Result<(), String> {
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.remove_track(index);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[tauri::command]
pub async fn insert_queue_tracks(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tracks: Vec<Track>,
) -> Result<(), String> {
    let refs: Vec<TrackRef> = tracks.iter().map(track_to_ref).collect();
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.insert_after_current(refs);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[tauri::command]
pub async fn reorder_queue(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tracks: Vec<Track>,
) -> Result<(), String> {
    let refs: Vec<TrackRef> = tracks.iter().map(track_to_ref).collect();
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.reorder(refs);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[tauri::command]
pub async fn append_queue_tracks(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    tracks: Vec<Track>,
) -> Result<(), String> {
    let refs: Vec<TrackRef> = tracks.iter().map(track_to_ref).collect();
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.append_tracks(refs);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    Ok(())
}

#[tauri::command]
pub async fn record_play(state: State<'_, AppState>, id: String) -> Result<i64, String> {
    db::record_play(&state.db, &id).await
}

#[tauri::command]
pub async fn finish_play(state: State<'_, AppState>, history_id: i64, elapsed_secs: f64, duration: f64, end_reason: Option<String>) -> Result<(), String> {
    db::finish_play(&state.db, history_id, elapsed_secs, duration, end_reason.as_deref()).await
}

#[tauri::command]
pub async fn clear_play_history(state: State<'_, AppState>) -> Result<(), String> {
    db::clear_play_history(&state.db).await
}

#[tauri::command]
pub async fn get_recently_played_tracks(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<Track>, String> {
    db::get_recently_played_tracks(&state.db, limit).await
}

#[tauri::command]
pub async fn get_most_listened_tracks(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<Track>, String> {
    db::get_most_listened_tracks(&state.db, limit).await
}

#[tauri::command]
pub async fn get_least_listened_tracks(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<Track>, String> {
    db::get_least_listened_tracks(&state.db, limit).await
}

#[tauri::command]
pub async fn get_listening_stats(state: State<'_, AppState>) -> Result<db::ListeningStats, String> {
    db::get_listening_stats(&state.db).await
}

#[tauri::command]
pub async fn quit_app(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let track_id = state.current_track_id.lock().ok()
        .and_then(|g| g.clone());
    let pos = state.player.position() as f64;
    let vol = f32::from_bits(state.volume.load(std::sync::atomic::Ordering::Acquire));
    let (rm, sh, qi) = {
        if let Ok(q) = state.queue_state.lock() {
            (q.repeat_mode.as_str().to_string(), q.shuffle, q.current_index)
        } else {
            ("off".to_string(), false, 0)
        }
    };
    let gp = state.player.is_gapless();
    let cf = state.player.crossfade_seconds() as f64;
    let data = crate::session::SessionData {
        track_id, position: pos, volume: vol,
        repeat_mode: rm, shuffle: sh, queue_index: qi,
        gapless: gp, crossfade: cf,
    };
    crate::session::save_session(&app_handle, &data);
    crate::session::save_clean_exit(&app_handle);
    app_handle.exit(0);
    Ok(())
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn toggle_devtools(app_handle: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.open_devtools();
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_play(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let result = state.player.toggle();
    let _ = app_handle.emit("player:status", serde_json::json!({"playing": result}));
    update_smtc_playback(&state, result, state.player.position() as f64);
    Ok(result)
}

#[tauri::command]
pub async fn stop_playback(
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.player.stop();
    update_smtc_playback(&state, false, 0.0);
    Ok(())
}

#[tauri::command]
pub async fn seek(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    pos_secs: f64,
) -> Result<(), String> {
    state.player.seek(pos_secs as f32);
    let _ = app_handle.emit("player:status", serde_json::json!({"position": pos_secs}));
    update_smtc_playback(&state, state.player.is_playing(), pos_secs);
    Ok(())
}

#[tauri::command]
pub async fn set_volume(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    vol: f32,
) -> Result<(), String> {
    state.player.set_volume(vol);
    let _ = app_handle.emit("player:status", serde_json::json!({"volume": vol}));
    Ok(())
}

#[tauri::command]
pub async fn set_eq_band(
    state: State<'_, AppState>,
    index: u8,
    gain_db: f32,
) -> Result<(), String> {
    if index >= 10 {
        return Err("EQ band index must be 0-9".into());
    }
    state.player.set_eq_band(index, gain_db.clamp(-12.0, 12.0));
    Ok(())
}

#[tauri::command]
pub async fn set_eq_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    db::set_setting(&state.db, "eq_enabled", if enabled { "1" } else { "0" }).await?;
    state.player.set_eq_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub async fn set_karaoke(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    db::set_setting(&state.db, "karaoke_enabled", if enabled { "1" } else { "0" }).await?;
    state.player.set_karaoke(enabled);
    Ok(())
}

#[tauri::command]
pub async fn get_karaoke(state: State<'_, AppState>) -> Result<bool, String> {
    let enabled = db::get_setting(&state.db, "karaoke_enabled")
        .await?
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or_else(|| state.player.is_karaoke());
    Ok(enabled)
}

#[derive(Debug, Serialize)]
pub struct EqState {
    pub enabled: bool,
    pub preamp_db: f32,
}

#[tauri::command]
pub async fn get_eq_state(state: State<'_, AppState>) -> Result<EqState, String> {
    let enabled = db::get_setting(&state.db, "eq_enabled")
        .await?
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or(true);
    let preamp_db: f32 = db::get_setting(&state.db, "eq_preamp")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    Ok(EqState { enabled, preamp_db })
}

#[tauri::command]
pub async fn set_eq_preamp(state: State<'_, AppState>, gain_db: f32) -> Result<(), String> {
    let gain = gain_db.clamp(-12.0, 12.0);
    db::set_setting(&state.db, "eq_preamp", &gain.to_string()).await?;
    state.player.set_preamp(gain);
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioDevicesResponse {
    pub devices: Vec<String>,
    pub current: String,
    pub is_default: bool,
}

#[tauri::command]
pub async fn get_audio_devices(state: State<'_, AppState>) -> Result<AudioDevicesResponse, String> {
    let devices = crate::audio::output::list_output_devices();
    let current = state.player.current_output_device();
    let default = crate::audio::output::default_output_device_name().unwrap_or_default();
    let is_default = current == default || current == "Default Audio Device";
    Ok(AudioDevicesResponse {
        devices,
        current,
        is_default,
    })
}

#[tauri::command]
pub async fn set_audio_device(state: State<'_, AppState>, device: Option<String>) -> Result<(), String> {
    state.player.set_output_device(device.clone());
    let dev_str = device.unwrap_or_default();
    let _ = db::set_setting(&state.db, "current_output_device", &dev_str).await;
    Ok(())
}

#[tauri::command]
pub async fn get_device_sound_settings(
    state: State<'_, AppState>,
    device: String,
) -> Result<Option<serde_json::Value>, String> {
    let key = format!("device_sound_settings:{}", device);
    if let Ok(Some(json_str)) = db::get_setting(&state.db, &key).await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
            return Ok(Some(val));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn save_device_sound_settings(
    state: State<'_, AppState>,
    device: String,
    settings: serde_json::Value,
) -> Result<(), String> {
    let key = format!("device_sound_settings:{}", device);
    let json_str = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    db::set_setting(&state.db, &key, &json_str).await?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParametricEqStateResponse {
    pub filters: Vec<crate::library::eq::ParametricFilter>,
    pub preamp_db: f32,
    pub enabled: bool,
    pub mode: String,
    pub profile_name: String,
}

#[tauri::command]
pub async fn apply_parametric_eq(
    state: State<'_, AppState>,
    filters: Vec<crate::library::eq::ParametricFilter>,
    preamp_db: f32,
    enabled: bool,
    profile_name: Option<String>,
) -> Result<(), String> {
    let engine_filters: Vec<crate::audio::eq::Filter> = filters.iter().cloned().map(Into::into).collect();
    state.player.set_parametric_filters(engine_filters);
    state.player.set_preamp(preamp_db);
    state.player.set_eq_enabled(enabled);
    state.player.set_eq_mode("parametric");

    let json_filters = serde_json::to_string(&filters).map_err(|e| e.to_string())?;
    db::set_setting(&state.db, "eq_parametric_filters", &json_filters).await?;
    db::set_setting(&state.db, "eq_preamp", &preamp_db.to_string()).await?;
    db::set_setting(&state.db, "eq_enabled", if enabled { "1" } else { "0" }).await?;
    db::set_setting(&state.db, "eq_mode", "parametric").await?;
    if let Some(name) = profile_name {
        db::set_setting(&state.db, "eq_parametric_name", &name).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_eq_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    state.player.set_eq_mode(&mode);
    let _ = db::set_setting(&state.db, "eq_mode", &mode).await;
    Ok(())
}

#[tauri::command]
pub async fn get_parametric_eq_state(
    state: State<'_, AppState>,
) -> Result<ParametricEqStateResponse, String> {
    let filters_str = db::get_setting(&state.db, "eq_parametric_filters").await?.unwrap_or_default();
    let filters: Vec<crate::library::eq::ParametricFilter> = if !filters_str.is_empty() {
        serde_json::from_str(&filters_str).unwrap_or_default()
    } else {
        Vec::new()
    };
    let preamp_db: f32 = db::get_setting(&state.db, "eq_preamp")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let enabled = db::get_setting(&state.db, "eq_enabled")
        .await?
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or(true);
    let mode = db::get_setting(&state.db, "eq_mode")
        .await?
        .unwrap_or_else(|| "parametric".into());
    let profile_name = db::get_setting(&state.db, "eq_parametric_name")
        .await?
        .unwrap_or_else(|| "Pasted profile".into());

    Ok(ParametricEqStateResponse {
        filters,
        preamp_db,
        enabled,
        mode,
        profile_name,
    })
}

#[tauri::command]
pub async fn parse_autoeq_text(
    text: String,
) -> Result<crate::library::eq::AutoEqImportResult, String> {
    crate::library::eq::parse_autoeq(&text)
}

/// Apply the persisted EQ state to the audio engine: active profile bands,
/// enabled flag, and global preamp (Airmedy `ApplyActiveProfile` parity).
/// Called once at startup after the player thread is live.
pub async fn apply_eq_state(pool: &sqlx::SqlitePool, player: &crate::audio::player::Player) -> Result<(), String> {
    let mode = db::get_setting(pool, "eq_mode").await?.unwrap_or_else(|| "graphic".into());
    let enabled = db::get_setting(pool, "eq_enabled")
        .await?
        .map(|s| s != "0" && s.to_lowercase() != "false")
        .unwrap_or(true);
    let preamp: f32 = db::get_setting(pool, "eq_preamp")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    if mode == "parametric" {
        if let Ok(Some(filters_str)) = db::get_setting(pool, "eq_parametric_filters").await {
            if let Ok(filters) = serde_json::from_str::<Vec<crate::library::eq::ParametricFilter>>(&filters_str) {
                let engine_filters: Vec<crate::audio::eq::Filter> = filters.into_iter().map(Into::into).collect();
                player.set_parametric_filters(engine_filters);
                player.set_eq_mode("parametric");
            }
        }
    } else {
        if let Some(profile) = crate::library::eq::get_active(pool).await? {
            for (i, gain) in profile.bands.iter().enumerate() {
                player.set_eq_band(i as u8, *gain);
            }
        }
        player.set_eq_mode("graphic");
    }

    if let Ok(Some(dev)) = db::get_setting(pool, "current_output_device").await {
        if !dev.is_empty() {
            player.set_output_device(Some(dev));
        }
    }

    player.set_eq_enabled(enabled);
    player.set_preamp(preamp);
    Ok(())
}

#[derive(Serialize)]
pub struct PlayerStateResponse {
    pub track_id: Option<String>,
    pub is_playing: bool,
    pub position: f64,
    pub volume: f64,
    pub repeat_mode: String,
    pub shuffle: bool,
    pub queue_index: usize,
    pub queue_len: usize,
    pub gapless: bool,
    pub crossfade: f64,
}

#[tauri::command]
pub async fn get_position(state: State<'_, AppState>) -> Result<f64, String> {
    Ok(state.player.position() as f64)
}

#[tauri::command]
pub async fn get_player_state(state: State<'_, AppState>) -> Result<PlayerStateResponse, String> {
    let track_id = state.current_track_id.lock().map_err(|e| e.to_string())?.clone();
    let (repeat_mode, shuffle, queue_index, queue_len) = {
        let q = state.queue_state.lock().map_err(|e| e.to_string())?;
        (q.repeat_mode.as_str().to_string(), q.shuffle, q.current_index, q.len())
    };
    Ok(PlayerStateResponse {
        track_id,
        is_playing: state.player.is_playing(),
        position: state.player.position() as f64,
        volume: f32::from_bits(state.volume.load(std::sync::atomic::Ordering::Acquire)) as f64,
        repeat_mode,
        shuffle,
        queue_index,
        queue_len,
        gapless: state.player.is_gapless(),
        crossfade: state.player.crossfade_seconds() as f64,
    })
}

#[tauri::command]
pub async fn get_playlists(
    state: State<'_, AppState>,
) -> Result<Vec<playlist::Playlist>, String> {
    playlist::get_playlists(&state.db).await
}

#[tauri::command]
pub async fn create_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    is_smart: Option<bool>,
    smart_rules: Option<String>,
) -> Result<playlist::Playlist, String> {
    let is_smart = is_smart.unwrap_or(false);
    let smart_rules = smart_rules.as_deref();
    let p = playlist::create_playlist(&state.db, &name, is_smart, smart_rules).await?;
    if is_smart {
        if let Some(rules) = smart_rules {
            if !rules.is_empty() {
                let _ = crate::library::smart_playlist::refresh_smart_playlist(&state.db, &p.id).await;
            }
        }
    }
    let _ = app_handle.emit("playlist:created", &p);
    Ok(p)
}

#[tauri::command]
pub async fn get_playlist(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<playlist::Playlist>, String> {
    playlist::get_playlist(&state.db, &id).await
}

#[tauri::command]
pub async fn update_playlist_rules(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    smart_rules: Option<String>,
) -> Result<(), String> {
    playlist::update_playlist_rules(&state.db, &id, smart_rules.as_deref()).await?;
    let _ = crate::library::smart_playlist::refresh_smart_playlist(&state.db, &id).await;
    let _ = app_handle.emit("playlist:rules-updated", &id);
    Ok(())
}

#[tauri::command]
pub async fn refresh_smart_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<i32, String> {
    let count = crate::library::smart_playlist::refresh_smart_playlist(&state.db, &id).await?;
    let _ = app_handle.emit("playlist:tracks-changed", &id);
    Ok(count)
}

#[tauri::command]
pub async fn delete_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    playlist::delete_playlist(&state.db, &id).await?;
    let _ = app_handle.emit("playlist:deleted", &id);
    Ok(())
}

#[tauri::command]
pub async fn rename_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    playlist::rename_playlist(&state.db, &id, &name).await?;
    let _ = app_handle.emit("playlist:created", &serde_json::json!({"id": id, "name": name}));
    Ok(())
}

#[tauri::command]
pub async fn export_playlist_m3u(
    state: State<'_, AppState>,
    id: String,
    output_path: String,
) -> Result<(), String> {
    playlist::export_playlist_m3u(&state.db, &id, &output_path).await
}

#[tauri::command]
pub async fn import_playlist_m3u(
    state: State<'_, AppState>,
    file_path: String,
    name: String,
) -> Result<playlist::Playlist, String> {
    playlist::import_playlist_m3u(&state.db, &file_path, &name).await
}

#[tauri::command]
pub async fn export_library_backup(
    state: State<'_, AppState>,
    dest_path: String,
) -> Result<crate::library::backup::BackupSummary, String> {
    let path = std::path::Path::new(&dest_path);
    crate::library::backup::export_backup(&state.db, path).await
}

#[tauri::command]
pub async fn restore_library_backup(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    src_path: String,
) -> Result<crate::library::backup::RestoreSummary, String> {
    let path = std::path::Path::new(&src_path);
    let summary = crate::library::backup::restore_backup(&state.db, path).await?;
    let _ = app_handle.emit("library:sync-finished", ());
    Ok(summary)
}

#[tauri::command]
pub async fn get_playlist_track_count(
    state: State<'_, AppState>,
    id: String,
) -> Result<i32, String> {
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| format!("{e}"))?;
    Ok(count.0)
}

#[tauri::command]
pub async fn create_folder_playlist(
    state: State<'_, AppState>,
    name: String,
    folder_path: String,
) -> Result<playlist::Playlist, String> {
    playlist::create_folder_playlist(&state.db, &name, &folder_path).await
}

#[tauri::command]
pub async fn refresh_folder_playlists(
    state: State<'_, AppState>,
) -> Result<(), String> {
    playlist::refresh_all_folder_playlists(&state.db).await
}

#[tauri::command]
pub async fn reorder_playlist_tracks(
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<(), String> {
    let mut tx = state.db.begin().await.map_err(|e| format!("{e}"))?;
    for (i, track_id) in track_ids.iter().enumerate() {
        sqlx::query("UPDATE playlist_tracks SET position = ? WHERE playlist_id = ? AND track_id = ?")
            .bind(i as i32)
            .bind(&playlist_id)
            .bind(track_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to reorder: {e}"))?;
    }
    tx.commit().await.map_err(|e| format!("{e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn move_playlist_track(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    track_id: String,
    prev_track_id: Option<String>,
    next_track_id: Option<String>,
) -> Result<String, String> {
    let new_rank = playlist::move_playlist_track(
        &state.db,
        &playlist_id,
        &track_id,
        prev_track_id.as_deref(),
        next_track_id.as_deref(),
    )
    .await?;
    let _ = app_handle.emit("playlist:tracks-changed", &playlist_id);
    Ok(new_rank)
}

#[tauri::command]
pub async fn add_to_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    track_id: String,
) -> Result<(), String> {
    playlist::add_track_to_playlist(&state.db, &playlist_id, &track_id).await?;
    let _ = app_handle.emit("playlist:tracks-changed", &playlist_id);
    Ok(())
}

#[tauri::command]
pub async fn add_tracks_to_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<(), String> {
    for id in &track_ids {
        playlist::add_track_to_playlist(&state.db, &playlist_id, id).await?;
    }
    let _ = app_handle.emit("playlist:tracks-changed", &playlist_id);
    Ok(())
}

#[tauri::command]
pub async fn remove_from_playlist(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    track_id: String,
) -> Result<(), String> {
    playlist::remove_track_from_playlist(&state.db, &playlist_id, &track_id).await?;
    let _ = app_handle.emit("playlist:tracks-changed", &playlist_id);
    Ok(())
}

#[tauri::command]
pub async fn get_playlist_tracks(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<Vec<Track>, String> {
    playlist::get_playlist_tracks(&state.db, &playlist_id).await
}

#[tauri::command]
pub async fn get_favorite_ids(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    db::get_favorite_ids(&state.db).await
}

#[tauri::command]
pub async fn toggle_favorite(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let result = db::toggle_favorite(&state.db, &id).await?;
    let _ = app_handle.emit("library:track-updated", &id);
    Ok(result)
}

#[tauri::command]
pub async fn get_library_folders(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    db::get_library_folders(&state.db).await
}

#[tauri::command]
pub async fn add_library_folder(state: State<'_, AppState>, path: String) -> Result<(), String> {
    db::add_library_folder(&state.db, &path).await
}

#[tauri::command]
pub async fn add_library_folders(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<scanner::ScanResult, String> {
    if paths.is_empty() {
        return Ok(scanner::ScanResult { added: 0, updated: 0, removed: 0, track_ids: Vec::new() });
    }
    if state.is_scanning.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("A scan is already in progress".into());
    }
    let mut total_added: usize = 0;
    let mut total_removed: usize = 0;
    for path in &paths {
        let _ = app_handle.emit("folder-scan-progress", serde_json::json!({
            "path": path,
            "status": "scanning",
        }));
        let _ = db::add_library_folder(&state.db, path).await;
        match scanner::scan_folder(&state.db, Path::new(path), &state.track_artwork_cache).await {
            Ok(r) => {
                total_added += r.added;
                total_removed += r.removed;
                let _ = app_handle.emit("folder-scan-progress", serde_json::json!({
                    "path": path,
                    "status": "done",
                    "added": r.added,
                    "updated": r.updated,
                    "removed": r.removed,
                }));
            }
            Err(e) => {
                let _ = app_handle.emit("folder-scan-progress", serde_json::json!({
                    "path": path,
                    "status": "error",
                    "error": e,
                }));
            }
        }
    }
    state.is_scanning.store(false, std::sync::atomic::Ordering::SeqCst);
    let result = scanner::ScanResult { added: total_added, updated: 0, removed: total_removed, track_ids: Vec::new() };
    let tracks = db::get_all_tracks(&state.db).await?;
    rebuild_search_with_lyrics(&state.db, &state.search, &tracks).await;
    if let Ok(mut cache) = state.artwork_cache.lock() {
        cache.clear();
    }
    let _ = crate::library::smart_playlist::refresh_all_smart_playlists(&state.db).await;
    let _ = app_handle.emit("library:sync-finished", &result);
    let _ = run_genre_enrichment(app_handle.clone(), state.db.clone(), state.is_fetching_genre.clone(), Some(100)).await;
    let _ = run_audio_analysis(app_handle.clone(), state.db.clone(), state.is_analyzing.clone()).await;
    Ok(result)
}

#[tauri::command]
pub async fn remove_library_folder(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    db::remove_library_folder(&state.db, &path).await?;
    let _ = crate::library::smart_playlist::refresh_all_smart_playlists(&state.db).await;
    if let Ok(tracks) = db::get_all_tracks(&state.db).await {
        rebuild_search_with_lyrics(&state.db, &state.search, &tracks).await;
    }
    let _ = app_handle.emit("library:sync-finished", serde_json::json!({"removed_folder": path}));
    Ok(())
}
#[tauri::command]
pub async fn analyze_audio(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    run_audio_analysis(app_handle, state.db.clone(), state.is_analyzing.clone()).await
}

pub async fn run_audio_analysis(
    app_handle: tauri::AppHandle,
    db: sqlx::SqlitePool,
    is_analyzing: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<String, String> {
    use std::sync::atomic::Ordering;

    if is_analyzing.swap(true, Ordering::SeqCst) {
        // Let the auto-scan pipeline's waitForEvent resolve instead of hanging
        let _ = app_handle.emit("audio-analysis-done", serde_json::json!({"analyzed": 0, "already_running": true}));
        return Ok("already_running".to_string());
    }

    let tracks = match db::get_tracks_needing_analysis(&db).await {
        Ok(t) => t,
        Err(e) => {
            is_analyzing.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    let total = tracks.len();

    if total == 0 {
        is_analyzing.store(false, Ordering::SeqCst);
        let _ = app_handle.emit("audio-analysis-done", serde_json::json!({"analyzed": 0}));
        return Ok("nothing_to_do".to_string());
    }

    let guard = is_analyzing.clone();
    let app = app_handle.clone();

    tauri::async_runtime::spawn(async move {
        let mut analyzed_count = 0u32;
        for (i, track) in tracks.iter().enumerate() {
            let file_name = std::path::Path::new(&track.file_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = std::path::PathBuf::from(&track.file_path);
            let duration = track.duration;
            let track_title = if !track.title.is_empty() { track.title.clone() } else { file_name.clone() };

            let (analysis, fail_reason) = tokio::task::spawn_blocking(move || {
                if !path.exists() {
                    return (None, Some("file_missing"));
                }
                if duration < 3.0 {
                    return (None, Some("too_short"));
                }
                match std::panic::catch_unwind(|| crate::audio::analyzer::analyze(&path)) {
                    Ok(result) => {
                        if result.bpm == 0.0 && result.musical_key.is_empty() {
                            (None, Some("unsupported_or_empty"))
                        } else {
                            (Some((result.bpm, result.musical_key, result.energy)), None)
                        }
                    }
                    Err(_) => {
                        (None, Some("panic"))
                    }
                }
            }).await.unwrap_or((None, Some("task_error")));

            match analysis {
                Some((bpm, key, energy)) => {
                    analyzed_count += 1;
                    let _ = sqlx::query("UPDATE tracks SET bpm = ?, musical_key = ?, energy = ?, analysis_failed_reason = NULL WHERE id = ?")
                        .bind(bpm).bind(&key).bind(energy).bind(&track.id)
                        .execute(&db).await;
                    let _ = app.emit("audio-analysis-progress", serde_json::json!({
                        "processed": i + 1,
                        "total": total,
                        "current": track_title,
                        "status": "analyzed",
                        "bpm": bpm,
                        "key": key,
                    }));
                }
                None => {
                    let reason = fail_reason.unwrap_or("unsupported");
                    let _ = sqlx::query("UPDATE tracks SET analysis_failed_reason = ? WHERE id = ?")
                        .bind(reason)
                        .bind(&track.id)
                        .execute(&db).await;
                    let _ = app.emit("audio-analysis-progress", serde_json::json!({
                        "processed": i + 1,
                        "total": total,
                        "current": track_title,
                        "status": "skipped",
                    }));
                }
            }
        }

        guard.store(false, Ordering::SeqCst);
        let _ = app.emit("audio-analysis-done", serde_json::json!({"analyzed": analyzed_count}));
        let _ = crate::library::smart_playlist::refresh_all_smart_playlists(&db).await;
    });

    Ok("started".to_string())
}

#[tauri::command]
pub async fn batch_fetch_artist_artwork(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    retry_missing: Option<bool>,
) -> Result<String, String> {
    if retry_missing.unwrap_or(false) {
        let reset = db::reset_artist_artwork_not_found(&state.db).await?;
        if reset > 0 {
            log::info!("Re-arming {reset} artists for artwork retry");
        }
    }

    let db = state.db.clone();
    let worker = state.artist_artwork_worker.clone();

    tauri::async_runtime::spawn(async move {
        let artists = match db::get_unique_artists(&db).await {
            Ok(a) => a,
            Err(e) => {
                log::error!("Failed to fetch artists: {e}");
                let _ = app_handle.emit("artist-artwork-done", serde_json::json!({"processed": 0}));
                return;
            }
        };
        let total = artists.len();

        if total == 0 {
            let _ = app_handle.emit("artist-artwork-done", serde_json::json!({"processed": total}));
            return;
        }

        for artist in &artists {
            worker.enqueue(artist.clone()).await;
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let remaining = worker.in_flight_count().await;
            let processed = total - remaining;
            let _ = app_handle.emit("artist-artwork-progress", serde_json::json!({
                "processed": processed,
                "total": total,
                "current": "",
            }));
            if remaining == 0 {
                break;
            }
        }

        let _ = app_handle.emit("artist-artwork-done", serde_json::json!({"processed": total}));
    });

    Ok("started".to_string())
}

#[tauri::command]
pub async fn delete_tracks(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    delete_files: bool,
) -> Result<(), String> {
    let file_paths = db::delete_tracks(&state.db, &ids).await?;
    if delete_files {
        for path in &file_paths {
            let _ = fs::remove_file(path);
        }
    }
    let mut search = state.search.lock().map_err(|e| e.to_string())?;
    for id in &ids {
        let _ = search.remove_track(id);
    }
    let _ = search.commit();
    let _ = app_handle.emit("library:track-deleted", &ids);
    Ok(())
}

#[tauri::command]
pub async fn get_lyrics(file_path: String) -> Result<Option<String>, String> {
    let p = Path::new(&file_path);
    let base = p.with_extension("");
    for ext in &["lrc", "txt"] {
        let lyric_path = base.with_extension(ext);
        if lyric_path.exists() {
            return fs::read_to_string(&lyric_path)
                .map(Some)
                .map_err(|e| format!("Failed to read lyrics: {e}"));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn show_in_explorer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let track = db::get_track(&state.db, &id)
        .await?
        .ok_or("Track not found")?;
    let path = std::path::PathBuf::from(&track.file_path);
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open finder: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("Failed to open file manager: {e}"))?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LyricsResult {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub lyrics: String,
    pub source: String,
}

#[tauri::command]
pub async fn find_lyrics(
    state: State<'_, AppState>,
    title: String,
    artist: String,
    album: String,
    save: Option<String>,
) -> Result<Vec<LyricsResult>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut results = Vec::new();

    // Try LRCLIB
    let lrclib_url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}",
        urlencoding(&artist),
        urlencoding(&title),
        urlencoding(&album),
    );
    if let Ok(resp) = client.get(&lrclib_url).send().await {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(lyrics) = data["syncedLyrics"].as_str().filter(|l| !l.is_empty())
                    .or_else(|| data["plainLyrics"].as_str().filter(|l| !l.is_empty()))
                {
                    results.push(LyricsResult {
                        title: data["trackName"].as_str().unwrap_or(&title).to_string(),
                        artist: data["artistName"].as_str().unwrap_or(&artist).to_string(),
                        album: data["albumName"].as_str().unwrap_or(&album).to_string(),
                        lyrics: lyrics.to_string(),
                        source: "LRCLIB".to_string(),
                    });
                }
            }
        }
    }

    // Try KuGou (if LRCLIB returned nothing)
    if results.is_empty() {
        let search_url = format!(
            "https://lyrics.kugou.com/search?ver=1&man=yes&client=pc&keyword={}%20{}&duration=&hash=",
            urlencoding(&title),
            urlencoding(&artist),
        );
        if let Ok(resp) = client.get(&search_url).send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(candidates) = data["candidates"].as_array() {
                        if let Some(first) = candidates.first() {
                            if let (Some(id), Some(access_token)) = (
                                first["id"].as_str(),
                                first["access_token"].as_str().or(Some("")),
                            ) {
                                let dl_url = format!(
                                    "https://lyrics.kugou.com/download?ver=1&client=pc&id={id}&accessToken={access_token}&fmt=lrc"
                                );
                                if let Ok(dl_resp) = client.get(&dl_url).send().await {
                                    if dl_resp.status().is_success() {
                                        if let Ok(dl_data) = dl_resp.json::<serde_json::Value>().await {
                                            if let Some(content) = dl_data["content"].as_str() {
                                                if let Ok(decoded) = base64_decode(content) {
                                                    if let Ok(lyrics) = String::from_utf8(decoded) {
                                                        let lrc_text = if lyrics.trim().starts_with("[ti:")
                                                            || lyrics.trim().contains("[00:")
                                                        {
                                                            lyrics
                                                        } else {
                                                            // If not LRC format, wrap as plain text
                                                            lyrics
                                                        };
                                                        results.push(LyricsResult {
                                                            title: first["song_name"].as_str().unwrap_or(&title).to_string(),
                                                            artist: first["singer_name"].as_str().unwrap_or(&artist).to_string(),
                                                            album: String::new(),
                                                            lyrics: lrc_text,
                                                            source: "KuGou".to_string(),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(ref track_id) = save {
        if let Some(first) = results.first() {
            let source = if first.lyrics.contains('[') && first.lyrics.contains(']') {
                "provider-synced"
            } else {
                "provider-plain"
            };
            let _ = db::upsert_lyrics(&state.db, track_id, &first.lyrics, source, "", "").await;
        }
    }

    Ok(results)
}

fn save_lyrics_file(cache_root: &std::path::Path, track_id: &str, content: &str) {
    let safe_id = crate::security::sanitize_filename(track_id);
    if safe_id.is_empty() {
        return;
    }
    let dir = cache_root.join("lyrics");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{safe_id}.lrc"));
    let _ = std::fs::write(&path, content);
}

#[tauri::command]
pub async fn save_lyrics(
    state: State<'_, AppState>,
    track_id: String,
    content: String,
    source: String,
) -> Result<(), String> {
    db::upsert_lyrics(&state.db, &track_id, &content, &source, "", "").await?;
    let root = state.cache_root.lock().unwrap().clone();
    save_lyrics_file(&root, &track_id, &content);
    if let Ok(Some(track)) = db::get_track(&state.db, &track_id).await {
        if let Ok(mut search) = state.search.lock() {
            let _ = search.index_track_with_lyrics(&track, Some(&content));
            let _ = search.commit();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_track_lyrics(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<Option<db::LyricRow>, String> {
    db::get_lyrics(&state.db, &track_id).await
}

#[tauri::command]
pub async fn get_all_lyrics(state: State<'_, AppState>) -> Result<Vec<db::LyricRow>, String> {
    db::get_all_lyrics(&state.db).await
}

#[tauri::command]
pub async fn batch_fetch_lyrics(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    folder_path: Option<String>,
    retry_missing: Option<bool>,
) -> Result<String, String> {
    if retry_missing.unwrap_or(false) {
        let reset = db::reset_lyrics_not_found(&state.db).await?;
        if reset > 0 {
            log::info!("Re-arming {reset} tracks for lyrics retry");
        }
    }

    if state.is_fetching.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let _ = app_handle.emit("batch-lyrics-all-done", serde_json::json!({"already_running": true}));
        return Ok("already_running".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let db = state.db.clone();
    let guard = state.is_fetching.clone();
    let cache_root = state.cache_root.lock().unwrap().clone();

    tauri::async_runtime::spawn(async move {
        let tracks = match db::get_all_tracks(&db).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("Failed to get tracks in background fetch: {e}");
                let _ = app_handle.emit("batch-lyrics-all-done", serde_json::json!({"error": e}));
                guard.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        };

        let folders: Vec<String> = if let Some(ref fp) = folder_path {
            vec![fp.clone()]
        } else {
            match db::get_library_folders(&db).await {
                Ok(f) => f,
                Err(e) => {
                    log::error!("Failed to get folders in background fetch: {e}");
                    let _ = app_handle.emit("batch-lyrics-all-done", serde_json::json!({"error": e}));
                    guard.store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
            }
        };

        for folder in &folders {
            let folder_tracks: Vec<_> = tracks.iter().filter(|t| t.file_path.starts_with(folder)).collect();
            let total = folder_tracks.len();
            if total == 0 {
                continue;
            }

            let mut fetched = 0u32;
            let mut skipped_already_have = 0u32;
            let mut skipped_not_found = 0u32;

            for (i, track) in folder_tracks.iter().enumerate() {
                let existing = match db::get_lyrics(&db, &track.id).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let has_content = existing.as_ref().map_or(false, |r| {
                    !r.content.is_empty() || !r.meta_content.is_empty()
                });
                let is_not_found = existing.as_ref().map_or(false, |r| r.source == "not-found");

                if has_content {
                    skipped_already_have += 1;
                    continue;
                }
                if is_not_found {
                    skipped_not_found += 1;
                    continue;
                }

                let p = std::path::Path::new(&track.file_path);
                let base = p.with_extension("");
                let mut sibling_found = false;
                for ext in &["lrc", "txt"] {
                    let lyric_path = base.with_extension(ext);
                    if lyric_path.exists() {
                        if let Ok(text) = std::fs::read_to_string(&lyric_path) {
                            if !text.trim().is_empty() {
                                let _ = db::upsert_lyrics(&db, &track.id, "", "", &text, "sibling-file").await;
                                sibling_found = true;
                                skipped_already_have += 1;
                                break;
                            }
                        }
                    }
                }
                if sibling_found {
                    continue;
                }

                app_handle.emit("batch-lyrics-progress", serde_json::json!({
                    "current": i + 1,
                    "total": total,
                    "track": track.title,
                    "status": "searching",
                    "folder": folder,
                })).ok();

                let mut found: Option<(String, String)> = None;

                let lrclib_url = format!(
                    "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}",
                    urlencoding(&track.artist),
                    urlencoding(&track.title),
                    urlencoding(&track.album),
                );
                if let Ok(resp) = client.get(&lrclib_url).send().await {
                    if resp.status().is_success() {
                        if let Ok(data) = resp.json::<serde_json::Value>().await {
                            if let Some(lyrics) = data["syncedLyrics"].as_str()
                                .or_else(|| data["plainLyrics"].as_str())
                                .filter(|l| !l.is_empty())
                            {
                                let source = if lyrics.contains('[') && lyrics.contains(']') {
                                    "lrclib-synced"
                                } else {
                                    "lrclib-plain"
                                };
                                found = Some((lyrics.to_string(), source.to_string()));
                            }
                        }
                    }
                }

                if found.is_none() {
                    let search_url = format!(
                        "https://lyrics.kugou.com/search?ver=1&man=yes&client=pc&keyword={}%20{}&duration=&hash=",
                        urlencoding(&track.title),
                        urlencoding(&track.artist),
                    );
                    if let Ok(resp) = client.get(&search_url).send().await {
                        if resp.status().is_success() {
                            if let Ok(data) = resp.json::<serde_json::Value>().await {
                                if let Some(candidates) = data["candidates"].as_array() {
                                    if let Some(first) = candidates.first() {
                                        if let (Some(id), Some(access_token)) = (
                                            first["id"].as_str(),
                                            first["access_token"].as_str().or(Some("")),
                                        ) {
                                            let dl_url = format!(
                                                "https://lyrics.kugou.com/download?ver=1&client=pc&id={id}&accessToken={access_token}&fmt=lrc"
                                            );
                                            if let Ok(dl_resp) = client.get(&dl_url).send().await {
                                                if dl_resp.status().is_success() {
                                                    if let Ok(dl_data) = dl_resp.json::<serde_json::Value>().await {
                                                        if let Some(content) = dl_data["content"].as_str() {
                                                            if let Ok(decoded) = base64_decode(content) {
                                                                if let Ok(lyrics) = String::from_utf8(decoded) {
                                                                    let source = if lyrics.contains('[') && lyrics.contains(']') {
                                                                        "kugou-synced"
                                                                    } else {
                                                                        "kugou-plain"
                                                                    };
                                                                    found = Some((lyrics, source.to_string()));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let was_found = found.is_some();
                if let Some((lyrics, source)) = found {
                    let _ = db::upsert_lyrics(&db, &track.id, &lyrics, &source, "", "").await;
                    save_lyrics_file(&cache_root, &track.id, &lyrics);
                    fetched += 1;
                } else {
                    let _ = db::upsert_lyrics(&db, &track.id, "", "not-found", "", "").await;
                    skipped_not_found += 1;
                }

                app_handle.emit("batch-lyrics-progress", serde_json::json!({
                    "current": i + 1,
                    "total": total,
                    "track": track.title,
                    "status": if was_found { "found" } else { "not_found" },
                    "folder": folder,
                })).ok();

                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }

            app_handle.emit("batch-lyrics-folder-done", serde_json::json!({
                "folder": folder,
                "fetched": fetched,
                "skipped_already_have": skipped_already_have,
                "skipped_not_found": skipped_not_found,
                "total": total,
            })).ok();
        }

        app_handle.emit("batch-lyrics-all-done", serde_json::json!({})).ok();
        guard.store(false, std::sync::atomic::Ordering::SeqCst);
    });

    Ok("started".to_string())
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("Base64 decode error: {e}"))
}

fn urlencoding(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*b as char);
            }
            b' ' => encoded.push_str("%20"),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

#[derive(Debug, Deserialize)]
pub struct UpdateMetadataInput {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: i32,
    pub track_number: u32,
    pub disc_number: u32,
}

#[tauri::command]
pub async fn update_track_metadata(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    input: UpdateMetadataInput,
) -> Result<(), String> {
    let track = db::get_track(&state.db, &input.id)
        .await?
        .ok_or("Track not found")?;

    // Update database
    let mut t = track.clone();
    t.title = input.title;
    t.artist = input.artist;
    t.album = input.album;
    t.album_artist = input.album_artist;
    t.genre = input.genre.clone();
    t.raw_genre_names = input.genre;
    t.year = input.year;
    t.track_number = input.track_number;
    t.disc_number = input.disc_number;
    db::upsert_track(&state.db, &mut t).await?;

    // Re-process genres
    use crate::library::track::{normalize_genre_name, split_genre_string, Genre as GenreModel};
    if !t.raw_genre_names.is_empty() {
        let genre_names = split_genre_string(&t.raw_genre_names);
        let mut genre_ids: Vec<String> = Vec::new();
        for name in &genre_names {
            let nk = normalize_genre_name(name);
            let gid = format!("genre-{}", nk);
            let genre = GenreModel {
                id: gid.clone(),
                name: name.clone(),
                normalization_key: nk,
                track_count: 0,
            };
            let _ = db::upsert_genre(&mut *state.db.acquire().await.map_err(|e| e.to_string())?, &genre).await;
            genre_ids.push(gid);
        }
        let _ = db::set_track_genres(&mut *state.db.acquire().await.map_err(|e| e.to_string())?, &t.id, &genre_ids).await;
    }

    // Update search index incrementally
    let mut search = state.search.lock().map_err(|e| e.to_string())?;
    let _ = search.index_track(&t);
    // Commit so metadata edits are visible in search immediately
    let _ = search.commit();

    let _ = app_handle.emit("library:track-updated", &input.id);
    Ok(())
}

#[derive(Serialize)]
pub struct SearchAllResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<SearchAlbumResult>,
    pub artists: Vec<String>,
    pub genres: Vec<String>,
    pub composers: Vec<String>,
}

#[derive(Serialize)]
pub struct SearchAlbumResult {
    pub name: String,
    pub artist: String,
    pub year: i32,
    pub track_count: i64,
    pub first_track_id: String,
}

#[tauri::command]
pub async fn search_all(
    state: State<'_, AppState>,
    query: String,
) -> Result<SearchAllResults, String> {
    let q = format!("%{}%", query.to_lowercase());

    // Search tracks (use existing search index)
    let hits = {
        let search = state.search.lock().map_err(|e| e.to_string())?;
        search.search(&query, 50)
    };
    let mut tracks = Vec::new();
    for hit in hits {
        if let Ok(Some(mut t)) = db::get_track(&state.db, &hit.track_id).await {
            t.matched_lyric = hit.matched_lyric;
            tracks.push(t);
        }
    }

    // Search albums
    let albums = sqlx::query_as::<_, (String, String, i32, i64, String)>(
        "SELECT album, COALESCE(album_artist, artist, ''), MAX(year) as year, COUNT(*) as track_count, MIN(id) as first_track_id
         FROM tracks WHERE LOWER(album) LIKE ?1 OR LOWER(album_artist) LIKE ?1 OR LOWER(artist) LIKE ?1
         GROUP BY album ORDER BY album"
    )
    .bind(&q)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("{e}"))?
    .into_iter()
    .map(|(name, artist, year, count, first_id)| SearchAlbumResult {
        name, artist, year, track_count: count, first_track_id: first_id,
    })
    .collect::<Vec<_>>();

    // Search artists
    let artists: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT artist FROM tracks WHERE LOWER(artist) LIKE ?1 AND artist != '' ORDER BY artist LIMIT 20"
    )
    .bind(&q)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("{e}"))?;

    // Search genres
    let genres: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM genres WHERE LOWER(name) LIKE ?1 ORDER BY name LIMIT 20"
    )
    .bind(&q)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("{e}"))?;

    // Search composers
    let composers: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT composer FROM tracks WHERE LOWER(composer) LIKE ?1 AND composer != '' ORDER BY composer LIMIT 20"
    )
    .bind(&q)
    .fetch_all(&state.db)
    .await
    .map_err(|e| format!("{e}"))?;

    Ok(SearchAllResults { tracks, albums, artists, genres, composers })
}

#[tauri::command]
pub async fn get_playlists_for_track(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<Vec<String>, String> {
    playlist::get_playlists_for_track(&state.db, &track_id).await
}

use std::sync::Mutex;

static POWER_HANDLE: Mutex<Option<crate::power::PowerHandle>> = Mutex::new(None);

#[tauri::command]
pub async fn prevent_sleep() -> Result<(), String> {
    if let Ok(mut handle) = POWER_HANDLE.lock() {
        if handle.is_none() {
            *handle = crate::power::prevent_sleep();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn allow_sleep() -> Result<(), String> {
    if let Ok(mut handle) = POWER_HANDLE.lock() {
        let h = handle.take();
        drop(handle);
        crate::power::allow_sleep(h);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_artist_artwork(
    state: State<'_, AppState>,
    artist: String,
    size: Option<String>,
) -> Result<Option<Vec<u8>>, String> {
    let (key, checked) = db::get_artist_artwork_state(&state.db, &artist).await?;
    if let Some(k) = key {
        let variant = size.as_deref();
        if state.artist_artwork_cache.exists(&k, variant) {
            return state.artist_artwork_cache.read(&k, variant).map(Some);
        }
        if variant.is_some() {
            if let Ok(data) = state.artist_artwork_cache.read(&k, None) {
                return Ok(Some(data));
            }
        }
        // Key exists but the cached image file is gone — re-fetch it.
        state.artist_artwork_worker.enqueue(artist).await;
        return Ok(None);
    }
    if !checked {
        state.artist_artwork_worker.enqueue(artist).await;
    }
    Ok(None)
}

#[tauri::command]
pub async fn remote_start(app_handle: tauri::AppHandle, state: State<'_, AppState>, port: u16, password: String) -> Result<(), String> {
    if password.chars().count() < 4 {
        return Err("Remote password must be at least 4 characters".to_string());
    }
    state.remote.start(
        app_handle,
        state.player.clone(),
        state.db.clone(),
        state.queue_state.clone(),
        state.volume.clone(),
        port,
        password,
    );
    Ok(())
}

#[tauri::command]
pub async fn remote_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.remote.stop();
    Ok(())
}

#[tauri::command]
pub async fn remote_status(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.remote.is_running())
}

async fn load_lastfm_config(state: &AppState) -> crate::lastfm::ScrobbleConfig {
    let username = db::get_setting(&state.db, "lastfm_username")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let session_key = db::get_setting(&state.db, "lastfm_session_key")
        .await
        .ok()
        .flatten()
        .and_then(|v| crate::security::decrypt_secret(&v).ok())
        .unwrap_or_default();
    let enabled = db::get_setting(&state.db, "lastfm_enabled")
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let api_key = db::get_setting(&state.db, "lastfm_api_key")
        .await
        .ok()
        .flatten()
        .and_then(|v| crate::security::decrypt_secret(&v).ok())
        .unwrap_or_default();
    let api_secret = db::get_setting(&state.db, "lastfm_api_secret")
        .await
        .ok()
        .flatten()
        .and_then(|v| crate::security::decrypt_secret(&v).ok())
        .unwrap_or_default();
    crate::lastfm::ScrobbleConfig {
        username,
        session_key,
        enabled,
        api_key,
        api_secret,
    }
}

#[tauri::command]
pub async fn now_playing(
    state: State<'_, AppState>,
    artist: String,
    track: String,
    album: String,
    duration: u32,
) -> Result<(), String> {
    let config = load_lastfm_config(&state).await;
    let client = reqwest::Client::new();
    crate::lastfm::now_playing(&config, &artist, &track, &album, duration, &client).await
}

#[tauri::command]
pub async fn scrobble(
    state: State<'_, AppState>,
    artist: String,
    track: String,
    album: String,
) -> Result<(), String> {
    let config = load_lastfm_config(&state).await;
    let client = reqwest::Client::new();
    let timestamp = chrono::Utc::now().timestamp();
    crate::lastfm::scrobble(&config, &artist, &track, &album, timestamp, &client).await
}

#[tauri::command]
pub async fn get_lastfm_auth_url(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let config = load_lastfm_config(&state).await;
    Ok(crate::lastfm::get_auth_url(&config.api_key))
}

#[tauri::command]
pub async fn get_lastfm_config(
    state: State<'_, AppState>,
) -> Result<crate::lastfm::ScrobbleConfig, String> {
    Ok(load_lastfm_config(&state).await)
}

#[tauri::command]
pub async fn set_lastfm_session(
    state: State<'_, AppState>,
    token: String,
) -> Result<crate::lastfm::ScrobbleConfig, String> {
    let config = load_lastfm_config(&state).await;
    let client = reqwest::Client::new();
    let (username, session_key) = crate::lastfm::get_session(&token, &config, &client).await?;
    let enc_session = crate::security::encrypt_secret(&session_key).unwrap_or_else(|_| session_key.clone());
    db::set_setting(&state.db, "lastfm_username", &username).await?;
    db::set_setting(&state.db, "lastfm_session_key", &enc_session).await?;
    db::set_setting(&state.db, "lastfm_enabled", "true").await?;
    Ok(crate::lastfm::ScrobbleConfig {
        username,
        session_key,
        enabled: true,
        api_key: config.api_key,
        api_secret: config.api_secret,
    })
}

#[tauri::command]
pub async fn set_lastfm_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    db::set_setting(&state.db, "lastfm_enabled", if enabled { "true" } else { "false" }).await
}

#[tauri::command]
pub async fn disconnect_lastfm(
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::set_setting(&state.db, "lastfm_username", "").await?;
    db::set_setting(&state.db, "lastfm_session_key", "").await?;
    db::set_setting(&state.db, "lastfm_enabled", "false").await
}

#[derive(Serialize, Deserialize)]
pub struct ApiKeyValidation {
    pub valid: bool,
    pub message: String,
}

#[tauri::command]
pub async fn set_lastfm_api_key(
    state: State<'_, AppState>,
    api_key: String,
    api_secret: String,
) -> Result<(), String> {
    let enc_key = crate::security::encrypt_secret(&api_key).unwrap_or_else(|_| api_key.clone());
    let enc_secret = crate::security::encrypt_secret(&api_secret).unwrap_or_else(|_| api_secret.clone());
    db::set_setting(&state.db, "lastfm_api_key", &enc_key).await?;
    db::set_setting(&state.db, "lastfm_api_secret", &enc_secret).await?;
    Ok(())
}

#[tauri::command]
pub async fn validate_lastfm_api_key(
    api_key: String,
) -> Result<ApiKeyValidation, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    match crate::lastfm::validate_api_key(&api_key, &client).await {
        Ok(msg) => Ok(ApiKeyValidation { valid: true, message: msg }),
        Err(msg) => Ok(ApiKeyValidation { valid: false, message: msg }),
    }
}

#[tauri::command]
pub async fn get_radio_stations(state: State<'_, AppState>) -> Result<Vec<crate::radio::RadioStation>, String> {
    db::get_radio_stations(&state.db).await
}

#[tauri::command]
pub async fn add_radio_station(state: State<'_, AppState>, station: crate::radio::RadioStation) -> Result<(), String> {
    db::add_radio_station(&state.db, &station).await
}

#[tauri::command]
pub async fn delete_radio_station(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::delete_radio_station(&state.db, &id).await
}

#[tauri::command]
pub async fn update_radio_station(state: State<'_, AppState>, station: crate::radio::RadioStation) -> Result<(), String> {
    db::update_radio_station(&state.db, &station).await
}

#[tauri::command]
pub async fn play_radio(state: State<'_, AppState>, app_handle: tauri::AppHandle, url: String) -> Result<(), String> {
    if state.player.is_radio_active() {
        state.player.stop_radio();
    }
    state.player.play_radio(url, app_handle);
    Ok(())
}

#[tauri::command]
pub async fn ping_audio(state: State<'_, AppState>) -> Result<(), String> {
    state.player.ping()
}

#[tauri::command]
pub async fn is_audio_ready(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.player.is_ready())
}

#[tauri::command]
pub async fn stop_radio(state: State<'_, AppState>) -> Result<(), String> {
    state.player.stop_radio();
    Ok(())
}

#[tauri::command]
pub async fn radio_status(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.player.is_radio_active())
}

// --- EQ Profile Commands ---

#[derive(Serialize)]
pub struct EQProfileResult {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_active: bool,
    pub bands: Vec<f32>,
}

impl From<crate::library::eq::EQProfile> for EQProfileResult {
    fn from(p: crate::library::eq::EQProfile) -> Self {
        Self {
            id: p.id,
            name: p.name,
            is_default: p.is_default,
            is_active: p.is_active,
            bands: p.bands.to_vec(),
        }
    }
}

#[tauri::command]
pub async fn get_eq_profiles(state: State<'_, AppState>) -> Result<Vec<EQProfileResult>, String> {
    let profiles = crate::library::eq::get_all(&state.db).await?;
    Ok(profiles.into_iter().map(|p| p.into()).collect())
}

#[tauri::command]
pub async fn get_active_eq_profile(state: State<'_, AppState>) -> Result<Option<EQProfileResult>, String> {
    let profile = crate::library::eq::get_active(&state.db).await?;
    Ok(profile.map(|p| p.into()))
}

#[tauri::command]
pub async fn create_eq_profile(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> Result<EQProfileResult, String> {
    let profile = crate::library::eq::create(&state.db, &name).await?;
    let _ = app_handle.emit("eq:profiles-updated", ());
    Ok(profile.into())
}

#[tauri::command]
pub async fn rename_eq_profile(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
    name: String,
) -> Result<(), String> {
    crate::library::eq::rename(&state.db, &id, &name).await?;
    let _ = app_handle.emit("eq:profiles-updated", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_eq_profile(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    crate::library::eq::delete(&state.db, &id).await?;
    let _ = app_handle.emit("eq:profiles-updated", ());
    Ok(())
}

#[tauri::command]
pub async fn apply_eq_profile(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    crate::library::eq::set_active(&state.db, &id).await?;
    let profile = crate::library::eq::get_by_id(&state.db, &id)
        .await?
        .ok_or("Profile not found")?;
    for (i, gain) in profile.bands.iter().enumerate() {
        state.player.set_eq_band(i as u8, *gain);
    }
    db::set_setting(&state.db, "eq_enabled", "1").await?;
    state.player.set_eq_enabled(true);
    let _ = app_handle.emit("eq:active-profile-changed", &id);
    Ok(())
}

#[tauri::command]
pub async fn update_eq_band(state: State<'_, AppState>, profile_id: String, index: u8, gain_db: f32) -> Result<(), String> {
    if index >= 10 {
        return Err(format!("EQ band index out of range: {index}"));
    }
    crate::library::eq::update_band(&state.db, &profile_id, index, gain_db).await?;
    let profile = crate::library::eq::get_by_id(&state.db, &profile_id)
        .await?
        .ok_or("Profile not found")?;
    if profile.is_active {
        state.player.set_eq_band(index, gain_db.clamp(-12.0, 12.0));
    }
    Ok(())
}

#[tauri::command]
pub async fn import_autoeq_profile(
    state: State<'_, AppState>,
    name: String,
    text: String,
) -> Result<EQProfileResult, String> {
    let profile = crate::library::eq::import_autoeq(&state.db, &name, &text).await?;
    Ok(profile.into())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CrossfeedPayload {
    pub enabled: bool,
    pub level: String, // "default", "cmoy", "meier"
}

#[tauri::command]
pub async fn get_crossfeed_settings(
    state: State<'_, AppState>,
) -> Result<CrossfeedPayload, String> {
    let enabled_str = db::get_setting(&state.db, "crossfeed_enabled").await?.unwrap_or_else(|| "0".into());
    let level_str = db::get_setting(&state.db, "crossfeed_level").await?.unwrap_or_else(|| "default".into());
    Ok(CrossfeedPayload {
        enabled: enabled_str == "1",
        level: level_str,
    })
}

#[tauri::command]
pub async fn set_crossfeed_settings(
    state: State<'_, AppState>,
    settings: CrossfeedPayload,
) -> Result<(), String> {
    db::set_setting(&state.db, "crossfeed_enabled", if settings.enabled { "1" } else { "0" }).await?;
    db::set_setting(&state.db, "crossfeed_level", &settings.level).await?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignalPathInfo {
    pub title: String,
    pub artist: String,
    pub file_path: String,
    pub file_format: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub bitrate: u32,
    pub is_bit_perfect: bool,
    pub replaygain_db: f32,
    pub lufs_gain_db: f32,
    pub crossfeed_enabled: bool,
    pub crossfeed_level: String,
    pub eq_enabled: bool,
    pub eq_profile_name: String,
    pub output_sample_rate: u32,
    pub output_format: String,
    pub output_device: String,
}

#[tauri::command]
pub async fn get_signal_path(
    state: State<'_, AppState>,
) -> Result<SignalPathInfo, String> {
    let track_id_opt = {
        let tid_guard = state.current_track_id.lock().unwrap_or_else(|p| p.into_inner());
        tid_guard.clone()
    };
    let track = if let Some(ref id) = track_id_opt {
        db::get_track(&state.db, id).await.map_err(|e| e.to_string())?
    } else {
        None
    };

    let track = track.ok_or_else(|| "No track is currently playing".to_string())?;

    let out_state = state.player.output_state();
    let eq_enabled_str = db::get_setting(&state.db, "eq_enabled").await?.unwrap_or_else(|| "0".into());
    let active_eq = crate::library::eq::get_active(&state.db).await?;
    let crossfeed_enabled = db::get_setting(&state.db, "crossfeed_enabled").await?.unwrap_or_else(|| "0".into()) == "1";
    let crossfeed_level = db::get_setting(&state.db, "crossfeed_level").await?.unwrap_or_else(|| "default".into());

    let lufs_gain = if let Ok(Some(loudness)) = db::get_track_loudness(&state.db, &track.id).await {
        let norm_mode = db::get_setting(&state.db, "normalization_mode").await?.unwrap_or_else(|| "track".into());
        let target = -14.0f32;
        if norm_mode == "off" {
            0.0
        } else {
            crate::audio::lufs::gain_for_target(loudness.loudness_lufs as f32, target)
        }
    } else {
        0.0
    };

    let format_str = match out_state.format {
        1 => "32-bit Float (WASAPI Direct)",
        2 => "24-bit Int (WASAPI Direct)",
        3 => "16-bit Int (WASAPI Direct)",
        _ => "Shared System Mixer",
    };

    Ok(SignalPathInfo {
        title: track.title,
        artist: track.artist,
        file_path: track.file_path,
        file_format: track.file_format.to_uppercase(),
        sample_rate: track.sample_rate,
        bit_depth: track.bit_depth,
        channels: track.channels,
        bitrate: track.bitrate,
        is_bit_perfect: out_state.bit_perfect_active,
        replaygain_db: track.replaygain_track_gain as f32,
        lufs_gain_db: lufs_gain,
        crossfeed_enabled,
        crossfeed_level,
        eq_enabled: eq_enabled_str == "1",
        eq_profile_name: active_eq.map(|e| e.name).unwrap_or_else(|| "None".into()),
        output_sample_rate: if out_state.sample_rate > 0 { out_state.sample_rate } else { track.sample_rate },
        output_format: format_str.to_string(),
        output_device: if out_state.bit_perfect_active { "WASAPI Exclusive DAC".into() } else { "Windows Default Audio Device".into() },
    })
}

#[tauri::command]
pub async fn get_library_health_summary(
    state: State<'_, AppState>,
) -> Result<crate::library::health::HealthSummary, String> {
    crate::library::health::get_health_summary(&state.db).await
}

#[tauri::command]
pub async fn get_suspect_tracks(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<crate::library::health::SuspectTrackItem>, String> {
    crate::library::health::get_suspect_tracks(&state.db, limit.unwrap_or(100)).await
}

#[tauri::command]
pub async fn get_duplicate_tracks(
    state: State<'_, AppState>,
) -> Result<Vec<crate::library::health::DuplicateGroup>, String> {
    crate::library::health::get_duplicate_tracks(&state.db).await
}

#[tauri::command]
pub async fn get_damaged_tracks(
    state: State<'_, AppState>,
) -> Result<Vec<crate::library::health::DamagedTrackItem>, String> {
    crate::library::health::get_damaged_tracks(&state.db).await
}

#[tauri::command]
pub async fn verify_track_integrity(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<Option<crate::library::health::DamagedTrackItem>, String> {
    crate::library::health::verify_track_integrity(&state.db, &track_id).await
}

#[tauri::command]
pub async fn delete_track_to_trash(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<(), String> {
    let track = db::get_track(&state.db, &track_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Track not found")?;

    crate::library::health::move_to_trash(&track.file_path)?;
    db::delete_track(&state.db, &track_id).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_split_albums(
    state: State<'_, AppState>,
) -> Result<Vec<crate::library::health::SplitAlbumItem>, String> {
    crate::library::health::get_split_albums(&state.db).await
}

#[tauri::command]
pub async fn merge_split_album(
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    target_album_artist: String,
) -> Result<u64, String> {
    crate::library::health::merge_split_album(&state.db, &track_ids, &target_album_artist).await
}

#[tauri::command]
pub async fn get_incomplete_albums(
    state: State<'_, AppState>,
) -> Result<Vec<crate::library::health::IncompleteAlbumItem>, String> {
    crate::library::health::get_incomplete_albums(&state.db).await
}

#[tauri::command]
pub async fn get_dual_script_artists(
    state: State<'_, AppState>,
) -> Result<Vec<crate::library::health::DualScriptArtistPair>, String> {
    crate::library::health::get_dual_script_artists(&state.db).await
}

#[tauri::command]
pub async fn merge_artist_names(
    state: State<'_, AppState>,
    from_artist: String,
    to_artist: String,
) -> Result<u64, String> {
    crate::library::health::merge_artist_names(&state.db, &from_artist, &to_artist).await
}

#[tauri::command]
pub async fn analyze_track_quality(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<crate::library::health::QualityResult, String> {
    let track = db::get_track(&state.db, &track_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Track not found")?;

    let path = std::path::Path::new(&track.file_path);
    let quality = crate::library::health::check_file_quality(path, track.duration)?;

    crate::library::health::init_health_table(&state.db).await?;
    sqlx::query(
        "INSERT INTO track_health (track_id, cutoff_hz, verdict, real_bits, checked_at)
         VALUES (?, ?, ?, ?, datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
            cutoff_hz = excluded.cutoff_hz,
            verdict = excluded.verdict,
            real_bits = excluded.real_bits,
            checked_at = datetime('now')"
    )
    .bind(&track_id)
    .bind(quality.cutoff_hz)
    .bind(quality.verdict.as_str())
    .bind(quality.real_bits.map(|b| b as i64))
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(quality)
}

#[tauri::command]
pub async fn process_artwork_gpu(
    state: State<'_, AppState>,
    track_id: String,
    target_size: Option<u32>,
) -> Result<Option<Vec<u8>>, String> {
    #[cfg(windows)]
    {
        let gpu = state.gpu_engine.as_ref().ok_or("GPU engine not available")?;
        let track = db::get_track(&state.db, &track_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Track not found")?;

        let path = std::path::Path::new(&track.file_path);
        let parent_dir = path.parent().ok_or("Invalid file path")?;

        // Try common artwork filenames in the track's directory
        let candidates = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "album.jpg", "album.png"];
        for name in &candidates {
            let art_path = parent_dir.join(name);
            if art_path.exists() {
                let size = target_size.unwrap_or(512);
                let data = gpu.process_artwork(&art_path, size)
                    .await
                    .map_err(|e| format!("GPU artwork processing failed: {e}"))?;
                return Ok(Some(data));
            }
        }
        Ok(None)
    }

    #[cfg(not(windows))]
    {
        let _ = (state, track_id, target_size);
        Ok(None)
    }
}

// --- Session / Crash Recovery Commands ---

#[tauri::command]
pub async fn save_session(
    app_handle: tauri::AppHandle,
    _state: State<'_, AppState>,
    track_id: Option<String>,
    position: f64,
    volume: f32,
    repeat_mode: String,
    shuffle: bool,
    queue_index: usize,
    gapless: bool,
    crossfade: f64,
) -> Result<(), String> {
    let data = SessionData {
        track_id,
        position,
        volume,
        repeat_mode,
        shuffle,
        queue_index,
        gapless,
        crossfade,
    };
    crate::session::save_session(&app_handle, &data);
    Ok(())
}

#[derive(Serialize)]
pub struct RestoreSessionResult {
    pub session: Option<SessionData>,
    pub was_crash: bool,
    pub crash_log: Option<String>,
}

#[tauri::command]
pub async fn restore_session(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RestoreSessionResult, String> {
    let data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let session = crate::session::load_session(&app_handle);
    let was_crash = state.was_crash.load(std::sync::atomic::Ordering::Acquire);
    let crash_log = if was_crash {
        crate::session::get_last_crash_log(&data_dir)
    } else {
        None
    };
    Ok(RestoreSessionResult {
        session,
        was_crash,
        crash_log,
    })
}

#[tauri::command]
pub async fn clear_session(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::session::clear_session(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn dismiss_crash_report(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if let Ok(data_dir) = app_handle.path().app_data_dir() {
        let crash_dir = data_dir.join("crashes");
        let _ = std::fs::remove_dir_all(&crash_dir);
        crate::crash::clean_up_crash_marker(&data_dir);
    }
    Ok(())
}

#[tauri::command]
pub fn consume_pending_external_file(
    state: State<'_, crate::state::AppState>,
) -> Option<serde_json::Value> {
    state.pending_external_file.lock().ok()?.take()
}

#[tauri::command]
pub async fn play_external_file(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    file_path: String,
    duration_secs: Option<f64>,
) -> Result<serde_json::Value, String> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err("File not found".to_string());
    }
    // Tags are best-effort: a metadata read failure must never veto audio.
    // Falls back to the caller-supplied duration (e.g. feed value), else 0.
    let mut full_track = match Track::from_path(path) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("External file tags unreadable, playing anyway: {e}");
            let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let file_format = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            Track {
                id: uuid::Uuid::new_v4().to_string(),
                file_path: file_path.clone(),
                title: String::new(),
                artist: String::new(),
                album: String::new(),
                album_artist: String::new(),
                album_id: String::new(),
                composer: String::new(),
                track_number: 0,
                disc_number: 0,
                year: 0,
                genre: String::new(),
                raw_genre_names: String::new(),
                duration: duration_secs.unwrap_or(0.0),
                sample_rate: 0,
                bit_depth: 0,
                channels: 2,
                bitrate: 0,
                file_size,
                file_format,
                has_artwork: false,
                replaygain_track_gain: 0.0,
                replaygain_album_gain: 0.0,
                cue_parent_id: None,
                cue_offset: 0.0,
                artwork_key: None,
                bpm: 0.0,
                musical_key: String::new(),
                energy: 0.0,
                analysis_failed_reason: None,
                file_modified: 0,
                matched_lyric: None,
            }
        }
    };

    if full_track.title.trim().is_empty() {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            full_track.title = stem.to_string();
        }
    }
    if full_track.artist.trim().is_empty() {
        full_track.artist = "Unknown Artist".to_string();
    }
    if full_track.duration <= 0.0 {
        if let Some(d) = duration_secs {
            full_track.duration = d;
        }
    }

    let gain = 0.0;
    let track_id = full_track.id.clone();
    state.is_external_playback.store(true, std::sync::atomic::Ordering::Release);
    state.player.clear_next();
    state.player.play_track(path.to_path_buf(), app_handle.clone(), gain, full_track.sample_rate, Some(track_id.clone()))
        .map_err(|e| format!("Failed to play: {e}"))?;

    // Store active track and current track id for cross-window and SMTC sync
    if let Ok(mut tid) = state.current_track_id.lock() {
        *tid = Some(track_id.clone());
    }
    if let Ok(mut at) = state.active_track.lock() {
        *at = Some(full_track.clone());
    }

    // Immediately push full metadata and album artwork to Windows SMTC & Tray
    update_smtc_track(&state, &full_track);

    let _ = app_handle.emit("track-started", &track_id);
    let _ = app_handle.emit("player:status", serde_json::json!({"playing": true, "track_id": track_id}));

    let payload = serde_json::json!({
        "id": track_id,
        "file_path": file_path,
        "title": full_track.title,
        "artist": full_track.artist,
        "album": full_track.album,
        "album_artist": full_track.album_artist,
        "composer": full_track.composer,
        "track_number": full_track.track_number,
        "disc_number": full_track.disc_number,
        "year": full_track.year,
        "genre": full_track.genre,
        "raw_genre_names": full_track.raw_genre_names,
        "duration": full_track.duration,
        "sample_rate": full_track.sample_rate,
        "bit_depth": full_track.bit_depth,
        "bitrate": full_track.bitrate,
        "file_size": full_track.file_size,
        "file_format": full_track.file_format,
        "has_artwork": full_track.has_artwork,
        "replaygain_track_gain": full_track.replaygain_track_gain,
        "replaygain_album_gain": full_track.replaygain_album_gain,
    });
    // Also store in pending so FilePlayerView can consume it
    if let Ok(mut pending) = state.pending_external_file.lock() {
        *pending = Some(payload.clone());
    }
    let _ = app_handle.emit("external-playback-started", &payload);
    Ok(payload)
}

#[tauri::command]
pub fn is_external_playback(state: State<'_, AppState>) -> bool {
    state.is_external_playback.load(std::sync::atomic::Ordering::Acquire)
}

#[tauri::command]
pub async fn generate_smart_mix(
    seed_track_id: String,
    count: u32,
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<Track>, String> {
    let seed = db::get_track(&state.db, &seed_track_id)
        .await?
        .ok_or_else(|| "Seed track not found".to_string())?;
    let all = db::get_all_tracks(&state.db).await?;
    let moods = db::get_all_moods(&state.db).await.unwrap_or_default();
    let mix = crate::audio::smart_mix::generate_smart_mix(&seed, &all, &moods, count as usize);
    Ok(mix)
}

const GENRE_WHITELIST: &[&str] = &[
    "rock", "pop", "hip-hop", "hip hop", "rap", "jazz", "blues", "classical",
    "electronic", "dance", "house", "techno", "trance", "ambient", "dubstep",
    "r&b", "rnb", "soul", "funk", "reggae", "ska", "punk", "metal", "hard rock",
    "alternative", "indie", "folk", "country", "bluegrass", "latin", "salsa",
    "bossa nova", "samba", "reggaeton", "k-pop", "j-pop", "anime", "vocaloid",
    "world", "african", "cuban", "caribbean", "celtic", "gospel", "christian",
    "new age", "soundtrack", "score", "musical", "comedy", "spoken word",
    "audiobook", "podcast", "children's", "kids", "experimental", "noise",
    "lo-fi", "lofi", "chill", "downtempo", "trip-hop", "trip hop",
    "drum and bass", "drum & bass", "dnb", "garage", "grime", "trap",
    "synth-pop", "synthpop", "new wave", "post-punk", "post punk",
    "progressive", "prog rock", "psychedelic", "shoegaze", "dream pop",
    "math rock", "emo", "post-rock", "post rock", "slowcore", "singer-songwriter",
    "singer songwriter", "acoustic", "piano", "orchestral", "chamber",
    "opera", "baroque", "romantic", "medieval", "renaissance",
    "folk rock", "country rock", "southern rock", "blues rock",
    "jazz fusion", "fusion", "latin jazz", "swing", "big band",
    "disco", "funk rock", "boogie", "motown", "doo-wop", "surf",
    "grunge", "nu metal", "death metal", "black metal", "doom metal",
    "power metal", "symphonic metal", "folk metal", "industrial",
    "darkwave", "ethereal", "gothic", "witch house",
];

#[tauri::command]
pub async fn batch_fetch_genre(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    retry_missing: Option<bool>,
) -> Result<String, String> {
    if retry_missing.unwrap_or(false) {
        let reset = db::reset_genre_not_found(&state.db).await?;
        if reset > 0 {
            log::info!("Re-arming {reset} tracks for genre retry");
        }
    }
    run_genre_enrichment(app_handle, state.db.clone(), state.is_fetching_genre.clone(), None).await
}

#[tauri::command]
pub async fn get_genre_progress(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let (tagged, total, checked_missing) = db::get_genre_coverage(&state.db).await?;
    let is_running = state.is_fetching_genre.load(std::sync::atomic::Ordering::Relaxed);
    Ok(serde_json::json!({
        "tagged": tagged,
        "total": total,
        "missing": total - tagged,
        "checked_missing": checked_missing,
        "running": is_running,
    }))
}

fn clean_search_term(s: &str) -> String {
    let mut clean = s.to_string();
    let markers = [
        "(feat.", "[feat.", "(ft.", "[ft.", "(with", "[with",
        "(remaster", "[remaster", "(deluxe", "[deluxe",
        "(live", "[live", "(explicit", "[explicit",
        "(original mix", "[original mix", "(radio edit", "[radio edit",
        "(extended", "[extended", "(instrumental", "[instrumental",
    ];
    for m in markers {
        if let Some(idx) = clean.to_lowercase().find(m) {
            clean.truncate(idx);
        }
    }
    clean.trim().trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']' || c == '-').trim().to_string()
}

async fn fetch_itunes_genre(client: &reqwest::Client, artist: &str, title: &str) -> Option<String> {
    let clean_t = clean_search_term(title);
    let clean_a = clean_search_term(artist);

    // 1. Try "artist + title"
    let term1 = if !clean_t.is_empty() { format!("{clean_a} {clean_t}") } else { clean_a.clone() };
    let url1 = format!(
        "https://itunes.apple.com/search?term={}&entity=song&limit=5",
        urlencoding(&term1)
    );
    if let Ok(resp) = client.get(&url1).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                for item in results {
                    if let Some(genre) = item.get("primaryGenreName").and_then(|v| v.as_str()) {
                        if !genre.trim().is_empty() {
                            return Some(genre.trim().to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Fallback to artist alone (Apple Music provides primary genre for artist)
    if !clean_a.is_empty() {
        let url2 = format!(
            "https://itunes.apple.com/search?term={}&entity=musicArtist&limit=3",
            urlencoding(&clean_a)
        );
        if let Ok(resp) = client.get(&url2).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
                    for item in results {
                        if let Some(genre) = item.get("primaryGenreName").and_then(|v| v.as_str()) {
                            if !genre.trim().is_empty() {
                                return Some(genre.trim().to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

async fn fetch_deezer_genre(client: &reqwest::Client, artist: &str, title: &str) -> Option<String> {
    let clean_t = clean_search_term(title);
    let clean_a = clean_search_term(artist);
    let term = if !clean_t.is_empty() { format!("{clean_a} {clean_t}") } else { clean_a.clone() };
    let url = format!(
        "https://api.deezer.com/search?q={}",
        urlencoding(&term)
    );
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                if let Some(first) = data.first() {
                    if let Some(album_id) = first.get("album").and_then(|a| a.get("id")).and_then(|id| id.as_u64()) {
                        let album_url = format!("https://api.deezer.com/album/{album_id}");
                        if let Ok(album_resp) = client.get(&album_url).send().await {
                            if let Ok(album_json) = album_resp.json::<serde_json::Value>().await {
                                if let Some(genres) = album_json.get("genres").and_then(|g| g.get("data")).and_then(|d| d.as_array()) {
                                    if let Some(first_g) = genres.first() {
                                        if let Some(name) = first_g.get("name").and_then(|n| n.as_str()) {
                                            if !name.trim().is_empty() {
                                                return Some(name.trim().to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub async fn run_genre_enrichment(
    app_handle: tauri::AppHandle,
    db: sqlx::SqlitePool,
    is_fetching_genre: std::sync::Arc<std::sync::atomic::AtomicBool>,
    max_tracks: Option<usize>,
) -> Result<String, String> {
    if is_fetching_genre.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let _ = app_handle.emit("batch-genre-done", serde_json::json!({"already_running": true}));
        return Ok("already_running".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent("Eiryn/0.1.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let guard = is_fetching_genre.clone();

    tauri::async_runtime::spawn(async move {
        let api_key = db::get_setting(&db, "lastfm_api_key")
            .await
            .ok()
            .flatten()
            .and_then(|v| crate::security::decrypt_secret(&v).ok())
            .unwrap_or_default();
        if api_key.is_empty() {
            log::info!("No Last.fm API key set — using public iTunes & Deezer catalog for genre enrichment");
        }

        let tracks = match db::get_tracks_needing_genre(&db, max_tracks).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("Failed to get tracks needing genre: {e}");
                guard.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = app_handle.emit("batch-genre-done", serde_json::json!({"error": e}));
                return;
            }
        };

        let total = tracks.len();
        let mut fetched = 0u32;
        let mut skipped = 0u32;

        for (i, track) in tracks.iter().enumerate() {
            app_handle.emit("batch-genre-progress", serde_json::json!({
                "current": i + 1,
                "total": total,
                "track": track.title,
                "artist": track.artist,
                "status": "searching",
            })).ok();

            let mut matched_tag: Option<String> = None;

            if !api_key.is_empty() {
                // Try track.getTopTags first
                if let Ok(tags) = crate::lastfm::track_get_top_tags(&api_key, &track.artist, &track.title, &client).await {
                    for tag in &tags {
                        let lower = tag.to_lowercase();
                        for whitelisted in GENRE_WHITELIST {
                            if lower.contains(whitelisted) {
                                matched_tag = Some(whitelisted.to_string());
                                break;
                            }
                        }
                        if matched_tag.is_some() { break; }
                    }
                }

                // Fall back to artist.getTopTags
                if matched_tag.is_none() {
                    if let Ok(tags) = crate::lastfm::artist_get_top_tags(&api_key, &track.artist, &client).await {
                        for tag in &tags {
                            let lower = tag.to_lowercase();
                            for whitelisted in GENRE_WHITELIST {
                                if lower.contains(whitelisted) {
                                    matched_tag = Some(whitelisted.to_string());
                                    break;
                                }
                            }
                            if matched_tag.is_some() { break; }
                        }
                    }
                }
            }

            // Fall back to iTunes / Deezer catalog (Cadence parity)
            if matched_tag.is_none() {
                if let Some(genre) = fetch_itunes_genre(&client, &track.artist, &track.title).await {
                    matched_tag = Some(genre);
                } else if let Some(genre) = fetch_deezer_genre(&client, &track.artist, &track.title).await {
                    matched_tag = Some(genre);
                }
            }

            if let Some(ref genre) = matched_tag {
                let _ = db::update_track_genre_from_lastfm(&db, &track.id, genre).await;
                fetched += 1;
                app_handle.emit("batch-genre-progress", serde_json::json!({
                    "current": i + 1,
                    "total": total,
                    "track": track.title,
                    "artist": track.artist,
                    "status": "found",
                    "genre": genre,
                })).ok();
            } else {
                // Mark as checked even if nothing found (prevents re-fetching)
                let _ = db::mark_track_genre_checked(&db, &track.id).await;
                skipped += 1;
                app_handle.emit("batch-genre-progress", serde_json::json!({
                    "current": i + 1,
                    "total": total,
                    "track": track.title,
                    "artist": track.artist,
                    "status": "not_found",
                })).ok();
            }

            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        app_handle.emit("batch-genre-done", serde_json::json!({
            "fetched": fetched,
            "skipped": skipped,
            "total": total,
        })).ok();

        let _ = crate::library::smart_playlist::refresh_all_smart_playlists(&db).await;
        guard.store(false, std::sync::atomic::Ordering::SeqCst);
    });

    Ok("started".to_string())
}

#[tauri::command]
pub async fn build_genre_queue(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    track_id: String,
) -> Result<Vec<Track>, String> {
    let track = db::get_track(&state.db, &track_id)
        .await?
        .ok_or_else(|| "Track not found".to_string())?;

    let tracks = if !track.genre.is_empty() {
        db::get_tracks_by_normalized_genre(&state.db, &track.genre).await?
    } else if !track.artist.is_empty() {
        db::get_tracks_by_artist_shuffled(&state.db, &track.artist).await?
    } else {
        db::get_tracks_shuffled(&state.db, 100).await?
    };

    if tracks.is_empty() {
        return Err("No tracks found for genre queue".to_string());
    }

    // Set the queue via existing path
    let refs: Vec<TrackRef> = tracks.iter().map(track_to_ref).collect();
    {
        let mut q = state.queue_state.lock().map_err(|e| e.to_string())?;
        q.set_tracks(refs, 0);
    }
    let queue_path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?.join("queue_state.json");
    save_queue_state(&state, &queue_path);
    let _ = app_handle.emit("queue:replaced", serde_json::json!({"length": tracks.len()}));

    Ok(tracks)
}

#[tauri::command]
pub async fn toggle_mini_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("mini-player") {
        let _ = w.close();
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.show();
            let _ = main.unminimize();
            let _ = main.set_focus();
        }
        return Ok(());
    }
    let (w, h) = (420.0, 140.0);
    let built = tauri::WebviewWindowBuilder::new(&app, "mini-player", tauri::WebviewUrl::App("index.html#/mini-player".into()))
        .title("Eiryn Mini Player")
        .inner_size(w, h)
        .min_inner_size(w, h)
        .max_inner_size(w, h)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(false)
        .visible(false)
        .build();
    match built {
        Ok(win) => {
            #[cfg(target_os = "windows")]
            apply_remove_dwm_border(&win);
            let main = app.get_webview_window("main");
            let monitor = main
                .as_ref()
                .filter(|m| m.is_visible().unwrap_or(false))
                .and_then(|m| m.current_monitor().ok().flatten())
                .or_else(|| app.primary_monitor().ok().flatten())
                .or_else(|| win.current_monitor().ok().flatten());
            if let Some(monitor) = monitor {
                let scale = monitor.scale_factor();
                let area = monitor.work_area();
                let x = area.position.x as f64 + area.size.width as f64 - (w + 20.0) * scale;
                let y = area.position.y as f64 + area.size.height as f64 - (h + 20.0) * scale;
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                let _ = win.set_size(tauri::LogicalSize::new(w, h));
            }
            if let Some(main) = main {
                let _ = main.hide();
            }
            let _ = win.show();
            let _ = win.set_focus();
            Ok(())
        }
        Err(e) => Err(format!("Failed to open mini player: {e}")),
    }
}

#[tauri::command]
pub async fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("mini-player") {
        let _ = w.close();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn apply_remove_dwm_border(win: &tauri::WebviewWindow) {
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            #[link(name = "dwmapi")]
            extern "system" {
                fn DwmSetWindowAttribute(
                    hwnd: *mut std::ffi::c_void,
                    dwAttribute: u32,
                    pvAttribute: *const std::ffi::c_void,
                    cbAttribute: u32,
                ) -> i32;
            }
            // DWMWA_BORDER_COLOR = 34; 0xFFFFFFFE = DWMWA_COLOR_NONE (suppresses 1px accent border)
            let color_none: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd.0 as *mut std::ffi::c_void,
                34,
                &color_none as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            // DWMWA_WINDOW_CORNER_PREFERENCE = 33; 1 = DWMWCP_DONOTROUND
            let do_not_round: u32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd.0 as *mut std::ffi::c_void,
                33,
                &do_not_round as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

#[tauri::command]
pub async fn remove_window_border(app: tauri::AppHandle, label: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(win) = app.get_webview_window(&label) {
            apply_remove_dwm_border(&win);
        }
    }
    Ok(())
}


