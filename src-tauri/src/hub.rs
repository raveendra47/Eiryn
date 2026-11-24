//! Background status hub: keeps System Media Transport Controls (SMTC),
//! Windows 11 Quick Settings media widget, tray icon tooltip, and tray menu
//! in real-time sync with playback state and active tracks.

use std::path::Path;
use std::time::{Duration, Instant};
use tauri::Manager;
use crate::library::db;
use crate::library::track::Track;
use crate::media::NowPlaying;
use crate::state::AppState;

/// Converts a local filesystem path to a `file://` URI formatted specifically
/// for Windows WinRT SMTC and `souvlaki` (`file://D:\...` without extra leading slashes).
fn to_file_uri(p: &Path) -> String {
    let clean = p.to_string_lossy().replace('/', "\\");
    let trimmed = clean.trim_start_matches('\\');
    format!("file://{trimmed}")
}

/// Resolves a local `file:///` URI for a track's album artwork so Windows SMTC
/// can display it in the Windows 11 Action Center / Quick Settings flyout.
pub fn resolve_artwork_file_url(state: &AppState, track: &Track) -> Option<String> {
    // 1. Check if artwork_key is already cached on disk
    if let Some(ref key) = track.artwork_key {
        let md = state.track_artwork_cache.path_for_key(key, Some("md"));
        if md.exists() {
            return Some(to_file_uri(&md));
        }
        let orig = state.track_artwork_cache.path_for_key(key, None);
        if orig.exists() {
            return Some(to_file_uri(&orig));
        }
    }

    // 2. Resolve audio file path (handling CUE children if applicable)
    let file_path = if let Some(ref parent_id) = track.cue_parent_id {
        let db = state.db.clone();
        let pid = parent_id.clone();
        tauri::async_runtime::block_on(async {
            db::get_track(&db, &pid).await.ok().flatten().map(|p| p.file_path)
        }).unwrap_or_else(|| track.file_path.clone())
    } else {
        track.file_path.clone()
    };

    let track_path = Path::new(&file_path);
    if track_path.exists() {
        // 2a. Try reading embedded artwork from audio tags
        if let Some(bytes) = crate::library::track::extract_artwork_bytes(track_path) {
            if let Ok(key) = state.track_artwork_cache.save(&bytes) {
                let md = state.track_artwork_cache.path_for_key(&key, Some("md"));
                let p = if md.exists() { md } else { state.track_artwork_cache.path_for_key(&key, None) };
                if p.exists() {
                    return Some(to_file_uri(&p));
                }
            }
        }

        // 2b. Try standard folder artwork in the track directory
        if let Some(parent) = track_path.parent() {
            let names = ["cover", "folder", "front", "album", "artwork"];
            let exts = ["jpg", "jpeg", "png", "webp"];
            for name in &names {
                for ext in &exts {
                    let candidate = parent.join(format!("{name}.{ext}"));
                    if candidate.exists() {
                        if let Ok(bytes) = std::fs::read(&candidate) {
                            if let Ok(key) = state.track_artwork_cache.save(&bytes) {
                                let md = state.track_artwork_cache.path_for_key(&key, Some("md"));
                                let p = if md.exists() { md } else { state.track_artwork_cache.path_for_key(&key, None) };
                                if p.exists() {
                                    return Some(to_file_uri(&p));
                                }
                            }
                        }
                        return Some(to_file_uri(&candidate));
                    }
                }
            }
        }
    }

    None
}

