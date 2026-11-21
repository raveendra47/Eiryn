//! Library health and audio quality forensics.
//!
//! - Fake lossless detection: FFT analysis detecting frequency cutoffs (MP3 lowpass, CD upsampled to 96k, 16-bit padded to 24-bit).
//! - Duplicate detection: Finds identical recordings (lossless vs. lossy duplicates).
//! - Windows Recycle Bin safe deletion.

use std::path::Path;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Row};

const ANALYSIS_SECS: f64 = 20.0;
const CLIFF_DB: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Good,
    Suspect,
    Lossy,
    Upsampled,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Good => "good",
            Verdict::Suspect => "suspect",
            Verdict::Lossy => "lossy",
            Verdict::Upsampled => "upsampled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "lossy" => Verdict::Lossy,
            "suspect" => Verdict::Suspect,
            "upsampled" => Verdict::Upsampled,
            _ => Verdict::Good,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QualityResult {
    pub cutoff_hz: Option<f32>,
    pub verdict: Verdict,
    pub real_bits: Option<u8>,
}

pub fn verdict(rate: u32, cutoff_hz: Option<f32>) -> Verdict {
    let Some(c) = cutoff_hz else { return Verdict::Good };
    if rate <= 50_000 {
        if c < 17_500.0 {
            Verdict::Lossy
        } else if c < 19_500.0 {
            Verdict::Suspect
        } else {
            Verdict::Good
        }
    } else if c < 19_500.0 {
        Verdict::Lossy
    } else if c < 23_500.0 {
        Verdict::Upsampled
    } else {
        Verdict::Good
    }
}

fn band_levels(mono: &[f32], rate: u32) -> Option<(Vec<f32>, f32)> {
    let n: usize = if rate > 50_000 { 8192 } else { 4096 };
    let bins = n / 2 + 1;
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let window_sum: f32 = window.iter().sum();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let mut power = vec![0f64; bins];
    let mut frames = 0u32;
    let mut start = 0;

    while start + n <= mono.len() {
        let chunk = &mono[start..start + n];
        start += n / 2;
        let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt();
        if rms < 0.003 {
            continue;
        }
        for i in 0..n {
            input[i] = chunk[i] * window[i];
        }
        if fft.process_with_scratch(&mut input, &mut output, &mut scratch).is_ok() {
            for (p, c) in power.iter_mut().zip(&output) {
                let amp = 2.0 * c.norm() / window_sum;
                *p += f64::from(amp * amp);
            }
            frames += 1;
        }
    }

    if frames < 15 {
        return None;
    }

    let hz_per_bin = rate as f32 / n as f32;
    let band_bins = (250.0 / hz_per_bin).round().max(1.0) as usize;
    let bands: Vec<f32> = power
        .chunks(band_bins)
        .filter(|c| c.len() == band_bins)
        .map(|c| (10.0 * (c.iter().sum::<f64>() / (c.len() as f64 * f64::from(frames)) + 1e-20).log10()) as f32)
        .collect();

    Some((bands, band_bins as f32 * hz_per_bin))
}

fn find_cliff(bands: &[f32], band_hz: f32) -> Option<f32> {
    let per_khz = (1000.0 / band_hz).round().max(1.0) as usize;
    let first = (10_000.0 / band_hz) as usize;
    let mean = |s: &[f32]| s.iter().sum::<f32>() / s.len() as f32;

    let mut best: Option<(usize, f32)> = None;
    for i in first.max(per_khz)..bands.len().saturating_sub(per_khz * 2) {
        let before = mean(&bands[i - per_khz..i]);
        let after = mean(&bands[i..i + per_khz]);
        let drop = before - after;
        if drop < CLIFF_DB || before < -112.0 {
            continue;
        }
        let tail_top = bands[i + per_khz..].iter().copied().fold(f32::MIN, f32::max);
        if tail_top <= before - CLIFF_DB && best.map_or(true, |(_, d)| drop > d) {
            best = Some((i, drop));
        }
    }

    let (i, _) = best?;
    let before = mean(&bands[i - per_khz..i]);
    let at = (i - per_khz..i + per_khz).find(|&j| bands[j] < before - 12.0).unwrap_or(i);
    Some(at as f32 * band_hz)
}

