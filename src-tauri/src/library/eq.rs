use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const EQ_FREQUENCIES: [f32; 10] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EQProfile {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_active: bool,
    pub bands: [f32; 10],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EQBandUpdate {
    pub index: u8,
    pub gain_db: f32,
}

struct DefaultPreset {
    key: &'static str,
    name: &'static str,
    gains: [f32; 10],
}

/// Bundled presets, exact Airmedy curves (VLC/Spotify/Apple/Sony/Pioneer).
/// Gains in dB, Q = 1.0 for all bands, 0.5 dB steps.
const DEFAULT_PRESETS: &[DefaultPreset] = &[
    DefaultPreset { key: "flat", name: "Flat", gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] },
    DefaultPreset { key: "classical", name: "Classical", gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.0, -7.0, -7.0, -9.5] },
    DefaultPreset { key: "club", name: "Club", gains: [0.0, 0.0, 8.0, 5.5, 5.5, 5.5, 3.0, 0.0, 0.0, 0.0] },
    DefaultPreset { key: "dance", name: "Dance", gains: [9.5, 7.0, 2.5, 0.0, 0.0, -5.5, -7.0, -7.0, 0.0, 0.0] },
    DefaultPreset { key: "full_bass", name: "Full Bass", gains: [-8.0, 9.5, 9.5, 5.5, 1.5, -4.0, -8.0, -10.5, -11.0, -11.0] },
    DefaultPreset { key: "full_treble", name: "Full Treble", gains: [-9.5, -9.5, -9.5, -4.0, 2.5, 11.0, 12.0, 12.0, 12.0, 12.0] },
    DefaultPreset { key: "full_bass_treble", name: "Full Bass & Treble", gains: [7.0, 5.5, 0.0, -7.0, -5.0, 1.5, 8.0, 11.0, 12.0, 12.0] },
    DefaultPreset { key: "headphones", name: "Headphones", gains: [5.0, 11.0, 5.5, -3.0, -2.5, 1.5, 5.0, 9.5, 12.0, 12.0] },
    DefaultPreset { key: "large_hall", name: "Large Hall", gains: [10.5, 10.5, 5.5, 5.5, 0.0, -5.0, -5.0, -5.0, 0.0, 0.0] },
    DefaultPreset { key: "live", name: "Live", gains: [-5.0, 0.0, 4.0, 5.5, 5.5, 5.5, 4.0, 2.5, 2.5, 2.5] },
    DefaultPreset { key: "party", name: "Party", gains: [7.0, 7.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 7.0, 7.0] },
    DefaultPreset { key: "pop", name: "Pop", gains: [-1.5, 5.0, 7.0, 8.0, 5.5, 0.0, -2.5, -2.5, -1.5, -1.5] },
    DefaultPreset { key: "reggae", name: "Reggae", gains: [0.0, 0.0, 0.0, -5.5, 0.0, 6.5, 6.5, 0.0, 0.0, 0.0] },
    DefaultPreset { key: "rock", name: "Rock", gains: [8.0, 5.0, -5.5, -8.0, -3.0, 4.0, 9.0, 11.0, 11.0, 11.0] },
    DefaultPreset { key: "jazz", name: "Jazz", gains: [3.0, 2.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 2.0, 3.0] },
    DefaultPreset { key: "hip_hop", name: "Hip-Hop", gains: [5.0, 4.0, 3.0, 1.0, -1.0, -1.0, 0.0, -1.0, 1.0, 2.0] },
    DefaultPreset { key: "ska", name: "Ska", gains: [-2.5, -5.0, -4.0, 0.0, 4.0, 5.5, 9.0, 9.5, 11.0, 9.5] },
    DefaultPreset { key: "soft", name: "Soft", gains: [5.0, 1.5, 0.0, -2.5, 0.0, 4.0, 8.0, 9.5, 11.0, 12.0] },
    DefaultPreset { key: "soft_rock", name: "Soft Rock", gains: [4.0, 4.0, 2.5, 0.0, -4.0, -5.5, -3.0, 0.0, 2.5, 9.0] },
    DefaultPreset { key: "techno", name: "Techno", gains: [8.0, 5.5, 0.0, -5.5, -5.0, 0.0, 8.0, 9.5, 9.5, 9.0] },
    DefaultPreset { key: "bass_booster", name: "Bass Booster", gains: [5.5, 4.5, 3.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] },
    DefaultPreset { key: "treble_booster", name: "Treble Booster", gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 3.0, 4.5, 5.5] },
    DefaultPreset { key: "acoustic_vocal", name: "Acoustic / Vocal", gains: [2.5, 1.5, 0.5, 0.0, 1.0, 2.0, 2.5, 2.0, 1.5, 1.0] },
    DefaultPreset { key: "electronic_dance", name: "Electronic / Dance", gains: [4.5, 3.5, 1.5, -0.5, -1.5, 0.0, 1.5, 2.5, 3.5, 4.0] },
    DefaultPreset { key: "rnb_soul", name: "R&B / Soul", gains: [4.5, 3.5, 1.5, 0.0, 1.0, 1.5, 2.0, 1.5, 2.5, 3.0] },
    DefaultPreset { key: "vocal_booster", name: "Vocal Booster", gains: [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 2.5, 1.5, 1.0, 0.0] },
    DefaultPreset { key: "loudness", name: "Loudness", gains: [5.0, 3.5, 1.5, 0.0, -1.0, 0.0, 1.0, 2.0, 3.5, 4.5] },
    DefaultPreset { key: "spoken_word_podcast", name: "Spoken Word / Podcast", gains: [-3.0, -2.0, -1.0, 1.0, 2.5, 3.5, 3.0, 2.0, 1.0, 0.0] },
    DefaultPreset { key: "harman_target", name: "Harman Target", gains: [3.5, 3.0, 1.5, 0.0, 0.0, 0.5, 1.5, 2.5, 3.5, 4.0] },
    DefaultPreset { key: "sony_excited", name: "Sony Excited", gains: [4.0, 3.0, 1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0] },
    DefaultPreset { key: "sony_mellow", name: "Sony Mellow", gains: [2.0, 1.5, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.5, 2.0] },
];

