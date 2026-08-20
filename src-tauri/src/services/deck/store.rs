// DeckStore — SQLite persistence for the Deck Builder (deckbuilder.db).
//
// Stores parsed Scryfall oracle data, grp↔oracle mappings, tags, community-source
// cache, and saved decks. Corruption recovery mirrors HistoryDb: rename to
// `<name>.corrupt.bak`, recreate fresh. All errors are prefixed with DECK-001
// (store-level) — the ingest layer re-labels to DECK-012 when it means
// "ingest aborted, previous data retained".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::services::history_db::epoch_seconds;

// --- Public ingest / cache / saved-deck types ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestOracle {
    pub oracle_id: String,
    pub name: String,
    pub name_front_lower: String,
    pub cmc: f32,
    pub mana_cost: String,
    pub color_identity: String,
    pub type_line: String,
    pub keywords: String,
    pub oracle_text: String,
    pub edhrec_rank: Option<u32>,
    pub legal_brawl: bool,
    pub legal_standardbrawl: bool,
    pub is_commander_eligible: bool,
    pub power: Option<i32>,
    pub layout: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestArena {
    pub arena_id: i32,
    pub oracle_id: String,
    pub set_code: String,
    pub collector_number: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestTag {
    pub tag_id: String,
    pub slug: String,
    pub label: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CacheHit {
    pub body: String,
    pub fresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedDeckInput {
    pub id: Option<i64>,
    pub name: String,
    pub format: String,
    pub commander_grp: i32,
    pub request_json: String,
    pub result_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedDeckRow {
    pub id: i64,
    pub name: String,
    pub format: String,
    pub commander_grp: i32,
    pub request_json: String,
    pub result_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// --- Migrations ---

const MIGRATIONS: &[fn(&Transaction) -> Result<(), rusqlite::Error>] = &[migrate_v1];

fn migrate_v1(tx: &Transaction) -> Result<(), rusqlite::Error> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS oracle (
            oracle_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            name_front_lower TEXT NOT NULL,
            cmc REAL NOT NULL,
            mana_cost TEXT NOT NULL,
            color_identity TEXT NOT NULL,
            type_line TEXT NOT NULL,
            keywords TEXT NOT NULL,
            oracle_text TEXT NOT NULL,
            edhrec_rank INTEGER,
            legal_brawl INTEGER NOT NULL,
            legal_standardbrawl INTEGER NOT NULL,
            is_commander_eligible INTEGER NOT NULL,
            power INTEGER,
            layout TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_oracle_name ON oracle(name_front_lower);
        CREATE TABLE IF NOT EXISTS oracle_arena (
            arena_id INTEGER PRIMARY KEY,
            oracle_id TEXT NOT NULL,
            set_code TEXT NOT NULL,
            collector_number TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS grp_oracle (
            grp_id INTEGER PRIMARY KEY,
            oracle_id TEXT NOT NULL,
            join_method TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS tag (
            tag_id TEXT PRIMARY KEY,
            slug TEXT NOT NULL,
            label TEXT NOT NULL,
            parent_id TEXT
        );
        CREATE TABLE IF NOT EXISTS tagging (
            oracle_id TEXT NOT NULL,
            tag_id TEXT NOT NULL,
            PRIMARY KEY(oracle_id, tag_id)
        );
        CREATE TABLE IF NOT EXISTS oracle_role (
            oracle_id TEXT NOT NULL,
            role TEXT NOT NULL,
            PRIMARY KEY(oracle_id, role)
        );
        CREATE TABLE IF NOT EXISTS source_cache (
            host TEXT NOT NULL,
            key TEXT NOT NULL,
            fetched_at INTEGER NOT NULL,
            ttl_secs INTEGER NOT NULL,
            body TEXT NOT NULL,
            PRIMARY KEY(host, key)
        );
        CREATE TABLE IF NOT EXISTS source_hits (
            host TEXT NOT NULL,
            day TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY(host, day)
        );
        CREATE TABLE IF NOT EXISTS saved_deck (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            format TEXT NOT NULL,
            commander_grp INTEGER NOT NULL,
            request_json TEXT NOT NULL,
            result_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )
}

// --- Store ---

pub struct DeckStore {
    pub(crate) conn: Connection,
}

impl DeckStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        match Self::try_open(path) {
            Ok(db) => Ok(db),
            Err(e) => {
                let corrupt_path = Self::corrupt_backup_path(path);
                log::warn!(
                    "DECK-001: Corrupt deckbuilder database detected ({}), renaming to {:?}",
                    e,
                    corrupt_path
                );
                let _ = std::fs::rename(path, &corrupt_path);
                Self::try_open(path).map_err(|e2| {
                    format!(
                        "DECK-001: Failed to recreate deckbuilder database after corruption: {}",
                        e2
                    )
                })
            }
        }
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("DECK-001: Failed to open in-memory deckbuilder database: {}", e))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("DECK-001: Failed to set pragmas: {}", e))?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn try_open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("DECK-001: Failed to open deckbuilder database: {}", e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("DECK-001: Failed to set pragmas: {}", e))?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn corrupt_backup_path(path: &Path) -> PathBuf {
        let mut backup = path.to_path_buf();
        let name = backup
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        backup.set_file_name(format!("{}.corrupt.bak", name));
        backup
    }

    pub(crate) fn run_migrations(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
            )
            .map_err(|e| format!("DECK-001: Failed to create schema_version table: {}", e))?;

        let current_version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("DECK-001: Failed to read schema version: {}", e))?;

        let start = current_version as usize;
        if start >= MIGRATIONS.len() {
            return Ok(());
        }

        for (i, migration) in MIGRATIONS[start..].iter().enumerate() {
            let version = (start + i + 1) as i64;
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| format!("DECK-001: Failed to begin migration transaction: {}", e))?;
            migration(&tx)
                .map_err(|e| format!("DECK-001: Migration v{} failed: {}", version, e))?;
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                rusqlite::params![version],
            )
            .map_err(|e| format!("DECK-001: Failed to record migration version: {}", e))?;
            tx.commit()
                .map_err(|e| format!("DECK-001: Failed to commit migration v{}: {}", version, e))?;
        }
        Ok(())
    }

    // --- meta ---

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get::<_, String>(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(format!("DECK-001: get_meta failed: {}", other)),
            })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .map(|_| ())
            .map_err(|e| format!("DECK-001: set_meta failed: {}", e))
    }

    // --- bulk replace ---

    pub fn replace_bulk(
        &mut self,
        oracles: &[IngestOracle],
        arena: &[IngestArena],
        tags: &[IngestTag],
        taggings: &[(String, String)],
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("DECK-001: begin tx: {}", e))?;

        tx.execute_batch(
            "DELETE FROM oracle;
             DELETE FROM oracle_arena;
             DELETE FROM tag;
             DELETE FROM tagging;",
        )
        .map_err(|e| format!("DECK-001: clear bulk tables: {}", e))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO oracle (oracle_id, name, name_front_lower, cmc, mana_cost,
                        color_identity, type_line, keywords, oracle_text, edhrec_rank,
                        legal_brawl, legal_standardbrawl, is_commander_eligible, power, layout)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                )
                .map_err(|e| format!("DECK-001: prepare oracle insert: {}", e))?;
            for o in oracles {
                stmt.execute(rusqlite::params![
                    o.oracle_id,
                    o.name,
                    o.name_front_lower,
                    o.cmc,
                    o.mana_cost,
                    o.color_identity,
                    o.type_line,
                    o.keywords,
                    o.oracle_text,
                    o.edhrec_rank,
                    o.legal_brawl as i32,
                    o.legal_standardbrawl as i32,
                    o.is_commander_eligible as i32,
                    o.power,
                    o.layout,
                ])
                .map_err(|e| format!("DECK-001: insert oracle {}: {}", o.oracle_id, e))?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO oracle_arena (arena_id, oracle_id, set_code, collector_number)
                     VALUES (?1,?2,?3,?4)",
                )
                .map_err(|e| format!("DECK-001: prepare arena insert: {}", e))?;
            for a in arena {
                stmt.execute(rusqlite::params![
                    a.arena_id,
                    a.oracle_id,
                    a.set_code,
                    a.collector_number,
                ])
                .map_err(|e| format!("DECK-001: insert arena {}: {}", a.arena_id, e))?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO tag (tag_id, slug, label, parent_id) VALUES (?1,?2,?3,?4)",
                )
                .map_err(|e| format!("DECK-001: prepare tag insert: {}", e))?;
            for t in tags {
                stmt.execute(rusqlite::params![t.tag_id, t.slug, t.label, t.parent_id])
                    .map_err(|e| format!("DECK-001: insert tag {}: {}", t.tag_id, e))?;
            }
        }

        {
            let mut stmt = tx
                .prepare("INSERT INTO tagging (oracle_id, tag_id) VALUES (?1,?2)")
                .map_err(|e| format!("DECK-001: prepare tagging insert: {}", e))?;
            for (oracle_id, tag_id) in taggings {
                stmt.execute(rusqlite::params![oracle_id, tag_id])
                    .map_err(|e| format!("DECK-001: insert tagging: {}", e))?;
            }
        }

        tx.commit().map_err(|e| format!("DECK-001: commit bulk: {}", e))?;
        Ok(())
    }

    pub fn replace_grp_oracle(
        &mut self,
        rows: &[(i32, String, &'static str)],
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("DECK-001: begin tx: {}", e))?;
        tx.execute_batch("DELETE FROM grp_oracle;")
            .map_err(|e| format!("DECK-001: clear grp_oracle: {}", e))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO grp_oracle (grp_id, oracle_id, join_method) VALUES (?1,?2,?3)",
                )
                .map_err(|e| format!("DECK-001: prepare grp_oracle insert: {}", e))?;
            for (grp, oracle_id, method) in rows {
                stmt.execute(rusqlite::params![grp, oracle_id, method])
                    .map_err(|e| format!("DECK-001: insert grp_oracle {}: {}", grp, e))?;
            }
        }
        tx.commit().map_err(|e| format!("DECK-001: commit grp_oracle: {}", e))?;
        Ok(())
    }

    pub fn replace_roles(&mut self, rows: &[(String, String)]) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("DECK-001: begin tx: {}", e))?;
        tx.execute_batch("DELETE FROM oracle_role;")
            .map_err(|e| format!("DECK-001: clear oracle_role: {}", e))?;
        {
            let mut stmt = tx
                .prepare("INSERT OR IGNORE INTO oracle_role (oracle_id, role) VALUES (?1,?2)")
                .map_err(|e| format!("DECK-001: prepare role insert: {}", e))?;
            for (oracle_id, role) in rows {
                stmt.execute(rusqlite::params![oracle_id, role])
                    .map_err(|e| format!("DECK-001: insert role: {}", e))?;
            }
        }
        tx.commit().map_err(|e| format!("DECK-001: commit roles: {}", e))?;
        Ok(())
    }

    // --- readers ---

    pub fn oracle_count(&self) -> Result<u64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM oracle", [], |row| row.get::<_, i64>(0))
            .map(|n| n as u64)
            .map_err(|e| format!("DECK-001: oracle_count: {}", e))
    }

    pub fn arena_id_map(&self) -> Result<HashMap<i32, String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT arena_id, oracle_id FROM oracle_arena")
            .map_err(|e| format!("DECK-001: prepare arena_id_map: {}", e))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("DECK-001: query arena_id_map: {}", e))?;
        let mut map = HashMap::new();
        for r in rows {
            let (a, o) = r.map_err(|e| format!("DECK-001: row arena_id_map: {}", e))?;
            map.insert(a, o);
        }
        Ok(map)
    }

    pub fn name_map(&self) -> Result<HashMap<String, String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name_front_lower, oracle_id FROM oracle")
            .map_err(|e| format!("DECK-001: prepare name_map: {}", e))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("DECK-001: query name_map: {}", e))?;
        let mut map = HashMap::new();
        for r in rows {
            let (n, o) = r.map_err(|e| format!("DECK-001: row name_map: {}", e))?;
            map.insert(n, o);
        }
        Ok(map)
    }

    pub fn load_tag_rows(&self) -> Result<Vec<IngestTag>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag_id, slug, label, parent_id FROM tag")
            .map_err(|e| format!("DECK-001: prepare load_tag_rows: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IngestTag {
                    tag_id: row.get(0)?,
                    slug: row.get(1)?,
                    label: row.get(2)?,
                    parent_id: row.get(3)?,
                })
            })
            .map_err(|e| format!("DECK-001: query load_tag_rows: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DECK-001: row load_tag_rows: {}", e))
    }

    pub fn load_taggings(&self) -> Result<Vec<(String, String)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT oracle_id, tag_id FROM tagging ORDER BY oracle_id, tag_id")
            .map_err(|e| format!("DECK-001: prepare load_taggings: {}", e))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("DECK-001: query load_taggings: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DECK-001: row load_taggings: {}", e))
    }

    pub fn load_oracle_rows(&self) -> Result<Vec<IngestOracle>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT oracle_id, name, name_front_lower, cmc, mana_cost, color_identity,
                        type_line, keywords, oracle_text, edhrec_rank, legal_brawl,
                        legal_standardbrawl, is_commander_eligible, power, layout
                 FROM oracle",
            )
            .map_err(|e| format!("DECK-001: prepare load_oracle_rows: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IngestOracle {
                    oracle_id: row.get(0)?,
                    name: row.get(1)?,
                    name_front_lower: row.get(2)?,
                    cmc: row.get::<_, f64>(3)? as f32,
                    mana_cost: row.get(4)?,
                    color_identity: row.get(5)?,
                    type_line: row.get(6)?,
                    keywords: row.get(7)?,
                    oracle_text: row.get(8)?,
                    edhrec_rank: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
                    legal_brawl: row.get::<_, i32>(10)? != 0,
                    legal_standardbrawl: row.get::<_, i32>(11)? != 0,
                    is_commander_eligible: row.get::<_, i32>(12)? != 0,
                    power: row.get(13)?,
                    layout: row.get(14)?,
                })
            })
            .map_err(|e| format!("DECK-001: query load_oracle_rows: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DECK-001: row load_oracle_rows: {}", e))
    }

    pub fn load_grp_oracle(&self) -> Result<Vec<(i32, String)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT grp_id, oracle_id FROM grp_oracle")
            .map_err(|e| format!("DECK-001: prepare load_grp_oracle: {}", e))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("DECK-001: query load_grp_oracle: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DECK-001: row load_grp_oracle: {}", e))
    }

    pub fn load_roles(&self) -> Result<HashMap<String, Vec<String>>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT oracle_id, role FROM oracle_role")
            .map_err(|e| format!("DECK-001: prepare load_roles: {}", e))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("DECK-001: query load_roles: {}", e))?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for r in rows {
            let (oracle_id, role) = r.map_err(|e| format!("DECK-001: row load_roles: {}", e))?;
            map.entry(oracle_id).or_default().push(role);
        }
        Ok(map)
    }

    // --- source cache ---

    pub fn cache_get(&self, host: &str, key: &str, now: i64) -> Result<Option<CacheHit>, String> {
        self.conn
            .query_row(
                "SELECT body, fetched_at, ttl_secs FROM source_cache WHERE host=?1 AND key=?2",
                rusqlite::params![host, key],
                |row| {
                    let body: String = row.get(0)?;
                    let fetched_at: i64 = row.get(1)?;
                    let ttl_secs: i64 = row.get(2)?;
                    Ok(CacheHit {
                        body,
                        fresh: now < fetched_at + ttl_secs,
                    })
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(format!("DECK-001: cache_get: {}", other)),
            })
    }

    pub fn cache_put(
        &self,
        host: &str,
        key: &str,
        body: &str,
        ttl_secs: i64,
        now: i64,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO source_cache (host, key, fetched_at, ttl_secs, body)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![host, key, now, ttl_secs, body],
            )
            .map(|_| ())
            .map_err(|e| format!("DECK-001: cache_put: {}", e))
    }

    pub fn hits_today(&self, host: &str, day: &str) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT count FROM source_hits WHERE host=?1 AND day=?2",
                rusqlite::params![host, day],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n as u32)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                other => Err(format!("DECK-001: hits_today: {}", other)),
            })
    }

    pub fn hit_increment(&self, host: &str, day: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO source_hits (host, day, count) VALUES (?1, ?2, 1)
                 ON CONFLICT(host, day) DO UPDATE SET count = count + 1",
                rusqlite::params![host, day],
            )
            .map(|_| ())
            .map_err(|e| format!("DECK-001: hit_increment: {}", e))
    }

    pub fn prune_cache(&self, now: i64) -> Result<usize, String> {
        self.conn
            .execute(
                "DELETE FROM source_cache WHERE fetched_at + ttl_secs < ?1 - 2592000",
                rusqlite::params![now],
            )
            .map_err(|e| format!("DECK-001: prune_cache: {}", e))
    }

    // --- saved decks ---

    pub fn save_deck(&self, input: &SavedDeckInput) -> Result<i64, String> {
        let now = epoch_seconds();
        match input.id {
            Some(id) => {
                let rows = self
                    .conn
                    .execute(
                        "UPDATE saved_deck SET name=?1, format=?2, commander_grp=?3,
                                request_json=?4, result_json=?5, updated_at=?6
                         WHERE id=?7",
                        rusqlite::params![
                            input.name,
                            input.format,
                            input.commander_grp,
                            input.request_json,
                            input.result_json,
                            now,
                            id,
                        ],
                    )
                    .map_err(|e| format!("DECK-011: save_deck update: {}", e))?;
                if rows == 0 {
                    return Err(format!("DECK-011: save_deck: no row with id {}", id));
                }
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO saved_deck (name, format, commander_grp, request_json,
                                result_json, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                        rusqlite::params![
                            input.name,
                            input.format,
                            input.commander_grp,
                            input.request_json,
                            input.result_json,
                            now,
                        ],
                    )
                    .map_err(|e| format!("DECK-011: save_deck insert: {}", e))?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_decks(&self) -> Result<Vec<SavedDeckRow>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, format, commander_grp, request_json, result_json,
                        created_at, updated_at
                 FROM saved_deck ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("DECK-011: prepare list_decks: {}", e))?;
        let rows = stmt
            .query_map([], row_to_saved_deck)
            .map_err(|e| format!("DECK-011: query list_decks: {}", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DECK-011: row list_decks: {}", e))
    }

    pub fn get_deck(&self, id: i64) -> Result<Option<SavedDeckRow>, String> {
        self.conn
            .query_row(
                "SELECT id, name, format, commander_grp, request_json, result_json,
                        created_at, updated_at
                 FROM saved_deck WHERE id=?1",
                rusqlite::params![id],
                row_to_saved_deck,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(format!("DECK-011: get_deck: {}", other)),
            })
    }

    pub fn delete_deck(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM saved_deck WHERE id=?1", rusqlite::params![id])
            .map(|_| ())
            .map_err(|e| format!("DECK-011: delete_deck: {}", e))
    }
}

fn row_to_saved_deck(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedDeckRow> {
    Ok(SavedDeckRow {
        id: row.get(0)?,
        name: row.get(1)?,
        format: row.get(2)?,
        commander_grp: row.get(3)?,
        request_json: row.get(4)?,
        result_json: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}


#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
