// HistoryDb — SQLite persistence for collection, economy, cosmetic, and mastery snapshots.
//
// Stores historical data in {app_data_dir}/history.db for analytics and diff views.
// Uses a forward-only migration runner. All errors produce diagnostic codes (HST-*).
// Follows the FeedDb pattern: owns a rusqlite::Connection, managed as Mutex<HistoryDb>.

mod queries;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};
use serde::Serialize;

use crate::models::{GrantedCard, MasteryState, PlayerCosmetics, PlayerInventory};

// --- Return types (Serialize for IPC) ---

#[derive(Debug, Clone, Serialize)]
pub struct EconomySnapshotRow {
    pub id: i64,
    pub timestamp: i64,
    pub trigger: String,
    pub gold: i64,
    pub gems: i64,
    pub vault_progress: f64,
    pub wc_common: i32,
    pub wc_uncommon: i32,
    pub wc_rare: i32,
    pub wc_mythic: i32,
    pub wc_track_position: i32,
    pub draft_tokens: i32,
    pub sealed_tokens: i32,
    pub total_boosters: i32,
    pub boosters_json: String,
    pub tokens_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionSnapshotSummary {
    pub id: i64,
    pub timestamp: i64,
    pub trigger: String,
    pub unique_cards: i32,
    pub total_copies: i32,
    pub scan_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardGrantRow {
    pub id: i64,
    pub timestamp: i64,
    pub grp_id: i32,
    pub set_code: String,
    pub source: String,
    pub card_added: bool,
    pub gems_compensation: i32,
    pub vault_progress: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotSummary {
    pub economy_id: Option<i64>,
    pub collection_id: Option<i64>,
    pub cosmetics_id: Option<i64>,
    pub mastery_id: Option<i64>,
}

// --- Migration functions ---

const MIGRATIONS: &[fn(&Transaction) -> Result<(), rusqlite::Error>] = &[migrate_v1];

fn migrate_v1(tx: &Transaction) -> Result<(), rusqlite::Error> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS economy_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            trigger_kind TEXT NOT NULL,
            gold INTEGER NOT NULL,
            gems INTEGER NOT NULL,
            vault_progress REAL NOT NULL,
            wc_common INTEGER NOT NULL,
            wc_uncommon INTEGER NOT NULL,
            wc_rare INTEGER NOT NULL,
            wc_mythic INTEGER NOT NULL,
            wc_track_position INTEGER NOT NULL,
            draft_tokens INTEGER NOT NULL,
            sealed_tokens INTEGER NOT NULL,
            total_boosters INTEGER NOT NULL,
            extras TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_econ_ts ON economy_snapshots(timestamp DESC);

        CREATE TABLE IF NOT EXISTS booster_entries (
            snapshot_id INTEGER NOT NULL REFERENCES economy_snapshots(id) ON DELETE CASCADE,
            collation_id INTEGER NOT NULL,
            set_code TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (snapshot_id, collation_id)
        );

        CREATE TABLE IF NOT EXISTS custom_token_entries (
            snapshot_id INTEGER NOT NULL REFERENCES economy_snapshots(id) ON DELETE CASCADE,
            token_id TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (snapshot_id, token_id)
        );

        CREATE TABLE IF NOT EXISTS collection_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            trigger_kind TEXT NOT NULL,
            unique_cards INTEGER NOT NULL,
            total_copies INTEGER NOT NULL,
            scan_score REAL,
            content_hash INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_coll_ts ON collection_snapshots(timestamp DESC);

        CREATE TABLE IF NOT EXISTS collection_entries (
            snapshot_id INTEGER NOT NULL REFERENCES collection_snapshots(id) ON DELETE CASCADE,
            grp_id INTEGER NOT NULL,
            quantity INTEGER NOT NULL,
            PRIMARY KEY (snapshot_id, grp_id)
        );

        CREATE TABLE IF NOT EXISTS cosmetic_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            trigger_kind TEXT NOT NULL,
            art_styles INTEGER NOT NULL DEFAULT 0,
            avatars INTEGER NOT NULL DEFAULT 0,
            pets INTEGER NOT NULL DEFAULT 0,
            sleeves INTEGER NOT NULL DEFAULT 0,
            emotes INTEGER NOT NULL DEFAULT 0,
            titles INTEGER NOT NULL DEFAULT 0,
            extras TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cosm_ts ON cosmetic_snapshots(timestamp DESC);

        CREATE TABLE IF NOT EXISTS mastery_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            trigger_kind TEXT NOT NULL,
            current_level INTEGER NOT NULL DEFAULT 0,
            max_level INTEGER NOT NULL DEFAULT 0,
            current_xp INTEGER NOT NULL DEFAULT 0,
            nodes_completed INTEGER NOT NULL DEFAULT 0,
            milestones_completed INTEGER NOT NULL DEFAULT 0,
            has_premium INTEGER NOT NULL DEFAULT 0,
            extras TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_mast_ts ON mastery_snapshots(timestamp DESC);

        CREATE TABLE IF NOT EXISTS card_grants (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            grp_id INTEGER NOT NULL,
            set_code TEXT NOT NULL,
            source TEXT NOT NULL,
            card_added INTEGER NOT NULL,
            gems_compensation INTEGER NOT NULL DEFAULT 0,
            vault_progress INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_grants_ts ON card_grants(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_grants_grp ON card_grants(grp_id);
        CREATE INDEX IF NOT EXISTS idx_grants_source ON card_grants(source);",
    )?;
    Ok(())
}

// --- HistoryDb ---

pub struct HistoryDb {
    pub(crate) conn: Connection,
}

impl HistoryDb {
    /// Open (or create) the history database at the given path.
    /// On corruption, renames the file and creates a fresh database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        match Self::try_open(path) {
            Ok(db) => Ok(db),
            Err(e) => {
                // Attempt corruption recovery
                let corrupt_path = Self::corrupt_backup_path(path);
                log::warn!(
                    "HST-011: Corrupt database detected ({}), renaming to {:?}",
                    e,
                    corrupt_path
                );
                let _ = std::fs::rename(path, &corrupt_path);
                Self::try_open(path).map_err(|e2| {
                    format!("HST-003: Failed to recreate history database after corruption: {}", e2)
                })
            }
        }
    }

    fn try_open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("HST-003: Failed to open history database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("HST-003: Failed to set pragmas: {}", e))?;

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

    fn run_migrations(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_version (
                    version INTEGER NOT NULL
                );",
            )
            .map_err(|e| format!("HST-004: Failed to create schema_version table: {}", e))?;

        let current_version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("HST-004: Failed to read schema version: {}", e))?;

        let start = current_version as usize;
        if start >= MIGRATIONS.len() {
            return Ok(());
        }

        for (i, migration) in MIGRATIONS[start..].iter().enumerate() {
            let version = (start + i + 1) as i64;
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| format!("HST-004: Failed to begin migration transaction: {}", e))?;

            migration(&tx).map_err(|e| {
                format!("HST-004: Migration v{} failed: {}", version, e)
            })?;

            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                rusqlite::params![version],
            )
            .map_err(|e| format!("HST-004: Failed to record migration version: {}", e))?;

