use crate::library::track::Track;
use lofty::file::TaggedFileExt;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{Connection, Executor};
use sqlx::Row;
use std::path::Path;

pub async fn init_pool(path: &Path) -> Result<(SqlitePool, bool), String> {
    // Phase 1: single dedicated connection for PRAGMAs, integrity check and
    // migrations. Every statement runs on one connection, so its schema
    // snapshot is always current (a multi-connection pool can cache a stale
    // pre-migration schema and then reject `ON CONFLICT` upserts).
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let init_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts.clone())
        .await
        .map_err(|e| format!("Failed to create init pool: {e}"))?;

    // Apply performance PRAGMAs before migrations
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&init_pool)
        .await
        .map_err(|e| format!("Failed to set WAL mode: {e}"))?;

    sqlx::query("PRAGMA synchronous=NORMAL")
        .execute(&init_pool)
        .await
        .map_err(|e| format!("Failed to set synchronous mode: {e}"))?;

    sqlx::query("PRAGMA cache_size=-64000") // 64MB cache
        .execute(&init_pool)
        .await
        .map_err(|e| format!("Failed to set cache size: {e}"))?;

    sqlx::query("PRAGMA temp_store=MEMORY")
        .execute(&init_pool)
        .await
        .map_err(|e| format!("Failed to set temp store: {e}"))?;

    sqlx::query("PRAGMA mmap_size=268435456") // 256MB memory-mapped I/O
        .execute(&init_pool)
        .await
        .map_err(|e| format!("Failed to set mmap size: {e}"))?;

    // Integrity check — detect corruption early
    let integrity: Result<Vec<(String,)>, _> = sqlx::query_as("PRAGMA integrity_check")
        .fetch_all(&init_pool)
        .await;
    match integrity {
        Ok(rows) if rows.iter().all(|(r,)| r == "ok") => {}
        Ok(rows) => {
            let result = rows.first().map(|(r,)| r.as_str()).unwrap_or("unknown");
            let backup_path = path.with_extension("corrupted.db");
            log::error!("Database integrity check failed: {} — backing up to {:?}", result, backup_path);
            let _ = std::fs::rename(path, &backup_path);
            // Re-create the pool with a fresh database
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("db-wal"));
            let _ = std::fs::remove_file(path.with_extension("db-shm"));
            return Err(format!("Database corrupt, backed up to {:?}. A fresh database will be created.", backup_path));
        }
        Err(e) => {
            log::warn!("Could not run integrity_check: {}", e);
        }
    }

    let cue_schema_migrated = run_migrations(&init_pool).await?;

    // Close the init pool so all its connections release the file, then open
    // the real pool: every pooled connection now opens against the final
    // schema and caches it correctly.
    init_pool.close().await;

    let pool = SqlitePoolOptions::new()
        .max_connections(15) // Increased from 5 for better parallelism
        .connect_with(opts)
        .await
        .map_err(|e| format!("Failed to create pool: {e}"))?;

    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let _ = repair_artwork_status(&pool_clone).await;
    });

    Ok((pool, cue_schema_migrated))
}

