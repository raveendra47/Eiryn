use crate::library::track::Track;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_smart: bool,
    pub smart_rules: Option<String>,
    pub last_updated: Option<String>,
    pub created_at: String,
    pub is_folder_playlist: bool,
    pub folder_path: Option<String>,
}

pub async fn create_playlist(pool: &SqlitePool, name: &str, is_smart: bool, smart_rules: Option<&str>) -> Result<Playlist, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO playlists (id, name, is_smart, smart_rules) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(is_smart)
        .bind(smart_rules)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to create playlist: {e}"))?;
    Ok(Playlist {
        id,
        name: name.to_string(),
        description: String::new(),
        is_smart,
        smart_rules: smart_rules.map(|s| s.to_string()),
        last_updated: if is_smart { Some(String::new()) } else { None },
        created_at: String::new(),
        is_folder_playlist: false,
        folder_path: None,
    })
}

pub async fn create_folder_playlist(pool: &SqlitePool, name: &str, folder_path: &str) -> Result<Playlist, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let normalized = folder_path.trim_end_matches('\\').to_string() + "\\";
    sqlx::query("INSERT INTO playlists (id, name, is_smart, is_folder_playlist, folder_path) VALUES (?, ?, 1, 1, ?)")
        .bind(&id)
        .bind(name)
        .bind(&normalized)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to create folder playlist: {e}"))?;
    update_folder_playlist_tracks(pool, &id, &normalized).await?;
    Ok(Playlist {
        id,
        name: name.to_string(),
        description: String::new(),
        is_smart: true,
        smart_rules: None,
        last_updated: Some(String::new()),
        created_at: String::new(),
        is_folder_playlist: true,
        folder_path: Some(normalized),
    })
}

pub async fn update_folder_playlist_tracks(pool: &SqlitePool, playlist_id: &str, folder_path: &str) -> Result<(), String> {
    let normalized = folder_path.trim_end_matches('\\').to_string() + "\\";
    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?")
        .bind(playlist_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear playlist tracks: {e}"))?;
    sqlx::query(
        "INSERT INTO playlist_tracks (playlist_id, track_id, position)
         SELECT ?, id, ROW_NUMBER() OVER (ORDER BY file_path)
         FROM tracks
         WHERE file_path LIKE ? || '%'
         ORDER BY file_path"
    )
    .bind(playlist_id)
    .bind(&normalized)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to populate folder playlist: {e}"))?;
    sqlx::query("UPDATE playlists SET last_updated = datetime('now') WHERE id = ?")
        .bind(playlist_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update last_updated: {e}"))?;
    Ok(())
}

pub async fn refresh_all_folder_playlists(pool: &SqlitePool) -> Result<(), String> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, folder_path FROM playlists WHERE is_folder_playlist = 1"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch folder playlists: {e}"))?;
    for (id, path) in &rows {
        update_folder_playlist_tracks(pool, id, path).await?;
    }
    Ok(())
}

pub async fn rename_playlist(pool: &SqlitePool, id: &str, name: &str) -> Result<(), String> {
    sqlx::query("UPDATE playlists SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to rename playlist: {e}"))?;
    Ok(())
}

pub async fn delete_playlist(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM playlists WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete playlist: {e}"))?;
    Ok(())
}

pub async fn get_playlists(pool: &SqlitePool) -> Result<Vec<Playlist>, String> {
    let rows = sqlx::query_as::<_, PlaylistRow>(
        "SELECT id, name, description, is_smart, smart_rules, last_updated, created_at, is_folder_playlist, folder_path FROM playlists ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to get playlists: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_playlist(pool: &SqlitePool, id: &str) -> Result<Option<Playlist>, String> {
    let row = sqlx::query_as::<_, PlaylistRow>(
        "SELECT id, name, description, is_smart, smart_rules, last_updated, created_at, is_folder_playlist, folder_path FROM playlists WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get playlist: {e}"))?;
    Ok(row.map(|r| r.into()))
}

pub async fn update_playlist_rules(pool: &SqlitePool, id: &str, smart_rules: Option<&str>) -> Result<(), String> {
    sqlx::query("UPDATE playlists SET smart_rules = ?, is_smart = 1 WHERE id = ?")
        .bind(smart_rules)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update playlist rules: {e}"))?;
    Ok(())
}

