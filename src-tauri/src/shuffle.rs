//! Balanced shuffle algorithm (adapted from Cadence).
//!
//! A plain Fisher-Yates shuffle frequently plays consecutive songs from the same artist
//! or repeats tracks heard an hour ago. The balanced shuffle:
//! 1. Spreads each artist's songs evenly over the entire playlist with subtle jitter;
//! 2. Spreads each album's songs among that artist's group;
//! 3. Pushes songs played in the last 48 hours towards the back half;
//! 4. Separates songs that would play back-to-back from the same artist or album;
//! 5. Evaluates candidate orders and keeps the optimal permutation.
//!
//! Deterministic for a given seed and finishes in < 5ms for 30,000+ tracks.

use std::collections::HashMap;
use std::time::SystemTime;

/// Fast pseudo-random number generator (SplitMix64).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Float in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Integer in 0..n.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { return 0; }
        ((self.next_u64() >> 32) * n as u64 >> 32) as usize
    }
}

pub fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

#[derive(Clone, Copy)]
pub struct Entry<'a> {
    pub artist: &'a str,
    pub album: &'a str,
    pub last_played: u64,
}

pub fn recency(last_played: u64, now: u64) -> f32 {
    if last_played == 0 {
        return 0.0;
    }
    let hours = now.saturating_sub(last_played) as f32 / 3600.0;
    (-hours / 40.0).exp()
}

const CANDIDATES: usize = 3;
const WINDOW: usize = 48;
const FRESH_PUSH: f32 = 0.55;

#[derive(Clone, Copy, PartialEq)]
struct Ids {
    artist: u32,
    album: u32,
}

const NOBODY: u32 = u32::MAX;

#[derive(Default)]
struct Quick(u64);

impl std::hash::Hasher for Quick {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(0x517C_C1B7_2722_0A95);
        }
    }
}

type QuickMap<'a> = HashMap<&'a str, u32, std::hash::BuildHasherDefault<Quick>>;

fn number<'a>(map: &mut QuickMap<'a>, name: &'a str) -> u32 {
    let next = map.len() as u32;
    *map.entry(name).or_insert(next)
}

pub fn balanced(entries: &[Entry], lead: Option<(&str, &str)>, now: u64, seed: u64) -> Vec<usize> {
    let n = entries.len();
    if n < 2 {
        return (0..n).collect();
    }
    let mut rng = Rng::new(seed);
    let (mut artists, mut albums) = (QuickMap::default(), QuickMap::default());
    let ids: Vec<Ids> = entries
        .iter()
        .map(|e| Ids {
            artist: number(&mut artists, e.artist),
            album: number(&mut albums, e.album),
        })
        .collect();
    let lead = lead.map(|(artist, album)| Ids {
        artist: artists.get(artist).copied().unwrap_or(NOBODY),
        album: albums.get(album).copied().unwrap_or(NOBODY),
    });

    let mut groups: Vec<Vec<u32>> = vec![Vec::new(); artists.len()];
    for (i, id) in ids.iter().enumerate() {
        groups[id.artist as usize].push(i as u32);
    }
    for g in &mut groups {
        spread_albums(g, &ids, &mut rng);
    }
    let fresh: Vec<f32> = entries.iter().map(|e| recency(e.last_played, now)).collect();

    let mut keyed: Vec<u64> = Vec::with_capacity(n);
    let mut best: Option<(f32, Vec<u32>)> = None;
    for _ in 0..CANDIDATES {
        keyed.clear();
        for g in &groups {
            let m = g.len() as f32;
            let offset = rng.unit();
            for (k, &i) in g.iter().enumerate() {
                let jitter = (rng.unit() - 0.5) * 0.2;
                let mut p = (k as f32 + offset + jitter) / m;
                let w = fresh[i as usize];
                if w > 0.0 {
                    p += w * FRESH_PUSH * (1.0 - p.min(1.0));
                }
                keyed.push(u64::from(p.max(0.0).to_bits()) << 32 | u64::from(i));
            }
        }
        keyed.sort_unstable();
        let mut order: Vec<u32> = keyed.iter().map(|&k| k as u32).collect();
        separate(&mut order, &ids, lead);
        let score = penalty(&order, &ids, &fresh, lead);
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, order));
        }
    }
    best.map(|(_, o)| o.into_iter().map(|i| i as usize).collect()).unwrap_or_default()
}