async fn run_migrations(pool: &SqlitePool) -> Result<bool, String> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracks (
            id          TEXT PRIMARY KEY,
            file_path   TEXT NOT NULL,
            title       TEXT NOT NULL DEFAULT '',
            artist      TEXT NOT NULL DEFAULT '',
            album       TEXT NOT NULL DEFAULT '',
            album_artist TEXT NOT NULL DEFAULT '',
            album_id    TEXT NOT NULL DEFAULT '',
            composer    TEXT NOT NULL DEFAULT '',
            track_number INTEGER DEFAULT 0,
            disc_number  INTEGER DEFAULT 0,
            year        INTEGER DEFAULT 0,
            genre       TEXT NOT NULL DEFAULT '',
            raw_genre_names TEXT NOT NULL DEFAULT '',
            duration    REAL NOT NULL DEFAULT 0,
            sample_rate INTEGER NOT NULL DEFAULT 0,
            bit_depth   INTEGER NOT NULL DEFAULT 0,
            channels    INTEGER NOT NULL DEFAULT 0,
            bitrate     INTEGER NOT NULL DEFAULT 0,
            file_size   INTEGER NOT NULL DEFAULT 0,
            file_format TEXT NOT NULL DEFAULT '',
            has_artwork INTEGER NOT NULL DEFAULT 0,
            replaygain_track_gain REAL NOT NULL DEFAULT 0.0,
            replaygain_album_gain REAL NOT NULL DEFAULT 0.0,
            cue_parent_id TEXT,
            cue_offset REAL NOT NULL DEFAULT 0.0,
            file_modified INTEGER NOT NULL DEFAULT 0,
            artwork_key TEXT,
            bpm         REAL NOT NULL DEFAULT 0.0,
            musical_key TEXT NOT NULL DEFAULT '',
            energy      REAL NOT NULL DEFAULT 0.0,
            analysis_failed_reason TEXT,
            genre_checked INTEGER NOT NULL DEFAULT 0,
            play_count  INTEGER NOT NULL DEFAULT 0,
            last_played_at TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            modified_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Migration: add file_modified for existing databases
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN file_modified INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration to add has_artwork to existing tables
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN has_artwork INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration to add composer to existing tables
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN composer TEXT NOT NULL DEFAULT ''",
    )
    .execute(pool)
    .await;

    // Migration to add ReplayGain columns
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN replaygain_track_gain REAL NOT NULL DEFAULT 0.0",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN replaygain_album_gain REAL NOT NULL DEFAULT 0.0",
    )
    .execute(pool)
    .await;

    // Migration to add CUE columns
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN cue_parent_id TEXT",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN cue_offset REAL NOT NULL DEFAULT 0.0",
    )
    .execute(pool)
    .await;

    // Migration to add album_id for existing tables
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN album_id TEXT NOT NULL DEFAULT ''",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_album_id ON tracks(album_id)",
    )
    .execute(pool)
    .await;

    // Backfill empty album_ids for existing databases
    if let Ok(unbackfilled) = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, album_artist, album FROM tracks WHERE album_id = '' OR album_id IS NULL",
    )
    .fetch_all(pool)
    .await
    {
        for (id, album_artist, album) in unbackfilled {
            let aid = crate::library::track::compute_album_id(&album_artist, &album);
            let _ = sqlx::query("UPDATE tracks SET album_id = ? WHERE id = ?")
                .bind(&aid)
                .bind(&id)
                .execute(pool)
                .await;
        }
    }

    // Migration to add bitrate to existing tables
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN bitrate INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration to add artwork_key to tracks
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN artwork_key TEXT",
    )
    .execute(pool)
    .await;

    // Migration to add play_count to tracks
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration to add last_played_at to tracks
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN last_played_at TEXT",
    )
    .execute(pool)
    .await;

    // Back-fill play_count and last_played_at from existing play_history
    let _ = sqlx::query(
        "UPDATE tracks SET play_count = (
            SELECT COUNT(*) FROM play_history WHERE play_history.track_id = tracks.id
        ), last_played_at = (
            SELECT MAX(played_at) FROM play_history WHERE play_history.track_id = tracks.id
        )
        WHERE id IN (SELECT track_id FROM play_history)"
    )
    .execute(pool)
    .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlists (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            is_smart    INTEGER NOT NULL DEFAULT 0,
            smart_rules TEXT,
            last_updated TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    let _ = sqlx::query("ALTER TABLE playlists ADD COLUMN is_smart INTEGER NOT NULL DEFAULT 0").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE playlists ADD COLUMN smart_rules TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE playlists ADD COLUMN last_updated TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE playlists ADD COLUMN is_folder_playlist INTEGER NOT NULL DEFAULT 0").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE playlists ADD COLUMN folder_path TEXT").execute(pool).await;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position    INTEGER NOT NULL,
            rank        TEXT NOT NULL DEFAULT '',
            added_at    TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (playlist_id, track_id)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    let _ = sqlx::query("ALTER TABLE playlist_tracks ADD COLUMN rank TEXT NOT NULL DEFAULT ''").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_playlist_tracks_rank ON playlist_tracks(playlist_id, rank)").execute(pool).await;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS favorites (
            track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS library_folders (
            path TEXT PRIMARY KEY,
            added_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS play_history (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            played_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_play_history_track_id ON play_history(track_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_play_history_played_at ON play_history(played_at)",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Migration to add played_secs to play_history
    let _ = sqlx::query(
        "ALTER TABLE play_history ADD COLUMN played_secs REAL NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration: playback outcome for insights (completed/skipped/stopped).
    let _ = sqlx::query(
        "ALTER TABLE play_history ADD COLUMN end_reason TEXT",
    )
    .execute(pool)
    .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS eq_profiles (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            is_default  INTEGER NOT NULL DEFAULT 0,
            is_active   INTEGER NOT NULL DEFAULT 0,
            preset_key  TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    // Migration: stable built-in preset keys (Airmedy parity).
    let _ = sqlx::query(
        "ALTER TABLE eq_profiles ADD COLUMN preset_key TEXT NOT NULL DEFAULT ''",
    )
    .execute(pool)
    .await;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS eq_bands (
            profile_id  TEXT NOT NULL REFERENCES eq_profiles(id) ON DELETE CASCADE,
            band_index  INTEGER NOT NULL,
            gain        REAL NOT NULL DEFAULT 0.0,
            PRIMARY KEY (profile_id, band_index)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS radio_stations (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            url         TEXT NOT NULL,
            genre       TEXT NOT NULL DEFAULT '',
            logo_url    TEXT NOT NULL DEFAULT '',
            country     TEXT NOT NULL DEFAULT '',
            language    TEXT NOT NULL DEFAULT '',
            bitrate     INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS lyrics (
            track_id    TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
            content     TEXT NOT NULL DEFAULT '',
            source      TEXT NOT NULL DEFAULT '',
            meta_content TEXT NOT NULL DEFAULT '',
            meta_source TEXT NOT NULL DEFAULT '',
            updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS artists (
            name        TEXT PRIMARY KEY,
            artwork_key TEXT

        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Migration: add artwork_checked — artist artwork fetch attempted, all sources failed
    let _ = sqlx::query(
        "ALTER TABLE artists ADD COLUMN artwork_checked INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS genres (
            id                TEXT PRIMARY KEY,
            name              TEXT NOT NULL UNIQUE,
            normalization_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_genres_normalization_key ON genres(normalization_key)",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS track_genres (
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            genre_id TEXT NOT NULL REFERENCES genres(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            PRIMARY KEY (track_id, genre_id)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Migration: add raw_genre_names to tracks
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN raw_genre_names TEXT NOT NULL DEFAULT ''",
    )
    .execute(pool)
    .await;

    // Migration: add bpm, musical_key, energy for Smart Mix
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN bpm REAL NOT NULL DEFAULT 0.0",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN musical_key TEXT NOT NULL DEFAULT ''",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN energy REAL NOT NULL DEFAULT 0.0",
    )
    .execute(pool)
    .await;

    // Migration: add analysis_failed_reason — prevents re-checking permanently missing files
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN analysis_failed_reason TEXT",
    )
    .execute(pool)
    .await;

    // Migration: add genre_checked — tracks already checked by Last.fm enrichment (avoid re-fetch)
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN genre_checked INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Backfill: legacy `__none__` sentinel (no genres found) → mark checked, drop sentinel value
    let _ = sqlx::query(
        "UPDATE tracks SET genre_checked = 1, raw_genre_names = '' WHERE raw_genre_names = '__none__'",
    )
    .execute(pool)
    .await;

    // Migration: add channels for telemetry
    let _ = sqlx::query(
        "ALTER TABLE tracks ADD COLUMN channels INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;

    // Migration: drop UNIQUE(file_path) from tracks so CUE child tracks can
    // coexist with their parent (both share the audio file's path). SQLite
    // cannot alter constraints, so rebuild the table when a legacy schema
    // with the UNIQUE constraint is detected. A partial unique index below
    // keeps the physical-file path invariant while exempting CUE children.
    let legacy_sql: Option<(String,)> = sqlx::query_as(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'tracks'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to inspect tracks schema: {e}"))?;

    let legacy_unique = legacy_sql
        .as_ref()
        .map(|(sql,)| sql.contains("UNIQUE"))
        .unwrap_or(false);

    let mut cue_schema_migrated = false;
    if legacy_unique {
        log::info!("Migrating tracks schema: removing UNIQUE(file_path) for CUE support");
        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| format!("Failed to acquire connection for tracks migration: {e}"))?;

        // PRAGMAs are per-connection; restore the previous foreign_keys state
        let fk_before: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| format!("Failed to read foreign_keys pragma: {e}"))?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .map_err(|e| format!("Failed to disable foreign_keys: {e}"))?;

        let mut tx = conn
            .begin()
            .await
            .map_err(|e| format!("Failed to begin tracks migration: {e}"))?;
        sqlx::query(
            "CREATE TABLE tracks_new (
                id          TEXT PRIMARY KEY,
                file_path   TEXT NOT NULL,
                title       TEXT NOT NULL DEFAULT '',
                artist      TEXT NOT NULL DEFAULT '',
                album       TEXT NOT NULL DEFAULT '',
                album_artist TEXT NOT NULL DEFAULT '',
                album_id    TEXT NOT NULL DEFAULT '',
                composer    TEXT NOT NULL DEFAULT '',
                track_number INTEGER DEFAULT 0,
                disc_number  INTEGER DEFAULT 0,
                year        INTEGER DEFAULT 0,
                genre       TEXT NOT NULL DEFAULT '',
                raw_genre_names TEXT NOT NULL DEFAULT '',
                duration    REAL NOT NULL DEFAULT 0,
                sample_rate INTEGER NOT NULL DEFAULT 0,
                bit_depth   INTEGER NOT NULL DEFAULT 0,
                channels    INTEGER NOT NULL DEFAULT 0,
                bitrate     INTEGER NOT NULL DEFAULT 0,
                file_size   INTEGER NOT NULL DEFAULT 0,
                file_format TEXT NOT NULL DEFAULT '',
                has_artwork INTEGER NOT NULL DEFAULT 0,
                replaygain_track_gain REAL NOT NULL DEFAULT 0.0,
                replaygain_album_gain REAL NOT NULL DEFAULT 0.0,
                cue_parent_id TEXT,
                cue_offset REAL NOT NULL DEFAULT 0.0,
                file_modified INTEGER NOT NULL DEFAULT 0,
                artwork_key TEXT,
                bpm         REAL NOT NULL DEFAULT 0.0,
                musical_key TEXT NOT NULL DEFAULT '',
                energy      REAL NOT NULL DEFAULT 0.0,
                analysis_failed_reason TEXT,
                genre_checked INTEGER NOT NULL DEFAULT 0,
                play_count  INTEGER NOT NULL DEFAULT 0,
                last_played_at TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                modified_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to create tracks_new: {e}"))?;
        sqlx::query(
            "INSERT INTO tracks_new (id, file_path, title, artist, album, album_artist,
                composer, track_number, disc_number, year, genre, raw_genre_names,
                duration, sample_rate, bit_depth, channels, bitrate, file_size,
                file_format, has_artwork, replaygain_track_gain, replaygain_album_gain,
                cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy,
                analysis_failed_reason, genre_checked, file_modified, play_count,
                last_played_at, created_at, modified_at)
             SELECT id, file_path, title, artist, album, album_artist,
                composer, track_number, disc_number, year, genre, raw_genre_names,
                duration, sample_rate, bit_depth, channels, bitrate, file_size,
                file_format, has_artwork, replaygain_track_gain, replaygain_album_gain,
                cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy,
                analysis_failed_reason, genre_checked, file_modified, play_count,
                last_played_at, created_at, modified_at
             FROM tracks",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to copy tracks: {e}"))?;
        sqlx::query("DROP TABLE tracks")
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to drop legacy tracks: {e}"))?;
        sqlx::query("ALTER TABLE tracks_new RENAME TO tracks")
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("Failed to rename tracks table: {e}"))?;
        tx.commit()
            .await
            .map_err(|e| format!("Failed to commit tracks migration: {e}"))?;

        sqlx::query(&format!("PRAGMA foreign_keys = {fk_before}"))
            .execute(&mut *conn)
            .await
            .map_err(|e| format!("Failed to restore foreign_keys pragma: {e}"))?;

        // Legacy rows corrupted by the old UNIQUE(file_path) collapse: CUE
        // children could never persist under that schema, so any row with
        // cue_parent_id set is a parent overwritten with the last child's
        // metadata. Drop them — a rescan regenerates the real children.
        let tainted: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM tracks WHERE cue_parent_id IS NOT NULL",
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| format!("Failed to find legacy CUE rows: {e}"))?;
        if !tainted.is_empty() {
            let placeholders: Vec<String> = (0..tainted.len())
                .map(|i| format!("?{}", i + 1))
                .collect();
            let ph = placeholders.join(",");
            for table in ["playlist_tracks", "favorites", "play_history"] {
                let sql = format!("DELETE FROM {table} WHERE track_id IN ({ph})");
                let mut q = sqlx::query(&sql);
                for id in &tainted {
                    q = q.bind(id);
                }
                let _ = q.execute(&mut *conn).await;
            }
            let del_sql = format!("DELETE FROM tracks WHERE id IN ({ph})");
            let mut q = sqlx::query(&del_sql);
            for id in &tainted {
                q = q.bind(id);
            }
            q.execute(&mut *conn)
                .await
                .map_err(|e| format!("Failed to delete legacy CUE rows: {e}"))?;
            log::info!("Removed {} legacy CUE-corrupted track rows", tainted.len());
        }
        cue_schema_migrated = true;
    }

    // Physical files keep a unique path; CUE children (cue_parent_id set) are exempt.
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_tracks_parent_path ON tracks(file_path) WHERE cue_parent_id IS NULL",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create tracks path index: {e}"))?;

    // NOTE: schema for settings/loudness/analysis/percentiles/podcasts lives
    // here (in the synchronous migration path), not in the async repair task
    // below: tests and early startup code query these tables immediately
    // after init_pool returns, before any spawned task is guaranteed to run.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // FFmpeg ebur128 loudness (EBU R128). Separate table so the tracks
    // upsert path and its SELECT projections stay untouched. `analyzed_version`
    // lets the backfill worker re-analyze after algorithm bumps
    // (`audio::lufs::ANALYZER_VERSION`).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS track_loudness (
            track_id         TEXT PRIMARY KEY,
            loudness_lufs    REAL NOT NULL,
            loudness_range   REAL NOT NULL,
            true_peak        REAL NOT NULL,
            analyzed_version INTEGER NOT NULL DEFAULT 1,
            analyzed_at      TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(track_id) REFERENCES tracks(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Full-spectrum analysis (FFmpeg ebur128+astats+aspectralstats, one
    // invocation). Separate table so track upserts can never clobber
    // measurements; versioned for re-analysis bumps. Mood columns are
    // derived from raw features + corpus percentiles (see recompute_moods).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS track_analysis (
            track_id         TEXT PRIMARY KEY,
            loudness_lufs    REAL,
            loudness_range   REAL,
            true_peak        REAL,
            rms_db           REAL,
            crest            REAL,
            spectral_centroid REAL,
            spectral_rolloff REAL,
            spectral_flatness REAL,
            spectral_flux    REAL,
            zcr              REAL,
            energy           REAL,
            danceability     REAL,
            brightness       REAL,
            analyzed_version INTEGER NOT NULL DEFAULT 1,
            analyzed_at      TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(track_id) REFERENCES tracks(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Corpus percentiles for mood normalization (Airmedy feature_percentiles).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS feature_percentiles (
            feature_name TEXT PRIMARY KEY,
            p1  REAL NOT NULL,
            p5  REAL NOT NULL,
            p50 REAL NOT NULL,
            p95 REAL NOT NULL,
            p99 REAL NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Podcast subscriptions. etag/last_modified drive conditional GETs so
    // routine refreshes are nearly free.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS podcast_feeds (
            id            TEXT PRIMARY KEY,
            url           TEXT NOT NULL UNIQUE,
            title         TEXT NOT NULL DEFAULT '',
            description   TEXT NOT NULL DEFAULT '',
            image_url     TEXT NOT NULL DEFAULT '',
            etag          TEXT NOT NULL DEFAULT '',
            last_modified TEXT NOT NULL DEFAULT '',
            refresh_at    TEXT NOT NULL DEFAULT (datetime('now')),
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // Podcast episodes. Identity is (feed_id, guid) with NULL guids never
    // conflicting (SQLite NULLs are distinct); guid-less episodes match by
    // enclosure URL in code. Refresh updates metadata + audio_url (dynamic
    // ad insertion rotates enclosures) but never position_secs, local_path,
    // or the decoder-won duration_secs.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS podcast_episodes (
            id               TEXT PRIMARY KEY,
            feed_id          TEXT NOT NULL REFERENCES podcast_feeds(id) ON DELETE CASCADE,
            guid             TEXT,
            title            TEXT NOT NULL DEFAULT '',
            description      TEXT NOT NULL DEFAULT '',
            pub_date         INTEGER,
            audio_url        TEXT NOT NULL DEFAULT '',
            enclosure_len    INTEGER,
            local_path       TEXT NOT NULL DEFAULT '',
            bytes_downloaded INTEGER NOT NULL DEFAULT 0,
            duration_secs    REAL,
            position_secs    REAL NOT NULL DEFAULT 0.0,
            completed        INTEGER NOT NULL DEFAULT 0,
            added_at         TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(feed_id, guid)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_episodes_feed_date ON podcast_episodes(feed_id, pub_date)",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;
    // Heal pre-fix rows: empty-string guids participated in UNIQUE(feed_id,
    // guid) and collapsed feeds to one episode. NULLs never conflict.
    let _ = sqlx::query("UPDATE podcast_episodes SET guid = NULL WHERE guid = ''")
        .execute(pool)
        .await;
    // Per-feed auto-download target (0 = off).
    let _ = sqlx::query(
        "ALTER TABLE podcast_feeds ADD COLUMN auto_download_count INTEGER NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await;
    // Completion timestamp for retention rules (P2 playback writes it).
    let _ = sqlx::query(
        "ALTER TABLE podcast_episodes ADD COLUMN completed_at TEXT",
    )
    .execute(pool)
    .await;

    Ok(cue_schema_migrated)
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM settings WHERE key = ?"
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get setting: {e}"))?;
    Ok(row.map(|r| r.0))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to set setting: {e}"))?;
    Ok(())
}

/// One row of the `track_loudness` table (FFmpeg ebur128 measurement).
#[derive(Debug, Clone)]
pub struct TrackLoudness {
    pub track_id: String,
    pub loudness_lufs: f64,
    pub loudness_range: f64,
    pub true_peak: f64,
    pub analyzed_version: i64,
}

pub async fn upsert_track_loudness(
    pool: &SqlitePool,
    track_id: &str,
    loudness_lufs: f64,
    loudness_range: f64,
    true_peak: f64,
    analyzed_version: i64,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO track_loudness
            (track_id, loudness_lufs, loudness_range, true_peak, analyzed_version, analyzed_at)
         VALUES (?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
            loudness_lufs = excluded.loudness_lufs,
            loudness_range = excluded.loudness_range,
            true_peak = excluded.true_peak,
            analyzed_version = excluded.analyzed_version,
            analyzed_at = datetime('now')",
    )
    .bind(track_id)
    .bind(loudness_lufs)
    .bind(loudness_range)
    .bind(true_peak)
    .bind(analyzed_version)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to upsert track loudness: {e}"))?;
    Ok(())
}

pub async fn get_track_loudness(
    pool: &SqlitePool,
    track_id: &str,
) -> Result<Option<TrackLoudness>, String> {
    let row: Option<(String, f64, f64, f64, i64)> = sqlx::query_as(
        "SELECT track_id, loudness_lufs, loudness_range, true_peak, analyzed_version
         FROM track_loudness WHERE track_id = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get track loudness: {e}"))?;
    Ok(row.map(|r| TrackLoudness {
        track_id: r.0,
        loudness_lufs: r.1,
        loudness_range: r.2,
        true_peak: r.3,
        analyzed_version: r.4,
    }))
}

/// Mean LUFS + worst true peak across analyzed siblings on the same album
/// (album-normalization look-ahead, Airmedy parity). Matches on `album` and,
/// when set, `album_artist` to avoid cross-artist compilation collisions.
/// Returns `(mean_lufs, worst_peak, sibling_count)`.
pub async fn get_album_loudness_stats(
    pool: &SqlitePool,
    album: &str,
    album_artist: &str,
    exclude_track_id: &str,
) -> Result<Option<(f64, f64, i64)>, String> {
    if album.is_empty() {
        return Ok(None);
    }
    let row: (Option<f64>, Option<f64>, i64) = if album_artist.is_empty() {
        sqlx::query_as(
            "SELECT AVG(l.loudness_lufs), MAX(l.true_peak), COUNT(*)
             FROM track_loudness l
             JOIN tracks t ON t.id = l.track_id
             WHERE t.album = ? AND t.id != ?",
        )
        .bind(album)
        .bind(exclude_track_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to get album loudness: {e}"))?
    } else {
        sqlx::query_as(
            "SELECT AVG(l.loudness_lufs), MAX(l.true_peak), COUNT(*)
             FROM track_loudness l
             JOIN tracks t ON t.id = l.track_id
             WHERE t.album = ? AND t.album_artist = ? AND t.id != ?",
        )
        .bind(album)
        .bind(album_artist)
        .bind(exclude_track_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to get album loudness: {e}"))?
    };
    match row {
        (Some(avg), peak, n) if n > 0 => Ok(Some((avg, peak.unwrap_or(-70.0), n))),
        _ => Ok(None),
    }
}

/// Tracks (physical files only, no CUE children) missing a current loudness
/// measurement, oldest files first. Bounded so the worker stays incremental.
pub async fn get_tracks_pending_loudness(
    pool: &SqlitePool,
    analyzed_version: i64,
    limit: i64,
) -> Result<Vec<Track>, String> {
    // Reuse the full-track projection via get_track on each id to avoid
    // duplicating the wide SELECT. Pending sets are small (bounded by limit).
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT t.id FROM tracks t
         LEFT JOIN track_loudness l ON l.track_id = t.id
         WHERE t.cue_parent_id IS NULL
           AND (l.track_id IS NULL OR l.analyzed_version < ?)
         ORDER BY t.file_modified ASC
         LIMIT ?",
    )
    .bind(analyzed_version)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch pending loudness tracks: {e}"))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(t) = get_track(pool, &id).await? {
            out.push(t);
        }
    }
    Ok(out)
}

/// Share of physical tracks with a current loudness measurement (0.0–1.0).
pub async fn get_loudness_progress(pool: &SqlitePool, analyzed_version: i64) -> Result<(i64, i64), String> {
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to count tracks: {e}"))?;
    let done: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM track_loudness l
         JOIN tracks t ON t.id = l.track_id
         WHERE t.cue_parent_id IS NULL AND l.analyzed_version >= ?",
    )
    .bind(analyzed_version)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to count analyzed tracks: {e}"))?;
    Ok((done.0, total.0))
}

/// One row of the `track_analysis` table (full-spectrum FFmpeg measurement).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackAnalysis {
    pub track_id: String,
    pub loudness_lufs: Option<f64>,
    pub loudness_range: Option<f64>,
    pub true_peak: Option<f64>,
    pub rms_db: Option<f64>,
    pub crest: Option<f64>,
    pub spectral_centroid: Option<f64>,
    pub spectral_rolloff: Option<f64>,
    pub spectral_flatness: Option<f64>,
    pub spectral_flux: Option<f64>,
    pub zcr: Option<f64>,
    pub energy: Option<f64>,
    pub danceability: Option<f64>,
    pub brightness: Option<f64>,
    pub analyzed_version: i64,
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_track_analysis(
    pool: &SqlitePool,
    track_id: &str,
    loudness_lufs: f64,
    loudness_range: f64,
    true_peak: f64,
    rms_db: f64,
    crest: f64,
    spectral_centroid: f64,
    spectral_rolloff: f64,
    spectral_flatness: f64,
    spectral_flux: f64,
    zcr: f64,
    analyzed_version: i64,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO track_analysis
            (track_id, loudness_lufs, loudness_range, true_peak, rms_db, crest,
             spectral_centroid, spectral_rolloff, spectral_flatness, spectral_flux, zcr,
             analyzed_version, analyzed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
            loudness_lufs = excluded.loudness_lufs,
            loudness_range = excluded.loudness_range,
            true_peak = excluded.true_peak,
            rms_db = excluded.rms_db,
            crest = excluded.crest,
            spectral_centroid = excluded.spectral_centroid,
            spectral_rolloff = excluded.spectral_rolloff,
            spectral_flatness = excluded.spectral_flatness,
            spectral_flux = excluded.spectral_flux,
            zcr = excluded.zcr,
            energy = NULL, danceability = NULL, brightness = NULL,
            analyzed_version = excluded.analyzed_version,
            analyzed_at = datetime('now')",
    )
    .bind(track_id)
    .bind(loudness_lufs)
    .bind(loudness_range)
    .bind(true_peak)
    .bind(rms_db)
    .bind(crest)
    .bind(spectral_centroid)
    .bind(spectral_rolloff)
    .bind(spectral_flatness)
    .bind(spectral_flux)
    .bind(zcr)
    .bind(analyzed_version)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to upsert track analysis: {e}"))?;
    Ok(())
}

pub async fn get_track_analysis(
    pool: &SqlitePool,
    track_id: &str,
) -> Result<Option<TrackAnalysis>, String> {
    let row: Option<(
        String, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>,
        Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>,
        Option<f64>, Option<f64>, Option<f64>, i64,
    )> = sqlx::query_as(
        "SELECT track_id, loudness_lufs, loudness_range, true_peak, rms_db, crest,
                spectral_centroid, spectral_rolloff, spectral_flatness, spectral_flux, zcr,
                energy, danceability, brightness, analyzed_version
         FROM track_analysis WHERE track_id = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get track analysis: {e}"))?;
    Ok(row.map(|r| TrackAnalysis {
        track_id: r.0,
        loudness_lufs: r.1,
        loudness_range: r.2,
        true_peak: r.3,
        rms_db: r.4,
        crest: r.5,
        spectral_centroid: r.6,
        spectral_rolloff: r.7,
        spectral_flatness: r.8,
        spectral_flux: r.9,
        zcr: r.10,
        energy: r.11,
        danceability: r.12,
        brightness: r.13,
        analyzed_version: r.14,
    }))
}

/// Physical tracks missing a current full-spectrum measurement.
pub async fn get_tracks_pending_analysis(
    pool: &SqlitePool,
    analyzed_version: i64,
    limit: i64,
) -> Result<Vec<Track>, String> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT t.id FROM tracks t
         LEFT JOIN track_analysis a ON a.track_id = t.id
         WHERE t.cue_parent_id IS NULL
           AND (a.track_id IS NULL OR a.analyzed_version < ?)
         ORDER BY t.file_modified ASC
         LIMIT ?",
    )
    .bind(analyzed_version)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch pending analysis tracks: {e}"))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(t) = get_track(pool, &id).await? {
            out.push(t);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
pub struct FeaturePercentiles {
    pub p1: f64,
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

pub async fn upsert_percentiles(
    pool: &SqlitePool,
    feature: &str,
    p: FeaturePercentiles,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO feature_percentiles (feature_name, p1, p5, p50, p95, p99)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(feature_name) DO UPDATE SET
            p1 = excluded.p1, p5 = excluded.p5, p50 = excluded.p50,
            p95 = excluded.p95, p99 = excluded.p99",
    )
    .bind(feature)
    .bind(p.p1)
    .bind(p.p5)
    .bind(p.p50)
    .bind(p.p95)
    .bind(p.p99)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to upsert percentiles: {e}"))?;
    Ok(())
}

pub async fn get_percentiles(
    pool: &SqlitePool,
    feature: &str,
) -> Result<Option<FeaturePercentiles>, String> {
    let row: Option<(f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT p1, p5, p50, p95, p99 FROM feature_percentiles WHERE feature_name = ?",
    )
    .bind(feature)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to get percentiles: {e}"))?;
    Ok(row.map(|r| FeaturePercentiles { p1: r.0, p5: r.1, p50: r.2, p95: r.3, p99: r.4 }))
}

/// Raw column values across analyzed tracks for percentile computation.
pub async fn get_raw_feature_values(
    pool: &SqlitePool,
    column: &str,
) -> Result<Vec<f64>, String> {
    // Column allowlist: never interpolate unchecked identifiers.
    let col = match column {
        "rms_db" => "rms_db",
        "spectral_centroid" => "spectral_centroid",
        "spectral_flux" => "spectral_flux",
        "crest" => "crest",
        "loudness_range" => "loudness_range",
        _ => return Err(format!("Unknown analysis feature: {column}")),
    };
    let rows: Vec<(Option<f64>,)> = sqlx::query_as(&format!(
        "SELECT {col} FROM track_analysis WHERE {col} IS NOT NULL"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch raw feature values: {e}"))?;
    Ok(rows.into_iter().filter_map(|r| r.0).collect())
}

/// Write derived mood values for one analyzed track.
pub async fn set_track_mood(
    pool: &SqlitePool,
    track_id: &str,
    energy: f64,
    danceability: f64,
    brightness: f64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE track_analysis SET energy = ?, danceability = ?, brightness = ? WHERE track_id = ?",
    )
    .bind(energy)
    .bind(danceability)
    .bind(brightness)
    .bind(track_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to set track mood: {e}"))?;
    Ok(())
}

/// All stored moods keyed by track id: (energy, danceability).
/// Used by SmartMix 2.0; tracks without analysis are simply absent.
pub async fn get_all_moods(
    pool: &SqlitePool,
) -> Result<std::collections::HashMap<String, (f64, f64)>, String> {
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT track_id, energy, danceability FROM track_analysis
         WHERE energy IS NOT NULL AND danceability IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch moods: {e}"))?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, e, d)| match (e, d) {
            (Some(e), Some(d)) => Some((id, (e, d))),
            _ => None,
        })
        .collect())
}

/// All analyzed track ids with their stored tempo (BPM from tracks) for a
/// full mood recompute after percentile refresh.
pub async fn get_analyzed_tracks_with_tempo(
    pool: &SqlitePool,
) -> Result<Vec<(String, f64)>, String> {
    sqlx::query_as(
        "SELECT a.track_id, COALESCE(t.bpm, 0.0) FROM track_analysis a
         JOIN tracks t ON t.id = a.track_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch analyzed tracks: {e}"))
}

pub async fn get_library_folders(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT path FROM library_folders ORDER BY added_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch library folders: {e}"))?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn add_library_folder(pool: &SqlitePool, path: &str) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO library_folders (path) VALUES (?)")
        .bind(path)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to add library folder: {e}"))?;
    Ok(())
}

pub async fn remove_library_folder(pool: &SqlitePool, path: &str) -> Result<(), String> {
    let normalized = path.trim_end_matches(&['/', '\\'][..]);
    let sep = std::path::MAIN_SEPARATOR;
    let with_sep = format!("{}{}", normalized, sep);

    // Match tracks whose file_path starts with the folder path + separator,
    // or is exactly the folder path itself.
    // Use ^ as the LIKE escape character to avoid conflicts with path backslashes.
    let escaped = with_sep.replace('%', "^%").replace('_', "^_");
    let like_pattern = format!("{}%", escaped);

    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM tracks WHERE file_path = ? OR file_path LIKE ? ESCAPE '^'"
    )
    .bind(normalized)
    .bind(&like_pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch tracks for removal: {e}"))?;

    if !ids.is_empty() {
        // Also clean up any CUE child tracks whose parent is among the removed tracks
        delete_tracks(pool, &ids).await?;
    }

    sqlx::query("DELETE FROM library_folders WHERE path = ?")
        .bind(path)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to remove library folder: {e}"))?;
    Ok(())
}

pub async fn get_favorite_ids(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT track_id FROM favorites ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch favorites: {e}"))?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn toggle_favorite(pool: &SqlitePool, id: &str) -> Result<bool, String> {
    let exists: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM favorites WHERE track_id = ?"
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to check favorite: {e}"))?;
    if exists.0 > 0 {
        sqlx::query("DELETE FROM favorites WHERE track_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to remove favorite: {e}"))?;
        Ok(false)
    } else {
        sqlx::query("INSERT INTO favorites (track_id) VALUES (?)")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to add favorite: {e}"))?;
        Ok(true)
    }
}

pub async fn upsert_track<'e>(executor: impl Executor<'e, Database = sqlx::Sqlite>, track: &mut Track) -> Result<String, String> {
    // CUE children are keyed by their deterministic id ("{parent_id}-cue-NN");
    // physical tracks by path among parents only (partial index
    // idx_tracks_parent_path). This keeps stored ids stable across rescans.
    let conflict_target = if track.cue_parent_id.is_some() {
        "ON CONFLICT(id) DO UPDATE SET"
    } else {
        "ON CONFLICT(file_path) WHERE cue_parent_id IS NULL DO UPDATE SET"
    };

    let row: (String,) = sqlx::query_as(&format!(
        "INSERT INTO tracks (id, file_path, album_id, title, artist, album, album_artist,
            composer, track_number, disc_number, year, genre, raw_genre_names, duration,
            sample_rate, bit_depth, channels, bitrate, file_size, file_format,
            has_artwork, replaygain_track_gain, replaygain_album_gain,
            cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy,
            analysis_failed_reason, file_modified, created_at, modified_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
         {conflict_target}
            album_id=excluded.album_id,
            title=excluded.title, artist=excluded.artist, album=excluded.album,
            album_artist=excluded.album_artist, composer=excluded.composer,
            track_number=excluded.track_number, disc_number=excluded.disc_number,
            year=excluded.year, genre=excluded.genre, raw_genre_names=excluded.raw_genre_names,
            duration=excluded.duration,
            sample_rate=excluded.sample_rate, bit_depth=excluded.bit_depth, channels=excluded.channels, bitrate=excluded.bitrate,
            file_size=excluded.file_size, file_format=excluded.file_format,
            has_artwork=excluded.has_artwork, modified_at=datetime('now'),
            replaygain_track_gain=excluded.replaygain_track_gain,
            replaygain_album_gain=excluded.replaygain_album_gain,
            cue_parent_id=excluded.cue_parent_id,
            cue_offset=excluded.cue_offset,
            artwork_key=excluded.artwork_key,
            bpm=excluded.bpm, musical_key=excluded.musical_key, energy=excluded.energy,
            analysis_failed_reason=excluded.analysis_failed_reason,
            file_modified=excluded.file_modified
         RETURNING id",
    ))
    .bind(&track.id)
    .bind(&track.file_path)
    .bind(&track.album_id)
    .bind(&track.title)
    .bind(&track.artist)
    .bind(&track.album)
    .bind(&track.album_artist)
    .bind(&track.composer)
    .bind(track.track_number as i64)
    .bind(track.disc_number as i64)
    .bind(track.year as i64)
    .bind(&track.genre)
    .bind(&track.raw_genre_names)
    .bind(track.duration)
    .bind(track.sample_rate as i64)
    .bind(track.bit_depth as i64)
    .bind(track.channels as i64)
    .bind(track.bitrate as i64)
    .bind(track.file_size as i64)
    .bind(&track.file_format)
    .bind(if track.has_artwork { 1 } else { 0 })
    .bind(track.replaygain_track_gain)
    .bind(track.replaygain_album_gain)
    .bind(track.cue_parent_id.as_deref())
    .bind(track.cue_offset)
    .bind(track.artwork_key.as_deref())
    .bind(track.bpm)
    .bind(&track.musical_key)
    .bind(track.energy)
    .bind(track.analysis_failed_reason.as_deref())
    .bind(track.file_modified)
    .fetch_one(executor)
    .await
    .map_err(|e| format!("Failed to upsert track: {e}"))?;

    track.id = row.0.clone();
    Ok(row.0)
}

pub async fn delete_track(pool: &SqlitePool, id: &str) -> Result<(), String> {
    delete_tracks(pool, &[id.to_string()]).await?;
    Ok(())
}

pub async fn delete_tracks(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Collect file paths before deletion
    let mut file_paths = Vec::new();
    let mut all_ids = ids.to_vec();

    // Find and include CUE child tracks whose parent is among the deleted tracks
    let cue_child_ids: Vec<String> = {
        let params: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let placeholders = params.join(",");
        let sql = format!("SELECT id FROM tracks WHERE cue_parent_id IN ({})", placeholders);
        let mut q = sqlx::query_scalar::<_, String>(&sql);
        for id in ids {
            q = q.bind(id);
        }
        q.fetch_all(pool).await.unwrap_or_default()
    };
    all_ids.extend(cue_child_ids);
    all_ids.dedup();

    for id in &all_ids {
        if let Ok(Some(track)) = get_track(pool, id).await {
            file_paths.push(track.file_path);
        }
    }

    // Build parameterized IN clause
    let params: Vec<String> = all_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
    let placeholders = params.join(",");

    let play_history_sql = format!("DELETE FROM play_history WHERE track_id IN ({})", placeholders);
    let favorites_sql = format!("DELETE FROM favorites WHERE track_id IN ({})", placeholders);
    let playlist_tracks_sql = format!("DELETE FROM playlist_tracks WHERE track_id IN ({})", placeholders);
    let tracks_sql = format!("DELETE FROM tracks WHERE id IN ({})", placeholders);

    let mut query = sqlx::query(&play_history_sql);
    for id in &all_ids {
        query = query.bind(id);
    }
    query.execute(pool).await.map_err(|e| format!("Failed to delete play history: {e}"))?;

    let mut query = sqlx::query(&favorites_sql);
    for id in &all_ids {
        query = query.bind(id);
    }
    query.execute(pool).await.map_err(|e| format!("Failed to delete favorites: {e}"))?;

    let mut query = sqlx::query(&playlist_tracks_sql);
    for id in &all_ids {
        query = query.bind(id);
    }
    query.execute(pool).await.map_err(|e| format!("Failed to delete playlist_tracks: {e}"))?;

    let mut query = sqlx::query(&tracks_sql);
    for id in &all_ids {
        query = query.bind(id);
    }
    query.execute(pool).await.map_err(|e| format!("Failed to delete tracks: {e}"))?;

    Ok(file_paths)
}

pub async fn get_all_tracks(pool: &SqlitePool) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
         "SELECT id, file_path, title, artist, album, album_artist, composer,
           track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
           bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain, cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks ORDER BY title"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_track(pool: &SqlitePool, id: &str) -> Result<Option<Track>, String> {
    let row = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain, cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch track: {e}"))?;
    Ok(row.map(|r| r.into()))
}

pub async fn record_play(pool: &SqlitePool, track_id: &str) -> Result<i64, String> {
    // Single statement: a separate SELECT last_insert_rowid() may run on a
    // different pooled connection (max 15) and return the wrong id, silently
    // misattributing the play on finish.
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO play_history (track_id) VALUES (?) RETURNING id",
    )
    .bind(track_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to record play: {e}"))?;
    Ok(row.0)
}

pub async fn finish_play(
    pool: &SqlitePool,
    history_id: i64,
    elapsed_secs: f64,
    duration: f64,
    end_reason: Option<&str>,
) -> Result<(), String> {
    let track_id: Option<(String,)> = sqlx::query_as(
        "SELECT track_id FROM play_history WHERE id = ?"
    )
    .bind(history_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch track_id: {e}"))?;
    let Some((track_id,)) = track_id else {
        return Ok(());
    };
    // Discard: under 10s is never persisted (Airmedy parity).
    if elapsed_secs < 10.0 {
        sqlx::query("DELETE FROM play_history WHERE id = ?")
            .bind(history_id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to discard short play: {e}"))?;
        return Ok(());
    }
    let reason = match end_reason {
        Some("completed") | Some("skipped") | Some("stopped") => end_reason,
        _ => None,
    };
    sqlx::query("UPDATE play_history SET played_secs = ?, end_reason = COALESCE(?, end_reason) WHERE id = ?")
        .bind(elapsed_secs)
        .bind(reason)
        .bind(history_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to update play duration: {e}"))?;
    // Qualified play (Airmedy scrobble rule): duration>=30s AND
    // (position>=half OR position>=240s). Only these bump play_count.
    let qualified = duration >= 30.0 && (elapsed_secs >= duration / 2.0 || elapsed_secs >= 240.0);
    if qualified {
        sqlx::query("UPDATE tracks SET play_count = play_count + 1, last_played_at = datetime('now') WHERE id = ?")
            .bind(&track_id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to update play count: {e}"))?;
    } else {
        sqlx::query("UPDATE tracks SET last_played_at = datetime('now') WHERE id = ?")
            .bind(&track_id)
            .execute(pool)
            .await
            .map_err(|e| format!("Failed to update last played: {e}"))?;
    }
    Ok(())
}

pub async fn clear_play_history(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM play_history")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear play history: {e}"))?;
    sqlx::query("UPDATE tracks SET play_count = 0, last_played_at = NULL")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to reset play counts: {e}"))?;
    Ok(())
}

pub async fn get_recently_played_tracks(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain,
          cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
          WHERE play_count > 0
          ORDER BY last_played_at DESC
          LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch recently played: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_most_listened_tracks(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain,
          cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
          WHERE play_count > 0
          ORDER BY play_count DESC
          LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch most listened: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_least_listened_tracks(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain,
          cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
          ORDER BY play_count ASC, COALESCE(last_played_at, '1970-01-01') ASC
          LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch least listened: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: i32,
    pub track_count: i64,
    pub duration: f64,
    pub artwork_key: Option<String>,
    pub first_track_id: String,
    pub is_hi_res: bool,
}

pub async fn get_albums(pool: &SqlitePool) -> Result<Vec<Album>, String> {
    let rows = sqlx::query_as::<_, Album>(
        "SELECT 
            album_id as id,
            album as title,
            COALESCE(NULLIF(album_artist, ''), artist) as artist,
            CAST(MIN(NULLIF(year, 0)) AS INTEGER) as year,
            COUNT(id) as track_count,
            TOTAL(duration) as duration,
            MAX(artwork_key) as artwork_key,
            MIN(id) as first_track_id,
            CAST(MAX(CASE WHEN sample_rate > 48000 OR bit_depth > 16 THEN 1 ELSE 0 END) AS BOOLEAN) as is_hi_res
         FROM tracks
         WHERE album != '' AND album_id != ''
         GROUP BY album_id
         ORDER BY title COLLATE NOCASE ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to get albums: {e}"))?;
    Ok(rows)
}

pub async fn get_album_tracks(pool: &SqlitePool, album_id: &str) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT * FROM tracks WHERE album_id = ?1 OR album = ?1 ORDER BY disc_number ASC, track_number ASC"
    )
    .bind(album_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to get album tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListeningStats {
    pub total_plays: i64,
    pub total_listening_minutes: f64,
}

pub async fn get_listening_stats(pool: &SqlitePool) -> Result<ListeningStats, String> {
    let row = sqlx::query(
        "SELECT COUNT(*) as total_plays,
                COALESCE(SUM(played_secs), 0) / 60.0 as total_listening_minutes
         FROM play_history"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch listening stats: {e}"))?;

    Ok(ListeningStats {
        total_plays: row.get(0),
        total_listening_minutes: row.get(1),
    })
}

pub async fn get_track_by_path(pool: &SqlitePool, path: &str) -> Result<Option<Track>, String> {
    let row = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain, cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks WHERE file_path = ? AND cue_parent_id IS NULL"
    )
    .bind(path)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch track by path: {e}"))?;
    Ok(row.map(|r| r.into()))
}

#[derive(sqlx::FromRow)]
pub(crate) struct TrackRow {
    id: String,
    file_path: String,
    #[sqlx(default)]
    album_id: String,
    title: String,
    artist: String,
    album: String,
    album_artist: String,
    composer: String,
    track_number: i32,
    disc_number: i32,
    year: i32,
    genre: String,
    raw_genre_names: String,
    duration: f64,
    sample_rate: i32,
    bit_depth: i32,
    channels: i32,
    bitrate: i32,
    file_size: i64,
    file_format: String,
    has_artwork: bool,
    replaygain_track_gain: f64,
    replaygain_album_gain: f64,
    cue_parent_id: Option<String>,
    cue_offset: f64,
    artwork_key: Option<String>,
    bpm: f64,
    musical_key: String,
    energy: f64,
    analysis_failed_reason: Option<String>,
    file_modified: i64,
}

impl From<TrackRow> for Track {
    fn from(r: TrackRow) -> Self {
        Track {
            id: r.id,
            file_path: r.file_path,
            title: r.title,
            artist: r.artist,
            album: r.album,
            album_artist: r.album_artist,
            album_id: r.album_id,
            composer: r.composer,
            track_number: r.track_number as u32,
            disc_number: r.disc_number as u32,
            year: r.year,
            genre: r.genre,
            raw_genre_names: r.raw_genre_names,
            duration: r.duration,
            sample_rate: r.sample_rate as u32,
            bit_depth: r.bit_depth as u32,
            channels: r.channels as u32,
            bitrate: r.bitrate as u32,
            file_size: r.file_size as u64,
            file_format: r.file_format,
            has_artwork: r.has_artwork,
            replaygain_track_gain: r.replaygain_track_gain,
            replaygain_album_gain: r.replaygain_album_gain,
            cue_parent_id: r.cue_parent_id,
            cue_offset: r.cue_offset,
            artwork_key: r.artwork_key,
            bpm: r.bpm,
            musical_key: r.musical_key,
            energy: r.energy,
            analysis_failed_reason: r.analysis_failed_reason,
            file_modified: r.file_modified,
            matched_lyric: None,
        }
    }
}

pub async fn get_radio_stations(pool: &SqlitePool) -> Result<Vec<crate::radio::RadioStation>, String> {
    #[derive(sqlx::FromRow)]
    struct RadioRow {
        id: String,
        name: String,
        url: String,
        genre: String,
        logo_url: String,
        country: String,
        language: String,
        bitrate: i64,
    }
    let rows: Vec<RadioRow> = sqlx::query_as(
        "SELECT id, name, url, genre, logo_url, country, language, bitrate FROM radio_stations ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch radio stations: {e}"))?;
    Ok(rows.into_iter().map(|r| crate::radio::RadioStation {
        id: r.id,
        name: r.name,
        url: r.url,
        genre: r.genre,
        logo_url: r.logo_url,
        country: r.country,
        language: r.language,
        bitrate: r.bitrate,
    }).collect())
}

pub async fn add_radio_station(pool: &SqlitePool, station: &crate::radio::RadioStation) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO radio_stations (id, name, url, genre, logo_url, country, language, bitrate) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&station.id)
    .bind(&station.name)
    .bind(&station.url)
    .bind(&station.genre)
    .bind(&station.logo_url)
    .bind(&station.country)
    .bind(&station.language)
    .bind(station.bitrate)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to add radio station: {e}"))?;
    Ok(())
}

pub async fn delete_radio_station(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM radio_stations WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete radio station: {e}"))?;
    Ok(())
}

pub async fn update_radio_station(pool: &SqlitePool, station: &crate::radio::RadioStation) -> Result<(), String> {
    sqlx::query(
        "UPDATE radio_stations SET name=?, url=?, genre=?, logo_url=?, country=?, language=?, bitrate=? WHERE id=?"
    )
    .bind(&station.name)
    .bind(&station.url)
    .bind(&station.genre)
    .bind(&station.logo_url)
    .bind(&station.country)
    .bind(&station.language)
    .bind(station.bitrate)
    .bind(&station.id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update radio station: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricRow {
    pub track_id: String,
    pub content: String,
    pub source: String,
    pub meta_content: String,
    pub meta_source: String,
}

pub async fn get_lyrics(pool: &SqlitePool, track_id: &str) -> Result<Option<LyricRow>, String> {
    let row = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT track_id, content, source, meta_content, meta_source FROM lyrics WHERE track_id = ?"
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch lyrics: {e}"))?;
    Ok(row.map(|(track_id, content, source, meta_content, meta_source)| LyricRow {
        track_id, content, source, meta_content, meta_source,
    }))
}

pub async fn get_all_lyrics(pool: &SqlitePool) -> Result<Vec<LyricRow>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT track_id, content, source, meta_content, meta_source FROM lyrics ORDER BY track_id"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch all lyrics: {e}"))?;
    Ok(rows.into_iter().map(|(track_id, content, source, meta_content, meta_source)| LyricRow {
        track_id, content, source, meta_content, meta_source,
    }).collect())
}

pub async fn upsert_lyrics<'e>(executor: impl Executor<'e, Database = sqlx::Sqlite>, track_id: &str, content: &str, source: &str, meta_content: &str, meta_source: &str) -> Result<(), String>
{
    sqlx::query(
        "INSERT INTO lyrics (track_id, content, source, meta_content, meta_source, updated_at)
         VALUES (?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
         content=excluded.content, source=excluded.source,
         meta_content=excluded.meta_content, meta_source=excluded.meta_source,
         updated_at=datetime('now')"
    )
    .bind(track_id)
    .bind(content)
    .bind(source)
    .bind(meta_content)
    .bind(meta_source)
    .execute(executor)
    .await
    .map_err(|e| format!("Failed to upsert lyrics: {e}"))?;
    Ok(())
}

pub async fn delete_lyrics(pool: &SqlitePool, track_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM lyrics WHERE track_id = ?")
        .bind(track_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete lyrics: {e}"))?;
    Ok(())
}

pub async fn get_artist_artwork_key(pool: &SqlitePool, name: &str) -> Result<Option<String>, String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT artwork_key FROM artists WHERE name = ? AND artwork_key IS NOT NULL AND artwork_key != ''"
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch artist artwork key: {e}"))?;
    Ok(row.map(|r| r.0))
}

pub async fn set_artist_artwork_key(pool: &SqlitePool, name: &str, key: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO artists (name, artwork_key) VALUES (?, ?) ON CONFLICT(name) DO UPDATE SET artwork_key=excluded.artwork_key"
    )
    .bind(name)
    .bind(key)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to set artist artwork key: {e}"))?;
    Ok(())
}

