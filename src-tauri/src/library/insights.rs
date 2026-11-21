//! Listening + library insights (Airmedy parity).
//!
//! Read models over `play_history` (outcomes via `end_reason`) and `tracks`.
//! Windows: `7d` (today−6, local), `30d` (today−29), `all`. Daily series are
//! zero-filled; `all` groups by month. No new write paths — tracking rules
//! live in `db::finish_play`.

use crate::library::db;
use crate::library::track::Track;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const TOP_LIMIT: i64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPoint {
    pub label: String,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreSlice {
    pub name: String,
    pub seconds: f64,
    pub is_other: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopArtist {
    pub name: String,
    pub seconds: f64,
    pub plays: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopTrack {
    pub track: Track,
    pub plays: i64,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListeningInsights {
    pub plays: i64,
    pub seconds: f64,
    pub attempts: i64,
    pub completed: i64,
    pub skipped: i64,
    pub stopped: i64,
    pub completion_rate: f64,
    pub skip_rate: f64,
    pub avg_session_secs: f64,
    pub streak_days: i64,
    pub change_percent: Option<f64>,
    pub activity: Vec<ActivityPoint>,
    pub genres: Vec<GenreSlice>,
    pub top_artists: Vec<TopArtist>,
    pub top_tracks: Vec<TopTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityBucket {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInsights {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
    pub playlists: i64,
    pub total_bytes: i64,
    pub growth: Vec<ActivityPoint>,
    pub quality: Vec<QualityBucket>,
}

fn is_bounded(period: &str) -> bool {
    period == "7d" || period == "30d"
}

fn days(period: &str) -> i64 {
    match period {
        "30d" => 30,
        _ => 7,
    }
}

/// SQL predicate on the local play date for a bounded window.
fn window_predicate(period: &str) -> String {
    if is_bounded(period) {
        format!(
            "date(played_at, 'localtime') >= date('now', 'localtime', '-{} days')",
            days(period) - 1
        )
    } else {
        "1 = 1".to_string()
    }
}

fn prior_window_predicate(period: &str) -> String {
    let d = days(period);
    format!(
        "date(played_at, 'localtime') >= date('now', 'localtime', '-{} days') \
         AND date(played_at, 'localtime') < date('now', 'localtime', '-{} days')",
        2 * d - 1,
        d
    )
}

async fn window_seconds(pool: &SqlitePool, predicate: &str) -> Result<f64, String> {
    let row: (Option<f64>,) = sqlx::query_as(&format!(
        "SELECT SUM(played_secs) FROM play_history WHERE {predicate}"
    ))
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to sum listening window: {e}"))?;
    Ok(row.0.unwrap_or(0.0))
}

pub async fn get_listening_insights(
    pool: &SqlitePool,
    period: &str,
) -> Result<ListeningInsights, String> {
    let period = if is_bounded(period) { period } else { "all" };
    let pred = window_predicate(period);

    let (plays, seconds): (i64, Option<f64>) = sqlx::query_as(&format!(
        "SELECT COUNT(*), SUM(played_secs) FROM play_history WHERE {pred}"
    ))
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch insight totals: {e}"))?;
    let seconds = seconds.unwrap_or(0.0);

    let outcome_rows: Vec<(Option<String>, i64, Option<f64>)> = sqlx::query_as(&format!(
        "SELECT end_reason, COUNT(*), SUM(played_secs) FROM play_history WHERE {pred} GROUP BY end_reason"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch outcomes: {e}"))?;
    let (mut completed, mut skipped, mut stopped) = (0i64, 0i64, 0i64);
    for (reason, n, _) in &outcome_rows {
        match reason.as_deref() {
            Some("completed") => completed = *n,
            Some("skipped") => skipped = *n,
            Some("stopped") => stopped = *n,
            _ => stopped += *n, // legacy rows without a reason count as stopped
        }
    }
    let denom = (completed + skipped + stopped).max(1) as f64;
    let completion_rate = completed as f64 / denom * 100.0;
    let skip_rate = skipped as f64 / denom * 100.0;
    let avg_session_secs = if plays > 0 { seconds / plays as f64 } else { 0.0 };

    // Activity series.
    let activity = if period == "all" {
        let rows: Vec<(String, Option<f64>)> = sqlx::query_as(
            "SELECT strftime('%Y-%m', played_at, 'localtime') AS m, SUM(played_secs)
             FROM play_history GROUP BY m ORDER BY m",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch monthly activity: {e}"))?;
        rows.into_iter()
            .map(|(m, s)| ActivityPoint { label: m, seconds: s.unwrap_or(0.0) })
            .collect()
    } else {
        let rows: Vec<(String, Option<f64>)> = sqlx::query_as(&format!(
            "SELECT date(played_at, 'localtime') AS d, SUM(played_secs)
             FROM play_history WHERE {pred} GROUP BY d"
        ))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch daily activity: {e}"))?;
        let map: std::collections::HashMap<String, f64> = rows
            .into_iter()
            .map(|(d, s)| (d, s.unwrap_or(0.0)))
            .collect();
        let today = chrono::Local::now().date_naive();
        (0..days(period))
            .rev()
            .map(|ago| {
                let date = today - chrono::Days::new(ago as u64);
                let label = date.format("%Y-%m-%d").to_string();
                ActivityPoint {
                    seconds: map.get(&label).copied().unwrap_or(0.0),
                    label,
                }
            })
            .collect()
    };

    // Streak: consecutive >0-sec local days ending today, else yesterday.
    let streak_days = {
        let rows: Vec<(String, Option<f64>)> = sqlx::query_as(
            "SELECT date(played_at, 'localtime') AS d, SUM(played_secs)
             FROM play_history GROUP BY d ORDER BY d DESC LIMIT 400",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch streak days: {e}"))?;
        let map: std::collections::HashMap<String, f64> = rows
            .into_iter()
            .map(|(d, s)| (d, s.unwrap_or(0.0)))
            .collect();
        let today = chrono::Local::now().date_naive();
        let mut streak = 0i64;
        let mut date = today;
        // Allow the streak to start yesterday when today is still silent.
        if map.get(&date.format("%Y-%m-%d").to_string()).copied().unwrap_or(0.0) <= 0.0 {
            date = today - chrono::Days::new(1);
        }
        loop {
            let secs = map.get(&date.format("%Y-%m-%d").to_string()).copied().unwrap_or(0.0);
            if secs > 0.0 {
                streak += 1;
                date = date - chrono::Days::new(1);
            } else {
                break;
            }
        }
        streak
    };

    // Change vs prior equal window (bounded only; None when prior is empty).
    let change_percent = if is_bounded(period) {
        let prior = window_seconds(pool, &prior_window_predicate(period)).await?;
        if prior > 0.0 {
            Some((seconds - prior) / prior * 100.0)
        } else {
            None
        }
    } else {
        None
    };

    // Genres: top 5 + Other remainder by seconds.
    let genre_rows: Vec<(String, Option<f64>)> = sqlx::query_as(&format!(
        "SELECT CASE WHEN t.genre = '' THEN 'Unknown' ELSE t.genre END AS g, SUM(h.played_secs)
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE {pred} GROUP BY g ORDER BY SUM(h.played_secs) DESC"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch genre split: {e}"))?;
    let mut genres: Vec<GenreSlice> = genre_rows
        .iter()
        .take(5)
        .map(|(name, s)| GenreSlice { name: name.clone(), seconds: s.unwrap_or(0.0), is_other: false })
        .collect();
    if genre_rows.len() > 5 {
        let other: f64 = genre_rows[5..].iter().map(|(_, s)| s.unwrap_or(0.0)).sum();
        genres.push(GenreSlice { name: "Other".to_string(), seconds: other, is_other: true });
    }

    // Top artists by seconds.
    let artist_rows: Vec<(String, Option<f64>, i64)> = sqlx::query_as(&format!(
        "SELECT CASE WHEN t.artist = '' THEN 'Unknown Artist' ELSE t.artist END AS a,
                SUM(h.played_secs), COUNT(*)
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE {pred} GROUP BY a ORDER BY SUM(h.played_secs) DESC LIMIT {TOP_LIMIT}"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch top artists: {e}"))?;
    let top_artists = artist_rows
        .into_iter()
        .map(|(name, s, plays)| TopArtist { name, seconds: s.unwrap_or(0.0), plays })
        .collect();

    // Top tracks by plays, then seconds.
    let track_rows: Vec<(String, i64, Option<f64>)> = sqlx::query_as(&format!(
        "SELECT track_id, COUNT(*), SUM(played_secs) FROM play_history
         WHERE {pred} GROUP BY track_id ORDER BY COUNT(*) DESC, SUM(played_secs) DESC LIMIT {TOP_LIMIT}"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch top tracks: {e}"))?;
    let mut top_tracks = Vec::with_capacity(track_rows.len());
    for (id, plays, s) in track_rows {
        if let Some(track) = db::get_track(pool, &id).await? {
            top_tracks.push(TopTrack { track, plays, seconds: s.unwrap_or(0.0) });
        }
    }

    Ok(ListeningInsights {
        plays,
        seconds,
        attempts: plays,
        completed,
        skipped,
        stopped,
        completion_rate,
        skip_rate,
        avg_session_secs,
        streak_days,
        change_percent,
        activity,
        genres,
        top_artists,
        top_tracks,
    })
}

/// Classify an audio file into Airmedy's quality buckets from stored tags.
fn classify_quality(file_format: &str, sample_rate: i64, bit_depth: i64) -> &'static str {
    match file_format {
        "dsf" | "dff" => "dsd",
        "flac" | "wav" | "aiff" | "alac" | "ape" | "tta" | "wv" => {
            if sample_rate > 48000 || bit_depth > 16 {
                "hi_res"
            } else {
                "lossless"
            }
        }
        "" => "unknown",
        _ => "lossy",
    }
}

pub async fn get_library_insights(
    pool: &SqlitePool,
    period: &str,
) -> Result<LibraryInsights, String> {
    let period = if is_bounded(period) { period } else { "all" };

    let (tracks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tracks")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to count tracks: {e}"))?;
    let (albums,): (i64,) =
        sqlx::query_as("SELECT COUNT(DISTINCT album) FROM tracks WHERE album != ''")
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Failed to count albums: {e}"))?;
    let (artists,): (i64,) =
        sqlx::query_as("SELECT COUNT(DISTINCT artist) FROM tracks WHERE artist != ''")
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Failed to count artists: {e}"))?;
    let (playlists,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM playlists")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to count playlists: {e}"))?;
    let (total_bytes,): (Option<i64>,) = sqlx::query_as("SELECT SUM(file_size) FROM tracks")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to sum library size: {e}"))?;

    // Growth: cumulative adds. Bounded → daily from baseline; all → yearly.
    let growth = if period == "all" {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT strftime('%Y', created_at, 'localtime') AS y, COUNT(*)
             FROM tracks GROUP BY y ORDER BY y",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch yearly growth: {e}"))?;
        let mut cum = 0i64;
        rows.into_iter()
            .map(|(y, n)| {
                cum += n;
                ActivityPoint { label: y, seconds: cum as f64 }
            })
            .collect()
    } else {
        let d = days(period);
        let rows: Vec<(String, i64)> = sqlx::query_as(&format!(
            "SELECT date(created_at, 'localtime') AS day, COUNT(*) FROM tracks
             WHERE date(created_at, 'localtime') >= date('now', 'localtime', '-{} days')
             GROUP BY day",
            d - 1
        ))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch daily growth: {e}"))?;
        let added: i64 = rows.iter().map(|(_, n)| n).sum();
        let mut cum = tracks - added;
        let map: std::collections::HashMap<String, i64> =
            rows.into_iter().collect();
        let today = chrono::Local::now().date_naive();
        (0..d)
            .rev()
            .map(|ago| {
                let date = today - chrono::Days::new(ago as u64);
                let label = date.format("%Y-%m-%d").to_string();
                cum += map.get(&label).copied().unwrap_or(0);
                ActivityPoint { label, seconds: cum as f64 }
            })
            .collect()
    };

    // Quality distribution from stored tags.
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT file_format, sample_rate, bit_depth, COUNT(*) FROM tracks GROUP BY file_format, sample_rate, bit_depth",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch quality split: {e}"))?;
    let mut buckets: std::collections::HashMap<&'static str, i64> = std::collections::HashMap::new();
    for (fmt, sr, bd, n) in &rows {
        *buckets.entry(classify_quality(fmt, *sr, *bd)).or_insert(0) += *n;
    }
    let mut quality: Vec<QualityBucket> = buckets
        .into_iter()
        .map(|(kind, count)| QualityBucket { kind: kind.to_string(), count })
        .collect();
    quality.sort_by(|a, b| b.count.cmp(&a.count));

    Ok(LibraryInsights {
        tracks,
        albums,
        artists,
        playlists,
        total_bytes: total_bytes.unwrap_or(0),
        growth,
        quality,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_predicates() {
        assert!(is_bounded("7d") && is_bounded("30d") && !is_bounded("all") && !is_bounded("bogus"));
        assert_eq!(days("30d"), 30);
        assert_eq!(days("7d"), 7);
        assert!(window_predicate("7d").contains("-6 days"));
        assert!(window_predicate("all").contains("1 = 1"));
    }

    #[test]
    fn test_classify_quality() {
        assert_eq!(classify_quality("flac", 44100, 16), "lossless");
        assert_eq!(classify_quality("flac", 96000, 24), "hi_res");
        assert_eq!(classify_quality("mp3", 44100, 0), "lossy");
        assert_eq!(classify_quality("dsf", 2822400, 1), "dsd");
        assert_eq!(classify_quality("", 0, 0), "unknown");
    }

    async fn seed_db() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("eiryn-insights-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (pool, _) = db::init_pool(&dir.join("test.db")).await.unwrap();
        for (id, title, artist, album, genre, fmt) in [
            ("t1", "One", "Alice", "A", "Rock", "mp3"),
            ("t2", "Two", "Alice", "A", "Rock", "flac"),
            ("t3", "Three", "Bob", "B", "Jazz", "flac"),
        ] {
            sqlx::query(
                "INSERT INTO tracks (id, file_path, title, artist, album, genre, duration,
                 file_format, sample_rate, bit_depth, file_size)
                 VALUES (?, ?, ?, ?, ?, ?, 200.0, ?, 44100, 16, 1000)",
            )
            .bind(id)
            .bind(format!("/music/{id}.tmp"))
            .bind(title)
            .bind(artist)
            .bind(album)
            .bind(genre)
            .bind(fmt)
            .execute(&pool)
            .await
            .unwrap();
        }
        // Two plays today (one completed, one skipped), one 40 days ago.
        for (tid, secs, reason, modifier) in [
            ("t1", 180.0, "completed", ""),
            ("t2", 45.0, "skipped", ""),
            ("t3", 200.0, "completed", ", '-40 days'"),
        ] {
            sqlx::query(&format!(
                "INSERT INTO play_history (track_id, played_at, played_secs, end_reason)
                 VALUES (?, datetime('now'{modifier}), ?, ?)"
            ))
            .bind(tid)
            .bind(secs)
            .bind(reason)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn test_listening_insights_windows() {
        let pool = seed_db().await;
        let w = get_listening_insights(&pool, "7d").await.unwrap();
        assert_eq!(w.plays, 2);
        assert!((w.seconds - 225.0).abs() < 0.01);
        assert_eq!(w.completed, 1);
        assert_eq!(w.skipped, 1);
        assert!((w.completion_rate - 50.0).abs() < 0.01);
        assert_eq!(w.activity.len(), 7);
        assert!(w.streak_days >= 1);
        assert_eq!(w.top_artists.len(), 1);
        assert_eq!(w.top_artists[0].name, "Alice");
        assert_eq!(w.top_tracks.len(), 2);
        assert_eq!(w.genres.len(), 1);
        assert_eq!(w.genres[0].name, "Rock");

        let all = get_listening_insights(&pool, "all").await.unwrap();
        assert_eq!(all.plays, 3);
        assert!(all.change_percent.is_none());
        assert!(!all.activity.is_empty());
    }

    #[tokio::test]
    async fn test_library_insights() {
        let pool = seed_db().await;
        let lib = get_library_insights(&pool, "all").await.unwrap();
        assert_eq!(lib.tracks, 3);
        assert_eq!(lib.albums, 2);
        assert_eq!(lib.artists, 2);
        assert!(!lib.growth.is_empty());
        let kinds: Vec<&str> = lib.quality.iter().map(|q| q.kind.as_str()).collect();
        assert!(kinds.contains(&"lossy") && kinds.contains(&"lossless"));
    }

    #[tokio::test]
    async fn test_finish_play_rules() {
        let pool = seed_db().await;
        // Discard: <10s row removed.
        let id = db::record_play(&pool, "t1").await.unwrap();
        db::finish_play(&pool, id, 5.0, 200.0, Some("skipped")).await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM play_history WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // Unqualified (>=10s but <half, <240s): kept, counted, no play_count bump.
        let id2 = db::record_play(&pool, "t1").await.unwrap();
        db::finish_play(&pool, id2, 60.0, 200.0, Some("stopped")).await.unwrap();
        let (plays,): (i64,) = sqlx::query_as("SELECT play_count FROM tracks WHERE id = 't1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plays, 0);
        // Qualified: bumps play_count.
        let id3 = db::record_play(&pool, "t1").await.unwrap();
        db::finish_play(&pool, id3, 150.0, 200.0, Some("completed")).await.unwrap();
        let (plays,): (i64,) = sqlx::query_as("SELECT play_count FROM tracks WHERE id = 't1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plays, 1);
    }
}
