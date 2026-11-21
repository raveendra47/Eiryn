use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRuleGroup {
    pub operator: String,
    pub rules: Vec<SmartRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SmartRule {
    Simple {
        field: String,
        operator: String,
        value: serde_json::Value,
    },
    Group {
        operator: String,
        rules: Vec<SmartRule>,
    },
}

fn value_as_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn value_as_i64(v: &serde_json::Value) -> i64 {
    match v {
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
        serde_json::Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn field_to_column(field: &str) -> &str {
    match field {
        "title" => "title",
        "artist" => "artist",
        "album" => "album",
        "album_artist" => "album_artist",
        "composer" => "composer",
        "year" => "year",
        "play_count" => "play_count",
        "duration" => "duration",
        "bitrate" => "bitrate",
        "sample_rate" => "sample_rate",
        "track_number" => "track_number",
        "disc_number" => "disc_number",
        "bpm" => "bpm",
        "musical_key" => "musical_key",
        "energy" => "energy",
        _ => "title",
    }
}

fn build_clause(field: &str, operator: &str, value: &serde_json::Value, params: &mut Vec<String>) -> String {
    match (field, operator) {
        // Genre — special: join with track_genres + genres tables
        ("genre", "is") => {
            let p = param_placeholder(params, &value_as_str(value));
            format!("t.id IN (SELECT tg.track_id FROM track_genres tg JOIN genres g ON g.id = tg.genre_id WHERE LOWER(g.name) = LOWER({p}))")
        }
        ("genre", "is_not") => {
            let p = param_placeholder(params, &value_as_str(value));
            format!("t.id NOT IN (SELECT tg.track_id FROM track_genres tg JOIN genres g ON g.id = tg.genre_id WHERE LOWER(g.name) = LOWER({p}))")
        }
        ("genre", "contains") => {
            let p = param_placeholder(params, &value_as_str(value));
            format!("t.id IN (SELECT tg.track_id FROM track_genres tg JOIN genres g ON g.id = tg.genre_id WHERE LOWER(g.name) LIKE '%' || LOWER({p}) || '%')")
        }
        ("genre", "not_contains") => {
            let p = param_placeholder(params, &value_as_str(value));
            format!("t.id NOT IN (SELECT tg.track_id FROM track_genres tg JOIN genres g ON g.id = tg.genre_id WHERE LOWER(g.name) LIKE '%' || LOWER({p}) || '%')")
        }

        // Favorite — special: check favorites table
        ("favorite", "is_true") => "t.id IN (SELECT track_id FROM favorites)".to_string(),
        ("favorite", "is_false") => "t.id NOT IN (SELECT track_id FROM favorites)".to_string(),

        // Last played — special: date-based
        ("last_played", "never") => "t.last_played_at IS NULL".to_string(),
        ("last_played", "in_last") => {
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("t.last_played_at >= datetime('now', '-' || {p} || ' days')")
        }
        ("last_played", "not_in_last") => {
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("(t.last_played_at IS NULL OR t.last_played_at < datetime('now', '-' || {p} || ' days'))")
        }

        // Date added — uses created_at
        ("date_added", "in_last") => {
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("t.created_at >= datetime('now', '-' || {p} || ' days')")
        }
        ("date_added", "not_in_last") => {
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("t.created_at < datetime('now', '-' || {p} || ' days')")
        }

        // Text fields — case-insensitive
        (_, "is") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_str(value));
            format!("LOWER(t.{col}) = LOWER({p})")
        }
        (_, "is_not") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_str(value));
            format!("LOWER(t.{col}) != LOWER({p})")
        }
        (_, "contains") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_str(value));
            format!("LOWER(t.{col}) LIKE '%' || LOWER({p}) || '%'")
        }
        (_, "not_contains") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_str(value));
            format!("LOWER(t.{col}) NOT LIKE '%' || LOWER({p}) || '%'")
        }

        // Numeric fields
        (_, "greater_than") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("t.{col} > {p}")
        }
        (_, "less_than") => {
            let col = field_to_column(field);
            let p = param_placeholder(params, &value_as_i64(value).to_string());
            format!("t.{col} < {p}")
        }
        (_, "between") => {
            let col = field_to_column(field);
            let arr = value.as_array().and_then(|a| {
                Some((a.get(0).and_then(|v| v.as_i64()).unwrap_or(0), a.get(1).and_then(|v| v.as_i64()).unwrap_or(0)))
            }).unwrap_or((0, 9999));
            let p1 = param_placeholder(params, &arr.0.to_string());
            let p2 = param_placeholder(params, &arr.1.to_string());
            format!("t.{col} BETWEEN {p1} AND {p2}")
        }

        _ => "1=1".to_string(),
    }
}

fn param_placeholder(params: &mut Vec<String>, value: &str) -> String {
    params.push(value.to_string());
    format!("?{}", params.len())
}

impl SmartRuleGroup {
    pub fn to_sql(&self) -> (String, Vec<String>) {
        let mut params = Vec::new();
        let sql = self.build(&mut params);
        (sql, params)
    }