pub async fn mark_artist_artwork_checked(pool: &SqlitePool, name: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO artists (name, artwork_checked) VALUES (?, 1)
         ON CONFLICT(name) DO UPDATE SET artwork_checked = excluded.artwork_checked"
    )
    .bind(name)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to mark artist artwork checked: {e}"))?;
    Ok(())
}

pub async fn get_artist_artwork_state(pool: &SqlitePool, name: &str) -> Result<(Option<String>, bool), String> {
    let row: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT artwork_key, COALESCE(artwork_checked, 0) FROM artists WHERE name = ?"
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch artist artwork state: {e}"))?;
    Ok(row.map(|(k, c)| (k, c != 0)).unwrap_or((None, false)))
}

/// (mtime, size) stamps for incremental scans, keyed by file path.
/// Size is compared alongside mtime so same-second rewrites and tag-only
/// touches with preserved mtimes are still re-parsed.
pub async fn get_track_stamps_for_folder(pool: &SqlitePool, folder: &str) -> Result<std::collections::HashMap<String, (i64, i64)>, String> {
    let sep = std::path::MAIN_SEPARATOR;
    let with_sep = format!("{}{}", folder, sep);
    let escaped = with_sep.replace('%', "^%").replace('_', "^_");
    let like_pattern = format!("{}%", escaped);
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        // Note: CUE children share their parent's file_path, so the HashMap
        // below intentionally collapses them into one entry — they have the
        // same physical file and therefore the same stamp. Not a bug.
        "SELECT file_path, file_modified, file_size FROM tracks WHERE file_path = ? OR file_path LIKE ? ESCAPE '^'"
    )
    .bind(folder)
    .bind(&like_pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch track stamps: {e}"))?;
    Ok(rows.into_iter().map(|(p, m, s)| (p, (m, s))).collect())
}

