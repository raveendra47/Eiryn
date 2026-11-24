//! Windows Media Controls (SMTC): media keys, Windows 11 Notification Flyout,
//! Volume & Lock Screen overlays powered by `souvlaki`.

use std::time::Duration;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::Emitter;

/// What the Windows SMTC overlay should display.
#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
    pub duration: f64,
}

#[derive(Clone, Debug)]
pub enum Update {
    Track(Option<NowPlaying>),
    Playback { playing: bool, position: f64 },
}

pub fn start(
    hwnd: isize,
    app_handle: tauri::AppHandle,
) -> std::sync::mpsc::Sender<Update> {
    let (tx, rx) = std::sync::mpsc::channel::<Update>();
    std::thread::Builder::new()
        .name("eiryn-media-smtc".into())
        .spawn(move || run(hwnd, rx, app_handle))
        .expect("failed to spawn media smtc thread");
    tx
}

fn run(
    hwnd: isize,
    rx: std::sync::mpsc::Receiver<Update>,
    app_handle: tauri::AppHandle,
) {
    let config = PlatformConfig {
        display_name: "Eiryn",
        dbus_name: "eiryn",
        hwnd: (hwnd != 0).then_some(hwnd as *mut std::ffi::c_void),
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Windows SMTC media controls unavailable: {e:?}");
            return;
        }
    };

    let app_for_event = app_handle.clone();
    let attached = controls.attach(move |event| {
        log::debug!("SMTC event received: {event:?}");
        let app = app_for_event.clone();
        tauri::async_runtime::spawn(async move {
            use tauri::Manager;
            let state = app.state::<crate::state::AppState>();
            match event {
                MediaControlEvent::Play => {
                    if !state.player.is_playing() {
                        let _ = state.player.toggle();
                        let _ = app.emit("player:status", serde_json::json!({"playing": true}));
                        crate::commands::update_smtc_playback(&state, true, state.player.position() as f64);
                    }
                }
                MediaControlEvent::Pause => {
                    if state.player.is_playing() {
                        let _ = state.player.toggle();
                        let _ = app.emit("player:status", serde_json::json!({"playing": false}));
                        crate::commands::update_smtc_playback(&state, false, state.player.position() as f64);
                    }
                }
                MediaControlEvent::Toggle => {
                    let playing = state.player.toggle();
                    let _ = app.emit("player:status", serde_json::json!({"playing": playing}));
                    crate::commands::update_smtc_playback(&state, playing, state.player.position() as f64);
                }
                MediaControlEvent::Next => {
                    let _ = crate::commands::next_track(state.clone(), app.clone()).await;
                }
                MediaControlEvent::Previous => {
                    let _ = crate::commands::prev_track(state.clone(), app.clone()).await;
                }
                MediaControlEvent::Stop => {
                    state.player.stop();
                    let _ = app.emit("player:status", serde_json::json!({"playing": false}));
                    crate::commands::update_smtc_playback(&state, false, 0.0);
                }
                MediaControlEvent::SetPosition(MediaPosition(p)) => {
                    state.player.seek(p.as_secs_f64() as f32);
                    let _ = app.emit("player:seek", p.as_secs_f64());
                    crate::commands::update_smtc_playback(&state, state.player.is_playing(), p.as_secs_f64());
                }
                MediaControlEvent::SeekBy(dir, by) => {
                    let current = state.player.position();
                    let delta = match dir {
                        SeekDirection::Forward => by.as_secs_f64(),
                        SeekDirection::Backward => -by.as_secs_f64(),
                    };
                    let new_pos = (current as f64 + delta).max(0.0);
                    state.player.seek(new_pos as f32);
                    let _ = app.emit("player:seek", new_pos);
                    crate::commands::update_smtc_playback(&state, state.player.is_playing(), new_pos);
                }
                MediaControlEvent::Seek(dir) => {
                    let current = state.player.position();
                    let delta = if matches!(dir, SeekDirection::Forward) { 10.0 } else { -10.0 };
                    let new_pos = (current as f64 + delta).max(0.0);
                    state.player.seek(new_pos as f32);
                    let _ = app.emit("player:seek", new_pos);
                    crate::commands::update_smtc_playback(&state, state.player.is_playing(), new_pos);
                }
                _ => {}
            }
        });
    });

    if let Err(e) = attached {
        log::warn!("Failed to attach SMTC media controls handler: {e:?}");
        return;
    }

    let mut current_duration = 0.0;
    for update in rx {
        let result = match update {
            Update::Track(Some(np)) => {
                current_duration = np.duration;
                let title = if np.title.trim().is_empty() { "Unknown Track" } else { np.title.trim() };
                let artist = if np.artist.trim().is_empty() { "Unknown Artist" } else { np.artist.trim() };
                let album = (!np.album.trim().is_empty()).then_some(np.album.trim());

                // Format cover_url to file://D:\... for Windows WinRT StorageFile compatibility
                let cover_clean = np.cover.as_ref().map(|url| {
                    if url.starts_with("file://") {
                        let stripped = url.trim_start_matches("file://");
                        let path = stripped.trim_start_matches('/').trim_start_matches('\\').replace('/', "\\");
                        format!("file://{path}")
                    } else {
                        url.clone()
                    }
                });

                let set_res = controls.set_metadata(MediaMetadata {
                    title: Some(title),
                    artist: Some(artist),
                    album,
                    cover_url: cover_clean.as_deref(),
                    duration: (np.duration > 0.0).then(|| Duration::from_secs_f64(np.duration)),
                });

                if let Err(e) = set_res {
                    log::warn!("Failed to set SMTC metadata with cover ({e:?}); retrying without cover...");
                    if let Err(e2) = controls.set_metadata(MediaMetadata {
                        title: Some(title),
                        artist: Some(artist),
                        album,
                        cover_url: None,
                        duration: (np.duration > 0.0).then(|| Duration::from_secs_f64(np.duration)),
                    }) {
                        log::warn!("Failed to set fallback SMTC metadata: {e2:?}");
                    }
                }

                controls.set_playback(MediaPlayback::Playing {
                    progress: Some(MediaPosition(Duration::from_secs_f64(0.0))),
                })
            }
            Update::Track(None) => controls.set_playback(MediaPlayback::Stopped),
            Update::Playback { playing, position } => {
                let progress = Some(MediaPosition(Duration::from_secs_f64(
                    position.clamp(0.0, current_duration.max(position)),
                )));
                controls.set_playback(if playing {
                    MediaPlayback::Playing { progress }
                } else {
                    MediaPlayback::Paused { progress }
                })
            }
        };

        if let Err(e) = result {
            log::debug!("Failed to update SMTC controls: {e:?}");
        }
    }
}
