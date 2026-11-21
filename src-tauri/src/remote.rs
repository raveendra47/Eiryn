use crate::audio::player::Player;
use crate::library::db;
use crate::library::track::Track;
use crate::queue::QueueState;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tiny_http::{Header, Response, Server};

const MAX_AUTH_FAILURES: u32 = 10;
const AUTH_LOCKOUT: Duration = Duration::from_secs(60);

/// Per-IP authentication failure tracking. After `MAX_AUTH_FAILURES` failed
/// attempts the address is locked out for `AUTH_LOCKOUT`.
#[derive(Default)]
struct AuthGuard {
    failures: HashMap<IpAddr, u32>,
    locked_until: HashMap<IpAddr, Instant>,
}

impl AuthGuard {
    fn is_locked(&self, ip: &IpAddr) -> bool {
        self.locked_until
            .get(ip)
            .map(|t| *t > Instant::now())
            .unwrap_or(false)
    }

    fn record_failure(&mut self, ip: IpAddr) {
        let fails = self.failures.entry(ip).or_insert(0);
        *fails += 1;
        if *fails >= MAX_AUTH_FAILURES {
            self.failures.remove(&ip);
            self.locked_until.insert(ip, Instant::now() + AUTH_LOCKOUT);
            log::warn!("Remote auth lockout for {ip} after {MAX_AUTH_FAILURES} failed attempts");
        }
    }

    fn record_success(&mut self, ip: &IpAddr) {
        self.failures.remove(ip);
        self.locked_until.remove(ip);
    }

    fn clear_expired(&mut self, ip: &IpAddr) {
        if let Some(t) = self.locked_until.get(ip) {
            if *t <= Instant::now() {
                self.locked_until.remove(ip);
            }
        }
    }
}

pub struct RemoteControl {
    running: Arc<AtomicBool>,
}

impl RemoteControl {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self, app_handle: AppHandle, player: Arc<Player>, pool: SqlitePool, queue: Arc<Mutex<QueueState>>, volume: Arc<AtomicU32>, port: u16, password: String) {
        if self.running.load(Ordering::Relaxed) {
            return;
        }
        self.running.store(true, Ordering::Relaxed);
        let running = self.running.clone();
        let auth = Arc::new(Mutex::new(AuthGuard::default()));

        thread::spawn(move || {
            let addr = format!("0.0.0.0:{}", port);
            let server = match Server::http(&addr) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Remote server failed to bind {addr}: {e}");
                    running.store(false, Ordering::Relaxed);
                    return;
                }
            };
            log::info!("Remote control server listening on {addr}");

            let rt = tokio::runtime::Runtime::new().unwrap();

            loop {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                match server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(request)) => {
                        let pw = password.clone();
                        let ah = app_handle.clone();
                        let a = auth.clone();
                        let p = player.clone();
                        let po = pool.clone();
                        let q = queue.clone();
                        let v = volume.clone();
                        rt.spawn(async move {
                            handle_request(request, pw, a, ah, p, po, q, v).await;
                        });
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
            running.store(false, Ordering::Relaxed);
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Constant-time comparison: iteration never short-circuits on a byte
/// mismatch, so timing leaks nothing beyond the (acceptable) length check.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn check_auth(request: &tiny_http::Request, password: &str) -> bool {
    // Fail closed: an empty configured password must never open the server.
    // remote_start rejects empty/short passwords at the boundary as well.
    if password.is_empty() {
        return false;
    }
    let auth_header = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("authorization"))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default();
    constant_time_eq(&auth_header, &format!("Bearer {}", password))
        || constant_time_eq(&auth_header, password)
}

fn json_response<T: serde::Serialize>(data: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_string(data).unwrap_or_default();
    Response::from_data(body)
        .with_header(
            Header::from_bytes("Content-Type", "application/json").unwrap(),
        )
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"error":"{msg}"}}"#);
    Response::from_string(body)
        .with_status_code(status)
        .with_header(
            Header::from_bytes("Content-Type", "application/json").unwrap(),
        )
}

fn ok_response(msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"ok":"{msg}"}}"#);
    Response::from_string(body)
        .with_header(
            Header::from_bytes("Content-Type", "application/json").unwrap(),
        )
}

