//! FFmpeg ebur128 loudness backfill worker.
//!
//! Measures pending physical tracks (no CUE children — the parent file is
//! measured once) sequentially in the background, persists to
//! `track_loudness`, and emits `loudness-progress` events. Throttles while
//! audio is playing to avoid CPU contention with playback.

use crate::audio::lufs::{ANALYZER_VERSION, LufsCache, calculate_lufs};
use crate::audio::player::Player;
use crate::library::db;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Max tracks per backfill run; the worker stays incremental by design.
pub const BACKFILL_BATCH: i64 = 500;

/// Measure one file and persist. Shared by the worker and the
/// `calculate_track_lufs` command. Returns the raw LUFS triple.
pub async fn measure_and_store(
    pool: &SqlitePool,
    lufs_cache: &Option<Arc<Mutex<LufsCache>>>,
    track_id: &str,
    file_path: &str,
) -> Result<(f32, f32, f32), String> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(format!("File not found: {file_path}"));
    }
    // Blocking ffmpeg call must not stall the async runtime.
    let result =
        tokio::task::spawn_blocking(move || calculate_lufs(&path))
            .await
            .map_err(|e| format!("Loudness task failed: {e}"))??;
    db::upsert_track_loudness(
        pool,
        track_id,
        result.integrated_loudness as f64,
        result.loudness_range as f64,
        result.true_peak as f64,
        ANALYZER_VERSION,
    )
    .await?;
    if let Some(cache) = lufs_cache {
        if let Ok(mut c) = cache.lock() {
            c.insert(Path::new(file_path), result.clone());
        }
    }
    Ok((
        result.integrated_loudness,
        result.loudness_range,
        result.true_peak,
    ))
}

/// Run one incremental backfill pass. Sequential: each ffmpeg process already
/// saturates decode, and sequential keeps playback throttle checks tight.
pub async fn run_backfill(
    pool: SqlitePool,
    player: Arc<Player>,
    lufs_cache: Option<Arc<Mutex<LufsCache>>>,
    app_handle: AppHandle,
    limit: i64,
) -> (i64, i64) {
    let pending = db::get_tracks_pending_loudness(&pool, ANALYZER_VERSION, limit)
        .await
        .unwrap_or_default();
    let mut done = 0i64;
    let mut failed = 0i64;
    for track in pending {
        // Throttle: wait out playback instead of competing with it.
        while player.is_playing() {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        match measure_and_store(&pool, &lufs_cache, &track.id, &track.file_path).await {
            Ok(_) => done += 1,
            Err(e) => {
                log::warn!("Loudness analysis failed for {}: {e}", track.file_path);
                failed += 1;
            }
        }
        let _ = app_handle.emit(
            "loudness-progress",
            serde_json::json!({"done": done, "failed": failed}),
        );
    }
    // Final totals for the settings UI.
    if let Ok((d, t)) = db::get_loudness_progress(&pool, ANALYZER_VERSION).await {
        let _ = app_handle.emit(
            "loudness-progress",
            serde_json::json!({"analyzed": d, "total": t, "done": done, "failed": failed}),
        );
    }
    (done, failed)
}

/// Guard flag helper: returns true if the caller won the worker slot.
pub fn try_claim_worker(flag: &std::sync::atomic::AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn release_worker(flag: &std::sync::atomic::AtomicBool) {
    flag.store(false, Ordering::SeqCst);
}