    fn build(&self, params: &mut Vec<String>) -> String {
        let clauses: Vec<String> = self.rules.iter().map(|r| r.build(params)).collect();
        let joiner = if self.operator == "or" { " OR " } else { " AND " };
        if clauses.len() == 1 {
            clauses.into_iter().next().unwrap()
        } else {
            format!("({})", clauses.join(joiner))
        }
    }
}

impl SmartRule {
    fn build(&self, params: &mut Vec<String>) -> String {
        match self {
            SmartRule::Simple { field, operator, value } => {
                build_clause(field, operator, value, params)
            }
            SmartRule::Group { operator, rules } => {
                let group = SmartRuleGroup {
                    operator: operator.clone(),
                    rules: rules.clone(),
                };
                group.build(params)
            }
        }
    }
}

pub async fn evaluate_smart_playlist(pool: &SqlitePool, playlist_id: &str) -> Result<Vec<String>, String> {
    let row: (bool, Option<String>) = sqlx::query_as(
        "SELECT is_smart, smart_rules FROM playlists WHERE id = ?"
    )
    .bind(playlist_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("{e}"))?;

    if !row.0 {
        return Err("Not a smart playlist".to_string());
    }

    let rules_json = row.1.unwrap_or_default();
    if rules_json.is_empty() {
        return Ok(Vec::new());
    }

    let group: SmartRuleGroup = serde_json::from_str(&rules_json)
        .map_err(|e| format!("Invalid smart rules: {e}"))?;

    let (where_clause, params) = group.to_sql();
    let sql = format!("SELECT t.id FROM tracks t WHERE {where_clause}");

    let mut query = sqlx::query_as::<_, (String,)>(&sql);
    for p in &params {
        query = query.bind(p);
    }
    let ids: Vec<String> = query.fetch_all(pool)
        .await
        .map_err(|e| format!("Smart playlist eval failed: {e}"))?
        .into_iter()
        .map(|r| r.0)
        .collect();

    Ok(ids)
}

pub async fn refresh_smart_playlist(pool: &SqlitePool, playlist_id: &str) -> Result<i32, String> {
    let track_ids = evaluate_smart_playlist(pool, playlist_id).await?;

    let mut tx = pool.begin().await.map_err(|e| format!("{e}"))?;

    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?")
        .bind(playlist_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("{e}"))?;

    for (i, tid) in track_ids.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)"
        )
        .bind(playlist_id)
        .bind(tid)
        .bind(i as i32)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("{e}"))?;
    }

    sqlx::query("UPDATE playlists SET last_updated = datetime('now') WHERE id = ?")
        .bind(playlist_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("{e}"))?;

    tx.commit().await.map_err(|e| format!("{e}"))?;

    Ok(track_ids.len() as i32)
}

pub async fn refresh_all_smart_playlists(pool: &SqlitePool) -> Result<i32, String> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM playlists WHERE is_smart = 1"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("{e}"))?;

    let mut total = 0;
    for id in &ids {
        match refresh_smart_playlist(pool, id).await {
            Ok(count) => total += count,
            Err(e) => eprintln!("[smart_playlist] Failed to refresh {id}: {e}"),
        }
    }
    Ok(total)
}

pub async fn seed_defaults(pool: &SqlitePool) -> Result<(), String> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM playlists")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("{e}"))?;

    if existing > 0 {
        return Ok(());
    }

    let defaults = vec![
        (
            "Recently Added",
            serde_json::json!({
                "operator": "and",
                "rules": [
                    {"field": "date_added", "operator": "in_last", "value": 30}
                ]
            }),
        ),
        (
            "Most Played",
            serde_json::json!({
                "operator": "and",
                "rules": [
                    {"field": "play_count", "operator": "greater_than", "value": 5}
                ]
            }),
        ),
        (
            "Favorites",
            serde_json::json!({
                "operator": "and",
                "rules": [
                    {"field": "favorite", "operator": "is_true", "value": true}
                ]
            }),
        ),
        (
            "Never Played",
            serde_json::json!({
                "operator": "and",
                "rules": [
                    {"field": "last_played", "operator": "never", "value": ""}
                ]
            }),
        ),
    ];

    for (name, rules) in &defaults {
        let id = uuid::Uuid::new_v4().to_string();
        let rules_str = serde_json::to_string(rules).map_err(|e| format!("{e}"))?;
        sqlx::query(
            "INSERT INTO playlists (id, name, description, is_smart, smart_rules, created_at, last_updated) VALUES (?, ?, ?, 1, ?, datetime('now'), datetime('now'))"
        )
        .bind(&id)
        .bind(name)
        .bind("")
        .bind(&rules_str)
        .execute(pool)
        .await
        .map_err(|e| format!("{e}"))?;
    }

    // Refresh all default smart playlists
    refresh_all_smart_playlists(pool).await?;

    Ok(())
}
