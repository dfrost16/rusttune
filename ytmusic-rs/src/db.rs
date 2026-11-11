use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMeta {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub thumb_url: String,
    pub local_path: Option<String>,
    pub file_bytes: u64,
    pub is_favorite: bool,
}

#[derive(Debug, Clone)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub track_count: usize,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String, // "dark" | "light"
    pub radio_autoplay: bool,
    pub swap_ab: bool,
    #[serde(default = "default_true")]
    pub web_server: bool,
    #[serde(default = "default_true")]
    pub web_pin_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            radio_autoplay: true,
            swap_ab: false,
            web_server: true,
            web_pin_enabled: true,
        }
    }
}

pub fn load_settings(path: &Path) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(path: &Path, s: &AppSettings) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(txt) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, txt);
    }
}

pub struct MusicDb {
    conn: Connection,
    cache_dir: PathBuf,
}

impl MusicDb {
    pub fn open(db_path: &Path, cache_dir: &Path) -> Result<Self, String> {
        if let Some(p) = db_path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=3000;");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tracks (
                video_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL DEFAULT '',
                duration_secs INTEGER NOT NULL DEFAULT 0,
                thumb_url TEXT NOT NULL DEFAULT '',
                local_path TEXT,
                file_bytes INTEGER NOT NULL DEFAULT 0,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                last_played_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS playlist_items (
                playlist_id INTEGER NOT NULL,
                video_id TEXT NOT NULL,
                added_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (playlist_id, video_id)
            );
            CREATE TABLE IF NOT EXISTS fav_vibes (
                vibe_id TEXT PRIMARY KEY
            );
            CREATE TABLE IF NOT EXISTS fav_artists (
                artist_name TEXT PRIMARY KEY
            );",
        )
        .map_err(|e| e.to_string())?;

