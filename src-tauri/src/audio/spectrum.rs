//! Full-spectrum audio measurement via sidecar FFmpeg (Airmedy parity).
//!
//! One decode, one invocation:
//! `ebur128` (integrated/LRA/true-peak) + `astats` (RMS/crest/ZCR) +
//! `aspectralstats` (centroid/rolloff/flatness/flux, averaged over all
//! channels and frames) with `ametadata=print` to dump per-frame spectral
//! values. Stderr is streamed line-by-line (a full track yields ~200k
//! metadata lines) — never buffered whole.
//!
//! The fast loudness-only path (`lufs::calculate_lufs`) is kept for the
//! loudness backfill; this module feeds the analysis worker, which also
//! upserts `track_loudness` from the same measurement.

use crate::audio::lufs::{find_ffmpeg, with_target, LufsResult, MEASURE_TIMEOUT};
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Version stamped on `track_analysis` rows; bump to force re-analysis.
pub const SPECTRAL_VERSION: i64 = 1;

#[derive(Debug, Clone, Default)]
pub struct FullAnalysis {
    pub lufs: LufsResult,
    pub rms_db: f64,
    pub crest: f64,
    pub spectral_centroid: f64,
    pub spectral_rolloff: f64,
    pub spectral_flatness: f64,
    pub spectral_flux: f64,
    pub zcr: f64,
}

#[derive(Default)]
struct SpectralAccum {
    centroid: f64,
    rolloff: f64,
    flatness: f64,
    flux: f64,
    frames: u64,
}

impl SpectralAccum {
    fn push(&mut self, measure: &str, value: f64) {
        match measure {
            "centroid" => self.centroid += value,
            "rolloff" => self.rolloff += value,
            "flatness" => self.flatness += value,
            "flux" => self.flux += value,
            _ => return,
        }
        // Count frames once per measure-group: centroid is always present.
        if measure == "centroid" {
            self.frames += 1;
        }
    }