pub async fn get_unique_artists(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT artist FROM tracks
         WHERE artist != ''
         AND NOT EXISTS (
             SELECT 1 FROM artists a WHERE a.name = tracks.artist
             AND ((a.artwork_key IS NOT NULL AND a.artwork_key != '') OR a.artwork_checked = 1)
         )
         ORDER BY artist"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch artist list: {e}"))?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_all_genres(pool: &SqlitePool) -> Result<Vec<crate::library::track::Genre>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT g.id, g.name, g.normalization_key, COUNT(tg.track_id) as track_count
         FROM genres g
         LEFT JOIN track_genres tg ON tg.genre_id = g.id
         GROUP BY g.id
         ORDER BY g.name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch genres: {e}"))?;
    Ok(rows.into_iter().map(|(id, name, normalization_key, track_count)| {
        crate::library::track::Genre { id, name, normalization_key, track_count }
    }).collect())
}

pub async fn get_tracks_by_genre_id(pool: &SqlitePool, genre_id: &str) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT t.id, t.file_path, t.title, t.artist, t.album, t.album_artist, t.composer,
         t.track_number, t.disc_number, t.year, t.genre, t.raw_genre_names, t.duration, t.sample_rate,
         t.bit_depth, t.channels, t.bitrate, t.file_size, t.file_format, t.has_artwork, t.replaygain_track_gain,
         t.replaygain_album_gain, t.cue_parent_id, t.cue_offset, t.artwork_key,
         t.bpm, t.musical_key, t.energy, t.analysis_failed_reason, t.file_modified
         FROM tracks t
         JOIN track_genres tg ON tg.track_id = t.id
         WHERE tg.genre_id = ?
         GROUP BY t.id
         ORDER BY t.year DESC, t.album, t.disc_number, t.track_number"
    )
    .bind(genre_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch tracks by genre: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn upsert_genre(conn: &mut sqlx::SqliteConnection, genre: &crate::library::track::Genre) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO genres (id, name, normalization_key) VALUES (?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET normalization_key = excluded.normalization_key"
    )
    .bind(&genre.id)
    .bind(&genre.name)
    .bind(&genre.normalization_key)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("Failed to upsert genre: {e}"))?;
    Ok(())
}

