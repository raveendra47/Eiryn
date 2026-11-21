#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
use eiryn_lib::gpu::GpuArtworkEngine;
use eiryn_lib::audio::player::Player;
use eiryn_lib::audio::lufs::LufsCache;
use eiryn_lib::commands;
use eiryn_lib::library::artist_artwork::ArtistArtworkCache;
use eiryn_lib::library::artist_artwork_worker::ArtistArtworkWorker;
use eiryn_lib::library::track_artwork::TrackArtworkCache;
use eiryn_lib::library::db;
use eiryn_lib::queue::QueueState;
use eiryn_lib::queue_persistence;
use eiryn_lib::remote;
use eiryn_lib::search::tantivy_index::TantivyIndex;
use eiryn_lib::library::scanner;
use std::path::PathBuf;
use eiryn_lib::library::eq;
use eiryn_lib::library::track::Track;
use eiryn_lib::state::AppState;
use eiryn_lib::tray::TrayMenu;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Listener;
use tauri::Manager;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{WebviewWindowBuilder, WebviewUrl};
#[cfg(not(windows))]
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutEvent, ShortcutState, GlobalShortcutExt};
use std::sync::atomic::Ordering;

fn app_data_dir(app: &tauri::App) -> PathBuf {
    let mut dir = app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    if tauri::is_dev() {
        dir.push("dev");
    }
    dir
}

fn update_tray_from_state(tray: &TrayMenu, app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let playing = state.player.is_playing();
    tray.set_playing(playing);

    let queue_guard = state.queue_state.lock();
    if let Ok(q) = queue_guard {
        tray.set_shuffle(q.shuffle);
        tray.set_repeat(q.repeat_mode.as_str());

        if let Some(_current) = q.current_track() {
            tray.set_prev_enabled(q.current_index > 0 || q.repeat_mode != eiryn_lib::queue::RepeatMode::Off);
            let has_next = q.current_index + 1 < q.len() || q.repeat_mode != eiryn_lib::queue::RepeatMode::Off;
            tray.set_next_enabled(has_next);

            let tid_guard = state.current_track_id.lock();
            if let Ok(tid) = tid_guard {
                if let Some(ref track_id) = *tid {
                    // DB lookups happen off the main thread so a slow query
                    // (e.g. during a scan) can never freeze the UI event loop.
                    let db = state.db.clone();
                    let id = track_id.clone();
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Ok(Some(track)) = db::get_track(&db, &id).await {
                            let favs = db::get_favorite_ids(&db).await.unwrap_or_default();
                            let st = app2.state::<AppState>();
                            let tray_guard = st.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(ref tm) = *tray_guard {
                                tm.set_track_info(&track.title, &track.artist);
                                tm.set_favorite(favs.contains(&id));
                            }
                        }
                    });
                }
            }

            if let Some(next) = q.peek_next() {
                tray.set_next_info(&next.title);
            } else {
                tray.set_next_info("");
            }
        } else {
            tray.set_track_info("", "");
            tray.set_next_info("");
            tray.set_next_enabled(false);
            tray.set_prev_enabled(false);
        }
    }
}