/// Canonical backend order for built-in presets (Airmedy parity).
const PRESET_ORDER: &[&str] = &[
    "flat", "classical", "club", "dance", "full_bass", "full_treble", "full_bass_treble",
    "headphones", "large_hall", "live", "party", "pop", "reggae", "rock", "jazz", "hip_hop",
    "ska", "soft", "soft_rock", "techno", "bass_booster", "treble_booster", "acoustic_vocal",
    "electronic_dance", "rnb_soul", "vocal_booster", "loudness", "spoken_word_podcast",
    "harman_target", "sony_excited", "sony_mellow",
];

fn preset_by_key(key: &str) -> Option<&'static DefaultPreset> {
    DEFAULT_PRESETS.iter().find(|p| p.key == key)
}

/// Quantize to 0.5 dB steps within [-12, 12] (Airmedy `UpdateBand` parity).
pub fn quantize_gain(gain_db: f32) -> f32 {
    ((gain_db * 2.0).round() / 2.0).clamp(-12.0, 12.0)
}

/// Seed or sync the 31 bundled presets (Airmedy parity).
///
/// Key-based, idempotent, safe to run on every startup:
/// - existing built-ins matched by `preset_key` (falling back to legacy name)
///   get their exact gains refreshed and keep their ids (stable references);
/// - missing presets are inserted; the first run activates `flat`;
/// - built-in rows whose key retired from the catalog are deleted;
/// - user profiles (`is_default = 0`) are never touched.
pub async fn seed_defaults(pool: &SqlitePool) -> Result<(), String> {
    #[derive(sqlx::FromRow)]
    struct KeyRow {
        id: String,
        name: String,
        preset_key: String,
    }
    let existing: Vec<KeyRow> = sqlx::query_as(
        "SELECT id, name, preset_key FROM eq_profiles WHERE is_default = 1",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("{e}"))?;

    let name_to_key: std::collections::HashMap<&str, &str> = DEFAULT_PRESETS
        .iter()
        .map(|p| (p.name, p.key))
        .collect();

    let mut seen_keys = std::collections::HashSet::new();
    for row in &existing {
        let key = if !row.preset_key.is_empty() {
            row.preset_key.clone()
        } else {
            name_to_key.get(row.name.as_str()).copied().unwrap_or("").to_string()
        };
        match preset_by_key(&key) {
            Some(preset) => {
                seen_keys.insert(preset.key);
                // Refresh exact gains + key + display name, keep id + active flag.
                sqlx::query("UPDATE eq_profiles SET preset_key = ?, name = ? WHERE id = ?")
                    .bind(preset.key)
                    .bind(preset.name)
                    .bind(&row.id)
                    .execute(pool)
                    .await
                    .map_err(|e| format!("Failed to sync preset {}: {e}", preset.name))?;
                for (band_idx, gain) in preset.gains.iter().enumerate() {
                    sqlx::query(
                        "INSERT INTO eq_bands (profile_id, band_index, gain) VALUES (?, ?, ?)
                         ON CONFLICT(profile_id, band_index) DO UPDATE SET gain = excluded.gain",
                    )
                    .bind(&row.id)
                    .bind(band_idx as i32)
                    .bind(gain)
                    .execute(pool)
                    .await
                    .map_err(|e| format!("Failed to sync band: {e}"))?;
                }
            }
            None => {
                // Retired built-in: remove (user profiles are never defaults).
                sqlx::query("DELETE FROM eq_profiles WHERE id = ?")
                    .bind(&row.id)
                    .execute(pool)
                    .await
                    .map_err(|e| format!("Failed to remove retired preset: {e}"))?;
            }
        }
    }

    for preset in DEFAULT_PRESETS {
        if seen_keys.contains(preset.key) {
            continue;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let active = if preset.key == "flat" {
            let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM eq_profiles WHERE is_active = 1")
                .fetch_one(pool)
                .await
                .map_err(|e| format!("{e}"))?;
            if n.0 == 0 { 1 } else { 0 }
        } else {
            0
        };
        sqlx::query(
            "INSERT INTO eq_profiles (id, name, is_default, is_active, preset_key) VALUES (?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(preset.name)
        .bind(active)
        .bind(preset.key)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to seed preset {}: {e}", preset.name))?;
        for (band_idx, gain) in preset.gains.iter().enumerate() {
            sqlx::query("INSERT INTO eq_bands (profile_id, band_index, gain) VALUES (?, ?, ?)")
                .bind(&id)
                .bind(band_idx as i32)
                .bind(gain)
                .execute(pool)
                .await
                .map_err(|e| format!("Failed to seed band: {e}"))?;
        }
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: String,
    name: String,
    is_default: bool,
    is_active: bool,
    preset_key: String,
}

async fn load_bands(pool: &SqlitePool, profile_id: &str) -> Result<[f32; 10], String> {
    #[derive(sqlx::FromRow)]
    struct BandRow {
        band_index: i32,
        gain: f32,
    }
    let rows: Vec<BandRow> = sqlx::query_as(
        "SELECT band_index, gain FROM eq_bands WHERE profile_id = ? ORDER BY band_index"
    )
    .bind(profile_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("{e}"))?;
    let mut bands = [0.0f32; 10];
    for row in rows {
        if row.band_index >= 0 && (row.band_index as usize) < 10 {
            bands[row.band_index as usize] = row.gain;
        }
    }
    Ok(bands)
}

fn row_to_profile(row: ProfileRow, bands: [f32; 10]) -> EQProfile {
    EQProfile {
        id: row.id,
        name: row.name,
        is_default: row.is_default,
        is_active: row.is_active,
        bands,
    }
}

pub async fn get_all(pool: &SqlitePool) -> Result<Vec<EQProfile>, String> {
    let rows: Vec<ProfileRow> = sqlx::query_as(
        "SELECT id, name, is_default, is_active, preset_key FROM eq_profiles"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("{e}"))?;
    // Built-ins in canonical backend order, then user profiles alphabetically
    // (Airmedy `GetAllProfiles` parity) — keeps every UI list identical.
    let order: std::collections::HashMap<&str, usize> = PRESET_ORDER
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i))
        .collect();
    let mut built_ins: Vec<(usize, EQProfile)> = Vec::new();
    let mut user: Vec<EQProfile> = Vec::new();
    for row in rows {
        let key = row.preset_key.clone();
        let bands = load_bands(pool, &row.id).await?;
        let profile = row_to_profile(row, bands);
        if profile.is_default {
            built_ins.push((order.get(key.as_str()).copied().unwrap_or(usize::MAX), profile));
        } else {
            user.push(profile);
        }
    }
    built_ins.sort_by_key(|(o, _)| *o);
    user.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(built_ins.into_iter().map(|(_, p)| p).chain(user).collect())
}

pub async fn get_active(pool: &SqlitePool) -> Result<Option<EQProfile>, String> {
    let row: Option<ProfileRow> = sqlx::query_as(
        "SELECT id, name, is_default, is_active, preset_key FROM eq_profiles WHERE is_active = 1 LIMIT 1"
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("{e}"))?;
    match row {
        Some(r) => {
            let bands = load_bands(pool, &r.id).await?;
            Ok(Some(row_to_profile(r, bands)))
        }
        None => Ok(None),
    }
}

pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<EQProfile>, String> {
    let row: Option<ProfileRow> = sqlx::query_as(
        "SELECT id, name, is_default, is_active, preset_key FROM eq_profiles WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("{e}"))?;
    match row {
        Some(r) => {
            let bands = load_bands(pool, &r.id).await?;
            Ok(Some(row_to_profile(r, bands)))
        }
        None => Ok(None),
    }
}

pub async fn create(pool: &SqlitePool, name: &str) -> Result<EQProfile, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO eq_profiles (id, name, is_default, is_active) VALUES (?, ?, 0, 0)")
        .bind(&id)
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?;
    for i in 0..10 {
        sqlx::query("INSERT INTO eq_bands (profile_id, band_index, gain) VALUES (?, ?, 0.0)")
            .bind(&id)
            .bind(i as i32)
            .execute(pool)
            .await
            .map_err(|e| format!("{e}"))?;
    }
    Ok(EQProfile {
        id,
        name: name.to_string(),
        is_default: false,
        is_active: false,
        bands: [0.0; 10],
    })
}

pub async fn rename(pool: &SqlitePool, id: &str, name: &str) -> Result<(), String> {
    let affected = sqlx::query("UPDATE eq_profiles SET name = ? WHERE id = ? AND is_default = 0")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?
        .rows_affected();
    if affected == 0 {
        return Err("Profile not found or is a default preset".into());
    }
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let profile: Option<ProfileRow> = sqlx::query_as(
        "SELECT id, name, is_default, is_active, preset_key FROM eq_profiles WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("{e}"))?;
    match profile {
        Some(p) if p.is_default => Err("Cannot delete a default preset".into()),
        Some(_) => {
            sqlx::query("DELETE FROM eq_profiles WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| format!("{e}"))?;
            Ok(())
        }
        None => Err("Profile not found".into()),
    }
}

pub async fn set_active(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("UPDATE eq_profiles SET is_active = 0")
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?;
    sqlx::query("UPDATE eq_profiles SET is_active = 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

pub async fn update_band(pool: &SqlitePool, profile_id: &str, index: u8, gain_db: f32) -> Result<(), String> {
    let gain = quantize_gain(gain_db);
    sqlx::query("UPDATE eq_bands SET gain = ? WHERE profile_id = ? AND band_index = ?")
        .bind(gain)
        .bind(profile_id)
        .bind(index as i32)
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametricFilter {
    pub kind: String,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub enabled: bool,
}

impl From<ParametricFilter> for crate::audio::eq::Filter {
    fn from(p: ParametricFilter) -> Self {
        let kind = match p.kind.to_uppercase().as_str() {
            "LS" | "LSC" | "LOWSHELF" | "LOW SHELF" => crate::audio::eq::FilterKind::LowShelf,
            "HS" | "HSC" | "HIGHSHELF" | "HIGH SHELF" => crate::audio::eq::FilterKind::HighShelf,
            "LP" | "LPQ" | "LOWPASS" | "LOW PASS" => crate::audio::eq::FilterKind::LowPass,
            "HP" | "HPQ" | "HIGHPASS" | "HIGH PASS" => crate::audio::eq::FilterKind::HighPass,
            _ => crate::audio::eq::FilterKind::Peak,
        };
        crate::audio::eq::Filter {
            kind,
            freq: p.freq as f64,
            gain: p.gain as f64,
            q: p.q as f64,
            enabled: p.enabled,
        }
    }
}

impl From<crate::audio::eq::Filter> for ParametricFilter {
    fn from(f: crate::audio::eq::Filter) -> Self {
        let kind = match f.kind {
            crate::audio::eq::FilterKind::LowShelf => "lowshelf".to_string(),
            crate::audio::eq::FilterKind::HighShelf => "highshelf".to_string(),
            crate::audio::eq::FilterKind::LowPass => "lowpass".to_string(),
            crate::audio::eq::FilterKind::HighPass => "highpass".to_string(),
            crate::audio::eq::FilterKind::Peak => "peak".to_string(),
        };
        Self {
            kind,
            freq: f.freq as f32,
            gain: f.gain as f32,
            q: f.q as f32,
            enabled: f.enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoEqImportResult {
    pub preamp_db: f32,
    pub filters: Vec<ParametricFilter>,
    pub mapped_bands: [f32; 10],
}

pub fn parse_autoeq(text: &str) -> Result<AutoEqImportResult, String> {
    let mut preamp = 0.0f32;
    let mut filters = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let lower = line.to_lowercase();
        if lower.starts_with("preamp") {
            let num_part = line.trim_start_matches(|c: char| !c.is_numeric() && c != '-' && c != '+');
            let cleaned = num_part.replace("dB", "").replace("db", "").trim().to_string();
            let first_token = cleaned.split_whitespace().next().unwrap_or("");
            if let Ok(val) = first_token.parse::<f32>() {
                preamp = val;
            }
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        let on = !tokens.iter().any(|t| t.eq_ignore_ascii_case("OFF"));

        let kind = tokens.iter().find_map(|t| {
            let clean = t.trim_matches(|c: char| c == ':' || c == '=').to_uppercase();
            match clean.as_str() {
                "PK" | "PEQ" | "BELL" | "PEAK" => Some("peak"),
                "LSC" | "LS" | "LSQ" | "LOWSHELF" => Some("lowshelf"),
                "HSC" | "HS" | "HSQ" | "HIGHSHELF" => Some("highshelf"),
                "LP" | "LPQ" | "LOWPASS" => Some("lowpass"),
                "HP" | "HPQ" | "HIGHPASS" => Some("highpass"),
                _ => None,
            }
        });

        let value_after = |keys: &[&str]| -> Option<f32> {
            for (i, token) in tokens.iter().enumerate() {
                let clean = token.trim_matches(|c: char| c == ':' || c == '=').to_lowercase();
                if keys.iter().any(|k| clean == *k) {
                    if let Some(next) = tokens.get(i + 1) {
                        let clean_next = next.trim_matches(|c: char| {
                            c == ':' || c == '=' || c == 'h' || c == 'H' || c == 'z' || c == 'Z'
                                || c == 'd' || c == 'D' || c == 'b' || c == 'B'
                        });
                        if let Ok(v) = clean_next.parse::<f32>() {
                            return Some(v);
                        }
                    }
                }
            }
            None
        };

        let Some(kind_str) = kind else {
            continue;
        };

        let Some(freq) = value_after(&["fc", "freq", "frequency"]) else {
            continue;
        };

        let gain = value_after(&["gain"]).unwrap_or(0.0);
        let q = value_after(&["q"]).unwrap_or(match kind_str {
            "lowshelf" | "highshelf" | "lowpass" | "highpass" => 0.707,
            _ => 1.41,
        });

        filters.push(ParametricFilter {
            kind: kind_str.to_string(),
            freq,
            gain,
            q,
            enabled: on,
        });
    }

    if filters.is_empty() {
        return Err("No valid parametric filters found in text.".to_string());
    }

    // Map parametric filters to Eiryn's 10 graphic bands:
    // [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0]
    let mut mapped_gains = [0.0f32; 10];
    for (i, &band_f) in EQ_FREQUENCIES.iter().enumerate() {
        let mut total_gain = 0.0f32;
        for f in &filters {
            if !f.enabled {
                continue;
            }
            let fc = f.freq.max(1.0);
            match f.kind.as_str() {
                "PK" => {
                    let delta = f.q * (band_f / fc - fc / band_f).abs();
                    let weight = 1.0 / (1.0 + delta * delta);
                    total_gain += f.gain * weight;
                }
                "LS" => {
                    if band_f <= fc {
                        total_gain += f.gain;
                    } else {
                        let ratio = band_f / fc;
                        total_gain += f.gain / (1.0 + (ratio - 1.0) * f.q * 2.0);
                    }
                }
                "HS" => {
                    if band_f >= fc {
                        total_gain += f.gain;
                    } else {
                        let ratio = fc / band_f;
                        total_gain += f.gain / (1.0 + (ratio - 1.0) * f.q * 2.0);
                    }
                }
                _ => {}
            }
        }
        mapped_gains[i] = quantize_gain(total_gain);
    }

    Ok(AutoEqImportResult {
        preamp_db: preamp,
        filters,
        mapped_bands: mapped_gains,
    })
}

pub async fn import_autoeq(pool: &SqlitePool, name: &str, text: &str) -> Result<EQProfile, String> {
    let result = parse_autoeq(text)?;
    let profile = create(pool, name).await?;
    for (idx, &gain) in result.mapped_bands.iter().enumerate() {
        update_band(pool, &profile.id, idx as u8, gain).await?;
    }
    get_by_id(pool, &profile.id).await?.ok_or_else(|| "Failed to retrieve imported profile".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_table_matches_airmedy() {
        // 31 bundled presets, exact Airmedy curves.
        assert_eq!(DEFAULT_PRESETS.len(), 31);
        assert_eq!(PRESET_ORDER.len(), 31);
        for preset in DEFAULT_PRESETS {
            assert_eq!(preset.gains.len(), 10);
            assert!(PRESET_ORDER.contains(&preset.key), "preset {} missing from order", preset.key);
            for g in preset.gains {
                assert!((-12.0..=12.0).contains(&g), "preset {} gain out of range", preset.key);
                assert_eq!(g * 2.0, (g * 2.0).round(), "preset {} not on 0.5dB grid", preset.key);
            }
        }
        // Spot-check signature curves.
        let rock = preset_by_key("rock").unwrap();
        assert_eq!(rock.gains, [8.0, 5.0, -5.5, -8.0, -3.0, 4.0, 9.0, 11.0, 11.0, 11.0]);
        let flat = preset_by_key("flat").unwrap();
        assert!(flat.gains.iter().all(|g| *g == 0.0));
    }

    #[test]
    fn test_quantize_gain() {
        assert_eq!(quantize_gain(3.26), 3.5);
        assert_eq!(quantize_gain(3.24), 3.0);
        assert_eq!(quantize_gain(-7.0), -7.0);
        assert_eq!(quantize_gain(20.0), 12.0);
        assert_eq!(quantize_gain(-20.0), -12.0);
    }
}