pub async fn set_track_genres(conn: &mut sqlx::SqliteConnection, track_id: &str, genre_ids: &[String]) -> Result<(), String> {
    sqlx::query("DELETE FROM track_genres WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to delete track genres: {e}"))?;
    for (i, gid) in genre_ids.iter().enumerate() {
        sqlx::query("INSERT INTO track_genres (track_id, genre_id, position) VALUES (?, ?, ?)")
            .bind(track_id)
            .bind(gid)
            .bind(i as i64)
            .execute(&mut *conn)
            .await
            .map_err(|e| format!("Failed to insert track genre: {e}"))?;
    }
    Ok(())
}

pub async fn get_tracks_needing_analysis(pool: &SqlitePool) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
         track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
         bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain,
         replaygain_album_gain, cue_parent_id, cue_offset, artwork_key,
         bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
         WHERE (bpm = 0 AND (musical_key IS NULL OR musical_key = ''))
           AND duration >= 3.0 AND cue_parent_id IS NULL
           AND (analysis_failed_reason IS NULL OR analysis_failed_reason = '')
         ORDER BY duration DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch unanalyzed tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn update_track_analysis(pool: &SqlitePool, track: &Track) -> Result<(), String> {
    sqlx::query(
        "UPDATE tracks SET bpm = ?, musical_key = ?, energy = ? WHERE id = ?"
    )
    .bind(track.bpm)
    .bind(&track.musical_key)
    .bind(track.energy)
    .bind(&track.id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update analysis: {e}"))?;
    Ok(())
}

pub async fn get_tracks_needing_genre(pool: &SqlitePool, limit: Option<usize>) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
         track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
         bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain,
         replaygain_album_gain, cue_parent_id, cue_offset, artwork_key,
         bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
         WHERE (raw_genre_names = '' OR raw_genre_names IS NULL)
         AND (genre = '' OR genre IS NULL)
         AND artist != '' AND title != '' AND genre_checked = 0
         ORDER BY RANDOM() LIMIT ?"
    )
    .bind(limit.map(|l| l as i64).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch ungenred tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_tracks_by_normalized_genre(pool: &SqlitePool, genre_name: &str) -> Result<Vec<Track>, String> {
    let nk = crate::library::track::normalize_genre_name(genre_name);
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT t.id, t.file_path, t.title, t.artist, t.album, t.album_artist, t.composer,
         t.track_number, t.disc_number, t.year, t.genre, t.raw_genre_names, t.duration,
         t.sample_rate, t.bit_depth, t.channels, t.bitrate, t.file_size, t.file_format, t.has_artwork,
         t.replaygain_track_gain, t.replaygain_album_gain, t.cue_parent_id, t.cue_offset,
         t.artwork_key, t.bpm, t.musical_key, t.energy, t.analysis_failed_reason, t.file_modified FROM tracks t
         JOIN track_genres tg ON tg.track_id = t.id
         JOIN genres g ON g.id = tg.genre_id
         WHERE g.normalization_key = ?
         ORDER BY t.year DESC, t.album, t.disc_number, t.track_number"
    )
    .bind(&nk)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch tracks by normalized genre: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_tracks_by_artist_shuffled(pool: &SqlitePool, artist: &str) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
         track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
         bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain,
         replaygain_album_gain, cue_parent_id, cue_offset, artwork_key,
         bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
         WHERE artist = ?
         ORDER BY RANDOM()"
    )
    .bind(artist)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch tracks by artist: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn get_tracks_shuffled(pool: &SqlitePool, limit: i64) -> Result<Vec<Track>, String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
         track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
         bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain,
         replaygain_album_gain, cue_parent_id, cue_offset, artwork_key,
         bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks
         ORDER BY RANDOM() LIMIT ?"
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch shuffled tracks: {e}"))?;
    Ok(rows.into_iter().map(|r| r.into()).collect())
}