pub async fn add_track_to_playlist(pool: &SqlitePool, playlist_id: &str, track_id: &str) -> Result<(), String> {
    let max_pos: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(position) FROM playlist_tracks WHERE playlist_id = ?"
    )
    .bind(playlist_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get max position: {e}"))?;
    let pos = max_pos.unwrap_or(0) + 1;

    let max_rank: Option<String> = sqlx::query_scalar(
        "SELECT MAX(rank) FROM playlist_tracks WHERE playlist_id = ? AND rank != ''"
    )
    .bind(playlist_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get max rank: {e}"))?;

    let rank = match max_rank {
        Some(ref r) if !r.is_empty() => lexorank::after(r),
        _ => lexorank::initial(),
    };

    sqlx::query("INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position, rank) VALUES (?, ?, ?, ?)")
        .bind(playlist_id)
        .bind(track_id)
        .bind(pos)
        .bind(&rank)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to add track to playlist: {e}"))?;
    Ok(())
}

pub async fn move_playlist_track(
    pool: &SqlitePool,
    playlist_id: &str,
    track_id: &str,
    prev_track_id: Option<&str>,
    next_track_id: Option<&str>,
) -> Result<String, String> {
    let prev_rank: Option<String> = match prev_track_id {
        Some(id) => sqlx::query_scalar("SELECT rank FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?")
            .bind(playlist_id)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?,
        None => None,
    };

    let next_rank: Option<String> = match next_track_id {
        Some(id) => sqlx::query_scalar("SELECT rank FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?")
            .bind(playlist_id)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?,
        None => None,
    };

    let new_rank = lexorank::between(prev_rank.as_deref(), next_rank.as_deref());

    sqlx::query("UPDATE playlist_tracks SET rank = ? WHERE playlist_id = ? AND track_id = ?")
        .bind(&new_rank)
        .bind(playlist_id)
        .bind(track_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update track rank: {e}"))?;

    // Auto-rebalance if rank gets deep (> 16 chars) to keep indices short
    if new_rank.len() > 16 {
        let _ = rebalance_playlist(pool, playlist_id).await;
    }

    Ok(new_rank)
}

pub async fn rebalance_playlist(pool: &SqlitePool, playlist_id: &str) -> Result<(), String> {
    let track_ids: Vec<String> = sqlx::query_scalar(
        "SELECT track_id FROM playlist_tracks
         WHERE playlist_id = ?
         ORDER BY CASE WHEN rank != '' THEN rank ELSE printf('%09d', position) END ASC"
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut cur = lexorank::initial();
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for (i, tid) in track_ids.iter().enumerate() {
        sqlx::query("UPDATE playlist_tracks SET rank = ?, position = ? WHERE playlist_id = ? AND track_id = ?")
            .bind(&cur)
            .bind(i as i32)
            .bind(playlist_id)
            .bind(tid)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        cur = lexorank::after(&cur);
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn remove_track_from_playlist(pool: &SqlitePool, playlist_id: &str, track_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?")
        .bind(playlist_id)
        .bind(track_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to remove track: {e}"))?;
    Ok(())
}

pub async fn get_playlists_for_track(pool: &SqlitePool, track_id: &str) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT playlist_id FROM playlist_tracks WHERE track_id = ?"
    )
    .bind(track_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to get playlists for track: {e}"))?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_playlist_tracks(pool: &SqlitePool, playlist_id: &str) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, crate::library::db::TrackRow>(
         "SELECT t.id, t.file_path, t.title, t.artist, t.album, t.album_artist, t.composer,
          t.track_number, t.disc_number, t.year, t.genre, t.raw_genre_names, t.duration,
           t.sample_rate, t.bit_depth, t.channels, t.bitrate, t.file_size, t.file_format, t.has_artwork,
          t.replaygain_track_gain, t.replaygain_album_gain, t.cue_parent_id, t.cue_offset, t.artwork_key,
          t.bpm, t.musical_key, t.energy, t.analysis_failed_reason, t.file_modified
          FROM tracks t
          JOIN playlist_tracks pt ON pt.track_id = t.id
          WHERE pt.playlist_id = ?
          ORDER BY CASE WHEN pt.rank != '' THEN pt.rank ELSE printf('%09d', pt.position) END ASC"
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to get playlist tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn import_playlist_m3u(pool: &SqlitePool, file_path: &str, name: &str) -> Result<Playlist, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read M3U file: {e}"))?;

    let playlist = create_playlist(pool, if name.is_empty() { "Imported Playlist" } else { name }, false, None).await?;

    let existing: std::collections::HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT id, file_path FROM tracks"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("{e}"))?
    .into_iter()
    .map(|(id, path)| (path, id))
    .collect();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let normalized = line.replace('/', "\\");
        if let Some(track_id) = existing.get(line).or_else(|| existing.get(&normalized)) {
            let _ = add_track_to_playlist(pool, &playlist.id, track_id).await;
        }
    }

    Ok(playlist)
}

pub async fn export_playlist_m3u(pool: &SqlitePool, id: &str, output_path: &str) -> Result<(), String> {
    let tracks = get_playlist_tracks(pool, id).await?;
    let playlist_name = sqlx::query_scalar::<_, String>("SELECT name FROM playlists WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to get playlist name: {e}"))?
        .unwrap_or_default();

    let mut content = format!("#EXTM3U\n#PLAYLIST:{playlist_name}\n");
    for t in &tracks {
        content.push_str(&format!("#EXTINF:{},{}\n", t.duration.round() as u64, t.title));
        content.push_str(&format!("{}\n", t.file_path));
    }

    fs::write(output_path, content).map_err(|e| format!("Failed to write M3U: {e}"))?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct PlaylistRow {
    id: String,
    name: String,
    description: String,
    is_smart: bool,
    smart_rules: Option<String>,
    last_updated: Option<String>,
    created_at: String,
    is_folder_playlist: bool,
    folder_path: Option<String>,
}

impl From<PlaylistRow> for Playlist {
    fn from(r: PlaylistRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            is_smart: r.is_smart,
            smart_rules: r.smart_rules,
            last_updated: r.last_updated,
            created_at: r.created_at,
            is_folder_playlist: r.is_folder_playlist,
            folder_path: r.folder_path,
        }
    }
}

pub mod lexorank {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    const MID: u8 = b'm';

    pub fn char_val(c: u8) -> Option<u32> {
        match c {
            b'0'..=b'9' => Some((c - b'0') as u32),
            b'a'..=b'z' => Some((c - b'a' + 10) as u32),
            _ => None,
        }
    }

    pub fn val_char(v: u32) -> u8 {
        DIGITS[(v as usize) % 36]
    }

    pub fn initial() -> String {
        "0|i00000".to_string()
    }

    pub fn between(prev: Option<&str>, next: Option<&str>) -> String {
        match (prev, next) {
            (None, None) => initial(),
            (Some(p), None) => after(p),
            (None, Some(n)) => before(n),
            (Some(p), Some(n)) => {
                if p >= n {
                    return after(p);
                }
                let p_bytes = p.as_bytes();
                let n_bytes = n.as_bytes();
                let max_len = p_bytes.len().max(n_bytes.len());
                let mut res = Vec::new();

                for i in 0..=max_len {
                    let p_byte = p_bytes.get(i).copied();
                    let n_byte = n_bytes.get(i).copied();

                    if p_byte.is_some() && p_byte == n_byte {
                        res.push(p_byte.unwrap());
                        continue;
                    }

                    let pv = p_byte.and_then(char_val).unwrap_or(0);
                    let nv = n_byte.and_then(char_val).unwrap_or(35);

                    if nv > pv + 1 {
                        let mid = (pv + nv) / 2;
                        res.push(val_char(mid));
                        let candidate = String::from_utf8(res).unwrap_or_default();
                        if candidate.as_str() > p && candidate.as_str() < n {
                            return candidate;
                        }
                        break;
                    } else {
                        if let Some(ch) = p_byte {
                            res.push(ch);
                        } else {
                            res.push(b'0');
                        }
                        let mut cand = res.clone();
                        cand.push(MID);
                        let cand_str = String::from_utf8(cand).unwrap_or_default();
                        if cand_str.as_str() > p && cand_str.as_str() < n {
                            return cand_str;
                        }
                    }
                }
                format!("{}m", p)
            }
        }
    }

    pub fn after(prev: &str) -> String {
        let mut bytes = prev.as_bytes().to_vec();
        for i in (0..bytes.len()).rev() {
            if let Some(v) = char_val(bytes[i]) {
                if v < 35 {
                    bytes[i] = val_char(v + 1);
                    return String::from_utf8(bytes).unwrap_or_else(|_| format!("{}m", prev));
                }
            }
        }
        format!("{}m", prev)
    }

    pub fn before(next: &str) -> String {
        let mut bytes = next.as_bytes().to_vec();
        for i in (0..bytes.len()).rev() {
            if let Some(v) = char_val(bytes[i]) {
                if v > 1 {
                    bytes[i] = val_char(v / 2);
                    return String::from_utf8(bytes).unwrap_or_else(|_| format!("0{}", next));
                }
            }
        }
        if next.starts_with("0|") {
            format!("0|0{}", &next[2..])
        } else {
            format!("0{}", next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lexorank::*;

    #[test]
    fn test_lexorank_ordering() {
        let first = initial();
        let second = after(&first);
        assert!(first < second);

        let middle = between(Some(&first), Some(&second));
        assert!(first < middle);
        assert!(middle < second);

        let before_first = before(&first);
        assert!(before_first < first);

        let mid2 = between(Some(&first), Some(&middle));
        assert!(first < mid2);
        assert!(mid2 < middle);
    }
}
