// FeedDb — SQLite persistence for feed entries.
//
// Stores feed entries in {app_data_dir}/feed.db so they survive app restarts.
// Schema is auto-created on first open. All errors produce diagnostic codes
// (FEED-001 through FEED-007) but are non-fatal — the in-memory feed still works.
//
// FeedDb does NOT hold an AppHandle — diagnostic emission happens in commands.rs,
// matching the existing pattern (CardDb, MemoryService, etc.).

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// What the frontend sends when persisting a feed entry.
#[derive(Debug, Deserialize)]
pub struct FeedEntryInput {
    pub timestamp: i64,
    pub session_id: String,
    pub category: String,
    pub kind: String,
    pub icon: Option<String>,
    pub title: String,
    pub details: Option<String>,
    pub deltas_json: Option<String>,
    pub thumbnails_json: Option<String>,
}

/// What the backend returns when querying feed entries.
#[derive(Debug, Serialize)]
pub struct FeedEntryRow {
    pub id: i64,
    pub timestamp: i64,
    pub session_id: String,
    pub category: String,
    pub kind: String,
    pub icon: Option<String>,
    pub title: String,
    pub details: Option<String>,
    pub deltas_json: Option<String>,
    pub thumbnails_json: Option<String>,
}

/// Summary of a single session's feed entries.
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub entry_count: i64,
    pub first_timestamp: i64,
    pub last_timestamp: i64,
}

/// Feed database — owns a SQLite connection to feed.db.
pub struct FeedDb {
    conn: Connection,
}

impl FeedDb {
    /// Open (or create) the feed database at the given path.
    ///
    /// Does NOT call init_schema — caller must call it separately.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| format!("FEED-003: Failed to open feed database: {}", e))?;

        // WAL mode for better concurrent read/write performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("FEED-003: Failed to set WAL mode: {}", e))?;

        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Create the feed_entries table and indexes if they don't exist.
    fn init_schema(&mut self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS feed_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp INTEGER NOT NULL,
                    session_id TEXT NOT NULL,
                    category TEXT NOT NULL CHECK(category IN ('economy','cards','event','system')),
                    kind TEXT NOT NULL DEFAULT 'entry',
                    icon TEXT,
                    title TEXT NOT NULL,
                    details TEXT,
                    deltas_json TEXT,
                    thumbnails_json TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_feed_timestamp ON feed_entries(timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_feed_session ON feed_entries(session_id);",
            )
            .map_err(|e| format!("FEED-004: Failed to create feed_entries table: {}", e))?;

        Ok(())
    }

    /// Insert a feed entry. Returns the new row ID.
    pub fn insert(&self, entry: &FeedEntryInput) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO feed_entries (timestamp, session_id, category, kind, icon, title, details, deltas_json, thumbnails_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    entry.timestamp,
                    entry.session_id,
                    entry.category,
                    entry.kind,
                    entry.icon,
                    entry.title,
                    entry.details,
                    entry.deltas_json,
                    entry.thumbnails_json,
                ],
            )
            .map_err(|e| format!("FEED-005: Failed to insert feed entry: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Query feed entries, newest first. Supports cursor-based pagination.
    ///
    /// - `limit`: max entries to return
    /// - `before_id`: if Some, only return entries with id < this value (older entries)
    pub fn get_entries(
        &self,
        limit: i32,
        before_id: Option<i64>,
    ) -> Result<Vec<FeedEntryRow>, String> {
        let mut rows = Vec::new();

        match before_id {
            Some(cursor) => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, timestamp, session_id, category, kind, icon, title, details, deltas_json, thumbnails_json
                         FROM feed_entries
                         WHERE id < ?1
                         ORDER BY id DESC
                         LIMIT ?2",
                    )
                    .map_err(|e| format!("FEED-006: {}", e))?;

                let mapped = stmt
                    .query_map(rusqlite::params![cursor, limit], |row| {
                        Ok(FeedEntryRow {
                            id: row.get(0)?,
                            timestamp: row.get(1)?,
                            session_id: row.get(2)?,
                            category: row.get(3)?,
                            kind: row.get(4)?,
                            icon: row.get(5)?,
                            title: row.get(6)?,
                            details: row.get(7)?,
                            deltas_json: row.get(8)?,
                            thumbnails_json: row.get(9)?,
                        })
                    })
                    .map_err(|e| format!("FEED-006: {}", e))?;

                for row in mapped {
                    rows.push(row.map_err(|e| format!("FEED-006: {}", e))?);
                }
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, timestamp, session_id, category, kind, icon, title, details, deltas_json, thumbnails_json
                         FROM feed_entries
                         ORDER BY id DESC
                         LIMIT ?1",
                    )
                    .map_err(|e| format!("FEED-006: {}", e))?;

                let mapped = stmt
                    .query_map(rusqlite::params![limit], |row| {
                        Ok(FeedEntryRow {
                            id: row.get(0)?,
                            timestamp: row.get(1)?,
                            session_id: row.get(2)?,
                            category: row.get(3)?,
                            kind: row.get(4)?,
                            icon: row.get(5)?,
                            title: row.get(6)?,
                            details: row.get(7)?,
                            deltas_json: row.get(8)?,
                            thumbnails_json: row.get(9)?,
                        })
                    })
                    .map_err(|e| format!("FEED-006: {}", e))?;

                for row in mapped {
                    rows.push(row.map_err(|e| format!("FEED-006: {}", e))?);
                }
            }
        }

        Ok(rows)
    }

    /// Get a summary of each session that has entries.
    pub fn get_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, COUNT(*) as entry_count, MIN(timestamp) as first_ts, MAX(timestamp) as last_ts
                 FROM feed_entries
                 GROUP BY session_id
                 ORDER BY last_ts DESC",
            )
            .map_err(|e| format!("FEED-007: {}", e))?;

        let mapped = stmt
            .query_map([], |row| {
                Ok(SessionSummary {
                    session_id: row.get(0)?,
                    entry_count: row.get(1)?,
                    first_timestamp: row.get(2)?,
                    last_timestamp: row.get(3)?,
                })
            })
            .map_err(|e| format!("FEED-007: {}", e))?;

        let mut results = Vec::new();
        for row in mapped {
            results.push(row.map_err(|e| format!("FEED-007: {}", e))?);
        }

        Ok(results)
    }
}