pub async fn update_track_genre_from_lastfm(pool: &SqlitePool, track_id: &str, genre: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| format!("Failed to begin tx: {e}"))?;

    sqlx::query("UPDATE tracks SET genre = ?, raw_genre_names = ?, genre_checked = 1 WHERE id = ?")
        .bind(genre)
        .bind(genre)
        .bind(track_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update genre on track: {e}"))?;

    if !genre.is_empty() {
        use crate::library::track::{normalize_genre_name, split_genre_string, Genre as GenreModel};
        let genre_names = split_genre_string(genre);
        let mut genre_ids: Vec<String> = Vec::new();
        for name in &genre_names {
            let nk = normalize_genre_name(name);
            let gid = format!("genre-{}", nk);
            let gm = GenreModel {
                id: gid.clone(),
                name: name.clone(),
                normalization_key: nk,
                track_count: 0,
            };
            let _ = upsert_genre(&mut *tx, &gm).await;
            genre_ids.push(gid);
        }
        let _ = set_track_genres(&mut *tx, track_id, &genre_ids).await;
    }

    tx.commit().await.map_err(|e| format!("Failed to commit genre update: {e}"))?;
    Ok(())
}

pub async fn get_genre_coverage(pool: &SqlitePool) -> Result<(i64, i64, i64), String> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NULL")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let tagged: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NULL AND (genre != '' AND genre IS NOT NULL OR raw_genre_names != '' AND raw_genre_names IS NOT NULL)")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let checked_missing: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NULL AND (genre = '' OR genre IS NULL) AND (raw_genre_names = '' OR raw_genre_names IS NULL) AND genre_checked = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok((tagged.0, total.0, checked_missing.0))
}