async fn handle_request(
    request: tiny_http::Request,
    password: String,
    auth: Arc<Mutex<AuthGuard>>,
    app_handle: AppHandle,
    player: Arc<Player>,
    pool: SqlitePool,
    queue: Arc<Mutex<QueueState>>,
    volume: Arc<AtomicU32>,
) {
    // tiny_http exposes the peer as Option<&SocketAddr>; fall back to an
    // unspecified address if it is missing so rate limiting still applies.
    let ip = request
        .remote_addr()
        .map(|sa| sa.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    {
        let mut guard = auth.lock().unwrap();
        guard.clear_expired(&ip);
        if guard.is_locked(&ip) {
            let resp = error_response(429, "Too many attempts, try again later");
            let _ = request.respond(resp);
            return;
        }
    }
    if !check_auth(&request, &password) {
        auth.lock().unwrap().record_failure(ip);
        let resp = error_response(401, "Unauthorized");
        let _ = request.respond(resp);
        return;
    }
    auth.lock().unwrap().record_success(&ip);

    let url = request.url().to_string();
    let method = request.method().as_str().to_string();

    let response = match (method.as_str(), url.as_str()) {
        ("GET", "/status") => {
            let status = serde_json::json!({
                "is_playing": player.is_playing(),
                "volume": f32::from_bits(volume.load(Ordering::Relaxed)),
                "position": player.position(),
            });
            json_response(&status)
        }
        ("POST", "/play") => {
            player.toggle();
            ok_response("toggled")
        }
        ("POST", "/pause") => {
            if player.is_playing() {
                player.toggle();
            }
            ok_response("paused")
        }
        ("POST", "/stop") => {
            player.stop();
            ok_response("stopped")
        }
        ("GET", "/queue") => {
            let q = queue.lock().unwrap();
            json_response(&q.original_list)
        }
        _ if method == "POST" && url.starts_with("/play-track/") => {
            let track_id = &url["/play-track/".len()..];
            if track_id.is_empty() {
                error_response(400, "Missing track ID")
            } else {
                match db::get_track(&pool, track_id).await {
                    Ok(Some(track)) => {
                        let (play_path, cue_off) = if let Some(ref pid) = track.cue_parent_id {
                            // Play parent file and seek to offset
                            let p = db::get_track(&pool, pid).await.ok().flatten()
                                .map(|t| std::path::PathBuf::from(t.file_path))
                                .unwrap_or_else(|| std::path::PathBuf::from(&track.file_path));
                            (p, track.cue_offset)
                        } else {
                            (std::path::PathBuf::from(&track.file_path), 0.0)
                        };
                        // Look up ReplayGain for the track
                        let rg_gain = {
                            if let Ok(Some(full)) = db::get_track(&pool, &track.id).await {
                                if full.replaygain_track_gain.abs() > 0.01 {
                                    full.replaygain_track_gain as f32
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            }
                        };
                        if let Err(e) = player.play_track(play_path, app_handle, rg_gain, track.sample_rate, Some(track.id.clone())) {
                            let resp = error_response(500, &e);
                            let _ = request.respond(resp);
                            return;
                        }
                        if cue_off > 0.0 {
                            player.seek(cue_off as f32);
                        }
                        ok_response("playing")
                    }
                    Ok(None) => error_response(404, "Track not found"),
                    Err(e) => error_response(500, &e),
                }
            }
        }
        _ if url.starts_with("/search") => {
            let query = url
                .split('?')
                .nth(1)
                .and_then(|qs| {
                    qs.split('&')
                        .find(|p| p.starts_with("q="))
                        .map(|p| &p[2..])
                })
                .unwrap_or_default();
            if query.is_empty() {
                error_response(400, "Missing query param q")
            } else {
                let tracks = db::get_all_tracks(&pool).await.unwrap_or_default();
                let filtered: Vec<&Track> = tracks
                    .iter()
                    .filter(|t| {
                        t.title.to_lowercase().contains(&query.to_lowercase())
                            || t.artist.to_lowercase().contains(&query.to_lowercase())
                            || t.album.to_lowercase().contains(&query.to_lowercase())
                    })
                    .collect();
                json_response(&filtered)
            }
        }
        _ => error_response(404, "Not found"),
    };

    let _ = request.respond(response);
}

