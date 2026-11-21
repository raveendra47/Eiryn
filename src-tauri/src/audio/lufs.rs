//! LUFS (Loudness Units Full Scale) normalization using EBU R128 standard.
//!
//! Measurement runs through a sidecar `ffmpeg.exe` (`-af ebur128`, same
//! filter Airmedy uses in-process) so every format in
//! `tauri.conf.json:fileAssociations` is covered — including APE/WV/DSD,
//! which the `maudio`-only analyzer cannot decode. Gain math, clip guard,
//! and the in-memory cache live here; persistence lives in the
//! `track_loudness` table (`library/db.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
use std::time::Duration;

/// Analyzer version stamped on every measurement. Bump when the extraction
/// changes so the backfill worker re-analyzes stale rows.
pub const ANALYZER_VERSION: i64 = 1;

/// Default target integrated loudness (Spotify/YouTube convention, matches
/// Airmedy `domain.DefaultTargetLUFS`).
pub const DEFAULT_TARGET_LUFS: f32 = -14.0;

/// Hard limits for the recommended gain adjustment.
pub const MAX_GAIN_DB: f32 = 15.0;

/// Per-file timeout for the ffmpeg measurement process.
pub const MEASURE_TIMEOUT: Duration = Duration::from_secs(180);

/// LUFS measurement result
#[derive(Debug, Clone)]
pub struct LufsResult {
    /// Integrated loudness in LUFS
    pub integrated_loudness: f32,
    /// Loudness range in LU
    pub loudness_range: f32,
    /// True peak in dBTP
    pub true_peak: f32,
    /// Recommended gain adjustment in dB (for [`DEFAULT_TARGET_LUFS`])
    pub recommended_gain: f32,
}

impl Default for LufsResult {
    fn default() -> Self {
        Self {
            integrated_loudness: -23.0, // EBU R128 reference
            loudness_range: 0.0,
            true_peak: 0.0,
            recommended_gain: 0.0,
        }
    }
}

/// Locate the ffmpeg binary: explicit `EIRYN_FFMPEG` override first, then the
/// bundled sidecar next to the executable, then `PATH`.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("EIRYN_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in [
                "ffmpeg-x86_64-pc-windows-msvc.exe",
                "ffmpeg.exe",
            ] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    // PATH lookup: `ffmpeg -version` must succeed.
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-version");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let probe = cmd.output().ok()?;
    if probe.status.success() {
        return Some(PathBuf::from("ffmpeg"));
    }
    None
}

/// Calculate LUFS for an audio file via sidecar ffmpeg + `ebur128` filter.
///
/// Returns the integrated loudness, range, and true peak together with the
/// recommended gain for [`DEFAULT_TARGET_LUFS`]. Use [`gain_for_target`] to
/// recompute the gain for a user-configured target.
pub fn calculate_lufs(file_path: &Path) -> Result<LufsResult, String> {
    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        "ffmpeg binary not found (set EIRYN_FFMPEG or bundle the sidecar)".to_string()
    })?;
    let stderr = run_ebur128(&ffmpeg, file_path)?;
    let (integrated, range, peak) = parse_ebur128_summary(&stderr)?;
    Ok(with_target(integrated, range, peak, DEFAULT_TARGET_LUFS))
}

/// Recompute [`LufsResult`] for a user-configured target loudness.
pub fn with_target(integrated: f32, range: f32, peak: f32, target_lufs: f32) -> LufsResult {
    LufsResult {
        integrated_loudness: integrated,
        loudness_range: range,
        true_peak: peak,
        recommended_gain: gain_for_target(integrated, target_lufs),
    }
}

/// `gain = target − integrated`, clamped to ±[`MAX_GAIN_DB`].
pub fn gain_for_target(integrated_lufs: f32, target_lufs: f32) -> f32 {
    (target_lufs - integrated_lufs).clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
}

/// Prevent-clipping guard (Airmedy parity): never boost above `-true_peak`.
pub fn apply_clip_guard(gain_db: f32, true_peak_db: f32) -> f32 {
    gain_db.min(-true_peak_db)
}