/// Starts the status hub background worker thread.
/// Observes track changes, play/pause transitions, and playback position,
/// ensuring Windows 11 SMTC and tray menu stay 100% in sync without depending on the frontend UI.
pub fn start_status_hub(app_handle: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("eiryn-status-hub".into())
        .spawn(move || {
            let state = app_handle.state::<AppState>();
            let mut last_track_id: Option<String> = None;
            let mut last_playing: bool = false;
            let mut last_playback_sync = Instant::now() - Duration::from_secs(60);

            loop {
                std::thread::sleep(Duration::from_millis(500));

                let is_playing = state.player.is_playing();
                let position = state.player.position() as f64;

                // 1. Check if state has an active track (e.g. set by play_track, play_track_ref, or play_external_file)
                let active_track_opt = state.active_track.lock().ok().and_then(|g| g.clone());

                let current_id = active_track_opt.as_ref().map(|t| t.id.clone()).or_else(|| {
                    state.current_track_id.lock().ok().and_then(|tid| tid.clone())
                }).or_else(|| {
                    state.queue_state.lock().ok().and_then(|q| q.current_track().map(|t| t.id.clone()))
                });

                // Track Changed
                if current_id != last_track_id {
                    let track_opt = if let Some(t) = active_track_opt {
                        Some(t)
                    } else if let Some(ref id) = current_id {
                        let db = state.db.clone();
                        let track_id = id.clone();
                        let loaded = tauri::async_runtime::block_on(async {
                            db::get_track(&db, &track_id).await.unwrap_or(None)
                        });
                        if let Some(ref t) = loaded {
                            if let Ok(mut at) = state.active_track.lock() {
                                *at = Some(t.clone());
                            }
                        }
                        loaded
                    } else {
                        None
                    };

                    if let Some(track) = track_opt {
                        let cover = resolve_artwork_file_url(&state, &track);
                        let mut title = track.title.trim().to_string();
                        if title.is_empty() {
                            if let Some(stem) = Path::new(&track.file_path).file_stem().and_then(|s| s.to_str()) {
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

                        let now_playing = NowPlaying {
                            title: title.clone(),
                            artist: artist.clone(),
                            album: track.album.clone(),
                            cover,
                            duration: track.duration,
                        };

                        // Update tray tooltip & label
                        let tooltip_text = format!("{title} — {artist}");
                        if let Ok(tray_guard) = state.tray_menu.lock() {
                            if let Some(ref tm) = *tray_guard {
                                tm.set_tooltip(Some(&tooltip_text));
                                tm.set_track_info(&title, &artist);
                            }
                        }

                        // Update SMTC metadata
                        if let Ok(guard) = state.media.lock() {
                            if let Some(ref tx) = *guard {
                                let _ = tx.send(crate::media::Update::Track(Some(now_playing)));
                            }
                        }

                        // Reset playback sync so Playback update follows immediately
                        last_playback_sync = Instant::now() - Duration::from_secs(60);
                        last_track_id = current_id;
                    } else if !is_playing {
                        // Truly stopped / empty queue (never wipe metadata while playing!)
                        if let Ok(tray_guard) = state.tray_menu.lock() {
                            if let Some(ref tm) = *tray_guard {
                                tm.set_tooltip(None);
                                tm.set_track_info("", "");
                                tm.set_playing(false);
                            }
                        }
                        if let Ok(guard) = state.media.lock() {
                            if let Some(ref tx) = *guard {
                                let _ = tx.send(crate::media::Update::Track(None));
                            }
                        }
                        last_track_id = None;
                    }
                }

                // Playback timeline & state sync
                if is_playing != last_playing || (is_playing && last_playback_sync.elapsed() > Duration::from_secs(4)) {
                    if let Ok(guard) = state.media.lock() {
                        if let Some(ref tx) = *guard {
                            let _ = tx.send(crate::media::Update::Playback {
                                playing: is_playing,
                                position,
                            });
                        }
                    }

                    if is_playing != last_playing {
                        if let Ok(tray_guard) = state.tray_menu.lock() {
                            if let Some(ref tm) = *tray_guard {
                                tm.set_playing(is_playing);
                            }
                        }
                        last_playing = is_playing;
                    }

                    last_playback_sync = Instant::now();
                }
            }
        })
        .expect("failed to spawn status hub");
}