pub fn spectrum_cutoff(mono: &[f32], rate: u32) -> Option<Option<f32>> {
    let (bands, band_hz) = band_levels(mono, rate)?;
    Some(find_cliff(&bands, band_hz))
}

/// How many low bits are zero in every sample of a 24-bit file (the file is really 24 − this bits).
pub fn padded_bits(samples: &[f32], bits: u32) -> u32 {
    let scale = (1u64 << (bits - 1)) as f32;
    let mut all = 0i64;
    for &s in samples {
        all |= (s * scale).round() as i64;
        if all & 1 != 0 {
            return 0;
        }
    }
    if all == 0 { 0 } else { all.trailing_zeros().min(bits - 1) }
}

pub fn check_file_quality(path: &Path, _duration: f64) -> Result<QualityResult, String> {
    let mut src = crate::audio::source::Source::open(path)
        .map_err(|e| format!("Failed to open audio decoder: {e}"))?;

    let rate = src.rate;
    let bits = src.bits;
    let want_frames = (rate as f64 * ANALYSIS_SECS) as usize;
    let mut mono = Vec::with_capacity(want_frames);
    let mut all_samples = Vec::new();

    while mono.len() < want_frames {
        match src.next_chunk() {
            Ok(Some((chunk, _rate, channels))) => {
                let ch = channels.max(1);
                for frame in chunk.chunks_exact(ch) {
                    let sum: f32 = frame.iter().copied().sum();
                    mono.push(sum / ch as f32);
                    if mono.len() >= want_frames {
                        break;
                    }
                }
                if bits == Some(24) {
                    all_samples.extend_from_slice(chunk);
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    if mono.is_empty() {
        return Err("Could not decode audio samples".into());
    }

    let cutoff = spectrum_cutoff(&mono, rate).flatten();
    let mut verd = verdict(rate, cutoff);

    let real_bits = match bits {
        Some(24) => {
            let zeros = padded_bits(&all_samples, 24);
            if zeros >= 8 {
                verd = Verdict::Upsampled;
                Some((24 - zeros) as u8)
            } else {
                None
            }
        }
        _ => None,
    };

    Ok(QualityResult {
        cutoff_hz: cutoff,
        verdict: verd,
        real_bits,
    })
}

pub async fn init_health_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS track_health (
            track_id        TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
            cutoff_hz       REAL,
            verdict         TEXT NOT NULL DEFAULT 'good',
            real_bits       INTEGER,
            integrity_status TEXT DEFAULT 'ok',
            integrity_error  TEXT,
            checked_at      TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create track_health table: {e}"))?;

    let _ = sqlx::query("ALTER TABLE track_health ADD COLUMN real_bits INTEGER").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE track_health ADD COLUMN integrity_status TEXT DEFAULT 'ok'").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE track_health ADD COLUMN integrity_error TEXT").execute(pool).await;

    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HealthSummary {
    pub total_analyzed: i64,
    pub good_count: i64,
    pub suspect_count: i64,
    pub lossy_count: i64,
    pub upsampled_count: i64,
    pub fake_24_count: i64,
    pub damaged_count: i64,
    pub duplicate_groups_count: i64,
}

pub async fn get_health_summary(pool: &SqlitePool) -> Result<HealthSummary, String> {
    init_health_table(pool).await?;

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let good: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE verdict = 'good' AND real_bits IS NULL AND (integrity_status IS NULL OR integrity_status != 'damaged')")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let suspect: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE verdict = 'suspect'")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let lossy: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE verdict = 'lossy'")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let upsampled: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE verdict = 'upsampled'")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let fake_24: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE real_bits IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let damaged: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM track_health WHERE integrity_status = 'damaged'")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let duplicates: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM (
            SELECT title, artist FROM tracks
            WHERE title != '' AND artist != ''
            GROUP BY LOWER(TRIM(title)), LOWER(TRIM(artist))
            HAVING COUNT(*) > 1
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(HealthSummary {
        total_analyzed: total.0,
        good_count: good.0,
        suspect_count: suspect.0,
        lossy_count: lossy.0,
        upsampled_count: upsampled.0,
        fake_24_count: fake_24.0,
        damaged_count: damaged.0,
        duplicate_groups_count: duplicates.0,
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SuspectTrackItem {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub file_path: String,
    pub file_format: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub cutoff_hz: Option<f32>,
    pub verdict: String,
    pub real_bits: Option<u8>,
}

pub async fn get_suspect_tracks(pool: &SqlitePool, limit: i64) -> Result<Vec<SuspectTrackItem>, String> {
    init_health_table(pool).await?;

    let rows = sqlx::query(
        "SELECT t.id, t.title, t.artist, t.file_path, t.file_format, t.sample_rate, t.bit_depth,
                h.cutoff_hz, h.verdict, h.real_bits
         FROM track_health h
         JOIN tracks t ON h.track_id = t.id
         WHERE h.verdict != 'good' OR h.real_bits IS NOT NULL
         ORDER BY h.cutoff_hz ASC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for r in rows {
        let cutoff: Option<f64> = r.try_get("cutoff_hz").ok();
        let real_bits: Option<i64> = r.try_get("real_bits").ok().flatten();
        items.push(SuspectTrackItem {
            id: r.get("id"),
            title: r.get("title"),
            artist: r.get("artist"),
            file_path: r.get("file_path"),
            file_format: r.get("file_format"),
            sample_rate: r.get::<i64, _>("sample_rate") as u32,
            bit_depth: r.get::<i64, _>("bit_depth") as u32,
            cutoff_hz: cutoff.map(|c| c as f32),
            verdict: r.get("verdict"),
            real_bits: real_bits.map(|b| b as u8),
        });
    }

    Ok(items)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DuplicateGroup {
    pub title: String,
    pub artist: String,
    pub tracks: Vec<SuspectTrackItem>,
}

pub async fn get_duplicate_tracks(pool: &SqlitePool) -> Result<Vec<DuplicateGroup>, String> {
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.artist, t.file_path, t.file_format, t.sample_rate, t.bit_depth
         FROM tracks t
         WHERE (LOWER(TRIM(t.title)), LOWER(TRIM(t.artist))) IN (
             SELECT LOWER(TRIM(title)), LOWER(TRIM(artist))
             FROM tracks
             WHERE title != '' AND artist != ''
             GROUP BY LOWER(TRIM(title)), LOWER(TRIM(artist))
             HAVING COUNT(*) > 1
         )
         ORDER BY LOWER(TRIM(t.title)), LOWER(TRIM(t.artist)), t.sample_rate DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut groups: std::collections::HashMap<(String, String), Vec<SuspectTrackItem>> = std::collections::HashMap::new();

    for r in rows {
        let title: String = r.get("title");
        let artist: String = r.get("artist");
        let key = (title.to_lowercase().trim().to_string(), artist.to_lowercase().trim().to_string());

        let item = SuspectTrackItem {
            id: r.get("id"),
            title,
            artist,
            file_path: r.get("file_path"),
            file_format: r.get("file_format"),
            sample_rate: r.get::<i64, _>("sample_rate") as u32,
            bit_depth: r.get::<i64, _>("bit_depth") as u32,
            cutoff_hz: None,
            verdict: "duplicate".into(),
            real_bits: None,
        };

        groups.entry(key).or_default().push(item);
    }

    let mut result = Vec::new();
    for ((_, _), tracks) in groups {
        if tracks.len() > 1 {
            result.push(DuplicateGroup {
                title: tracks[0].title.clone(),
                artist: tracks[0].artist.clone(),
                tracks,
            });
        }
    }

    result.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(result)
}

/// Moves a track file to the Windows Recycle Bin instead of deleting permanently.
#[cfg(windows)]
pub fn move_to_trash(file_path: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT};

    let path = Path::new(file_path);
    if !path.exists() {
        return Err("File does not exist".into());
    }

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0); // SHFileOperation requires double null terminator

    let mut op = SHFILEOPSTRUCTW {
        hwnd: Default::default(),
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(wide.as_ptr()),
        pTo: windows::core::PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16,
        fAnyOperationsAborted: false.into(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: windows::core::PCWSTR::null(),
    };

    let ret = unsafe { SHFileOperationW(&mut op) };
    if ret != 0 {
        return Err(format!("Recycle bin operation failed with error code: {ret}"));
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn move_to_trash(file_path: &str) -> Result<(), String> {
    std::fs::remove_file(file_path).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SplitAlbumVariant {
    pub album_artist: String,
    pub track_count: i64,
    pub track_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SplitAlbumItem {
    pub album_name: String,
    pub total_tracks: i64,
    pub variants: Vec<SplitAlbumVariant>,
}

pub async fn get_split_albums(pool: &SqlitePool) -> Result<Vec<SplitAlbumItem>, String> {
    let rows = sqlx::query(
        "SELECT album, COALESCE(NULLIF(album_artist, ''), artist) as effective_artist, id
         FROM tracks
         WHERE album != ''
         ORDER BY LOWER(TRIM(album)), effective_artist"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut album_map: std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>> = std::collections::HashMap::new();

    for r in rows {
        let album: String = r.get("album");
        let artist: String = r.get("effective_artist");
        let id: String = r.get("id");
        album_map.entry(album.trim().to_string())
            .or_default()
            .entry(artist.trim().to_string())
            .or_default()
            .push(id);
    }

    let mut result = Vec::new();
    for (album_name, artist_groups) in album_map {
        if artist_groups.len() > 1 {
            let mut total = 0i64;
            let mut variants = Vec::new();
            for (artist, ids) in artist_groups {
                let cnt = ids.len() as i64;
                total += cnt;
                variants.push(SplitAlbumVariant {
                    album_artist: artist,
                    track_count: cnt,
                    track_ids: ids,
                });
            }
            variants.sort_by(|a, b| b.track_count.cmp(&a.track_count));
            result.push(SplitAlbumItem {
                album_name,
                total_tracks: total,
                variants,
            });
        }
    }

    result.sort_by(|a, b| a.album_name.cmp(&b.album_name));
    Ok(result)
}

pub async fn merge_split_album(pool: &SqlitePool, track_ids: &[String], target_album_artist: &str) -> Result<u64, String> {
    let mut updated = 0u64;
    for id in track_ids {
        let res = sqlx::query("UPDATE tracks SET album_artist = ? WHERE id = ?")
            .bind(target_album_artist)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        updated += res.rows_affected();
    }
    Ok(updated)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IncompleteAlbumItem {
    pub album: String,
    pub artist: String,
    pub existing_tracks: Vec<u32>,
    pub missing_tracks: Vec<u32>,
    pub total_expected: u32,
}

pub async fn get_incomplete_albums(pool: &SqlitePool) -> Result<Vec<IncompleteAlbumItem>, String> {
    let rows = sqlx::query(
        "SELECT album, COALESCE(NULLIF(album_artist, ''), artist) as effective_artist, track_number
         FROM tracks
         WHERE album != '' AND track_number > 0
         ORDER BY LOWER(TRIM(album)), track_number ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut album_tracks: std::collections::HashMap<(String, String), Vec<u32>> = std::collections::HashMap::new();

    for r in rows {
        let album: String = r.get("album");
        let artist: String = r.get("effective_artist");
        let track_no: i64 = r.get("track_number");
        album_tracks.entry((album, artist)).or_default().push(track_no as u32);
    }

    let mut result = Vec::new();
    for ((album, artist), mut numbers) in album_tracks {
        numbers.sort_unstable();
        numbers.dedup();
        if numbers.len() >= 2 {
            let max_track = *numbers.last().unwrap();
            let min_track = *numbers.first().unwrap();
            if min_track == 1 && max_track > numbers.len() as u32 && max_track <= 50 {
                let existing_set: std::collections::HashSet<u32> = numbers.iter().copied().collect();
                let mut missing = Vec::new();
                for i in 1..=max_track {
                    if !existing_set.contains(&i) {
                        missing.push(i);
                    }
                }
                if !missing.is_empty() {
                    result.push(IncompleteAlbumItem {
                        album,
                        artist,
                        existing_tracks: numbers,
                        missing_tracks: missing,
                        total_expected: max_track,
                    });
                }
            }
        }
    }

    result.sort_by(|a, b| a.album.cmp(&b.album));
    Ok(result)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DualScriptArtistPair {
    pub name_a: String,
    pub name_b: String,
    pub track_count_a: i64,
    pub track_count_b: i64,
}

pub async fn get_dual_script_artists(pool: &SqlitePool) -> Result<Vec<DualScriptArtistPair>, String> {
    let rows = sqlx::query(
        "SELECT artist, COUNT(*) as cnt
         FROM tracks
         WHERE artist != '' AND artist != 'Unknown Artist'
         GROUP BY artist
         ORDER BY artist ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let artists: Vec<(String, i64)> = rows.into_iter().map(|r| (r.get("artist"), r.get("cnt"))).collect();
    let mut pairs = Vec::new();

    for i in 0..artists.len() {
        let (ref a, cnt_a) = artists[i];
        let a_has_cjk = a.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c));
        for j in (i + 1)..artists.len() {
            let (ref b, cnt_b) = artists[j];
            let b_has_cjk = b.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c));

            let norm_a = a.to_lowercase().replace([' ', '-', '_', '.', ','], "");
            let norm_b = b.to_lowercase().replace([' ', '-', '_', '.', ','], "");

            let matched = if a_has_cjk != b_has_cjk {
                norm_a.contains(&norm_b) || norm_b.contains(&norm_a)
            } else {
                norm_a == norm_b && a != b
            };

            if matched {
                pairs.push(DualScriptArtistPair {
                    name_a: a.clone(),
                    name_b: b.clone(),
                    track_count_a: cnt_a,
                    track_count_b: cnt_b,
                });
            }
        }
    }

    Ok(pairs)
}

pub async fn merge_artist_names(pool: &SqlitePool, from_artist: &str, to_artist: &str) -> Result<u64, String> {
    let res = sqlx::query("UPDATE tracks SET artist = ? WHERE artist = ?")
        .bind(to_artist)
        .bind(from_artist)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let _ = sqlx::query("UPDATE tracks SET album_artist = ? WHERE album_artist = ?")
        .bind(to_artist)
        .bind(from_artist)
        .execute(pool)
        .await;
    Ok(res.rows_affected())
}




#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Integrity {
    /// Audio matched the checksum stored in the file (None if the format/file has none).
    pub verified: Option<bool>,
    /// Number of packets that failed to decode.
    pub errors: u32,
    /// Whether the decoded audio was shorter than header says.
    pub truncated: bool,
    /// Unrecoverable decode or I/O failure.
    pub failed: Option<String>,
}

impl Integrity {
    pub fn is_bad(&self) -> bool {
        self.verified == Some(false) || self.errors > 0 || self.truncated || self.failed.is_some()
    }

    pub fn error_message(&self) -> Option<String> {
        if let Some(ref f) = self.failed {
            return Some(format!("Decode failure: {f}"));
        }
        if self.verified == Some(false) {
            return Some("FLAC checksum mismatch (STREAMINFO MD5 checksum does not match decoded PCM)".into());
        }
        if self.errors > 0 {
            return Some(format!("{} packet decoding error(s)", self.errors));
        }
        if self.truncated {
            return Some("Truncated audio stream (fewer frames than header specified)".into());
        }
        None
    }
}

/// Decodes the full file through Symphonia with checksum verification enabled (FLAC STREAMINFO MD5).
pub fn check_integrity(path: &Path) -> Integrity {
    let mut src = match crate::audio::source::Source::open_verified(path) {
        Ok(s) => s,
        Err(e) => return Integrity { failed: Some(format!("{e:#}")), ..Default::default() },
    };
    let mut frames = 0u64;
    let failed = loop {
        match src.next_chunk() {
            Ok(Some((buf, _, ch))) => frames += (buf.len() / ch.max(1)) as u64,
            Ok(None) => break None,
            Err(e) => break Some(format!("{e:#}")),
        }
    };
    let verified = if failed.is_none() { src.finalize() } else { None };
    let is_lossless = matches!(src.codec.as_str(), "FLAC" | "ALAC" | "WAV" | "AIFF" | "APE" | "WAVPACK");
    let truncated = is_lossless && src.total_frames.is_some_and(|t| frames + 2 < t);
    Integrity { verified, errors: src.errors, truncated, failed }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DamagedTrackItem {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub file_path: String,
    pub file_format: String,
    pub error: String,
}

pub async fn get_damaged_tracks(pool: &SqlitePool) -> Result<Vec<DamagedTrackItem>, String> {
    init_health_table(pool).await?;

    let rows = sqlx::query(
        "SELECT t.id, t.title, t.artist, t.album, t.file_path, t.file_format,
                COALESCE(h.integrity_error, 'Corrupted audio data or failed checksum') as error
         FROM track_health h
         JOIN tracks t ON h.track_id = t.id
         WHERE h.integrity_status = 'damaged'
         ORDER BY t.artist COLLATE NOCASE ASC, t.album COLLATE NOCASE ASC, t.title COLLATE NOCASE ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for r in rows {
        items.push(DamagedTrackItem {
            id: r.get("id"),
            title: r.get("title"),
            artist: r.get("artist"),
            album: r.get("album"),
            file_path: r.get("file_path"),
            file_format: r.get("file_format"),
            error: r.get("error"),
        });
    }

    Ok(items)
}

pub async fn verify_track_integrity(pool: &SqlitePool, track_id: &str) -> Result<Option<DamagedTrackItem>, String> {
    init_health_table(pool).await?;

    let row = sqlx::query("SELECT id, title, artist, album, file_path, file_format FROM tracks WHERE id = ?")
        .bind(track_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    let Some(r) = row else {
        return Err(format!("Track not found: {track_id}"));
    };

    let file_path: String = r.get("file_path");
    let path = std::path::PathBuf::from(&file_path);

    let integrity = tokio::task::spawn_blocking(move || {
        check_integrity(&path)
    })
    .await
    .map_err(|e| e.to_string())?;

    let (status, err_msg) = if let Some(msg) = integrity.error_message() {
        ("damaged", Some(msg))
    } else {
        ("ok", None)
    };

    sqlx::query(
        "INSERT INTO track_health (track_id, integrity_status, integrity_error, checked_at)
         VALUES (?, ?, ?, datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
            integrity_status = excluded.integrity_status,
            integrity_error = excluded.integrity_error,
            checked_at = excluded.checked_at"
    )
    .bind(track_id)
    .bind(status)
    .bind(&err_msg)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    if let Some(err) = err_msg {
        Ok(Some(DamagedTrackItem {
            id: r.get("id"),
            title: r.get("title"),
            artist: r.get("artist"),
            album: r.get("album"),
            file_path,
            file_format: r.get("file_format"),
            error: err,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padded_bits_detection() {
        let scale_24 = (1u64 << 23) as f32;
        let mut padded_samples = Vec::new();
        for i in 1..100 {
            let val_16 = (i as i64) << 8;
            padded_samples.push(val_16 as f32 / scale_24);
        }
        let zeros = padded_bits(&padded_samples, 24);
        assert_eq!(zeros, 8);

        let mut real_24_samples = Vec::new();
        for i in 1..100 {
            let val_24 = (i as i64 * 37) | 1;
            real_24_samples.push(val_24 as f32 / scale_24);
        }
        let real_zeros = padded_bits(&real_24_samples, 24);
        assert_eq!(real_zeros, 0);
    }
}