fn run_ebur128(ffmpeg: &Path, file_path: &Path) -> Result<String, String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-nostats")
        .arg("-i")
        .arg(file_path)
        .arg("-map")
        .arg("0:a")
        .arg("-af")
        .arg("ebur128=peak=true:framelog=quiet")
        .arg("-f")
        .arg("null")
        .arg("-")
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;

    // Poll with timeout; std has no `wait_timeout` without extra deps.
    let waited = std::time::Instant::now();
    loop {
        match child.try_wait().map_err(|e| format!("ffmpeg wait failed: {e}"))? {
            Some(_) => break,
            None => {
                if waited.elapsed() > MEASURE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "ffmpeg timed out after {}s: {}",
                        MEASURE_TIMEOUT.as_secs(),
                        file_path.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to read ffmpeg output: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() && parse_ebur128_summary(&stderr).is_err() {
        return Err(format!(
            "ffmpeg failed for {}: {}",
            file_path.display(),
            stderr.lines().last().unwrap_or("unknown error")
        ));
    }
    Ok(stderr)
}

/// Parse the `Summary:` block ffmpeg's `ebur128` filter prints to stderr:
///
/// ```text
/// [Parsed_ebur128_0 @ ...] Summary:
///   Integrated loudness:
///     I:         -14.9 LUFS
///   Loudness range:
///     LRA:         3.2 LU
///   True peak:
///     Peak:       -0.5 dBFS
/// ```
fn parse_ebur128_summary(stderr: &str) -> Result<(f32, f32, f32), String> {
    let mut integrated = None;
    let mut range = None;
    let mut peak = None;
    for line in stderr.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("I:") {
            integrated = parse_lufs_value(v);
        } else if let Some(v) = t.strip_prefix("LRA:") {
            range = parse_lufs_value(v);
        } else if let Some(v) = t.strip_prefix("Peak:") {
            peak = parse_lufs_value(v);
        }
    }
    match (integrated, range, peak) {
        (Some(i), Some(r), Some(p)) => Ok((i, r, p)),
        _ => Err("Could not parse ebur128 summary (I/LRA/Peak) from ffmpeg output".to_string()),
    }
}

/// First whitespace-separated token as f32; ffmpeg prints `n/a` for degenerate
/// inputs (e.g. digital silence), which is a measurement failure, not 0.
fn parse_lufs_value(s: &str) -> Option<f32> {
    s.split_whitespace().next()?.parse::<f32>().ok()
}

/// Apply LUFS-based gain adjustment to audio samples
///
/// This function applies the calculated gain adjustment to normalize
/// the perceived loudness of audio samples.
pub fn apply_lufs_gain(samples: &mut [f32], gain_db: f32) {
    if gain_db.abs() < 0.01 {
        return; // No adjustment needed
    }

    // Convert dB to linear gain
    let linear_gain = 10.0_f32.powf(gain_db / 20.0);

    // Apply gain with clipping prevention
    for sample in samples.iter_mut() {
        *sample = (*sample * linear_gain).clamp(-1.0, 1.0);
    }
}

/// Volume normalization modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizationMode {
    /// Normalize each track individually (consistent per-track volume)
    Track,
    /// Normalize based on album loudness (preserves dynamic range within albums)
    Album,
    /// No normalization
    Off,
}

impl Default for NormalizationMode {
    fn default() -> Self {
        NormalizationMode::Track
    }
}

impl NormalizationMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "track" => NormalizationMode::Track,
            "album" => NormalizationMode::Album,
            _ => NormalizationMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            NormalizationMode::Track => "track",
            NormalizationMode::Album => "album",
            NormalizationMode::Off => "off",
        }
    }
}

/// Cache for LUFS measurements to avoid re-analysis
#[derive(Debug, Clone)]
pub struct LufsCache {
    /// Map of file path hash to LUFS result
    measurements: std::collections::HashMap<u64, LufsResult>,
}

impl LufsCache {
    pub fn new() -> Self {
        Self {
            measurements: std::collections::HashMap::new(),
        }
    }

    pub fn get(&self, file_path: &Path) -> Option<&LufsResult> {
        let hash = self.hash_path(file_path);
        self.measurements.get(&hash)
    }

    pub fn insert(&mut self, file_path: &Path, result: LufsResult) {
        let hash = self.hash_path(file_path);
        self.measurements.insert(hash, result);
    }

    fn hash_path(&self, path: &Path) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for LufsCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SUMMARY: &str = r#"
[Parsed_ebur128_0 @ 0000011b7713ac00] Summary:
  Integrated loudness:
    I:         -14.9 LUFS
    Threshold: -25.1 LUFS
  Loudness range:
    LRA:         3.2 LU
    Threshold: -35.1 LUFS
    LRA low:   -16.9 LUFS
    LRA high:  -13.6 LUFS
  True peak:
    Peak:       -0.5 dBFS
"#;

    #[test]
    fn test_parse_ebur128_summary() {
        let (i, r, p) = parse_ebur128_summary(SAMPLE_SUMMARY).unwrap();
        assert!((i - -14.9).abs() < 0.001);
        assert!((r - 3.2).abs() < 0.001);
        assert!((p - -0.5).abs() < 0.001);
    }

    #[test]
    fn test_parse_missing_summary_errors() {
        assert!(parse_ebur128_summary("no summary here").is_err());
    }

    #[test]
    fn test_parse_na_values_error() {
        let na = "Summary:\nI: n/a\nLRA: n/a\nPeak: n/a\n";
        assert!(parse_ebur128_summary(na).is_err());
    }

    #[test]
    fn test_gain_for_target() {
        assert!((gain_for_target(-14.9, -14.0) - 0.9).abs() < 0.001);
        // Clamped to ±15 dB
        assert_eq!(gain_for_target(-40.0, -14.0), 15.0);
        assert_eq!(gain_for_target(10.0, -14.0), -15.0);
    }

    #[test]
    fn test_clip_guard() {
        // Boost capped at -true_peak
        assert!((apply_clip_guard(3.0, -0.5) - 0.5).abs() < 0.001);
        // Cuts pass through untouched
        assert!((apply_clip_guard(-4.0, -0.5) - -4.0).abs() < 0.001);
    }

    #[test]
    fn test_gain_application() {
        let mut samples = vec![0.5f32; 100];
        apply_lufs_gain(&mut samples, 6.0); // +6dB

        // Samples should be amplified but clipped at 1.0
        for sample in &samples {
            assert!(*sample <= 1.0);
            assert!(*sample >= 0.5);
        }
    }

    #[test]
    fn test_gain_noop() {
        let mut samples = vec![0.5f32; 100];
        let original = samples.clone();
        apply_lufs_gain(&mut samples, 0.0);

        assert_eq!(samples, original);
    }
}
