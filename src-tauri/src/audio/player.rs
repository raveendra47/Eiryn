//! High-performance pure-Rust audiophile playback engine for Eiryn.
//!
//! Powered by:
//! - `symphonia`: Pure Rust lossless/lossy decoder (FLAC, MP3, AAC, ALAC, WAV, OGG, Opus).
//! - `cpal`: Cross-platform shared-mode audio output.
//! - `wasapi`: Direct Windows WASAPI Exclusive bit-perfect output with MMCSS Pro Audio scheduling.
//! - `ringbuf`: Lock-free SPSC buffer between decoding and audio device callback.
//! - `rubato`: Audiophile-grade FFT sample rate conversion.
//! - RBJ biquad equalizer and bs2b headphone crossfeed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use ringbuf::traits::{Observer, Producer};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::crossfeed::{Crossfeed, CrossfeedSettings};
use super::eq::Equalizer;
use super::output::{Output, Shared};
use super::resample::Resampler;
use super::source::Source;
use crate::radio;

const STATE_STOPPED: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_PAUSED: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputState {
    pub bit_perfect_requested: bool,
    pub bit_perfect_active: bool,
    pub sample_rate: u32,
    pub format: usize,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
struct Boundary {
    start: u64,
    track_id: String,
    offset: f64,
}

enum Cmd {
    Ping {
        result_tx: mpsc::Sender<()>,
    },
    Play {
        path: PathBuf,
        handle: AppHandle,
        replaygain_db: f32,
        #[allow(dead_code)]
        sample_rate: u32,
        track_id: Option<String>,
        result_tx: mpsc::Sender<Result<(), String>>,
    },
    PrepareNext {
        path: PathBuf,
        handle: AppHandle,
        replaygain_db: f32,
        track_id: String,
        #[allow(dead_code)]
        sample_rate: u32,
    },
    SetGapless(bool),
    SetCrossfade(f32),
    SetBitPerfect(bool),
    PlayRadio {
        url: String,
        handle: AppHandle,
    },
    StopRadio,
    Toggle,
    Seek(f32),
    SetVolume(f32),
    SetReplayGain(f32),
    SetEqBand {
        index: u8,
        gain_db: f32,
    },
    SetEqEnabled(bool),
    SetKaraoke(bool),
    SetPreamp(f32),
    SetParametricFilters(Vec<super::eq::Filter>),
    SetEqMode(String),
    SetOutputDevice(Option<String>),
    ClearNext,
    Stop,
}

pub struct Player {
    cmd_tx: mpsc::Sender<Cmd>,
    position_secs: Arc<AtomicU32>,
    state: Arc<AtomicU8>,
    radio_active: Arc<AtomicBool>,
    gapless: Arc<AtomicBool>,
    crossfade_secs: Arc<AtomicU32>,
    bp_requested: Arc<AtomicBool>,
    bp_active: Arc<AtomicBool>,
    bp_rate: Arc<AtomicU32>,
    bp_format: Arc<AtomicU32>,
    bp_note: Arc<Mutex<Option<String>>>,
    transition_lock: Arc<AtomicBool>,
    engine_ready: Arc<AtomicBool>,
    preferred_device: Arc<Mutex<Option<String>>>,
    karaoke_enabled: Arc<AtomicBool>,
}

/// Channel mapping: maps any input channel count to output device channel count.
/// Stereo -> 5.1/7.1: front L/R populated, surrounds padded with silence.
/// Mono -> Stereo/Surround: replicated across channels.
/// 5.1/Surround -> Stereo: downmixed with proper ITU-R BS.775 normalization.
fn map_channels(input: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    out.clear();
    if from == to {
        out.extend_from_slice(input);
        return;
    }
    const K: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let frames = input.len() / from.max(1);
    out.reserve(frames * to);
    for f in input.chunks_exact(from) {
        match (from, to) {
            (1, _) => out.extend(std::iter::repeat_n(f[0], to)),
            (_, 1) => out.push(f.iter().sum::<f32>() / from as f32),
            (2, _) => {
                out.extend_from_slice(&f[..2]);
                out.extend(std::iter::repeat_n(0.0, to - 2));
            }
            (_, 2) => {
                let c = f.get(2).copied().unwrap_or(0.0);
                let (mut ls, mut rs) = (0.0, 0.0);
                for (i, s) in f.iter().enumerate().skip(4) {
                    if i % 2 == 0 {
                        ls += s;
                    } else {
                        rs += s;
                    }
                }
                let norm = 1.0 / (1.0 + K + K);
                out.push((f[0] + K * c + K * ls) * norm);
                out.push((f[1] + K * c + K * rs) * norm);
            }
            _ => {
                out.extend(f.iter().take(to).copied());
                out.extend(std::iter::repeat_n(0.0, to.saturating_sub(from)));
            }
        }
    }
}

/// Flushes buffered audio and synchronizes cleanly with the output renderer.
fn flush_output(
    shared: &Arc<Shared>,
    pending: &mut Vec<f32>,
    written: &mut u64,
    boundaries: &mut Vec<Boundary>,
    resampler: &mut Option<Resampler>,
) {
    shared.flush.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_millis(250);
    while shared.flush.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    shared.flush.store(false, Ordering::Release);
    shared.consumed.store(0, Ordering::Release);
    pending.clear();
    *written = 0;
    boundaries.clear();
    if let Some(ref mut r) = resampler {
        r.reset();
    }
}

impl Player {
    pub fn new(app_handle: AppHandle) -> (Self, Arc<AtomicU32>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let position_secs = Arc::new(AtomicU32::new(0));
        let state = Arc::new(AtomicU8::new(STATE_STOPPED));
        let radio_active = Arc::new(AtomicBool::new(false));
        let gapless = Arc::new(AtomicBool::new(true));
        let crossfade_secs = Arc::new(AtomicU32::new(f32::to_bits(0.0)));
        let bp_requested = Arc::new(AtomicBool::new(false));
        let bp_active = Arc::new(AtomicBool::new(false));
        let bp_rate = Arc::new(AtomicU32::new(0));
        let bp_format = Arc::new(AtomicU32::new(0));
        let bp_note = Arc::new(Mutex::new(None));
        let transition_lock = Arc::new(AtomicBool::new(false));
        let engine_ready = Arc::new(AtomicBool::new(false));
        let volume_atomic = Arc::new(AtomicU32::new(f32::to_bits(1.0)));

        let shared = Arc::new(Shared::new(1.0));

        let p_pos = position_secs.clone();
        let p_state = state.clone();
        let p_radio = radio_active.clone();
        let p_gapless = gapless.clone();
        let p_crossfade = crossfade_secs.clone();
        let p_bp_req = bp_requested.clone();
        let p_bp_act = bp_active.clone();
        let p_bp_rate = bp_rate.clone();
        let p_bp_fmt = bp_format.clone();
        let p_bp_note = bp_note.clone();
        let p_tlock = transition_lock.clone();
        let p_ready = engine_ready.clone();
        let p_vol = volume_atomic.clone();
        let preferred_device = Arc::new(Mutex::new(None));
        let p_pref_dev = preferred_device.clone();
        let karaoke_enabled = Arc::new(AtomicBool::new(false));
        let p_karaoke = karaoke_enabled.clone();

        std::thread::Builder::new()
            .name("eiryn-audio-worker".into())
            .spawn(move || {
                Self::worker_loop(
                    app_handle,
                    cmd_rx,
                    shared,
                    p_pos,
                    p_state,
                    p_radio,
                    p_gapless,
                    p_crossfade,
                    p_bp_req,
                    p_bp_act,
                    p_bp_rate,
                    p_bp_fmt,
                    p_bp_note,
                    p_tlock,
                    p_ready,
                    p_vol,
                    p_pref_dev,
                    p_karaoke,
                );
            })
            .expect("failed to spawn eiryn-audio-worker thread");

        (
            Self {
                cmd_tx,
                position_secs,
                state,
                radio_active,
                gapless,
                crossfade_secs,
                bp_requested,
                bp_active,
                bp_rate,
                bp_format,
                bp_note,
                transition_lock,
                engine_ready,
                preferred_device,
                karaoke_enabled,
            },
            volume_atomic,
        )
    }

    fn worker_loop(
        app_handle: AppHandle,
        cmd_rx: mpsc::Receiver<Cmd>,
        shared: Arc<Shared>,
        pos: Arc<AtomicU32>,
        state: Arc<AtomicU8>,
        radio_active: Arc<AtomicBool>,
        gapless: Arc<AtomicBool>,
        crossfade_secs: Arc<AtomicU32>,
        bp_requested: Arc<AtomicBool>,
        bp_active: Arc<AtomicBool>,
        bp_rate: Arc<AtomicU32>,
        bp_format: Arc<AtomicU32>,
        bp_note: Arc<Mutex<Option<String>>>,
        transition_lock: Arc<AtomicBool>,
        engine_ready: Arc<AtomicBool>,
        vol: Arc<AtomicU32>,
        pref_dev: Arc<Mutex<Option<String>>>,
        karaoke_flag: Arc<AtomicBool>,
    ) {
        let mut current_device: Option<String> = pref_dev.lock().ok().and_then(|g| g.clone());
        let mut out: Option<Output> = match super::output::open_shared(shared.clone(), current_device.as_deref()) {
            Ok(o) => Some(o),
            Err(e) => {
                log::error!("Failed to open default CPAL audio output: {e}");
                None
            }
        };

        engine_ready.store(true, Ordering::Release);

        let mut current_source: Option<Source> = None;
        let mut next_source: Option<Source> = None;
        let mut next_track_id: Option<String> = None;
        let mut current_track_id: String = String::new();
        let mut current_rg_db: f32 = 0.0;
        let mut next_rg_db: f32 = 0.0;
        let mut handle_opt: Option<AppHandle> = Some(app_handle.clone());

        let mut eq = Equalizer::new(2, 44100);
        let mut crossfeed = Crossfeed::new(CrossfeedSettings::default(), 44100);
        let mut limiter = super::limiter::Limiter::new(44100, 2);
        let mut karaoke = super::karaoke::KaraokeFilter::new(44100);
        let mut resampler: Option<Resampler> = None;
        let mut device_volumes: std::collections::HashMap<String, f32> = std::collections::HashMap::new();

        let mut _active_radio: Option<(std::thread::JoinHandle<()>, Arc<radio::SharedBuffer>)> = None;

        let mut pending: Vec<f32> = Vec::with_capacity(65536);
        let mut mapped_buf: Vec<f32> = Vec::with_capacity(16384);
        let mut written: u64 = 0;
        let mut boundaries: Vec<Boundary> = Vec::new();

        let mut source_rate: u32 = 44100;
        let mut last_pos_emit = Instant::now();

        loop {
            // 1. Process pending commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    Cmd::Ping { result_tx } => {
                        let _ = result_tx.send(());
                    }
                    Cmd::Play {
                        path,
                        handle,
                        replaygain_db,
                        sample_rate: _,
                        track_id,
                        result_tx,
                    } => {
                        handle_opt = Some(handle);
                        current_rg_db = replaygain_db;

                        // Stop radio if playing
                        if radio_active.load(Ordering::Acquire) {
                            radio_active.store(false, Ordering::Release);
                            _active_radio = None;
                        }

                        // Open source file
                        match Source::open(&path) {
                            Ok(src) => {
                                source_rate = src.rate;
                                current_track_id = track_id.unwrap_or_default();

                                // Check if bit-perfect is requested
                                let want_bp = bp_requested.load(Ordering::Acquire);
                                if want_bp {
                                    #[cfg(windows)]
                                    {
                                        let same_exclusive = out
                                            .as_ref()
                                            .is_some_and(|o| o.exclusive() && o.rate == source_rate);
                                        if !same_exclusive {
                                            // Release existing output device so Windows endpoint is free for exclusive mode
                                            drop(out.take());
                                            std::thread::sleep(Duration::from_millis(30));

                                            match super::exclusive::open(shared.clone(), source_rate, current_device.as_deref()) {
                                                Ok(new_out) => {
                                                    bp_active.store(true, Ordering::Release);
                                                    bp_rate.store(source_rate, Ordering::Release);
                                                    let fmt_val = new_out.bits.unwrap_or(24) as u32;
                                                    bp_format.store(fmt_val, Ordering::Release);
                                                    if let Ok(mut g) = bp_note.lock() {
                                                        *g = Some(format!(
                                                            "Bit-perfect WASAPI Exclusive: {} Hz, {}-bit",
                                                            source_rate, fmt_val
                                                        ));
                                                    }
                                                    out = Some(new_out);
                                                }
                                                Err(e) => {
                                                    log::warn!("Exclusive open failed: {e}; falling back to shared mode");
                                                    bp_active.store(false, Ordering::Release);
                                                    if let Ok(mut g) = bp_note.lock() {
                                                        *g = Some(format!("Exclusive mode unavailable ({e}); shared mixer active"));
                                                    }
                                                    out = super::output::open_shared(shared.clone(), current_device.as_deref()).ok();
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    bp_active.store(false, Ordering::Release);
                                    if out.is_none() || out.as_ref().map(|o| o.exclusive()).unwrap_or(false) {
                                        out = super::output::open_shared(shared.clone(), current_device.as_deref()).ok();
                                    }
                                }

                                flush_output(
                                    &shared,
                                    &mut pending,
                                    &mut written,
                                    &mut boundaries,
                                    &mut resampler,
                                );

                                if let Some(ref o) = out {
                                    let dev_ch = o.channels;
                                    eq.set_format(dev_ch, o.rate);
                                    crossfeed.set_rate(o.rate);
                                    karaoke.set_rate(o.rate);
                                    limiter.set_format(o.rate, dev_ch);
                                    if source_rate != o.rate && !o.exclusive() {
                                        resampler = Resampler::new(source_rate, o.rate, dev_ch).ok();
                                    } else {
                                        resampler = None;
                                    }
                                }

                                current_source = Some(src);
                                boundaries.push(Boundary {
                                    start: 0,
                                    track_id: current_track_id.clone(),
                                    offset: 0.0,
                                });
                                pos.store(f32::to_bits(0.0), Ordering::Release);
                                shared.playing.store(true, Ordering::Release);
                                state.store(STATE_PLAYING, Ordering::Release);

                                let _ = result_tx.send(Ok(()));
                            }
                            Err(e) => {
                                log::error!("Failed to open track {}: {e}", path.display());
                                let _ = result_tx.send(Err(e.to_string()));
                            }
                        }
                    }
                    Cmd::PrepareNext {
                        path,
                        handle,
                        replaygain_db,
                        track_id,
                        sample_rate: _,
                    } => {
                        handle_opt = Some(handle);
                        next_rg_db = replaygain_db;
                        next_track_id = Some(track_id);
                        next_source = Source::open(&path).ok();
                    }
                    Cmd::ClearNext => {
                        next_source = None;
                        next_track_id = None;
                    }
                    Cmd::SetGapless(enabled) => {
                        gapless.store(enabled, Ordering::Release);
                    }
                    Cmd::SetCrossfade(seconds) => {
                        crossfade_secs.store(f32::to_bits(seconds), Ordering::Release);
                    }
                    Cmd::SetBitPerfect(on) => {
                        bp_requested.store(on, Ordering::Release);
                        if !on {
                            bp_active.store(false, Ordering::Release);
                            bp_rate.store(0, Ordering::Release);
                            bp_format.store(0, Ordering::Release);
                            if let Ok(mut g) = bp_note.lock() {
                                *g = None;
                            }
                            if out.as_ref().map(|o| o.exclusive()).unwrap_or(false) {
                                drop(out.take());
                                std::thread::sleep(Duration::from_millis(30));
                                out = super::output::open_shared(shared.clone(), current_device.as_deref()).ok();
                                if let Some(ref o) = out {
                                    eq.set_format(o.channels, o.rate);
                                    crossfeed.set_rate(o.rate);
                                    karaoke.set_rate(o.rate);
                                    limiter.set_format(o.rate, o.channels);
                                    if source_rate != o.rate {
                                        resampler = Resampler::new(source_rate, o.rate, o.channels).ok();
                                    } else {
                                        resampler = None;
                                    }
                                }
                            }
                        }
                    }
                    Cmd::PlayRadio { url, handle } => {
                        handle_opt = Some(handle.clone());
                        let shared_buf = Arc::new(radio::SharedBuffer {
                            inner: std::sync::Mutex::new(radio::RadioBuffer {
                                data: Vec::new(),
                                finished: false,
                            }),
                            cond: std::sync::Condvar::new(),
                        });
                        let dl_stop = Arc::new(AtomicBool::new(false));
                        let dl_handle = radio::spawn_downloader(url, Arc::clone(&shared_buf), dl_stop);
                        _active_radio = Some((dl_handle, shared_buf.clone()));

                        let reader = radio::StreamReader {
                            shared: shared_buf,
                            pos: 0,
                        };

                        match Source::open_reader(Box::new(reader), None) {
                            Ok(src) => {
                                source_rate = src.rate;

                                flush_output(
                                    &shared,
                                    &mut pending,
                                    &mut written,
                                    &mut boundaries,
                                    &mut resampler,
                                );

                                if let Some(ref o) = out {
                                    let dev_ch = o.channels;
                                    eq.set_format(dev_ch, o.rate);
                                    crossfeed.set_rate(o.rate);
                                    karaoke.set_rate(o.rate);
                                    limiter.set_format(o.rate, dev_ch);
                                    if source_rate != o.rate && !o.exclusive() {
                                        resampler = Resampler::new(source_rate, o.rate, dev_ch).ok();
                                    } else {
                                        resampler = None;
                                    }
                                }

                                current_source = Some(src);
                                current_track_id = "radio".into();
                                boundaries.push(Boundary {
                                    start: 0,
                                    track_id: "radio".into(),
                                    offset: 0.0,
                                });
                                pos.store(f32::to_bits(0.0), Ordering::Release);
                                radio_active.store(true, Ordering::Release);
                                shared.playing.store(true, Ordering::Release);
                                state.store(STATE_PLAYING, Ordering::Release);
                                let _ = handle.emit("radio:playing", true);
                            }
                            Err(e) => {
                                log::error!("Failed to open radio stream: {e}");
                                radio_active.store(false, Ordering::Release);
                                _active_radio = None;
                            }
                        }
                    }
                    Cmd::StopRadio => {
                        if radio_active.load(Ordering::Acquire) {
                            radio_active.store(false, Ordering::Release);
                            _active_radio = None;
                            current_source = None;
                            flush_output(
                                &shared,
                                &mut pending,
                                &mut written,
                                &mut boundaries,
                                &mut resampler,
                            );
                            shared.playing.store(false, Ordering::Release);
                            state.store(STATE_STOPPED, Ordering::Release);
                            pos.store(f32::to_bits(0.0), Ordering::Release);
                        }
                    }
                    Cmd::Toggle => {
                        let cur = state.load(Ordering::Acquire);
                        if cur == STATE_PLAYING {
                            shared.playing.store(false, Ordering::Release);
                            state.store(STATE_PAUSED, Ordering::Release);
                        } else if cur == STATE_PAUSED {
                            shared.playing.store(true, Ordering::Release);
                            state.store(STATE_PLAYING, Ordering::Release);
                        }
                    }
                    Cmd::Seek(pos_secs) => {
                        if let Some(ref mut src) = current_source {
                            match src.seek(pos_secs as f64) {
                                Ok(reached) => {
                                    flush_output(
                                        &shared,
                                        &mut pending,
                                        &mut written,
                                        &mut boundaries,
                                        &mut resampler,
                                    );
                                    boundaries.push(Boundary {
                                        start: 0,
                                        track_id: current_track_id.clone(),
                                        offset: reached,
                                    });
                                    pos.store(f32::to_bits(reached as f32), Ordering::Release);
                                }
                                Err(e) => {
                                    log::warn!("Seek failed: {e}");
                                }
                            }
                        }
                    }
                    Cmd::SetVolume(new_vol) => {
                        let clamped = new_vol.clamp(0.0, 1.0);
                        vol.store(f32::to_bits(clamped), Ordering::Release);
                        shared.volume.store(f32::to_bits(clamped), Ordering::Release);
                        if let Some(ref o) = out {
                            device_volumes.insert(o.device.clone(), clamped);
                        }
                    }
                    Cmd::SetReplayGain(gain_db) => {
                        current_rg_db = gain_db;
                    }
                    Cmd::SetEqBand { index, gain_db } => {
                        eq.set_band(index, gain_db);
                    }
                    Cmd::SetEqEnabled(enabled) => {
                        eq.set_enabled(enabled);
                    }
                    Cmd::SetKaraoke(enabled) => {
                        karaoke_flag.store(enabled, Ordering::Release);
                    }
                    Cmd::SetPreamp(gain_db) => {
                        eq.set_preamp(gain_db);
                    }
                    Cmd::SetParametricFilters(filters) => {
                        eq.set_parametric_filters(filters);
                    }
                    Cmd::SetEqMode(mode) => {
                        eq.set_mode(&mode);
                    }
                    Cmd::SetOutputDevice(dev) => {
                        current_device = dev.clone();
                        if let Ok(mut g) = pref_dev.lock() {
                            *g = dev.clone();
                        }
                        log::info!("Audio output device set to: {:?}", current_device);
                        if out.as_ref().map(|o| o.exclusive()).unwrap_or(false) {
                            drop(out.take());
                            std::thread::sleep(Duration::from_millis(30));
                            match super::exclusive::open(shared.clone(), source_rate, current_device.as_deref()) {
                                Ok(new_out) => out = Some(new_out),
                                Err(_) => out = super::output::open_shared(shared.clone(), current_device.as_deref()).ok(),
                            }
                        } else {
                            drop(out.take());
                            out = super::output::open_shared(shared.clone(), current_device.as_deref()).ok();
                        }
                        if let Some(ref o) = out {
                            eq.set_format(o.channels, o.rate);
                            crossfeed.set_rate(o.rate);
                            karaoke.set_rate(o.rate);
                            limiter.set_format(o.rate, o.channels);

                            // DAC Safety Ramp & Volume Cap on new output device
                            let dev_key = o.device.clone();
                            let cur_v = f32::from_bits(vol.load(Ordering::Acquire));
                            if !device_volumes.contains_key(&dev_key) {
                                let safe_v = cur_v.min(0.30);
                                device_volumes.insert(dev_key, safe_v);
                                if safe_v < cur_v {
                                    log::warn!("DAC Safety: New audio device detected; capping initial volume to 30%");
                                    vol.store(safe_v.to_bits(), Ordering::Release);
                                    shared.volume.store(safe_v.to_bits(), Ordering::Release);
                                    if let Some(ref h) = handle_opt {
                                        let _ = h.emit("volume-changed", safe_v);
                                    }
                                }
                            }
                        }
                    }
                    Cmd::Stop => {
                        current_source = None;
                        next_source = None;
                        next_track_id = None;
                        flush_output(
                            &shared,
                            &mut pending,
                            &mut written,
                            &mut boundaries,
                            &mut resampler,
                        );
                        shared.playing.store(false, Ordering::Release);
                        state.store(STATE_STOPPED, Ordering::Release);
                        pos.store(f32::to_bits(0.0), Ordering::Release);
                        if let Some(ref h) = handle_opt {
                            let _ = h.emit("playback-ended", ());
                            let _ = h.emit(
                                "player:status",
                                serde_json::json!({"playing": false, "track_id": null}),
                            );
                        }
                    }
                }
            }

            // 2. Playback decoding & streaming
            let is_play = state.load(Ordering::Acquire) == STATE_PLAYING;
            let mut did_work = false;

            if is_play && out.is_some() {
                let o = out.as_mut().unwrap();
                let ch = o.channels;
                let out_rate = o.rate;

                // 2a. Push existing pending samples into the ring buffer
                if !pending.is_empty() {
                    let room = (o.prod.vacant_len() / ch) * ch;
                    if room > 0 {
                        let to_push = room.min(pending.len());
                        let n = o.prod.push_slice(&pending[..to_push]);
                        pending.drain(..n);
                        did_work = true;
                    }
                }

                // 2b. Only decode when pending is low and ring buffer has space
                if pending.len() < 4096 * ch && o.prod.vacant_len() >= 2048 * ch {
                    if let Some(ref mut src) = current_source {
                        match src.next_chunk() {
                            Ok(Some((chunk, _rate, src_ch))) => {
                                did_work = true;
                                let src_ch = src_ch.max(1);

                                // 1. Channel mapping to match device channels
                                map_channels(chunk, src_ch, ch, &mut mapped_buf);

                                // 2. DSP processing (only when not in bit-perfect exclusive mode)
                                if !o.exclusive() {
                                    let linear_gain = 10f32.powf(current_rg_db / 20.0);
                                    if (linear_gain - 1.0).abs() > 0.001 {
                                        for s in &mut mapped_buf {
                                            *s *= linear_gain;
                                        }
                                    }
                                    if karaoke_flag.load(Ordering::Relaxed) {
                                        karaoke.process(&mut mapped_buf, ch);
                                    }
                                    eq.process(&mut mapped_buf);
                                    crossfeed.process(&mut mapped_buf, ch);
                                    limiter.process(&mut mapped_buf);
                                }

                                // 3. Resampling to device rate
                                let before = pending.len();
                                if source_rate != out_rate && !o.exclusive() {
                                    let fresh = !resampler
                                        .as_ref()
                                        .is_some_and(|r| r.matches(source_rate, out_rate, ch));
                                    if fresh {
                                        if let Some(old) = &mut resampler {
                                            let _ = old.flush(&mut pending);
                                        }
                                        resampler = Resampler::new(source_rate, out_rate, ch).ok();
                                    }
                                    match &mut resampler {
                                        Some(r) => {
                                            if let Err(e) = r.push(&mapped_buf, &mut pending) {
                                                log::warn!("Resampler push error: {e}");
                                            }
                                        }
                                        None => pending.extend_from_slice(&mapped_buf),
                                    }
                                } else {
                                    if let Some(mut old) = resampler.take() {
                                        let _ = old.flush(&mut pending);
                                    }
                                    pending.extend_from_slice(&mapped_buf);
                                }

                                let added_frames = (pending.len() - before) / ch;
                                written += added_frames as u64;

                                // Immediately push whatever fits from pending into ring buffer
                                let room = (o.prod.vacant_len() / ch) * ch;
                                if room > 0 && !pending.is_empty() {
                                    let to_push = room.min(pending.len());
                                    let n = o.prod.push_slice(&pending[..to_push]);
                                    pending.drain(..n);
                                }
                            }
                            Ok(None) => {
                                // Current track reached EOF
                                if let Some(ref mut r) = resampler {
                                    let before = pending.len();
                                    let _ = r.flush(&mut pending);
                                    written += ((pending.len() - before) / ch) as u64;
                                }

                                if let Some(ns) = next_source.take() {
                                    source_rate = ns.rate;
                                    current_source = Some(ns);
                                    current_rg_db = next_rg_db;
                                    let tid = next_track_id.take().unwrap_or_default();
                                    current_track_id = tid.clone();
                                    boundaries.push(Boundary {
                                        start: written,
                                        track_id: tid.clone(),
                                        offset: 0.0,
                                    });
                                    transition_lock.store(true, Ordering::Release);
                                    if let Some(ref h) = handle_opt {
                                        let _ = h.emit("track-changed", &tid);
                                        let _ = h.emit(
                                            "player:status",
                                            serde_json::json!({"playing": true, "track_id": tid}),
                                        );
                                    }
                                    did_work = true;
                                } else {
                                    current_source = None;
                                }
                            }
                            Err(e) => {
                                log::error!("Decoder error: {e}");
                                current_source = None;
                            }
                        }
                    }
                }

                // 2c. Update position and active track from hardware consumed frames
                let consumed = shared.consumed.load(Ordering::Acquire);
                if let Some(pos_idx) = boundaries.iter().rposition(|b| b.start <= consumed) {
                    let b = &boundaries[pos_idx];
                    let cur_secs = b.offset + ((consumed.saturating_sub(b.start)) as f64 / out_rate as f64);
                    pos.store(f32::to_bits(cur_secs as f32), Ordering::Release);

                    if pos_idx > 0 {
                        boundaries.drain(..pos_idx);
                        let active_id = boundaries
                            .first()
                            .map(|b| b.track_id.clone())
                            .unwrap_or_default();
                        current_track_id = active_id.clone();
                        if let Some(ref h) = handle_opt {
                            let _ = h.emit("track-changed", &active_id);
                            let _ = h.emit(
                                "player:status",
                                serde_json::json!({"playing": true, "track_id": active_id}),
                            );
                        }
                    }
                }

                // 2d. Check if stream has ended and all buffers drained through the DAC
                if current_source.is_none() && pending.is_empty() && consumed >= written {
                    shared.playing.store(false, Ordering::Release);
                    state.store(STATE_STOPPED, Ordering::Release);
                    pos.store(f32::to_bits(0.0), Ordering::Release);
                    if let Some(ref h) = handle_opt {
                        let _ = h.emit("playback-ended", ());
                        let _ = h.emit(
                            "player:status",
                            serde_json::json!({"playing": false, "track_id": null}),
                        );
                    }
                }
            }

            // Periodic position event emit (~4 Hz)
            if is_play && last_pos_emit.elapsed() >= Duration::from_millis(250) {
                last_pos_emit = Instant::now();
                if let Some(ref h) = handle_opt {
                    let cur_pos = f32::from_bits(pos.load(Ordering::Acquire));
                    let _ = h.emit("player:position", serde_json::json!({ "position": cur_pos }));
                }
            }

            // Hearing Safety Guard: If output device unplugged or stream failed, pause immediately
            if shared.failed.swap(false, Ordering::AcqRel) {
                log::warn!("Audio output device disconnected or stream failed; pausing playback for ear/speaker safety");
                shared.playing.store(false, Ordering::Release);
                state.store(STATE_PAUSED, Ordering::Release);
                drop(out.take());
                if let Some(ref h) = handle_opt {
                    let _ = h.emit("playback-state-changed", "paused");
                    let _ = h.emit(
                        "player:status",
                        serde_json::json!({"playing": false, "track_id": current_track_id}),
                    );
                    let _ = h.emit("device-disconnected", ());
                }
            }

            if !did_work {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.engine_ready.load(Ordering::Acquire)
    }

    pub fn ping(&self) -> Result<(), String> {
        if !self.engine_ready.load(Ordering::Acquire) {
            return Err("Engine not yet initialized".to_string());
        }
        let (result_tx, result_rx) = mpsc::channel();
        self.cmd_tx
            .send(Cmd::Ping { result_tx })
            .map_err(|_| "Audio player channel closed".to_string())?;
        result_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Audio player did not respond within 3 seconds".to_string())
    }

    pub fn play_track(
        &self,
        path: PathBuf,
        handle: AppHandle,
        replaygain_db: f32,
        sample_rate: u32,
        track_id: Option<String>,
    ) -> Result<(), String> {
        let (result_tx, result_rx) = mpsc::channel();
        self.cmd_tx
            .send(Cmd::Play {
                path,
                handle,
                replaygain_db,
                sample_rate,
                track_id,
                result_tx,
            })
            .map_err(|_| "Audio player channel closed".to_string())?;
        result_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "Audio player did not respond within 10 seconds".to_string())?
    }

    pub fn prepare_next(
        &self,
        path: PathBuf,
        handle: AppHandle,
        replaygain_db: f32,
        track_id: String,
        sample_rate: u32,
    ) {
        let _ = self.cmd_tx.send(Cmd::PrepareNext {
            path,
            handle,
            replaygain_db,
            track_id,
            sample_rate,
        });
    }

    pub fn clear_next(&self) {
        let _ = self.cmd_tx.send(Cmd::ClearNext);
    }

    pub fn play_radio(&self, url: String, handle: AppHandle) {
        let _ = self.cmd_tx.send(Cmd::PlayRadio { url, handle });
    }

    pub fn stop_radio(&self) {
        let _ = self.cmd_tx.send(Cmd::StopRadio);
    }

    pub fn toggle(&self) -> bool {
        let current = self.state.load(Ordering::Relaxed);
        if current == STATE_PAUSED {
            let _ = self.cmd_tx.send(Cmd::Toggle);
            true
        } else if current == STATE_PLAYING {
            let _ = self.cmd_tx.send(Cmd::Toggle);
            false
        } else {
            false
        }
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
    }

    pub fn seek(&self, pos_secs: f32) {
        let _ = self.cmd_tx.send(Cmd::Seek(pos_secs));
    }

    pub fn set_volume(&self, vol: f32) {
        let _ = self.cmd_tx.send(Cmd::SetVolume(vol));
    }

    pub fn set_replaygain(&self, gain_db: f32) {
        let _ = self.cmd_tx.send(Cmd::SetReplayGain(gain_db));
    }

    pub fn set_eq_band(&self, index: u8, gain_db: f32) {
        let _ = self.cmd_tx.send(Cmd::SetEqBand { index, gain_db });
    }

    pub fn set_eq_enabled(&self, enabled: bool) {
        let _ = self.cmd_tx.send(Cmd::SetEqEnabled(enabled));
    }

    pub fn set_karaoke(&self, enabled: bool) {
        self.karaoke_enabled.store(enabled, Ordering::Release);
        let _ = self.cmd_tx.send(Cmd::SetKaraoke(enabled));
    }

    pub fn is_karaoke(&self) -> bool {
        self.karaoke_enabled.load(Ordering::Acquire)
    }

    pub fn set_preamp(&self, gain_db: f32) {
        let _ = self.cmd_tx.send(Cmd::SetPreamp(gain_db));
    }

    pub fn set_parametric_filters(&self, filters: Vec<super::eq::Filter>) {
        let _ = self.cmd_tx.send(Cmd::SetParametricFilters(filters));
    }

    pub fn set_eq_mode(&self, mode: &str) {
        let _ = self.cmd_tx.send(Cmd::SetEqMode(mode.to_string()));
    }

    pub fn set_output_device(&self, device_name: Option<String>) {
        if let Ok(mut g) = self.preferred_device.lock() {
            *g = device_name.clone();
        }
        let _ = self.cmd_tx.send(Cmd::SetOutputDevice(device_name));
    }

    pub fn current_output_device(&self) -> String {
        self.preferred_device
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .or_else(|| super::output::default_output_device_name())
            .unwrap_or_else(|| "Default Audio Device".to_string())
    }

    pub fn set_gapless(&self, enabled: bool) {
        let _ = self.cmd_tx.send(Cmd::SetGapless(enabled));
    }

    pub fn set_crossfade(&self, seconds: f32) {
        let _ = self.cmd_tx.send(Cmd::SetCrossfade(seconds));
    }

    pub fn position(&self) -> f32 {
        f32::from_bits(self.position_secs.load(Ordering::Relaxed))
    }

    pub fn is_playing(&self) -> bool {
        self.state.load(Ordering::Relaxed) == STATE_PLAYING
    }

    pub fn is_radio_active(&self) -> bool {
        self.radio_active.load(Ordering::Relaxed)
    }

    pub fn is_gapless(&self) -> bool {
        self.gapless.load(Ordering::Relaxed)
    }

    pub fn crossfade_seconds(&self) -> f32 {
        f32::from_bits(self.crossfade_secs.load(Ordering::Relaxed))
    }

    pub fn try_claim_transition(&self) -> bool {
        self.transition_lock.swap(false, Ordering::Acquire)
    }

    pub fn set_bit_perfect(&self, on: bool) {
        let _ = self.cmd_tx.send(Cmd::SetBitPerfect(on));
    }

    pub fn output_state(&self) -> OutputState {
        let note = self
            .bp_note
            .lock()
            .ok()
            .and_then(|g| g.clone());
        OutputState {
            bit_perfect_requested: self.bp_requested.load(Ordering::Acquire),
            bit_perfect_active: self.bp_active.load(Ordering::Acquire),
            sample_rate: self.bp_rate.load(Ordering::Acquire),
            format: self.bp_format.load(Ordering::Acquire) as usize,
            note,
        }
    }
}