pub async fn mark_track_genre_checked(pool: &SqlitePool, track_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE tracks SET raw_genre_names = '', genre_checked = 1 WHERE id = ?")
        .bind(track_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to mark track genre checked: {e}"))?;
    Ok(())
}

/// Re-arm tracks that Last.fm enrichment marked checked-but-not-found so a
/// "retry missing" run searches them again. Tracks with an actual genre
/// (from tags or a previous fetch) are left alone.
pub async fn reset_genre_not_found(pool: &SqlitePool) -> Result<usize, String> {
    let result = sqlx::query(
        "UPDATE tracks SET genre_checked = 0
         WHERE genre_checked = 1 AND genre = '' AND raw_genre_names = ''",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to reset genre not-found markers: {e}"))?;
    Ok(result.rows_affected() as usize)
}

/// Re-arm artists whose artwork fetch found nothing so the next batch retries
/// them. Artists with a stored artwork key are left alone.
pub async fn reset_artist_artwork_not_found(pool: &SqlitePool) -> Result<usize, String> {
    let result = sqlx::query(
        "UPDATE artists SET artwork_checked = 0
         WHERE artwork_checked = 1 AND (artwork_key IS NULL OR artwork_key = '')",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to reset artist artwork not-found markers: {e}"))?;
    Ok(result.rows_affected() as usize)
}

