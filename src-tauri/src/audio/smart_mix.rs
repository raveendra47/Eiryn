use crate::library::track::Track;

fn key_to_camelot(key: &str) -> Option<u8> {
    match key.trim().to_lowercase().as_str() {
        "c" => Some(8 + 12),
        "c#" | "db" => Some(1 + 12),
        "d" => Some(6 + 12),
        "d#" | "eb" => Some(11 + 12),
        "e" => Some(4 + 12),
        "f" => Some(9 + 12),
        "f#" | "gb" => Some(2 + 12),
        "g" => Some(7 + 12),
        "g#" | "ab" => Some(12 + 12),
        "a" => Some(5 + 12),
        "a#" | "bb" => Some(10 + 12),
        "b" => Some(3 + 12),
        "am" => Some(8),
        "a#m" | "bbm" => Some(1),
        "bm" => Some(6),
        "cm" => Some(11),
        "c#m" | "dbm" => Some(4),
        "dm" => Some(9),
        "d#m" | "ebm" => Some(2),
        "em" => Some(7),
        "fm" => Some(12),
        "f#m" | "gbm" => Some(5),
        "gm" => Some(10),
        "g#m" | "abm" => Some(3),
        _ => None,
    }
}

fn camelot_compatibility(a: u8, b: u8) -> f64 {
    if a == 0 || b == 0 {
        return 0.5; // unknown key — neutral score
    }

    if a == b {
        return 1.0;
    }

    let a_major = a > 12;
    let b_major = b > 12;
    let a_code = if a_major { a - 12 } else { a };
    let b_code = if b_major { b - 12 } else { b };

    let diff = if a_code > b_code {
        a_code - b_code
    } else {
        b_code - a_code
    };
    let diff = diff.min(12 - diff);

    if diff == 1 && a_major == b_major {
        return 0.8; // adjacent same wheel
    }
    if a_code == b_code && a_major != b_major {
        return 0.6; // same number, different letter (relative key)
    }
    if diff == 1 && a_major != b_major {
        return 0.4; // adjacent cross-letter
    }
    if diff <= 2 {
        return 0.2; // close but not ideal
    }

    0.0
}

fn bpm_proximity(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return 0.5;
    }
    let ratio = if a > b { a / b } else { b / a };
    if ratio <= 1.05 {
        1.0
    } else if ratio <= 1.10 {
        0.7
    } else if ratio <= 1.20 {
        0.4
    } else if ratio <= 1.33 {
        0.2
    } else {
        0.0
    }
}

fn energy_proximity(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return 0.5;
    }
    let diff = (a - b).abs();
    if diff <= 0.05 {
        1.0
    } else if diff <= 0.10 {
        0.8
    } else if diff <= 0.20 {
        0.5
    } else if diff <= 0.30 {
        0.3
    } else {
        0.1
    }
}

fn genre_compatibility(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.5;
    }
    if a.to_lowercase() == b.to_lowercase() {
        return 1.0;
    }
    0.3
}

fn pair_score(
    a: &Track,
    b: &Track,
    moods: &std::collections::HashMap<String, (f64, f64)>,
) -> f64 {
    let a_key = key_to_camelot(&a.musical_key);
    let b_key = key_to_camelot(&b.musical_key);
    let key_score = match (a_key, b_key) {
        (Some(ak), Some(bk)) => camelot_compatibility(ak, bk),
        _ => 0.5,
    };

    let bpm_score = bpm_proximity(a.bpm, b.bpm);
    let genre_score = genre_compatibility(&a.genre, &b.genre);

    // Mood-normalized energy/danceability when both tracks are analyzed;
    // otherwise the legacy raw-energy term and a neutral dance term.
    let (energy_score, dance_score) = match (moods.get(&a.id), moods.get(&b.id)) {
        (Some((ae, ad)), Some((be, bd))) => {
            (energy_proximity(*ae, *be), energy_proximity(*ad, *bd))
        }
        _ => (energy_proximity(a.energy, b.energy), 0.5),
    };

    key_score * 0.30
        + bpm_score * 0.25
        + energy_score * 0.20
        + dance_score * 0.10
        + genre_score * 0.15
}