            tx.commit()
                .map_err(|e| format!("HST-004: Failed to commit migration v{}: {}", version, e))?;
        }

        Ok(())
    }

    // --- Save methods ---

    /// Save an economy snapshot. Returns the snapshot row ID.
    pub fn save_economy_snapshot(
        &self,
        trigger: &str,
        inv: &PlayerInventory,
    ) -> Result<i64, String> {
        let now = epoch_seconds();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("HST-005: {}", e))?;

        tx.execute(
            "INSERT INTO economy_snapshots (timestamp, trigger_kind, gold, gems, vault_progress,
             wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
             draft_tokens, sealed_tokens, total_boosters)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                now,
                trigger,
                inv.gold,
                inv.gems,
                inv.vault_progress,
                inv.wc_common,
                inv.wc_uncommon,
                inv.wc_rare,
                inv.wc_mythic,
                inv.wc_track_position,
                inv.draft_tokens,
                inv.sealed_tokens,
                inv.total_boosters,
            ],
        )
        .map_err(|e| format!("HST-005: Economy snapshot insert failed: {}", e))?;

        let snapshot_id = tx.last_insert_rowid();

        // Insert booster entries
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO booster_entries (snapshot_id, collation_id, set_code, count)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|e| format!("HST-005: {}", e))?;

            for booster in &inv.boosters {
                let set_code = booster.set_code.as_deref().unwrap_or("UNK");
                stmt.execute(rusqlite::params![
                    snapshot_id,
                    booster.collation_id,
                    set_code,
                    booster.count,
                ])
                .map_err(|e| format!("HST-005: Booster entry insert failed: {}", e))?;
            }
        }

        // Insert custom token entries
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO custom_token_entries (snapshot_id, token_id, count)
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(|e| format!("HST-005: {}", e))?;

            for (token_id, count) in &inv.custom_tokens {
                stmt.execute(rusqlite::params![snapshot_id, token_id, count])
                    .map_err(|e| format!("HST-005: Token entry insert failed: {}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("HST-005: Economy snapshot commit failed: {}", e))?;

        Ok(snapshot_id)
    }

    /// Save a collection snapshot. Returns the snapshot row ID, or -1 if skipped (dedup).
    pub fn save_collection_snapshot(
        &self,
        trigger: &str,
        collection: &HashMap<i32, i32>,
        scan_score: Option<f64>,
    ) -> Result<i64, String> {
        let content_hash = compute_collection_hash(collection);

        // Dedup check
        let prev_hash: Option<i64> = self
            .conn
            .query_row(
                "SELECT content_hash FROM collection_snapshots ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        if prev_hash == Some(content_hash) {
            return Ok(-1); // Skipped — identical to previous
        }

        let now = epoch_seconds();
        let unique_cards = collection.len() as i32;
        let total_copies: i32 = collection.values().sum();

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("HST-006: {}", e))?;

        tx.execute(
            "INSERT INTO collection_snapshots (timestamp, trigger_kind, unique_cards, total_copies, scan_score, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![now, trigger, unique_cards, total_copies, scan_score, content_hash],
        )
        .map_err(|e| format!("HST-006: Collection snapshot insert failed: {}", e))?;

        let snapshot_id = tx.last_insert_rowid();

        // Batch insert collection entries
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO collection_entries (snapshot_id, grp_id, quantity)
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(|e| format!("HST-006: {}", e))?;

            for (&grp_id, &qty) in collection {
                stmt.execute(rusqlite::params![snapshot_id, grp_id, qty])
                    .map_err(|e| format!("HST-006: Collection entry insert failed: {}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("HST-006: Collection snapshot commit failed: {}", e))?;

        Ok(snapshot_id)
    }

    /// Save a cosmetics snapshot. Returns the snapshot row ID.
    pub fn save_cosmetic_snapshot(
        &self,
        trigger: &str,
        cosmetics: &PlayerCosmetics,
    ) -> Result<i64, String> {
        let now = epoch_seconds();
        let extras = if cosmetics.other.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&cosmetics.other).unwrap_or_default())
        };

        self.conn
            .execute(
                "INSERT INTO cosmetic_snapshots (timestamp, trigger_kind, art_styles, avatars, pets, sleeves, emotes, titles, extras)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    now,
                    trigger,
                    cosmetics.art_styles.len() as i32,
                    cosmetics.avatars.len() as i32,
                    cosmetics.pets.len() as i32,
                    cosmetics.sleeves.len() as i32,
                    cosmetics.emotes.len() as i32,
                    cosmetics.titles.len() as i32,
                    extras,
                ],
            )
            .map_err(|e| format!("HST-005: Cosmetic snapshot insert failed: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Save a mastery snapshot. Returns the snapshot row ID.
    pub fn save_mastery_snapshot(
        &self,
        trigger: &str,
        mastery: &MasteryState,
    ) -> Result<i64, String> {
        let now = epoch_seconds();

        self.conn
            .execute(
                "INSERT INTO mastery_snapshots (timestamp, trigger_kind, current_level, max_level, current_xp, nodes_completed, milestones_completed, has_premium)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    now,
                    trigger,
                    mastery.current_level as i32,
                    mastery.max_level as i32,
                    mastery.current_xp as i32,
                    mastery.nodes_completed as i32,
                    mastery.milestones_completed as i32,
                    if mastery.has_premium { 1 } else { 0 },
                ],
            )
            .map_err(|e| format!("HST-005: Mastery snapshot insert failed: {}", e))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Log granted cards from an inventory change. Returns count logged.
    pub fn log_card_grants(&self, source: &str, cards: &[GrantedCard]) -> Result<usize, String> {
        if cards.is_empty() {
            return Ok(0);
        }

        let now = epoch_seconds();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("HST-007: {}", e))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO card_grants (timestamp, grp_id, set_code, source, card_added, gems_compensation, vault_progress)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| format!("HST-007: {}", e))?;

            for card in cards {
                stmt.execute(rusqlite::params![
                    now,
                    card.grp_id,
                    card.set_code,
                    source,
                    if card.card_added { 1 } else { 0 },
                    card.gems_compensation,
                    card.vault_progress,
                ])
                .map_err(|e| format!("HST-007: Card grant insert failed: {}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("HST-007: Card grants commit failed: {}", e))?;

        Ok(cards.len())
    }

    /// In-memory database for tests. No file I/O, no WAL mode.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("HST-003: {}", e))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("HST-003: {}", e))?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }
}

// --- Helpers ---

pub(crate) fn epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn compute_collection_hash(collection: &HashMap<i32, i32>) -> i64 {
    let mut entries: Vec<(i32, i32)> = collection.iter().map(|(&k, &v)| (k, v)).collect();
    entries.sort_by_key(|(k, _)| *k);

    let mut hasher = DefaultHasher::new();
    for (grp_id, qty) in &entries {
        grp_id.hash(&mut hasher);
        qty.hash(&mut hasher);
    }
    hasher.finish() as i64
}