/// Re-arm tracks whose lyrics fetch recorded "not-found" so the next batch
/// searches them again. Tracks with content are left alone.
pub async fn reset_lyrics_not_found(pool: &SqlitePool) -> Result<usize, String> {
    let result = sqlx::query(
        "UPDATE lyrics SET source = '', content = '' WHERE source = 'not-found'",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to reset lyrics not-found markers: {e}"))?;
    Ok(result.rows_affected() as usize)
}

pub async fn repair_artwork_status(pool: &SqlitePool) -> Result<(), String> {
    let rows = sqlx::query_as::<_, TrackRow>(
        "SELECT id, file_path, title, artist, album, album_artist, composer,
          track_number, disc_number, year, genre, raw_genre_names, duration, sample_rate,
          bit_depth, channels, bitrate, file_size, file_format, has_artwork, replaygain_track_gain, replaygain_album_gain, cue_parent_id, cue_offset, artwork_key, bpm, musical_key, energy, analysis_failed_reason, file_modified FROM tracks WHERE has_artwork = 0"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for r in rows {
        let path = Path::new(&r.file_path);
        if path.exists() {
            if let Ok(tagged_file) = lofty::read_from_path(path) {
                let has_art = tagged_file
                    .tags()
                    .iter()
                    .any(|tag| !tag.pictures().is_empty());
                if has_art {
                    let _ = sqlx::query("UPDATE tracks SET has_artwork = 1 WHERE id = ?")
                        .bind(&r.id)
                        .execute(pool)
                        .await;
                }
            }
        }
    }

    // Back-fill artwork_key from other tracks in same album (handles pre-migration data)
    let _ = sqlx::query(
        "UPDATE tracks SET artwork_key = (
            SELECT t2.artwork_key FROM tracks t2
            WHERE t2.album = tracks.album
              AND t2.artwork_key IS NOT NULL AND t2.artwork_key != ''
            LIMIT 1
        ) WHERE artwork_key IS NULL AND album IN (
            SELECT album FROM tracks
            WHERE artwork_key IS NOT NULL AND artwork_key != '' AND album != ''
        )"
    )
    .execute(pool)
    .await;

    // Clean corrupted artwork_keys from old multi-column SELECT bug
    let _ = sqlx::query(
        "UPDATE tracks SET artwork_key = NULL WHERE artwork_key LIKE '%,%'"
    )
    .execute(pool)
    .await;

    // Back-fill raw_genre_names from the old genre column for pre-migration tracks
    let _ = sqlx::query(
        "UPDATE tracks SET raw_genre_names = genre WHERE raw_genre_names = '' AND genre != ''"
    )
    .execute(pool)
    .await;

    // Back-fill track_genres for tracks that have raw_genre_names but no genre links
    let unlinked: Vec<(String, String)> = sqlx::query_as(
        "SELECT t.id, t.raw_genre_names FROM tracks t
         WHERE t.raw_genre_names != ''
         AND NOT EXISTS (SELECT 1 FROM track_genres tg WHERE tg.track_id = t.id)"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (track_id, raw) in &unlinked {
        let names = crate::library::track::split_genre_string(raw);
        let mut gids: Vec<String> = Vec::new();
        for name in &names {
            let nk = crate::library::track::normalize_genre_name(name);
            let gid = format!("genre-{}", nk);
            let genre = crate::library::track::Genre {
                id: gid.clone(),
                name: name.clone(),
                normalization_key: nk,
                track_count: 0,
            };
            let _ = upsert_genre(&mut *pool.acquire().await.unwrap_or_else(|_| panic!("DB error")), &genre).await;
            gids.push(gid);
        }
        let _ = set_track_genres(&mut *pool.acquire().await.unwrap_or_else(|_| panic!("DB error")), track_id, &gids).await;
    }

    // Performance indexes for faster queries (Phase 1 optimization)
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_album_artist ON tracks(album_artist)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_genre ON tracks(genre)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_year ON tracks(year)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_duration ON tracks(duration)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_play_count ON tracks(play_count DESC)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_artwork_key ON tracks(artwork_key) WHERE artwork_key IS NOT NULL"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_playlist_tracks_position ON playlist_tracks(playlist_id, position)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_file_path ON tracks(file_path)"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_cue_parent ON tracks(cue_parent_id) WHERE cue_parent_id IS NOT NULL"
    )
    .execute(pool)
    .await;

    // Smart Mix indexes
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_bpm ON tracks(bpm) WHERE bpm > 0.0"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_energy ON tracks(energy) WHERE energy > 0.0"
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Migration failed: {e}"))?;

    // (Schema moved to run_migrations above; the repair task below only
    // heals data, never creates these tables.)

    // Normalization + EQ global settings defaults (Airmedy parity: target -14 LUFS).
    for (key, value) in [
        ("normalization_mode", "track"),
        ("normalization_target_lufs", "-14"),
        ("normalization_prevent_clip", "1"),
        ("eq_preamp", "0"),
        ("eq_enabled", "1"),
    ] {
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_track(id: &str, path: &str, cue_parent: Option<&str>) -> Track {
        Track {
            id: id.to_string(),
            file_path: path.to_string(),
            title: format!("T-{id}"),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            album_id: String::new(),
            composer: String::new(),
            track_number: 0,
            disc_number: 0,
            year: 0,
            genre: String::new(),
            raw_genre_names: String::new(),
            duration: 0.0,
            sample_rate: 0,
            bit_depth: 0,
            channels: 0,
            bitrate: 0,
            file_size: 0,
            file_format: "flac".to_string(),
            has_artwork: false,
            replaygain_track_gain: 0.0,
            replaygain_album_gain: 0.0,
            cue_parent_id: cue_parent.map(|s| s.to_string()),
            cue_offset: 0.0,
            artwork_key: None,
            bpm: 0.0,
            musical_key: String::new(),
            energy: 0.0,
            analysis_failed_reason: None,
            file_modified: 0,
            matched_lyric: None,
        }
    }

    fn temp_db_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eiryn-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn cue_children_persist_with_stable_ids() {
        let dir = temp_db_dir();
        let db_path = dir.join("test.db");
        let (pool, migrated) = init_pool(&db_path).await.unwrap();
        assert!(!migrated, "fresh database must not report a legacy migration");

        let path = dir.join("album.flac").to_string_lossy().to_string();
        let parent_id = uuid::Uuid::new_v4().to_string();
        let mut parent = test_track(&parent_id, &path, None);
        upsert_track(&pool, &mut parent).await.unwrap();
        assert_eq!(parent.id, parent_id);

        // Children share the parent's file_path and cue_parent_id — the exact
        // shape that collapsed under the old UNIQUE(file_path) schema.
        let mut c1 = test_track(&format!("{parent_id}-cue-01"), &path, Some(&parent_id));
        let mut c2 = test_track(&format!("{parent_id}-cue-02"), &path, Some(&parent_id));
        upsert_track(&pool, &mut c1).await.unwrap();
        upsert_track(&pool, &mut c2).await.unwrap();

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE file_path = ?")
            .bind(&path)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 3, "parent + 2 children must coexist");

        // Rescan: a fresh UUID for the same path must resolve to the stored id.
        let mut parent2 = test_track(&uuid::Uuid::new_v4().to_string(), &path, None);
        upsert_track(&pool, &mut parent2).await.unwrap();
        assert_eq!(parent2.id, parent_id, "id must be stable across rescans");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks")
                .fetch_one(&pool)
                .await
                .unwrap(),
            3,
            "no duplicate parent rows after rescan"
        );

        // Children keep their deterministic ids and update in place.
        let mut c1b = test_track(&format!("{parent_id}-cue-01"), &path, Some(&parent_id));
        upsert_track(&pool, &mut c1b).await.unwrap();
        assert_eq!(c1b.id, format!("{parent_id}-cue-01"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks")
                .fetch_one(&pool)
                .await
                .unwrap(),
            3
        );

        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn first_pool_statement_upserts_without_stale_schema() {
        // Regression: pooled connections used to open lazily DURING migrations
        // and cache a pre-index schema, making the first `ON CONFLICT` upsert
        // fail with "ON CONFLICT clause does not match any PRIMARY KEY or
        // UNIQUE constraint". The pool must only open after migrations finish.
        let dir = temp_db_dir();
        let db_path = dir.join("test.db");
        let (pool, migrated) = init_pool(&db_path).await.unwrap();
        assert!(!migrated, "fresh database must not report a legacy migration");
        let path = dir.join("album.flac").to_string_lossy().to_string();
        let mut parent = test_track(&uuid::Uuid::new_v4().to_string(), &path, None);
        let r = upsert_track(&pool, &mut parent).await;
        assert!(r.is_ok(), "first pool statement failed: {r:?}");
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn legacy_unique_schema_is_rebuilt_and_healed() {
        let dir = temp_db_dir();
        let db_path = dir.join("legacy.db");

        // Build a legacy database: old tracks table with UNIQUE(file_path),
        // one healthy row and one row corrupted by the old CUE collapse.
        {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db_path).create_if_missing(true))
                .await
                .unwrap();
            sqlx::query(
                "CREATE TABLE tracks (
                    id TEXT PRIMARY KEY,
                    file_path TEXT NOT NULL UNIQUE,
                    title TEXT NOT NULL DEFAULT '',
                    artist TEXT NOT NULL DEFAULT '',
                    album TEXT NOT NULL DEFAULT '',
                    album_artist TEXT NOT NULL DEFAULT '',
                    composer TEXT NOT NULL DEFAULT '',
                    track_number INTEGER DEFAULT 0,
                    disc_number INTEGER DEFAULT 0,
                    year INTEGER DEFAULT 0,
                    genre TEXT NOT NULL DEFAULT '',
                    duration REAL NOT NULL DEFAULT 0,
                    sample_rate INTEGER NOT NULL DEFAULT 0,
                    bit_depth INTEGER NOT NULL DEFAULT 0,
                    bitrate INTEGER NOT NULL DEFAULT 0,
                    file_size INTEGER NOT NULL DEFAULT 0,
                    file_format TEXT NOT NULL DEFAULT '',
                    has_artwork INTEGER NOT NULL DEFAULT 0,
                    replaygain_track_gain REAL NOT NULL DEFAULT 0.0,
                    replaygain_album_gain REAL NOT NULL DEFAULT 0.0,
                    cue_parent_id TEXT,
                    cue_offset REAL NOT NULL DEFAULT 0.0,
                    file_modified INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    modified_at TEXT NOT NULL DEFAULT (datetime('now'))
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO tracks (id, file_path, title) VALUES ('parent-1', 'C:\\music\\a.flac', 'healthy')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO tracks (id, file_path, title, cue_parent_id) VALUES ('parent-2', 'C:\\music\\b.flac', 'tainted', 'phantom-id')",
            )
            .execute(&pool)
            .await
            .unwrap();
            drop(pool);
        }

        let (pool, migrated) = init_pool(&db_path).await.unwrap();
        assert!(migrated, "legacy UNIQUE schema must be detected and rebuilt");

        // UNIQUE constraint gone from the rebuilt table.
        let (sql,): (String,) = sqlx::query_as(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'tracks'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!sql.contains("UNIQUE"));

        // Corrupted legacy row removed; healthy row preserved.
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks WHERE cue_parent_id IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT title FROM tracks WHERE id = 'parent-1'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "healthy"
        );

        // Parent + children now persist on the migrated schema.
        let path = "C:\\music\\b.flac".to_string();
        let mut parent = test_track(&uuid::Uuid::new_v4().to_string(), &path, None);
        upsert_track(&pool, &mut parent).await.unwrap();
        let child_id = format!("{}-cue-01", parent.id);
        let mut child = test_track(&child_id, &path, Some(&parent.id));
        upsert_track(&pool, &mut child).await.unwrap();
        assert_eq!(child.id, child_id);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tracks WHERE file_path = 'C:\\music\\b.flac'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );

        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