        // Seed starter playlists once if empty
        let pl_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
            .unwrap_or(0);
        if pl_count == 0 {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO playlists (name, created_at) VALUES ('Late Night Drive', 1), ('Chill Study', 2)",
                [],
            );
        }

        Ok(Self {
            conn,
            cache_dir: cache_dir.to_path_buf(),
        })
    }

    pub fn cache_file_path(&self, video_id: &str) -> PathBuf {
        self.cache_dir.join(format!("{video_id}.m4a"))
    }

    pub fn upsert_track(&self, t: &TrackMeta, mark_played: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.conn.execute(
            "INSERT INTO tracks (video_id, title, artist, album, duration_secs, thumb_url, local_path, file_bytes, is_favorite, last_played_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(video_id) DO UPDATE SET
               title = excluded.title,
               artist = excluded.artist,
               album = CASE WHEN excluded.album != '' THEN excluded.album ELSE tracks.album END,
               duration_secs = CASE WHEN excluded.duration_secs > 0 THEN excluded.duration_secs ELSE tracks.duration_secs END,
               thumb_url = CASE WHEN excluded.thumb_url != '' THEN excluded.thumb_url ELSE tracks.thumb_url END,
               last_played_at = CASE WHEN ?11 = 1 THEN excluded.last_played_at ELSE tracks.last_played_at END",
            params![
                t.video_id,
                t.title,
                t.artist,
                t.album,
                t.duration_secs as i64,
                t.thumb_url,
                t.local_path,
                t.file_bytes as i64,
                if t.is_favorite { 1 } else { 0 },
                if mark_played { now } else { 0 },
                if mark_played { 1 } else { 0 },
            ],
        );
    }

    pub fn set_offline(&self, t: &TrackMeta, local_path: Option<&str>, file_bytes: u64) {
        self.upsert_track(t, false);
        let _ = self.conn.execute(
            "UPDATE tracks SET local_path = ?2, file_bytes = ?3 WHERE video_id = ?1",
            params![t.video_id, local_path, file_bytes as i64],
        );
    }

    pub fn toggle_favorite(&self, t: &TrackMeta) -> bool {
        self.upsert_track(t, false);
        let cur: i64 = self
            .conn
            .query_row(
                "SELECT is_favorite FROM tracks WHERE video_id = ?1",
                params![t.video_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let next = if cur == 0 { 1 } else { 0 };
        let _ = self.conn.execute(
            "UPDATE tracks SET is_favorite = ?2 WHERE video_id = ?1",
            params![t.video_id, next],
        );
        next == 1
    }

    pub fn is_favorite(&self, video_id: &str) -> bool {
        self.conn
            .query_row(
                "SELECT is_favorite FROM tracks WHERE video_id = ?1",
                params![video_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            != 0
    }

    pub fn get_local_path(&self, video_id: &str) -> Option<PathBuf> {
        let p = self.cache_file_path(video_id);
        if p.is_file() {
            return Some(p);
        }
        None
    }

    pub fn list_offline(&self) -> Vec<TrackMeta> {
        self.query_tracks(
            "SELECT video_id, title, artist, album, duration_secs, thumb_url, local_path, file_bytes, is_favorite
             FROM tracks WHERE local_path IS NOT NULL AND local_path != '' ORDER BY last_played_at DESC, title ASC",
        )
        .into_iter()
        .filter(|t| t.local_path.as_ref().is_some_and(|p| Path::new(p).is_file()))
        .collect()
    }

    pub fn remove_offline(&self, video_id: &str) -> u64 {
        let mut freed = 0u64;
        let path = self.cache_file_path(video_id);
        if let Ok(meta) = std::fs::metadata(&path) {
            freed = meta.len();
        }
        let _ = std::fs::remove_file(&path);
        let _ = self.conn.execute(
            "UPDATE tracks SET local_path = NULL, file_bytes = 0 WHERE video_id = ?1",
            params![video_id],
        );
        freed
    }

    pub fn clear_all_offline(&self) -> (usize, u64) {
        let mut removed = 0usize;
        let mut freed = 0u64;
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Ok(meta) = entry.metadata() {
                        freed += meta.len();
                    }
                    if std::fs::remove_file(&p).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        let _ = self.conn.execute("UPDATE tracks SET local_path = NULL, file_bytes = 0", []);
        (removed, freed)
    }

    pub fn list_favorites(&self) -> Vec<TrackMeta> {
        self.query_tracks(
            "SELECT video_id, title, artist, album, duration_secs, thumb_url, local_path, file_bytes, is_favorite
             FROM tracks WHERE is_favorite = 1 ORDER BY last_played_at DESC, title ASC",
        )
    }

    pub fn list_recent(&self, limit: usize) -> Vec<TrackMeta> {
        let sql = format!(
            "SELECT video_id, title, artist, album, duration_secs, thumb_url, local_path, file_bytes, is_favorite
             FROM tracks WHERE last_played_at > 0 ORDER BY last_played_at DESC LIMIT {limit}"
        );
        self.query_tracks(&sql)
    }

    pub fn list_playlists(&self) -> Vec<PlaylistSummary> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT p.id, p.name, COUNT(pi.video_id)
             FROM playlists p
             LEFT JOIN playlist_items pi ON p.id = pi.playlist_id
             GROUP BY p.id
             ORDER BY p.id ASC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| {
            Ok(PlaylistSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                track_count: r.get::<_, i64>(2)?.max(0) as usize,
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn create_playlist(&self, name: &str) -> Option<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO playlists (name, created_at) VALUES (?1, ?2)",
            params![name, now],
        );
        self.conn
            .query_row(
                "SELECT id FROM playlists WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .ok()
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<(), String> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err("Playlist name cannot be empty".into());
        }
        self.conn
            .execute("UPDATE playlists SET name = ?2 WHERE id = ?1", params![id, clean])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64, delete_unreferenced_files: bool) -> (usize, u64) {
        let candidates: Vec<String> = if delete_unreferenced_files {
            self.playlist_tracks(id)
                .into_iter()
                .map(|t| t.video_id)
                .collect()
        } else {
            Vec::new()
        };
        let _ = self
            .conn
            .execute("DELETE FROM playlist_items WHERE playlist_id = ?1", params![id]);
        let _ = self
            .conn
            .execute("DELETE FROM playlists WHERE id = ?1", params![id]);

        let mut removed = 0usize;
        let mut freed = 0u64;
        for vid in candidates {
            let in_other_pl: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM playlist_items WHERE video_id = ?1",
                    params![vid],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if in_other_pl == 0 && !self.is_favorite(&vid) {
                freed += self.remove_offline(&vid);
                removed += 1;
            }
        }
        (removed, freed)
    }

    pub fn add_track_to_playlist(&self, playlist_id: i64, track: &TrackMeta) {
        self.upsert_track(track, false);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO playlist_items (playlist_id, video_id, added_at) VALUES (?1, ?2, ?3)",
            params![playlist_id, track.video_id, now],
        );
    }

    pub fn remove_track_from_playlist(&self, playlist_id: i64, video_id: &str) {
        let _ = self.conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
            params![playlist_id, video_id],
        );
    }

    pub fn playlist_offline_bytes(&self, playlist_id: i64) -> u64 {
        self.playlist_tracks(playlist_id)
            .into_iter()
            .filter(|t| t.local_path.is_some())
            .map(|t| t.file_bytes)
            .sum()
    }

    pub fn list_orphaned_offline(&self) -> Vec<TrackMeta> {
        self.query_tracks(
            "SELECT t.video_id, t.title, t.artist, t.album, t.duration_secs, t.thumb_url, t.local_path, t.file_bytes, t.is_favorite
             FROM tracks t
             WHERE t.local_path IS NOT NULL AND t.local_path != ''
               AND t.is_favorite = 0
               AND NOT EXISTS (SELECT 1 FROM playlist_items pi WHERE pi.video_id = t.video_id)
             ORDER BY t.file_bytes DESC, t.title ASC",
        )
        .into_iter()
        .filter(|t| t.local_path.as_ref().is_some_and(|p| Path::new(p).is_file()))
        .collect()
    }

    pub fn clean_orphaned_offline(&self) -> (usize, u64) {
        let orphans = self.list_orphaned_offline();
        let mut freed = 0u64;
        let count = orphans.len();
        for t in orphans {
            freed += self.remove_offline(&t.video_id);
        }
        (count, freed)
    }

    pub fn toggle_track_in_playlist(&self, playlist_id: i64, track: &TrackMeta) -> bool {
        self.upsert_track(track, false);
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
                params![playlist_id, track.video_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            let _ = self.conn.execute(
                "DELETE FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2",
                params![playlist_id, track.video_id],
            );
            false
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.conn.execute(
                "INSERT OR IGNORE INTO playlist_items (playlist_id, video_id, added_at) VALUES (?1, ?2, ?3)",
                params![playlist_id, track.video_id, now],
            );
            true
        }
    }

    pub fn track_playlist_ids(&self, video_id: &str) -> Vec<i64> {
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT playlist_id FROM playlist_items WHERE video_id = ?1")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![video_id], |r| r.get(0)) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Vec<TrackMeta> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT t.video_id, t.title, t.artist, t.album, t.duration_secs, t.thumb_url, t.local_path, t.file_bytes, t.is_favorite
             FROM tracks t
             JOIN playlist_items pi ON t.video_id = pi.video_id
             WHERE pi.playlist_id = ?1
             ORDER BY pi.added_at ASC",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![playlist_id], |r| {
            let lp: Option<String> = r.get(6)?;
            let valid_lp = lp.filter(|p| Path::new(p).is_file());
            Ok(TrackMeta {
                video_id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                duration_secs: r.get::<_, i64>(4)?.max(0) as u64,
                thumb_url: r.get(5)?,
                local_path: valid_lp,
                file_bytes: r.get::<_, i64>(7)?.max(0) as u64,
                is_favorite: r.get::<_, i64>(8)? != 0,
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn toggle_fav_vibe(&self, vibe_id: &str) -> bool {
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM fav_vibes WHERE vibe_id = ?1",
                params![vibe_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            let _ = self
                .conn
                .execute("DELETE FROM fav_vibes WHERE vibe_id = ?1", params![vibe_id]);
            false
        } else {
            let _ = self
                .conn
                .execute("INSERT OR IGNORE INTO fav_vibes (vibe_id) VALUES (?1)", params![vibe_id]);
            true
        }
    }

    pub fn list_fav_vibes(&self) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare("SELECT vibe_id FROM fav_vibes") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get(0)) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn toggle_fav_artist(&self, name: &str) -> bool {
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM fav_artists WHERE artist_name = ?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            let _ = self.conn.execute(
                "DELETE FROM fav_artists WHERE artist_name = ?1",
                params![name],
            );
            false
        } else {
            let _ = self.conn.execute(
                "INSERT OR IGNORE INTO fav_artists (artist_name) VALUES (?1)",
                params![name],
            );
            true
        }
    }

    pub fn list_fav_artists(&self) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare("SELECT artist_name FROM fav_artists") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get(0)) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    fn query_tracks(&self, sql: &str) -> Vec<TrackMeta> {
        let Ok(mut stmt) = self.conn.prepare(sql) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| {
            let lp: Option<String> = r.get(6)?;
            let valid_lp = lp.filter(|p| Path::new(p).is_file());
            Ok(TrackMeta {
                video_id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                duration_secs: r.get::<_, i64>(4)?.max(0) as u64,
                thumb_url: r.get(5)?,
                local_path: valid_lp,
                file_bytes: r.get::<_, i64>(7)?.max(0) as u64,
                is_favorite: r.get::<_, i64>(8)? != 0,
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_roundtrip_favorites_and_playlists() {
        let dir = std::env::temp_dir().join("ytmusic_test_db_v2");
        let _ = std::fs::remove_dir_all(&dir);
        let db = MusicDb::open(&dir.join("music.db"), &dir.join("cache")).unwrap();
        let t = TrackMeta {
            video_id: "dQw4w9WgXcQ".into(),
            title: "Test Track".into(),
            artist: "Test Artist".into(),
            album: "Test Album".into(),
            duration_secs: 213,
            thumb_url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg".into(),
            local_path: None,
            file_bytes: 0,
            is_favorite: false,
        };
        db.upsert_track(&t, true);
        assert_eq!(db.list_recent(10).len(), 1);
        assert!(db.toggle_favorite(&t));
        assert_eq!(db.list_favorites().len(), 1);

        let pl_id = db.create_playlist("Roadtrip").unwrap();
        assert!(db.toggle_track_in_playlist(pl_id, &t));
        assert_eq!(db.playlist_tracks(pl_id).len(), 1);
        assert!(!db.toggle_track_in_playlist(pl_id, &t));
        assert_eq!(db.playlist_tracks(pl_id).len(), 0);

        assert!(db.toggle_fav_vibe("synth"));
        assert_eq!(db.list_fav_vibes(), vec!["synth".to_string()]);
        assert!(db.toggle_fav_artist("M83"));
        assert_eq!(db.list_fav_artists(), vec!["M83".to_string()]);

        let cached = db.cache_file_path(&t.video_id);
        std::fs::write(&cached, b"test-m4a-bytes").unwrap();
        db.set_offline(&t, Some(cached.to_str().unwrap()), 14);
        assert_eq!(db.list_offline().len(), 1);
        assert_eq!(db.remove_offline(&t.video_id), 14);
        assert_eq!(db.list_offline().len(), 0);

        std::fs::write(&cached, b"more-bytes").unwrap();
        db.set_offline(&t, Some(cached.to_str().unwrap()), 10);
        assert_eq!(db.clear_all_offline(), (1, 10));
        assert_eq!(db.list_offline().len(), 0);

        // Rename, playlist track mgmt, and orphan cleanup
        db.rename_playlist(pl_id, "Renamed Mix").unwrap();
        db.toggle_favorite(&t); // un-favorite so orphan check applies
        db.add_track_to_playlist(pl_id, &t);
        assert_eq!(db.playlist_tracks(pl_id).len(), 1);
        db.remove_track_from_playlist(pl_id, &t.video_id);
        std::fs::write(&cached, b"orphan").unwrap();
        db.set_offline(&t, Some(cached.to_str().unwrap()), 6);
        assert_eq!(db.list_orphaned_offline().len(), 1);
        assert_eq!(db.clean_orphaned_offline(), (1, 6));
        assert_eq!(db.delete_playlist(pl_id, false), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
