use num_complex::Complex;
use realfft::RealFftPlanner;
use std::path::Path;

const ANALYSIS_SECS: f64 = 60.0;
const TARGET_SR: u32 = 44100;
const MIN_SAMPLES: usize = 8192;

pub struct AnalysisResult {
    pub bpm: f64,
    pub musical_key: String,
    pub energy: f64,
}

pub fn analyze(path: &Path) -> AnalysisResult {
    let samples = match decode_mono(path) {
        Some(s) => s,
        None => return AnalysisResult {
            bpm: 0.0, musical_key: String::new(), energy: 0.0,
        },
    };

    if samples.len() < MIN_SAMPLES {
        return AnalysisResult {
            bpm: 0.0, musical_key: String::new(), energy: 0.0,
        };
    }

    let bpm = detect_bpm(&samples, TARGET_SR);
    let key = detect_key(&samples, TARGET_SR);
    let energy = compute_energy(&samples);

    AnalysisResult { bpm, musical_key: key, energy }
}

fn decode_mono(path: &Path) -> Option<Vec<f32>> {
    let mut src = crate::audio::source::Source::open(path).ok()?;
    let rate = src.rate;
    if rate == 0 {
        return None;
    }
    let frames_needed = (ANALYSIS_SECS as u32 * rate) as usize;
    let mut mono = Vec::with_capacity(frames_needed.min(192000 * 60));

    while mono.len() < frames_needed {
        match src.next_chunk() {
            Ok(Some((buf, _r, channels))) => {
                let ch = channels.max(1);
                for frame in buf.chunks_exact(ch) {
                    let sum: f32 = frame.iter().copied().sum();
                    mono.push(sum / ch as f32);
                    if mono.len() >= frames_needed {
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    if mono.len() < MIN_SAMPLES {
        return None;
    }

    Some(mono)
}

fn detect_bpm(samples: &[f32], sr: u32) -> f64 {
    let envelope: Vec<f32> = samples.iter().map(|s| s.abs()).collect();

    let ds_factor = (sr / 200).max(1);
    let ds: Vec<f32> = envelope.iter().step_by(ds_factor as usize).copied().collect();
    let ds_sr = sr / ds_factor;

    let mean: f32 = ds.iter().sum::<f32>() / ds.len() as f32;
    let centered: Vec<f32> = ds.iter().map(|s| s - mean).collect();

    let min_lag = (ds_sr * 60 / 180) as usize;
    let max_lag = (ds_sr * 60 / 60) as usize;
    if min_lag >= max_lag || max_lag >= centered.len() {
        return 0.0;
    }

    let mut acf: Vec<f64> = Vec::with_capacity(max_lag - min_lag);
    for lag in min_lag..max_lag {
        let n = centered.len() - lag;
        let sum: f64 = centered[..n]
            .iter()
            .zip(&centered[lag..])
            .map(|(a, b)| *a as f64 * *b as f64)
            .sum();
        acf.push(sum / n as f64);
    }

    if acf.is_empty() {
        return 0.0;
    }

    let max_val = acf.iter().fold(0.0f64, |a, &b| a.max(b));
    let threshold = max_val * 0.5;
    if threshold <= 0.0 {
        return 0.0;
    }

    let mut peak_lag = min_lag as f64;
    for i in 1..acf.len() - 1 {
        if acf[i] > threshold && acf[i] > acf[i - 1] && acf[i] > acf[i + 1] {
            let y0 = acf[i - 1];
            let y1 = acf[i];
            let y2 = acf[i + 1];
            let denom = 2.0 * (2.0 * y1 - y0 - y2);
            let correction = if denom.abs() > 1e-12 {
                ((y2 - y0) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            peak_lag = (min_lag + i) as f64 + correction;
            break;
        }
    }

    let raw = 60.0 * ds_sr as f64 / peak_lag;
    if raw < 40.0 || raw > 220.0 {
        return 0.0;
    }
    (raw * 10.0).round() / 10.0
}

fn detect_key(samples: &[f32], sr: u32) -> String {
    let frame_size: usize = 4096;
    let hop = frame_size / 2;

    let mut chroma = [0.0f64; 12];

    let hann: Vec<f64> = (0..frame_size)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / frame_size as f64).cos()))
        .collect();

    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(frame_size);

    let mut offset: usize = 0;
    while offset + frame_size <= samples.len() {
        let mut frame = Vec::with_capacity(frame_size);
        for i in 0..frame_size {
            frame.push(samples[offset + i] as f64 * hann[i]);
        }

        let mut spectrum = vec![Complex::new(0.0f64, 0.0f64); frame_size / 2 + 1];
        let _ = fft.process(&mut frame, &mut spectrum);

        let max_mag = spectrum.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).fold(0.0f64, f64::max);
        if max_mag < 1e-10 {
            offset += hop;
            continue;
        }

        for (bin, c) in spectrum.iter().enumerate() {
            if bin == 0 {
                continue;
            }
            let freq = bin as f64 * sr as f64 / frame_size as f64;
            if freq < 65.0 || freq > 2096.0 {
                continue;
            }
            let midi = 12.0 * (freq / 440.0).log2() + 69.0;
            let pc = ((midi + 0.5).floor() as usize).rem_euclid(12);
            let mag = (c.re * c.re + c.im * c.im).sqrt() / max_mag;
            chroma[pc] += mag;
        }

        offset += hop;
    }

    let max_c = chroma.iter().fold(0.0f64, |a, &b| a.max(b));
    if max_c == 0.0 {
        return String::new();
    }
    for c in &mut chroma {
        *c /= max_c;
    }

    let key_names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    let ks_major: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
    let ks_minor: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

    let mut best_score = -1.0f64;
    let mut best_key = String::new();

    let norm_c = chroma.iter().map(|c| c * c).sum::<f64>().sqrt();
    if norm_c < 1e-12 {
        return String::new();
    }

    for (tonic, _) in key_names.iter().enumerate() {
        for (mode_name, profile) in [("", &ks_major), ("m", &ks_minor)] {
            let norm_p = profile.iter().map(|p| p * p).sum::<f64>().sqrt();
            if norm_p < 1e-12 {
                continue;
            }
            let mut score = 0.0f64;
            for i in 0..12 {
                let p_idx = (i + tonic) % 12;
                score += profile[p_idx] * chroma[i];
            }
            score /= norm_c * norm_p;

            if score > best_score {
                best_score = score;
                best_key = format!("{}{}", key_names[tonic], mode_name);
            }
        }
    }

    if best_score < 0.3 {
        String::new()
    } else {
        best_key
    }
}

fn compute_energy(samples: &[f32]) -> f64 {
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64).powi(2)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    (rms * 100.0).round() / 100.0
}
