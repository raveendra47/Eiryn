//! Mood derivation (Airmedy parity).
//!
//! Raw spectral features are normalized against corpus percentiles with a
//! sigmoid (steepness k=2.5), then combined with locked weights:
//!   energy       = .32·rms + .23·centroid + .17·flux + .18·tempo + .10·(1−crest)
//!   danceability = .45·tempoScore + .30·(1−onsetVar) + .15·(1−crest) + .10·(1−LRA)
//!   brightness   = norm(centroid)
//!
//! Onset variance has no Eiryn source yet (deferred aubio equivalent), so the
//! danceability onset term uses its neutral value 0.5 until then.

use crate::library::db::{self, FeaturePercentiles};
use sqlx::SqlitePool;

const SIGMOID_K: f64 = 2.5;

/// Sigmoid normalization against corpus percentiles (Airmedy `Normalize`).
/// Clamps to [P1, P99]; degenerate spread yields 0.5.
pub fn normalize(x: f64, p: &FeaturePercentiles) -> f64 {
    let spread = (p.p95 - p.p5) / 2.0;
    if spread.abs() < 1e-12 {
        return 0.5;
    }
    let clamped = x.clamp(p.p1, p.p99);
    let z = SIGMOID_K * (clamped - p.p50) / spread;
    1.0 / (1.0 + (-z).exp())
}

/// Triangular tempo score: 0 at ≤60 or ≥180 BPM, 1 at 115 BPM.
pub fn tempo_score(tempo_bpm: f64) -> f64 {
    if tempo_bpm <= 60.0 || tempo_bpm >= 180.0 {
        return 0.0;
    }
    if tempo_bpm <= 115.0 {
        (tempo_bpm - 60.0) / (115.0 - 60.0)
    } else {
        (180.0 - tempo_bpm) / (180.0 - 115.0)
    }
    .clamp(0.0, 1.0)
}

pub struct MoodInputs {
    pub rms_db: f64,
    pub spectral_centroid: f64,
    pub spectral_flux: f64,
    pub crest: f64,
    pub loudness_range: f64,
    pub tempo_bpm: f64,
}

pub struct MoodScores {
    pub energy: f64,
    pub danceability: f64,
    pub brightness: f64,
}

pub struct PercentileSet {
    pub rms_db: FeaturePercentiles,
    pub spectral_centroid: FeaturePercentiles,
    pub spectral_flux: FeaturePercentiles,
    pub crest: FeaturePercentiles,
    pub loudness_range: FeaturePercentiles,
}

impl PercentileSet {
    /// Load all five feature percentile sets; None when any is missing
    /// (mood stays NULL until the corpus is large enough).
    pub async fn load(pool: &SqlitePool) -> Result<Option<Self>, String> {
        let rms_db = db::get_percentiles(pool, "rms_db").await?;
        let spectral_centroid = db::get_percentiles(pool, "spectral_centroid").await?;
        let spectral_flux = db::get_percentiles(pool, "spectral_flux").await?;
        let crest = db::get_percentiles(pool, "crest").await?;
        let loudness_range = db::get_percentiles(pool, "loudness_range").await?;
        match (rms_db, spectral_centroid, spectral_flux, crest, loudness_range) {
            (Some(rms_db), Some(spectral_centroid), Some(spectral_flux), Some(crest), Some(loudness_range)) => {
                Ok(Some(Self { rms_db, spectral_centroid, spectral_flux, crest, loudness_range }))
            }
            _ => Ok(None),
        }
    }
}

pub fn derive_mood(inputs: &MoodInputs, pctl: &PercentileSet) -> MoodScores {
    let n_rms = normalize(inputs.rms_db, &pctl.rms_db);
    let n_centroid = normalize(inputs.spectral_centroid, &pctl.spectral_centroid);
    let n_flux = normalize(inputs.spectral_flux, &pctl.spectral_flux);
    let n_crest = normalize(inputs.crest, &pctl.crest);
    let n_lra = normalize(inputs.loudness_range, &pctl.loudness_range);
    let tempo = inputs.tempo_bpm.min(180.0).max(0.0);

    MoodScores {
        energy: 0.32 * n_rms
            + 0.23 * n_centroid
            + 0.17 * n_flux
            + 0.18 * raw_tempo_term(tempo)
            + 0.10 * (1.0 - n_crest),
        // Onset variance unavailable: neutral 0.5 for the (1−onset) term.
        danceability: 0.45 * tempo_score(tempo)
            + 0.30 * 0.5
            + 0.15 * (1.0 - n_crest)
            + 0.10 * (1.0 - n_lra),
        brightness: n_centroid,
    }
}

/// Airmedy's energy tempo term is `norm(min(tempo,180))` against a tempo
/// corpus; without one, the fixed-scale triangular score (same shape,
/// anchored to musical reality) stands in — see `raw_tempo_term`.
fn raw_tempo_term(tempo_bpm: f64) -> f64 {
    tempo_score(tempo_bpm.min(180.0))
}