fn main() {
    #[cfg(windows)]
    unsafe {
        // Initialize OLE in STA mode before any plugin can set COM to MTA
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }
    // Default to info-level for our own crate so the `[bp]` / `[audio]`
    // diagnostics are always visible in dev; RUST_LOG still overrides.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("eiryn_lib=info"))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_window_state::Builder::default().with_denylist(&["mini-player", "file-player"]).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.len() > 1 {
                let file_path = &argv[1];
                let path = std::path::Path::new(file_path);
                if path.exists() {
                    let ext = path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let audio_exts = ["mp3","flac","wav","m4a","ogg","wma","aac",
                                      "opus","wv","aiff","dsf","dff","ape","mpc","tta","mid"];
                    if audio_exts.contains(&ext.as_str()) {
                        log::info!("Second instance opened file: {}", file_path);
                        let state = app.state::<AppState>();
                        if let Ok(track) = Track::from_path(path) {
                            let track_id = uuid::Uuid::new_v4().to_string();
                            let payload = serde_json::json!({
                                "id": track_id,
                                "file_path": file_path,
                                "title": track.title,
                                "artist": track.artist,
                                "album": track.album,
                                "album_artist": track.album_artist,
                                "composer": track.composer,
                                "track_number": track.track_number,
                                "disc_number": track.disc_number,
                                "year": track.year,
                                "genre": track.genre,
                                "raw_genre_names": track.raw_genre_names,
                                "duration": track.duration,
                                "sample_rate": track.sample_rate,
                                "bit_depth": track.bit_depth,
                                "bitrate": track.bitrate,
                                "file_size": track.file_size,
                                "file_format": track.file_format,
                                "has_artwork": track.has_artwork,
                                "replaygain_track_gain": track.replaygain_track_gain,
                                "replaygain_album_gain": track.replaygain_album_gain,
                            });
                            if let Ok(mut pending) = state.pending_external_file.lock() {
                                *pending = Some(payload.clone());
                            }
                            if let Some(main_win) = app.get_webview_window("main") {
                                let _ = main_win.show();
                                let _ = main_win.set_focus();
                            }
                            let _ = app.emit("external-file-received", payload);
                        }
                    }
                }
            }
        }))
        .setup(|app| {
            let t_setup = std::time::Instant::now();
            macro_rules! stage {
                ($label:expr) => {
                    log::info!("[startup] {} (+{:?})", $label, t_setup.elapsed());
                };
            }
            let data_dir = app_data_dir(app);
            std::fs::create_dir_all(&data_dir).ok();

            // Setup panic hook with correct data dir
            let dh = data_dir.clone();
            eiryn_lib::crash::setup_panic_hook(dh);

            // Detect crash from previous session
            let app_handle = app.handle().clone();
            let was_crash = eiryn_lib::session::was_crash(&app_handle);
            if was_crash {
                log::warn!("Previous session ended with a crash");
                // Crash marker will be shown by frontend on session restore
            }
            eiryn_lib::session::mark_session_started(&app_handle);

            let crash_flag = std::sync::atomic::AtomicBool::new(was_crash);

            let db_path = data_dir.join("library.db");
            // Graceful startup: a corrupt DB is backed up by init_pool; retry
            // once on the fresh file instead of panicking on the main thread.
            let (pool, cue_schema_migrated) = match tauri::async_runtime::block_on(db::init_pool(&db_path)) {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Database init failed ({e}); retrying with a fresh database");
                    tauri::async_runtime::block_on(db::init_pool(&db_path))
                        .map_err(|e| format!("Failed to init database after retry: {e}"))?
                }
            };
            stage!("db init");
            
            let search_index_dir = data_dir.join("search_index");
            std::fs::create_dir_all(&search_index_dir).ok();
            // A corrupt search index should be rebuilt, not kill the app.
            let search = match TantivyIndex::new(&search_index_dir) {
                Ok(index) => index,
                Err(e) => {
                    log::warn!("Tantivy index failed to open ({e}); rebuilding from scratch");
                    let _ = std::fs::remove_dir_all(&search_index_dir);
                    std::fs::create_dir_all(&search_index_dir).ok();
                    TantivyIndex::new(&search_index_dir)
                        .map_err(|e| format!("Failed to rebuild tantivy search index: {e}"))?
                }
            };
            stage!("search index");

            let (player, volume) = Player::new(app_handle.clone());

            let cache_root = data_dir.clone();

            let artwork_cache_dir = cache_root.join("artist_artwork");
            let artist_cache = Arc::new(ArtistArtworkCache::new(artwork_cache_dir));
            let artist_worker = Arc::new(ArtistArtworkWorker::new());
            let track_artwork_cache = Arc::new(TrackArtworkCache::new(cache_root.join("track_artwork")));
            let lufs_cache = Arc::new(std::sync::Mutex::new(LufsCache::new()));

            #[cfg(windows)]
            let gpu_engine = match tauri::async_runtime::block_on(GpuArtworkEngine::initialize()) {
                Ok(engine) => {
                    log::info!("GPU artwork engine ready");
                    Some(engine)
                }
                Err(e) => {
                    log::warn!("GPU artwork engine unavailable (CPU fallback): {}", e);
                    None
                }
            };
            #[cfg(windows)]
            stage!("gpu init");

            let queue_path = data_dir.join("queue_state.json");
            let queue_state = queue_persistence::load_queue(&queue_path).unwrap_or_else(QueueState::new);

            let tray_menu_holder: Arc<std::sync::Mutex<Option<TrayMenu>>> = Arc::new(std::sync::Mutex::new(None));

            let state = AppState {
                player: Arc::new(player),
                db: pool.clone(),
                search: Arc::new(std::sync::Mutex::new(search)),
                queue_state: Arc::new(std::sync::Mutex::new(queue_state)),
                volume: volume.clone(),
                media: Arc::new(std::sync::Mutex::new(None)),
                remote: remote::RemoteControl::new(),
                artwork_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                artist_artwork_cache: artist_cache.clone(),
                artist_artwork_worker: artist_worker.clone(),
                track_artwork_cache: track_artwork_cache.clone(),
                palette_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                cache_root: Arc::new(std::sync::Mutex::new(cache_root)),
                is_scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                is_analyzing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                is_loudness_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                is_fetching: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                is_fetching_genre: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                current_track_id: Arc::new(std::sync::Mutex::new(None)),
                lufs_cache: Some(lufs_cache),
                #[cfg(windows)]
                gpu_engine,
                tray_menu: tray_menu_holder.clone(),
                was_crash: crash_flag,
                pending_external_file: Arc::new(std::sync::Mutex::new(None)),
                file_player_counter: std::sync::atomic::AtomicU32::new(0),
            };
            app.manage(state);

            // Create windows FIRST so WebView2 + frontend boot in parallel
            // with the remaining setup below (EQ/tray/watchers). All commands
            // work from here because state is already managed.
            // Create windows programmatically (none in tauri.conf.json)
            let args: Vec<String> = std::env::args().collect();
            let is_external_file = if args.len() > 1 {
                let file_path = &args[1];
                let path = std::path::Path::new(file_path);
                if path.exists() {
                    let ext = path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let audio_exts = ["mp3","flac","wav","m4a","ogg","wma","aac",
                                      "opus","wv","aiff","dsf","dff","ape","mpc","tta","mid"];
                    audio_exts.contains(&ext.as_str())
                } else {
                    false
                }
            } else {
                false
            };

            if is_external_file {
                let file_path = &args[1];
                log::info!("Opening audio file: {}", file_path);
                let state = app.state::<AppState>();

                let payload = match Track::from_path(std::path::Path::new(file_path)) {
                    Ok(track) => {
                        let track_id = uuid::Uuid::new_v4().to_string();
                        serde_json::json!({
                            "id": track_id,
                            "file_path": file_path,
                            "title": track.title,
                            "artist": track.artist,
                            "album": track.album,
                            "album_artist": track.album_artist,
                            "composer": track.composer,
                            "track_number": track.track_number,
                            "disc_number": track.disc_number,
                            "year": track.year,
                            "genre": track.genre,
                            "raw_genre_names": track.raw_genre_names,
                            "duration": track.duration,
                            "sample_rate": track.sample_rate,
                            "bit_depth": track.bit_depth,
                            "bitrate": track.bitrate,
                            "file_size": track.file_size,
                            "file_format": track.file_format,
                            "has_artwork": track.has_artwork,
                            "replaygain_track_gain": track.replaygain_track_gain,
                            "replaygain_album_gain": track.replaygain_album_gain,
                        })
                    }
                    Err(e) => {
                        log::warn!("Could not read metadata from {}: {}", file_path, e);
                        serde_json::json!({
                            "id": uuid::Uuid::new_v4().to_string(),
                            "file_path": file_path,
                            "title": "",
                            "artist": "",
                            "duration": 0,
                        })
                    }
                };

                if let Ok(mut pending) = state.pending_external_file.lock() {
                    *pending = Some(payload);
                }

                // Create file-player window (playback deferred to FilePlayerView)
                let counter = state.file_player_counter.fetch_add(1, Ordering::Relaxed);
                let label = format!("file-player-{counter}");
                let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html#/file-player".into()))
                    .title("Eiryn")
                    .inner_size(420.0, 640.0)
                    .resizable(false)
                    .decorations(false)
                    .center();
                if let Some(icon) = app.default_window_icon() {
                    builder = builder.icon(icon.clone())?;
                }
                let _win = builder.build()?;

                #[cfg(windows)]
                {
                    let hwnd = _win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
                    if hwnd != 0 {
                        let tx = eiryn_lib::media::start(hwnd, app.handle().clone());
                        if let Ok(mut guard) = app.state::<AppState>().media.lock() {
                            *guard = Some(tx);
                        }
                    }
                }
            } else {
                // Normal launch - main window created from tauri.conf.json with native decorations
                let _win = app.get_webview_window("main").expect("main window");

                #[cfg(windows)]
                {
                    let hwnd = _win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
                    if hwnd != 0 {
                        let tx = eiryn_lib::media::start(hwnd, app.handle().clone());
                        if let Ok(mut guard) = app.state::<AppState>().media.lock() {
                            *guard = Some(tx);
                        }
                    }
                }

                stage!("window ready");
            }

            let db_pool = app.state::<AppState>().db.clone();
            let _ = tauri::async_runtime::block_on(eq::seed_defaults(&db_pool));
            // Restore persisted EQ state into the live engine (bands + enabled + preamp).
            {
                let state = app.state::<AppState>();
                let _ = tauri::async_runtime::block_on(commands::apply_eq_state(
                    &db_pool,
                    &state.player,
                ));
            }
            stage!("eq seed+restore");

            // Restore persisted bit-perfect preference into the live engine.
            {
                let player = app.state::<AppState>().player.clone();
                if let Ok(Some(flag)) = tauri::async_runtime::block_on(
                    db::get_setting(&db_pool, "bit_perfect"),
                ) {
                    if flag == "1" {
                        player.set_bit_perfect(true);
                    }
                }
            }

            // Global shortcuts (media keys) for non-Windows platforms (Windows uses native SMTC via souvlaki)
            #[cfg(not(windows))]
            {
                let state = app.state::<AppState>();
                let player_shortcut = state.player.clone();
                let toggle_shortcut = Shortcut::new(Some(Modifiers::empty()), Code::MediaPlayPause);
                let next_shortcut = Shortcut::new(Some(Modifiers::empty()), Code::MediaTrackNext);
                let prev_shortcut = Shortcut::new(Some(Modifiers::empty()), Code::MediaTrackPrevious);
                let stop_shortcut = Shortcut::new(Some(Modifiers::empty()), Code::MediaStop);

                let _ = app.global_shortcut().on_shortcut(toggle_shortcut, move |_app, _shortcut, event: ShortcutEvent| {
                    if event.state == ShortcutState::Pressed {
                        player_shortcut.toggle();
                    }
                });
                let _ = app.global_shortcut().on_shortcut(next_shortcut, move |_a, _s, event: ShortcutEvent| {
                    if event.state == ShortcutState::Pressed {
                        let _ = _a.emit("media-key", "NextTrack");
                    }
                });
                let _ = app.global_shortcut().on_shortcut(prev_shortcut, move |_a, _s, event: ShortcutEvent| {
                    if event.state == ShortcutState::Pressed {
                        let _ = _a.emit("media-key", "PreviousTrack");
                    }
                });
                let _ = app.global_shortcut().on_shortcut(stop_shortcut, move |_a, _s, event: ShortcutEvent| {
                    if event.state == ShortcutState::Pressed {
                        let _ = _a.emit("media-key", "Stop");
                    }
                });
            }

            // Build tray menu
            let show_tray = tauri::async_runtime::block_on(
                db::get_setting(&pool, "show_tray_icon")
            ).ok().flatten().map(|v| v == "true").unwrap_or(true);

            if show_tray {
                if let Ok(tray_menu) = TrayMenu::new(app.handle()) {
                    let tray_icon = tray_menu.tray.clone();

                    let _app_clone = app.handle().clone();
                    // Menu event handler
                    tray_icon.on_menu_event(move |app_handle, event| {
                        let state = app_handle.state::<AppState>();
                        match event.id.0.as_str() {
                            "play_pause" => {
                                state.player.toggle();
                                // Update tray menu after state change
                                let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                                if let Some(ref tm) = *tray_guard {
                                    update_tray_from_state(tm, app_handle);
                                }
                            }
                            "next" => {
                                let _ = app_handle.emit("media-key", "NextTrack");
                            }
                            "prev" => {
                                let _ = app_handle.emit("media-key", "PreviousTrack");
                            }
                            "repeat" => {
                                let mode_str = if let Ok(mut q) = state.queue_state.lock() {
                                    let next = match q.repeat_mode {
                                        eiryn_lib::queue::RepeatMode::Off => eiryn_lib::queue::RepeatMode::All,
                                        eiryn_lib::queue::RepeatMode::All => eiryn_lib::queue::RepeatMode::One,
                                        eiryn_lib::queue::RepeatMode::One => eiryn_lib::queue::RepeatMode::Off,
                                    };
                                    let s = next.as_str().to_string();
                                    q.set_repeat_mode(next);
                                    if let Ok(p) = app_handle.path().app_data_dir() {
                                        queue_persistence::save_queue(&q, &p.join("queue_state.json"));
                                    }
                                    s
                                } else {
                                    "off".to_string()
                                };
                                let _ = app_handle.emit("repeat-mode-changed", &mode_str);
                                let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                                if let Some(ref tm) = *tray_guard {
                                    update_tray_from_state(tm, app_handle);
                                }
                            }
                            "shuffle" => {
                                let s = if let Ok(mut q) = state.queue_state.lock() {
                                    let shuffled = !q.shuffle;
                                    q.set_shuffle(shuffled);
                                    if let Ok(p) = app_handle.path().app_data_dir() {
                                        queue_persistence::save_queue(&q, &p.join("queue_state.json"));
                                    }
                                    shuffled
                                } else {
                                    false
                                };
                                let _ = app_handle.emit("shuffle-changed", s);
                                let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                                if let Some(ref tm) = *tray_guard {
                                    update_tray_from_state(tm, app_handle);
                                }
                            }
                            "favorite" => {
                                if let Ok(tid) = state.current_track_id.lock() {
                                    if let Some(ref id) = *tid {
                                        let db = state.db.clone();
                                        let id2 = id.clone();
                                        tauri::async_runtime::spawn(async move {
                                            let _ = db::toggle_favorite(&db, &id2).await;
                                        });
                                    }
                                }
                            }
                            "mini" => {
                                let app = app_handle.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = commands::toggle_mini_window(app).await;
                                });
                            }
                            "show" => {
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                            "quit" => {
                                let state = app_handle.state::<AppState>();
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
                                let data = eiryn_lib::session::SessionData {
                                    track_id, position: pos, volume: vol,
                                    repeat_mode: rm, shuffle: sh, queue_index: qi,
                                    gapless: gp, crossfade: cf,
                                };
                                eiryn_lib::session::save_session(app_handle, &data);
                                eiryn_lib::session::save_clean_exit(app_handle);
                                app_handle.exit(0);
                            }
                            _ => {}
                        }
                    });

                    let tray_icon2 = tray_menu.tray.clone();
                    tray_icon2.on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    });

                    // Initial menu update
                    update_tray_from_state(&tray_menu, app.handle());

                    // Store in AppState
                    *tray_menu_holder.lock().unwrap() = Some(tray_menu);

                    // Keep tray menu synchronized with player events from UI / audio engine
                    let app_tray = app.handle().clone();
                    app.listen("player:status", move |_| {
                        let state = app_tray.state::<AppState>();
                        let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(ref tm) = *tray_guard {
                            update_tray_from_state(tm, &app_tray);
                        }
                    });
                    let app_tray2 = app.handle().clone();
                    app.listen("track-started", move |_| {
                        let state = app_tray2.state::<AppState>();
                        let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(ref tm) = *tray_guard {
                            update_tray_from_state(tm, &app_tray2);
                        }
                    });
                    let app_tray3 = app.handle().clone();
                    app.listen("playback-ended", move |_| {
                        let state = app_tray3.state::<AppState>();
                        let tray_guard = state.tray_menu.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(ref tm) = *tray_guard {
                            update_tray_from_state(tm, &app_tray3);
                        }
                    });
                }
            }
            stage!("tray menu");

            // Start background artist artwork worker
            let worker_state = app.state::<AppState>();
            artist_worker.start(worker_state.db.clone(), artist_cache, app.handle().clone());

            // Start file watchers for all library folders
            let state = app.state::<AppState>();
            let folders = tauri::async_runtime::block_on(
                eiryn_lib::library::db::get_library_folders(&state.db)
            ).unwrap_or_default();
            for folder_path in &folders {
                let path = PathBuf::from(folder_path);
                if path.exists() {
                    if let Err(e) = eiryn_lib::library::start_windows_watcher(state.db.clone(), path) {
                        log::warn!("Failed to start file watcher for {}: {}", folder_path, e);
                    }
                }
            }
            stage!("watchers");

            // One-time healing after the CUE schema migration: legacy scans
            // collapsed CUE children into a single corrupted row (dropped by
            // the migration), so a full rescan regenerates parent + children.
            if cue_schema_migrated {
                log::info!("CUE schema migrated — rescanning library to restore CUE child tracks");
                let healing_db = state.db.clone();
                let healing_cache = state.track_artwork_cache.clone();
                tauri::async_runtime::spawn(async move {
                    let healing_folders = eiryn_lib::library::db::get_library_folders(&healing_db)
                        .await
                        .unwrap_or_default();
                    for fp in healing_folders {
                        let path = PathBuf::from(fp);
                        if path.exists() {
                            let db2 = healing_db.clone();
                            let cache2 = healing_cache.clone();
                            tauri::async_runtime::spawn(async move {
                                match scanner::scan_folder(&db2, &path, &cache2).await {
                                    Ok(result) => log::info!(
                                        "Healing rescan {}: added={}, updated={}, removed={}",
                                        path.display(), result.added, result.updated, result.removed
                                    ),
                                    Err(e) => log::error!("Healing rescan failed for {}: {}", path.display(), e),
                                }
                            });
                        }
                    }
                });
            }

            // Background search index rebuild if empty (e.g. after schema migration)
            {
                let search_arc = state.search.clone();
                let startup_db = state.db.clone();
                tauri::async_runtime::spawn(async move {
                    let count = {
                        if let Ok(s) = search_arc.lock() {
                            s.num_docs()
                        } else {
                            1
                        }
                    };
                    if count == 0 {
                        log::info!("Search index is empty, rebuilding with tracks and lyrics...");
                        if let Ok(tracks) = db::get_all_tracks(&startup_db).await {
                            if !tracks.is_empty() {
                                commands::rebuild_search_with_lyrics(&startup_db, &search_arc, &tracks).await;
                                log::info!("Search index rebuilt with {} tracks and lyrics", tracks.len());
                            }
                        }
                    }
                });
            }

            // Delayed background enrichment: genre (capped) + BPM audio analysis.
            // Full-spectrum analysis is manual-only (Library Settings button).
            {
                let startup_app = app.handle().clone();
                let startup_db = state.db.clone();
                let startup_analyzing = state.is_analyzing.clone();
                let startup_fetching = state.is_fetching_genre.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    let _ = commands::run_genre_enrichment(startup_app.clone(), startup_db.clone(), startup_fetching.clone(), Some(100)).await;
                    let _ = commands::run_audio_analysis(startup_app.clone(), startup_db.clone(), startup_analyzing).await;
                });
            }

            log::info!("[startup] setup complete (+{:?})", t_setup.elapsed());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_tracks,
            commands::get_track,
            commands::get_albums,
            commands::get_album_tracks,
            commands::search_tracks,
            commands::get_all_genres,
            commands::get_tracks_by_genre_id,
            commands::scan_folder,
            commands::get_artwork,
            commands::seek,
            commands::play_track,
            commands::toggle_play,
            commands::stop_playback,
            commands::set_volume,
            commands::set_eq_band,
            commands::set_eq_enabled,
            commands::set_karaoke,
            commands::get_karaoke,
            commands::set_eq_preamp,
            commands::get_eq_state,
            commands::get_position,
            commands::get_playlists,
            commands::get_playlist,
            commands::create_playlist,
            commands::delete_playlist,
            commands::rename_playlist,
            commands::update_playlist_rules,
            commands::refresh_smart_playlist,
            commands::export_playlist_m3u,
            commands::import_playlist_m3u,
            commands::export_library_backup,
            commands::restore_library_backup,
            commands::get_playlist_track_count,
            commands::reorder_playlist_tracks,
            commands::move_playlist_track,
            commands::add_to_playlist,
            commands::add_tracks_to_playlist,
            commands::remove_from_playlist,
            commands::get_playlist_tracks,
            commands::get_favorite_ids,
            commands::toggle_favorite,
            commands::get_library_folders,
            commands::add_library_folder,
            commands::add_library_folders,
            commands::remove_library_folder,
            commands::delete_tracks,
            commands::get_lyrics,
            commands::record_play,
            commands::finish_play,
            commands::clear_play_history,
            commands::get_recently_played_tracks,
            commands::get_most_listened_tracks,
            commands::get_least_listened_tracks,
            commands::get_listening_stats,
            commands::show_in_explorer,
            commands::search_all,
            commands::find_lyrics,
            commands::update_track_metadata,
            commands::get_playlists_for_track,
            commands::get_artist_artwork,
            commands::get_track_artwork,
            commands::get_file_artwork,
            commands::get_track_palette,
            commands::set_artwork,
            commands::prevent_sleep,
            commands::allow_sleep,
            commands::now_playing,
            commands::scrobble,
            commands::get_lastfm_auth_url,
            commands::get_lastfm_config,
            commands::set_lastfm_session,
            commands::set_lastfm_enabled,
            commands::disconnect_lastfm,
            commands::set_lastfm_api_key,
            commands::validate_lastfm_api_key,
            commands::remote_start,
            commands::remote_stop,
            commands::remote_status,
            commands::get_radio_stations,
            commands::add_radio_station,
            commands::delete_radio_station,
            commands::update_radio_station,
            commands::play_radio,
            commands::ping_audio,
            commands::is_audio_ready,
            commands::stop_radio,
            commands::radio_status,
            commands::get_eq_profiles,
            commands::get_active_eq_profile,
            commands::create_eq_profile,
            commands::rename_eq_profile,
            commands::delete_eq_profile,
            commands::apply_eq_profile,
            commands::update_eq_band,
            commands::save_lyrics,
            commands::get_track_lyrics,
            commands::get_all_lyrics,
            commands::batch_fetch_lyrics,
            commands::get_player_state,
            commands::set_queue,
            commands::next_track,
            commands::prev_track,
            commands::set_repeat_mode,
            commands::set_shuffle,
            commands::get_queue_state,
            commands::remove_queue_track,
            commands::insert_queue_tracks,
            commands::reorder_queue,
            commands::append_queue_tracks,
            commands::set_gapless,
            commands::set_crossfade,
            commands::set_bit_perfect,
            commands::get_output_state,
            commands::process_artwork_gpu,
            commands::create_folder_playlist,
            commands::refresh_folder_playlists,
            commands::prepare_next_track,
            commands::advance_queue,
            commands::save_session,
            commands::restore_session,
            commands::clear_session,
            commands::dismiss_crash_report,
            commands::consume_pending_external_file,
            commands::play_external_file,
            commands::generate_smart_mix,
            commands::analyze_audio,
            commands::batch_fetch_artist_artwork,
            commands::batch_fetch_genre,
            commands::get_genre_progress,
            commands::build_genre_queue,
            commands::quit_app,
            commands::get_track_loudness,
            commands::calculate_track_lufs,
            commands::run_loudness_backfill,
            commands::run_full_analysis_backfill,
            commands::get_analysis_progress,
            commands::recompute_moods,
            commands::get_track_analysis,
            commands::get_normalization_settings,
            commands::set_normalization_settings,
            commands::get_loudness_progress,
            commands::get_listening_insights,
            commands::get_library_insights,
            commands::get_setting_value,
            commands::set_setting_value,
            commands::import_autoeq_profile,
            commands::get_crossfeed_settings,
            commands::set_crossfeed_settings,
            commands::get_signal_path,
            commands::get_library_health_summary,
            commands::get_suspect_tracks,
            commands::get_duplicate_tracks,
            commands::get_damaged_tracks,
            commands::verify_track_integrity,
            commands::delete_track_to_trash,
            commands::analyze_track_quality,
            commands::get_split_albums,
            commands::merge_split_album,
            commands::get_incomplete_albums,
            commands::get_dual_script_artists,
            commands::merge_artist_names,
            commands::toggle_mini_window,
            commands::show_main_window,
            commands::remove_window_border,
            commands::get_audio_devices,
            commands::set_audio_device,
            commands::get_device_sound_settings,
            commands::save_device_sound_settings,
            commands::set_eq_mode,
            commands::apply_parametric_eq,
            commands::get_parametric_eq_state,
            commands::parse_autoeq_text,
            #[cfg(debug_assertions)]
            commands::toggle_devtools,
        ])
        .run(tauri::generate_context!())
        .expect("Failed to run application");
}