    fn means(&self) -> (f64, f64, f64, f64) {
        if self.frames == 0 {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let n = self.frames as f64;
        // 4 values pushed per frame (one per measure); centroid counts frames.
        (
            self.centroid / n,
            self.rolloff / n,
            self.flatness / n,
            self.flux / n,
        )
    }
}

/// Parse one `ametadata=print` line:
/// `[Parsed_ametadata_1 @ 0x...] lavfi.aspectralstats.1.centroid=2973.7`
/// Note the `{channel}.{measure}` shape: the first dot follows the channel.
fn parse_spectral_line(line: &str) -> Option<(&str, f64)> {
    let marker = "lavfi.aspectralstats.";
    let i = line.find(marker)?;
    let rest = &line[i + marker.len()..];
    let channel_dot = rest.find('.')?;
    let after_channel = &rest[channel_dot + 1..];
    let eq = after_channel.find('=')?;
    let measure = &after_channel[..eq];
    if measure.is_empty() || measure.contains(|c: char| !(c.is_ascii_alphabetic() || c == '_')) {
        return None;
    }
    let value: f64 = after_channel[eq + 1..].trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((measure, value))
}

#[derive(Default)]
struct AstatsAccum {
    crest_sum: f64,
    zcr_sum: f64,
    channels: u64,
    rms_db: Option<f64>,
    in_overall: bool,
}

fn parse_astats_line(acc: &mut AstatsAccum, line: &str, in_channel: &mut bool) {
    let t = line.trim();
    if t.ends_with("Overall") && t.contains("astats") {
        acc.in_overall = true;
        *in_channel = false;
        return;
    }
    if t.contains("astats") && t.contains("Channel:") {
        acc.in_overall = false;
        *in_channel = true;
        return;
    }
    let Some(colon) = t.find(':') else { return };
    let (key, val) = (t[..colon].trim(), t[colon + 1..].trim());
    // Keys are prefixed by "[Parsed_astats_0 @ ...] " — strip to the label.
    let key = key.rsplit(']').next().unwrap_or(key).trim();
    let Ok(value) = val.split_whitespace().next().unwrap_or("").parse::<f64>() else { return };
    if !value.is_finite() {
        return;
    }
    match key {
        "RMS level dB" if acc.in_overall && acc.rms_db.is_none() => {
            acc.rms_db = Some(value);
        }
        "Crest factor" if *in_channel && !acc.in_overall => {
            acc.crest_sum += value;
            acc.channels += 1;
        }
        "Zero crossings rate" if *in_channel && !acc.in_overall => {
            acc.zcr_sum += value;
        }
        _ => {}
    }
}

pub fn measure_full(file_path: &Path) -> Result<FullAnalysis, String> {
    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        "ffmpeg binary not found (set EIRYN_FFMPEG or bundle the sidecar)".to_string()
    })?;

    let mut cmd = Command::new(&ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-nostats")
        .arg("-i")
        .arg(file_path)
        .arg("-map")
        .arg("0:a")
        .arg("-af")
        .arg(
            "ebur128=peak=true:framelog=quiet,\
             astats=metadata=1:reset=0,\
             aspectralstats=measure=centroid+rolloff+flatness+flux,\
             ametadata=print",
        )
        .arg("-f")
        .arg("null")
        .arg("-")
        .stderr(Stdio::piped())
        .stdout(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;

    // Stream stderr on a reader thread so a wedged ffmpeg can never block us:
    // the main loop below enforces the timeout via try_wait. Bytes are
    // decoded lossily: tag dumps can contain non-UTF8, which must not kill
    // the reader (a dead reader silently drops the summaries at the end).
    let stderr = child.stderr.take().ok_or("Failed to pipe ffmpeg stderr")?;
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match std::io::BufRead::read_until(&mut reader, b'\n', &mut buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).into_owned();
                    if line_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut summary_lines: Vec<String> = Vec::new();
    let mut spectral = SpectralAccum::default();
    let mut astats = AstatsAccum::default();
    let mut in_channel = false;

    let start = Instant::now();
    let timeout = MEASURE_TIMEOUT + Duration::from_secs(180);
    loop {
        // Drain whatever arrived without blocking the timeout check.
        while let Ok(line) = line_rx.try_recv() {
            if line.contains("lavfi.aspectralstats.") {
                if let Some((measure, value)) = parse_spectral_line(&line) {
                    spectral.push(measure, value);
                }
                continue;
            }
            if line.contains("Parsed_ebur128_")
                || line.contains("Parsed_astats_")
                || is_bare_summary_line(&line)
            {
                if summary_lines.len() < 400 {
                    if line.contains("Parsed_astats_") {
                        parse_astats_line(&mut astats, &line, &mut in_channel);
                    }
                    summary_lines.push(line);
                } else if line.contains("Parsed_astats_") {
                    // astats summaries always fit in the cap, but never skip
                    // their parse even under flood.
                    parse_astats_line(&mut astats, &line, &mut in_channel);
                }
            }
        }
        match child.try_wait().map_err(|e| format!("ffmpeg wait failed: {e}"))? {
            Some(status) => {
                // The child has exited, so the pipe will hit EOF and the
                // reader thread will finish: block until it does, otherwise
                // the tail (I/LRA/Peak + astats summaries print last) is cut.
                while let Ok(line) = line_rx.recv() {
                    if line.contains("lavfi.aspectralstats.") {
                        if let Some((measure, value)) = parse_spectral_line(&line) {
                            spectral.push(measure, value);
                        }
                    } else if line.contains("Parsed_ebur128_")
                        || line.contains("Parsed_astats_")
                        || is_bare_summary_line(&line)
                    {
                        if line.contains("Parsed_astats_") {
                            parse_astats_line(&mut astats, &line, &mut in_channel);
                        }
                        if summary_lines.len() < 400 {
                            summary_lines.push(line);
                        }
                    }
                }
                if !status.success() && summary_lines.is_empty() {
                    return Err(format!("ffmpeg failed for {}", file_path.display()));
                }
                break;
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "ffmpeg timed out for {}",
                        file_path.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let summary = summary_lines.join("\n");
    // TMPDBG sample of unmatched lines.
    let _ = &spectral;
    let (integrated, range, peak) = parse_ebur128_text(&summary)?;
    let (centroid, rolloff, flatness, flux) = spectral.means();
    if spectral.frames == 0 {
        return Err("No spectral frames measured".to_string());
    }
    let crest = if astats.channels > 0 {
        astats.crest_sum / astats.channels as f64
    } else {
        0.0
    };
    let zcr = if astats.channels > 0 {
        astats.zcr_sum / astats.channels as f64
    } else {
        0.0
    };

    Ok(FullAnalysis {
        lufs: with_target(integrated, range, peak, crate::audio::lufs::DEFAULT_TARGET_LUFS),
        rms_db: astats.rms_db.unwrap_or(0.0),
        crest,
        spectral_centroid: centroid,
        spectral_rolloff: rolloff,
        spectral_flatness: flatness,
        spectral_flux: flux,
        zcr,
    })
}

/// Bare ebur128 summary lines (`    I: -14.9 LUFS`) carry no
/// `[Parsed_...]` prefix, unlike every other summary line.
fn is_bare_summary_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("I:") || t.starts_with("LRA:") || t.starts_with("Peak:")
}

/// Parse the ebur128 `Summary:` block. Lines carry a
/// `[Parsed_ebur128_0 @ ...]` prefix; match the label after the last `]`.
fn parse_ebur128_text(stderr: &str) -> Result<(f32, f32, f32), String> {
    fn value_after(line: &str, label: &str) -> Option<f32> {
        let body = line.rsplit(']').next().unwrap_or(line).trim();
        let rest = body.strip_prefix(label)?;
        rest.split_whitespace().next()?.parse().ok()
    }

    let mut integrated = None;
    let mut range = None;
    let mut peak = None;
    for line in stderr.lines() {
        if integrated.is_none() {
            integrated = value_after(line, "I:");
        }
        if range.is_none() {
            range = value_after(line, "LRA:");
        }
        if peak.is_none() {
            peak = value_after(line, "Peak:");
        }
    }
    match (integrated, range, peak) {
        (Some(i), Some(r), Some(p)) => Ok((i, r, p)),
        _ => Err("Could not parse ebur128 summary (I/LRA/Peak) from ffmpeg output".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EBUR128_SAMPLE: &str = r#"
[Parsed_ebur128_0 @ 0000011b7713ac00] Summary:
[Parsed_ebur128_0 @ 0000011b7713ac00]   Integrated loudness:
[Parsed_ebur128_0 @ 0000011b7713ac00]     I:         -14.9 LUFS
[Parsed_ebur128_0 @ 0000011b7713ac00]     Threshold: -25.1 LUFS
[Parsed_ebur128_0 @ 0000011b7713ac00]   Loudness range:
[Parsed_ebur128_0 @ 0000011b7713ac00]     LRA:         3.2 LU
[Parsed_ebur128_0 @ 0000011b7713ac00]   Loudness range:
[Parsed_ebur128_0 @ 0000011b7713ac00]   True peak:
[Parsed_ebur128_0 @ 0000011b7713ac00]     Peak:       -0.5 dBFS
"#;

    #[test]
    fn test_parse_ebur128_text() {
        let (i, r, p) = parse_ebur128_text(EBUR128_SAMPLE).unwrap();
        assert!((i - -14.9).abs() < 0.001);
        assert!((r - 3.2).abs() < 0.001);
        assert!((p - -0.5).abs() < 0.001);
        assert!(parse_ebur128_text("no summary").is_err());
    }

    #[test]
    fn test_parse_spectral_line() {
        let (m, v) = parse_spectral_line(
            "[Parsed_ametadata_1 @ 000001b179c21180] lavfi.aspectralstats.1.centroid=2973.7",
        )
        .unwrap();
        assert_eq!(m, "centroid");
        assert!((v - 2973.7).abs() < 0.001);
        assert!(parse_spectral_line("unrelated line").is_none());
        assert!(parse_spectral_line(
            "[Parsed_ametadata_1 @ 0x1] lavfi.aspectralstats.2.flux=nan"
        )
        .is_none());
    }

    #[test]
    fn test_spectral_accum_means() {
        let mut acc = SpectralAccum::default();
        for _ in 0..4 {
            acc.push("centroid", 1000.0);
            acc.push("rolloff", 5000.0);
            acc.push("flatness", 0.1);
            acc.push("flux", 0.002);
        }
        let (c, r, f, x) = acc.means();
        assert!((c - 1000.0).abs() < 1e-9);
        assert!((r - 5000.0).abs() < 1e-9);
        assert!((f - 0.1).abs() < 1e-12);
        assert!((x - 0.002).abs() < 1e-12);
    }

    /// Real-file end-to-end: requires ffmpeg on PATH + the fixture MP3.
    /// Run explicitly: cargo test --lib spectrum -- --ignored
    #[test]
    #[ignore]
    fn test_measure_full_real_file() {
        let m = measure_full(std::path::Path::new(
            "D:\\Music\\Music\\Aditya Hridayam - Dr. R. Thiagarajan.mp3",
        ))
        .expect("full measurement must succeed");
        assert!((m.lufs.integrated_loudness - -14.9).abs() < 0.5, "I={}", m.lufs.integrated_loudness);
        assert!((m.lufs.loudness_range - 3.2).abs() < 0.8, "LRA={}", m.lufs.loudness_range);
        assert!((m.rms_db - -19.4).abs() < 1.5, "RMS={}", m.rms_db);
        assert!((m.crest - 8.8).abs() < 2.0, "crest={}", m.crest);
        assert!(m.spectral_centroid > 1000.0 && m.spectral_centroid < 8000.0, "centroid={}", m.spectral_centroid);
        assert!(m.spectral_rolloff > m.spectral_centroid, "rolloff={}", m.spectral_rolloff);
        assert!(m.spectral_flatness > 0.0 && m.spectral_flatness < 1.0, "flatness={}", m.spectral_flatness);
        assert!(m.spectral_flux > 0.0, "flux={}", m.spectral_flux);
        assert!(m.zcr > 0.0 && m.zcr < 1.0, "zcr={}", m.zcr);
    }

    #[test]
    fn test_astats_channel_averaging() {
        let mut acc = AstatsAccum::default();
        let mut in_channel = false;
        for line in [
            "[Parsed_astats_0 @ 0x1] Channel: 1",
            "[Parsed_astats_0 @ 0x1] Crest factor: 8.0",
            "[Parsed_astats_0 @ 0x1] Zero crossings rate: 0.08",
            "[Parsed_astats_0 @ 0x1] Channel: 2",
            "[Parsed_astats_0 @ 0x1] Crest factor: 10.0",
            "[Parsed_astats_0 @ 0x1] Zero crossings rate: 0.10",
            "[Parsed_astats_0 @ 0x1] Overall",
            "[Parsed_astats_0 @ 0x1] RMS level dB: -19.4",
            "[Parsed_astats_0 @ 0x1] Crest factor: 9.0",
        ] {
            parse_astats_line(&mut acc, line, &mut in_channel);
        }
        assert_eq!(acc.channels, 2);
        assert!((acc.crest_sum / acc.channels as f64 - 9.0).abs() < 1e-9);
        assert!((acc.zcr_sum / acc.channels as f64 - 0.09).abs() < 1e-9);
        assert!((acc.rms_db.unwrap() - -19.4).abs() < 1e-9);
    }
}