/// Numpy-linear percentile over a sorted sample.
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = q / 100.0 * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
}

pub fn percentile_set(mut values: Vec<f64>) -> Option<FeaturePercentiles> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(FeaturePercentiles {
        p1: percentile(&values, 1.0),
        p5: percentile(&values, 5.0),
        p50: percentile(&values, 50.0),
        p95: percentile(&values, 95.0),
        p99: percentile(&values, 99.0),
    })
}

/// Recompute corpus percentiles for the five mood features.
pub async fn recompute_percentiles(pool: &SqlitePool) -> Result<usize, String> {
    let mut done = 0;
    for feature in ["rms_db", "spectral_centroid", "spectral_flux", "crest", "loudness_range"] {
        let values = db::get_raw_feature_values(pool, feature).await?;
        if let Some(p) = percentile_set(values) {
            db::upsert_percentiles(pool, feature, p).await?;
            done += 1;
        }
    }
    Ok(done)
}

/// Recompute stored mood for every analyzed track (after percentile refresh).
/// Tracks without percentiles yet keep NULL mood (SmartMix falls back).
pub async fn recompute_all_moods(pool: &SqlitePool) -> Result<usize, String> {
    let Some(pctl) = PercentileSet::load(pool).await? else {
        return Ok(0);
    };

    let tracks = db::get_analyzed_tracks_with_tempo(pool).await?;
    let mut done = 0;
    for (id, tempo) in tracks {
        if let Some(a) = db::get_track_analysis(pool, &id).await? {
            let (rms, centroid, flux, crest_v, lra) = match (
                a.rms_db, a.spectral_centroid, a.spectral_flux, a.crest, a.loudness_range,
            ) {
                (Some(r), Some(c), Some(f), Some(cr), Some(l)) => (r, c, f, cr, l),
                _ => continue,
            };
            let scores = derive_mood(
                &MoodInputs {
                    rms_db: rms,
                    spectral_centroid: centroid,
                    spectral_flux: flux,
                    crest: crest_v,
                    loudness_range: lra,
                    tempo_bpm: tempo,
                },
                &pctl,
            );
            db::set_track_mood(pool, &id, scores.energy, scores.danceability, scores.brightness).await?;
            done += 1;
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_percentiles() -> FeaturePercentiles {
        FeaturePercentiles { p1: 0.0, p5: 10.0, p50: 50.0, p95: 90.0, p99: 100.0 }
    }

    #[test]
    fn test_normalize() {
        let p = flat_percentiles();
        assert!((normalize(50.0, &p) - 0.5).abs() < 1e-9);
        assert!(normalize(200.0, &p) > 0.95);
        assert!(normalize(-200.0, &p) < 0.05);
        // Degenerate spread → neutral.
        let flat = FeaturePercentiles { p1: 5.0, p5: 5.0, p50: 5.0, p95: 5.0, p99: 5.0 };
        assert_eq!(normalize(5.0, &flat), 0.5);
    }

    #[test]
    fn test_tempo_score() {
        assert_eq!(tempo_score(60.0), 0.0);
        assert_eq!(tempo_score(180.0), 0.0);
        assert!((tempo_score(115.0) - 1.0).abs() < 1e-9);
        assert!(tempo_score(90.0) > tempo_score(70.0));
        assert!(tempo_score(150.0) > tempo_score(170.0));
    }

    #[test]
    fn test_percentile_linear() {
        let v: Vec<f64> = (0..=100).map(|x| x as f64).collect();
        assert!((percentile(&v, 50.0) - 50.0).abs() < 1e-9);
        assert!((percentile(&v, 1.0) - 1.0).abs() < 1e-9);
        assert!((percentile(&v, 99.0) - 99.0).abs() < 1e-9);
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
    }

    #[test]
    fn test_derive_mood_ranges_and_ordering() {
        let pctl = PercentileSet {
            rms_db: flat_percentiles(),
            spectral_centroid: flat_percentiles(),
            spectral_flux: flat_percentiles(),
            crest: flat_percentiles(),
            loudness_range: flat_percentiles(),
        };
        let loud = MoodInputs { rms_db: 90.0, spectral_centroid: 90.0, spectral_flux: 90.0, crest: 10.0, loudness_range: 10.0, tempo_bpm: 128.0 };
        let calm = MoodInputs { rms_db: 10.0, spectral_centroid: 10.0, spectral_flux: 10.0, crest: 90.0, loudness_range: 90.0, tempo_bpm: 70.0 };
        let loud_scores = derive_mood(&loud, &pctl);
        let calm_scores = derive_mood(&calm, &pctl);
        for s in [loud_scores.energy, loud_scores.danceability, loud_scores.brightness,
                  calm_scores.energy, calm_scores.danceability, calm_scores.brightness] {
            assert!((0.0..=1.0).contains(&s), "mood out of range: {s}");
        }
        assert!(loud_scores.energy > calm_scores.energy);
        assert!(loud_scores.brightness > calm_scores.brightness);
        assert!(loud_scores.danceability > calm_scores.danceability);
    }
}
