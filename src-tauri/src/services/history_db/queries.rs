// HistoryDb query and pruning methods.
//
// Split from mod.rs to keep file sizes under the 500-line soft limit.
// All methods here are `impl HistoryDb` — they access `self.conn` directly.

use std::collections::HashMap;

use super::{
    CardGrantRow, CollectionSnapshotSummary, EconomySnapshotRow, HistoryDb,
};

impl HistoryDb {
    /// Get economy snapshots, newest first.
    pub fn get_economy_history(
        &self,
        from_ts: Option<i64>,
        to_ts: Option<i64>,
        limit: i32,
    ) -> Result<Vec<EconomySnapshotRow>, String> {
        let (where_clause, params) = build_time_filter(from_ts, to_ts);
        let sql = format!(
            "SELECT id, timestamp, trigger_kind, gold, gems, vault_progress,
                    wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
                    draft_tokens, sealed_tokens, total_boosters
             FROM economy_snapshots
             {}
             ORDER BY timestamp DESC, id DESC
             LIMIT ?",
            where_clause
        );

        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = params;
        all_params.push(Box::new(limit));

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("HST-008: {}", e))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let id: i64 = row.get(0)?;
                Ok((id, EconomySnapshotRow {
                    id,
                    timestamp: row.get(1)?,
                    trigger: row.get(2)?,
                    gold: row.get(3)?,
                    gems: row.get(4)?,
                    vault_progress: row.get(5)?,
                    wc_common: row.get(6)?,
                    wc_uncommon: row.get(7)?,
                    wc_rare: row.get(8)?,
                    wc_mythic: row.get(9)?,
                    wc_track_position: row.get(10)?,
                    draft_tokens: row.get(11)?,
                    sealed_tokens: row.get(12)?,
                    total_boosters: row.get(13)?,
                    boosters_json: String::new(),
                    tokens_json: String::new(),
                }))
            })
            .map_err(|e| format!("HST-008: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            let (id, mut snapshot) = row.map_err(|e| format!("HST-008: {}", e))?;
            snapshot.boosters_json = self.get_boosters_json(id)?;
            snapshot.tokens_json = self.get_tokens_json(id)?;
            results.push(snapshot);
        }

        Ok(results)
    }

    fn get_boosters_json(&self, snapshot_id: i64) -> Result<String, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT set_code, count FROM booster_entries WHERE snapshot_id = ?1")
            .map_err(|e| format!("HST-008: {}", e))?;

        let entries: Vec<(String, i32)> = stmt
            .query_map(rusqlite::params![snapshot_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
            })
            .map_err(|e| format!("HST-008: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string()))
    }

    fn get_tokens_json(&self, snapshot_id: i64) -> Result<String, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT token_id, count FROM custom_token_entries WHERE snapshot_id = ?1")
            .map_err(|e| format!("HST-008: {}", e))?;

        let entries: HashMap<String, i32> = stmt
            .query_map(rusqlite::params![snapshot_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
            })
            .map_err(|e| format!("HST-008: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "{}".to_string()))
    }

    /// Get collection snapshot summaries (metadata only), newest first.
    pub fn get_collection_snapshots(
        &self,
        from_ts: Option<i64>,
        to_ts: Option<i64>,
        limit: i32,
    ) -> Result<Vec<CollectionSnapshotSummary>, String> {
        let (where_clause, params) = build_time_filter(from_ts, to_ts);
        let sql = format!(
            "SELECT id, timestamp, trigger_kind, unique_cards, total_copies, scan_score
             FROM collection_snapshots
             {}
             ORDER BY timestamp DESC, id DESC
             LIMIT ?",
            where_clause
        );

        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = params;
        all_params.push(Box::new(limit));

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("HST-008: {}", e))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(CollectionSnapshotSummary {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    trigger: row.get(2)?,
                    unique_cards: row.get(3)?,
                    total_copies: row.get(4)?,
                    scan_score: row.get(5)?,
                })
            })
            .map_err(|e| format!("HST-008: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("HST-008: {}", e))?);
        }
        Ok(results)
    }

    /// Get the full card map for a single collection snapshot (for diff/export).
    pub fn get_collection_snapshot_detail(
        &self,
        id: i64,
    ) -> Result<Option<HashMap<i32, i32>>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT grp_id, quantity FROM collection_entries WHERE snapshot_id = ?1",
            )
            .map_err(|e| format!("HST-008: {}", e))?;

        let rows = stmt
            .query_map(rusqlite::params![id], |row| {
                Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?))
            })
            .map_err(|e| format!("HST-008: {}", e))?;

        let mut map = HashMap::new();
        let mut found_any = false;
        for row in rows {
            let (grp_id, qty) = row.map_err(|e| format!("HST-008: {}", e))?;
            map.insert(grp_id, qty);
            found_any = true;
        }

        if found_any {
            Ok(Some(map))
        } else {
            Ok(None)
        }
    }

    /// Get card grant log entries, newest first.
    pub fn get_card_grants(
        &self,
        from_ts: Option<i64>,
        to_ts: Option<i64>,
        limit: i32,
        grp_id_filter: Option<i32>,
    ) -> Result<Vec<CardGrantRow>, String> {
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(from) = from_ts {
            params.push(Box::new(from));
            conditions.push(format!("timestamp >= ?{}", params.len()));
        }
        if let Some(to) = to_ts {
            params.push(Box::new(to));
            conditions.push(format!("timestamp <= ?{}", params.len()));
        }
        if let Some(grp_id) = grp_id_filter {
            params.push(Box::new(grp_id));
            conditions.push(format!("grp_id = ?{}", params.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        params.push(Box::new(limit));
        let sql = format!(
            "SELECT id, timestamp, grp_id, set_code, source, card_added, gems_compensation, vault_progress
             FROM card_grants
             {}
             ORDER BY timestamp DESC, id DESC
             LIMIT ?{}",
            where_clause,
            params.len()
        );

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("HST-008: {}", e))?;

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(CardGrantRow {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    grp_id: row.get(2)?,
                    set_code: row.get(3)?,
                    source: row.get(4)?,
                    card_added: row.get::<_, i32>(5)? != 0,
                    gems_compensation: row.get(6)?,
                    vault_progress: row.get(7)?,
                })
            })
            .map_err(|e| format!("HST-008: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("HST-008: {}", e))?);
        }
        Ok(results)
    }

    // --- Pruning ---

    /// Prune old snapshots according to retention policy.
    /// Returns total number of snapshots removed.
    pub fn prune(&self, daily_retention_days: u32, weekly_retention_days: u32) -> Result<usize, String> {
        let now = super::epoch_seconds();
        let daily_cutoff = now - (daily_retention_days as i64 * 86400);
        let weekly_cutoff = now - (weekly_retention_days as i64 * 86400);

        let mut total_pruned = 0;
        total_pruned += self.prune_table("economy_snapshots", daily_cutoff, weekly_cutoff)?;
        total_pruned += self.prune_table("collection_snapshots", daily_cutoff, weekly_cutoff)?;
        total_pruned += self.prune_table("cosmetic_snapshots", daily_cutoff, weekly_cutoff)?;
        total_pruned += self.prune_table("mastery_snapshots", daily_cutoff, weekly_cutoff)?;

        // Prune card_grants: simple age cutoff (keep last weekly_retention_days)
        let grants_pruned = self
            .conn
            .execute(
                "DELETE FROM card_grants WHERE timestamp < ?1",
                rusqlite::params![weekly_cutoff],
            )
            .map_err(|e| format!("HST-010: {}", e))?;
        total_pruned += grants_pruned;

        Ok(total_pruned)
    }

    fn prune_table(
        &self,
        table: &str,
        daily_cutoff: i64,
        weekly_cutoff: i64,
    ) -> Result<usize, String> {
        // Keep all snapshots newer than daily_cutoff.
        // Between daily_cutoff and weekly_cutoff: keep one per week (lowest ID per week).
        // Older than weekly_cutoff: keep one per month (lowest ID per month).

        let pruned_weekly = self
            .conn
            .execute(
                &format!(
                    "DELETE FROM {} WHERE timestamp < ?1 AND timestamp >= ?2
                     AND id NOT IN (
                         SELECT MIN(id) FROM {} WHERE timestamp < ?1 AND timestamp >= ?2
                         GROUP BY (timestamp / 604800)
                     )",
                    table, table
                ),
                rusqlite::params![daily_cutoff, weekly_cutoff],
            )
            .map_err(|e| format!("HST-010: {}", e))?;

        let pruned_monthly = self
            .conn
            .execute(
                &format!(
                    "DELETE FROM {} WHERE timestamp < ?1
                     AND id NOT IN (
                         SELECT MIN(id) FROM {} WHERE timestamp < ?1
                         GROUP BY (timestamp / 2592000)
                     )",
                    table, table
                ),
                rusqlite::params![weekly_cutoff],
            )
            .map_err(|e| format!("HST-010: {}", e))?;

        Ok(pruned_weekly + pruned_monthly)
    }
}

// --- Helpers ---

/// Build a WHERE clause for timestamp filtering.
fn build_time_filter(
    from_ts: Option<i64>,
    to_ts: Option<i64>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(from) = from_ts {
        params.push(Box::new(from));
        conditions.push(format!("timestamp >= ?{}", params.len()));
    }
    if let Some(to) = to_ts {
        params.push(Box::new(to));
        conditions.push(format!("timestamp <= ?{}", params.len()));
    }

    let clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    (clause, params)
}