pub fn generate_smart_mix(
    seed: &Track,
    candidates: &[Track],
    moods: &std::collections::HashMap<String, (f64, f64)>,
    count: usize,
) -> Vec<Track> {
    if candidates.is_empty() || count == 0 {
        return Vec::new();
    }

    let mut used_ids = std::collections::HashSet::new();
    used_ids.insert(seed.id.clone());

    let mut mix = Vec::with_capacity(count + 1);
    mix.push(seed.clone());

    let mut current = seed;
    for _ in 0..count {
        let scored: Vec<(f64, usize)> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| !used_ids.contains(&c.id))
            .map(|(i, c)| (pair_score(current, c, moods), i))
            .collect();

        if scored.is_empty() {
            break;
        }

        let best = scored
            .into_iter()
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        match best {
            Some((_, idx)) => {
                let next = &candidates[idx];
                used_ids.insert(next.id.clone());
                mix.push(next.clone());
                current = &mix[mix.len() - 1];
            }
            None => break,
        }
    }

    mix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camelot_c_major() {
        assert_eq!(key_to_camelot("C"), Some(20));
        assert_eq!(key_to_camelot("Am"), Some(8));
    }

    #[test]
    fn test_camelot_g_major() {
        assert_eq!(key_to_camelot("G"), Some(19));
        assert_eq!(key_to_camelot("Em"), Some(7));
    }

    #[test]
    fn test_camelot_f_major() {
        assert_eq!(key_to_camelot("F"), Some(21));
        assert_eq!(key_to_camelot("Dm"), Some(9));
    }

    #[test]
    fn test_camelot_flat_keys() {
        assert_eq!(key_to_camelot("Bb"), Some(22));
        assert_eq!(key_to_camelot("Eb"), Some(23));
        assert_eq!(key_to_camelot("Db"), Some(13));
        assert_eq!(key_to_camelot("G#m"), Some(3));
    }

    #[test]
    fn test_compatibility_identical() {
        assert_eq!(camelot_compatibility(20, 20), 1.0);
    }

    #[test]
    fn test_compatibility_adjacent() {
        let score = camelot_compatibility(8, 9);
        assert!(score >= 0.75 && score <= 0.85);
    }

    #[test]
    fn test_compatibility_relative() {
        let score = camelot_compatibility(8, 8 + 12);
        assert!(score >= 0.55 && score <= 0.65);
    }

    #[test]
    fn test_bpm_identical() {
        assert_eq!(bpm_proximity(120.0, 120.0), 1.0);
    }

    #[test]
    fn test_bpm_near() {
        assert_eq!(bpm_proximity(120.0, 125.0), 1.0); // 125/120 = 1.041 < 1.05
    }

    #[test]
    fn test_bpm_far() {
        assert_eq!(bpm_proximity(120.0, 80.0), 0.0); // 120/80 = 1.5
    }

    fn mix_track(id: &str, bpm: f64, energy: f64) -> Track {
        Track {
            id: id.to_string(),
            file_path: format!("/m/{id}.mp3"),
            title: id.to_string(),
            artist: "A".to_string(),
            album: String::new(),
            album_artist: String::new(),
            album_id: String::new(),
            composer: String::new(),
            track_number: 0,
            disc_number: 0,
            year: 0,
            genre: "Rock".to_string(),
            raw_genre_names: String::new(),
            duration: 200.0,
            sample_rate: 44100,
            bit_depth: 16,
            channels: 2,
            bitrate: 0,
            file_size: 0,
            file_format: "mp3".to_string(),
            has_artwork: false,
            replaygain_track_gain: 0.0,
            replaygain_album_gain: 0.0,
            cue_parent_id: None,
            cue_offset: 0.0,
            artwork_key: None,
            bpm,
            musical_key: "Am".to_string(),
            energy,
            analysis_failed_reason: None,
            file_modified: 0,
            matched_lyric: None,
        }
    }

    #[test]
    fn test_mood_energy_preferred_over_raw() {
        // Seed + two candidates identical except mood: same raw energy, but
        // candidate "near" shares the seed's mood energy.
        let seed = mix_track("seed", 120.0, 0.5);
        let near = mix_track("near", 120.0, 0.5);
        let far = mix_track("far", 120.0, 0.5);
        let candidates = vec![far.clone(), near.clone()];
        let mut moods = std::collections::HashMap::new();
        moods.insert("seed".to_string(), (0.9, 0.9));
        moods.insert("near".to_string(), (0.88, 0.9));
        moods.insert("far".to_string(), (0.1, 0.1));
        let mix = generate_smart_mix(&seed, &candidates, &moods, 1);
        assert_eq!(mix.len(), 2);
        assert_eq!(mix[1].id, "near");
    }

    #[test]
    fn test_mix_without_moods_falls_back() {
        let seed = mix_track("seed", 120.0, 0.5);
        let other = mix_track("other", 120.0, 0.52);
        let mix = generate_smart_mix(&seed, &[other], &std::collections::HashMap::new(), 1);
        assert_eq!(mix.len(), 2);
        assert_eq!(mix[1].id, "other");
    }
}