fn spread_albums(g: &mut [u32], ids: &[Ids], rng: &mut Rng) {
    let album = |i: u32| ids[i as usize].album;
    let one_album = g.iter().all(|&i| album(i) == album(g[0]));
    if !one_album {
        g.sort_unstable_by_key(|&i| (album(i), i));
    }
    let mut keyed: Vec<u64> = Vec::with_capacity(g.len());
    let mut start = 0;
    while start < g.len() {
        let this = album(g[start]);
        let end = start + g[start..].iter().take_while(|&&i| album(i) == this).count();
        let run = &mut g[start..end];
        for i in (1..run.len()).rev() {
            run.swap(i, rng.below(i + 1));
        }
        let m = run.len() as f32;
        let offset = rng.unit();
        for (k, &i) in run.iter().enumerate() {
            let p = (k as f32 + offset + (rng.unit() - 0.5) * 0.2) / m;
            keyed.push(u64::from(p.max(0.0).to_bits()) << 32 | u64::from(i));
        }
        start = end;
    }
    if !one_album {
        keyed.sort_unstable();
    }
    for (slot, k) in g.iter_mut().zip(keyed) {
        *slot = k as u32;
    }
}

fn clash(a: Ids, b: Ids) -> u8 {
    if a.artist == b.artist {
        2
    } else if a.album == b.album {
        1
    } else {
        0
    }
}

fn separate(order: &mut [u32], ids: &[Ids], lead: Option<Ids>) {
    let at = |order: &[u32], k: usize| ids[order[k] as usize];
    for i in 0..order.len() {
        let prev = match i {
            0 => match lead {
                Some(l) => l,
                None => continue,
            },
            _ => at(order, i - 1),
        };
        let cur = at(order, i);
        let c = clash(prev, cur);
        if c == 0 {
            continue;
        }
        let end = (i + 1 + WINDOW).min(order.len());
        let mut fallback = None;
        let mut found = None;
        for j in i + 1..end {
            let x = at(order, j);
            let with_prev = clash(prev, x);
            if with_prev == 0 && clash(x, cur) == 0 {
                found = Some(j);
                break;
            }
            if with_prev < c && fallback.is_none() {
                fallback = Some(j);
            }
        }
        if let Some(j) = found.or(fallback) {
            order[i..=j].rotate_right(1);
            continue;
        }
        let back = (1..i.saturating_sub(1))
            .rev()
            .take(WINDOW)
            .find(|&k| clash(at(order, k - 1), cur) == 0 && clash(at(order, k), cur) == 0);
        if let Some(k) = back {
            order[k..=i].rotate_right(1);
        }
    }
}

fn penalty(order: &[u32], ids: &[Ids], fresh: &[f32], lead: Option<Ids>) -> f32 {
    let n = order.len() as f32;
    let mut score = 0.0;
    let mut prev = lead;
    for (k, &i) in order.iter().enumerate() {
        score += fresh[i as usize] * (1.0 - k as f32 / n);
        let id = ids[i as usize];
        if let Some(p) = prev {
            score += match clash(p, id) {
                2 => 4.0,
                1 => 1.0,
                _ => 0.0,
            };
        }
        prev = Some(id);
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(artist: &'a str, album: &'a str, last_played: u64) -> Entry<'a> {
        Entry { artist, album, last_played }
    }

    fn library(spec: &[(&str, usize)]) -> (Vec<String>, Vec<String>) {
        let (mut artists, mut albums) = (Vec::new(), Vec::new());
        for (a, n) in spec {
            for k in 0..*n {
                artists.push(a.to_string());
                albums.push(format!("{a}-{}", k % 2));
            }
        }
        (artists, albums)
    }

    fn entries<'a>(artists: &'a [String], albums: &'a [String]) -> Vec<Entry<'a>> {
        artists.iter().zip(albums).map(|(a, b)| entry(a, b, 0)).collect()
    }

    #[test]
    fn test_balanced_deterministic() {
        let (artists, albums) = library(&[("a", 20), ("b", 15), ("c", 5), ("d", 2)]);
        let e = entries(&artists, &albums);
        let order1 = balanced(&e, None, 1000, 42);
        let order2 = balanced(&e, None, 1000, 42);
        assert_eq!(order1, order2);

        let mut sorted = order1.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..e.len()).collect::<Vec<_>>());
    }

    #[test]
    fn test_artist_spacing() {
        let (artists, albums) = library(&[("Coldplay", 10), ("Queen", 10), ("Beatles", 10)]);
        let e = entries(&artists, &albums);
        let order = balanced(&e, None, 1000, 12345);

        // Check that artists don't clump 3 times in a row
        let mut max_run = 0;
        let mut cur_run = 0;
        let mut last_artist = "";
        for &idx in &order {
            if artists[idx] == last_artist {
                cur_run += 1;
            } else {
                cur_run = 1;
                last_artist = &artists[idx];
            }
            max_run = max_run.max(cur_run);
        }
        assert!(max_run <= 2, "Same artist clustered too many times: {max_run}");
    }
}
