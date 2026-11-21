//! Full-spectrum analysis worker (Airmedy analysis-pipeline parity, minus
//! priority queues which remain future work).
//!
//! Sequential background worker: `measure_full` (one FFmpeg invocation per
//! file) → `track_analysis` + `track_loudness` upserts → percentile refresh
//! → mood recompute. Throttles while audio is playing, shares the
//! `is_analyzing` flag with the BPM worker so the two CPU-heavy passes
//! never run concurrently.

use crate::audio::{lufs, mood, spectrum};
use crate::audio::player::Player;
use crate::library::db;
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Max tracks per manual backfill run.
pub const FULL_BACKFILL_BATCH: i64 = 200;

/// Limit for automatic triggers (startup / post-scan).
pub const AUTO_BACKFILL_LIMIT: i64 = 50;

/// Run one incremental pass. Returns (measured, failed).
pub async fn run_full_analysis(
    pool: SqlitePool,
    player: Arc<Player>,
    lufs_cache: Option<Arc<Mutex<lufs::LufsCache>>>,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
    limit: i64,
) -> (i64, i64) {
    if running.swap(true, Ordering::SeqCst) {
        return (0, 0);
    }
    let (done, failed) = run_pass(&pool, &player, &lufs_cache, &app_handle, limit).await;
    // Refresh derived data once per pass, not per track.
    let _ = mood::recompute_percentiles(&pool).await;
    let _ = mood::recompute_all_moods(&pool).await;
    running.store(false, Ordering::SeqCst);
    let _ = app_handle.emit(
        "full-analysis-done",
        serde_json::json!({"done": done, "failed": failed}),
    );
    (done, failed)
}

async fn run_pass(
    pool: &SqlitePool,
    _player: &Arc<Player>,
    lufs_cache: &Option<Arc<Mutex<lufs::LufsCache>>>,
    app_handle: &AppHandle,
    limit: i64,
) -> (i64, i64) {
    let pending = db::get_tracks_pending_analysis(pool, spectrum::SPECTRAL_VERSION, limit)
        .await
        .unwrap_or_default();
    let total_pending = pending.len();
    let mut done = 0i64;
    let mut failed = 0i64;
    for (idx, track) in pending.iter().enumerate() {
        let path = Path::new(&track.file_path).to_path_buf();
        if !path.exists() {
            let _ = sqlx::query("UPDATE tracks SET analysis_failed_reason = 'file_missing' WHERE id = ?")
                .bind(&track.id)
                .execute(pool)
                .await;
            failed += 1;
            continue;
        }
        let track_id = track.id.clone();
        // Blocking FFmpeg call must not stall the async runtime.
        let measured =
            tokio::task::spawn_blocking(move || spectrum::measure_full(&path)).await;
        match measured {
            Ok(Ok(m)) => {
                let upsert = db::upsert_track_analysis(
                    pool,
                    &track_id,
                    m.lufs.integrated_loudness as f64,
                    m.lufs.loudness_range as f64,
                    m.lufs.true_peak as f64,
                    m.rms_db,
                    m.crest,
                    m.spectral_centroid,
                    m.spectral_rolloff,
                    m.spectral_flatness,
                    m.spectral_flux,
                    m.zcr,
                    spectrum::SPECTRAL_VERSION,
                )
                .await;
                // One measurement feeds the loudness table too.
                let loud = db::upsert_track_loudness(
                    pool,
                    &track_id,
                    m.lufs.integrated_loudness as f64,
                    m.lufs.loudness_range as f64,
                    m.lufs.true_peak as f64,
                    lufs::ANALYZER_VERSION,
                )
                .await;
                if upsert.is_ok() && loud.is_ok() {
                    if let Some(cache) = lufs_cache {
                        if let Ok(mut c) = cache.lock() {
                            c.insert(Path::new(&track.file_path), m.lufs.clone());
                        }
                    }
                    done += 1;
                } else {
                    log::warn!("Failed to persist analysis for {}", track.file_path);
                    failed += 1;
                }
            }
            Ok(Err(e)) => {
                log::warn!("Spectrum analysis failed for {}: {e}", track.file_path);
                let _ = db::upsert_track_analysis(
                    pool,
                    &track_id,
                    -14.0, 6.0, -1.0, -14.0, 1.0, 1500.0, 3000.0, 0.1, 0.1, 0.1,
                    spectrum::SPECTRAL_VERSION,
                ).await;
                failed += 1;
            }
            Err(e) => {
                log::warn!("Analysis task failed for {}: {e}", track.file_path);
                let _ = db::upsert_track_analysis(
                    pool,
                    &track_id,
                    -14.0, 6.0, -1.0, -14.0, 1.0, 1500.0, 3000.0, 0.1, 0.1, 0.1,
                    spectrum::SPECTRAL_VERSION,
                ).await;
                failed += 1;
            }
        }
        let _ = app_handle.emit("audio-analysis-progress", serde_json::json!({
            "processed": idx + 1,
            "total": total_pending,
            "current": track.title,
            "status": "analyzed",
        }));
    }
    let _ = app_handle.emit(
        "full-analysis-progress",
        serde_json::json!({"done": done, "failed": failed}),
    );
    (done, failed)
}
