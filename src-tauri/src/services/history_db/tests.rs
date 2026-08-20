// Integration tests for HistoryDb.
//
// All tests use in-memory SQLite — no file I/O, no MTGA dependency.
// Covers: migrations, save methods, dedup, queries, date-range filtering, pruning,
// cascade deletes, file-based open, and corrupt recovery.

use std::collections::HashMap;

use super::*;
use crate::models::inventory::{BoosterStack, PlayerCosmetics, PlayerInventory};
use crate::models::{GrantedCard, MasteryState};

// --- Test helpers ---

fn make_inventory() -> PlayerInventory {
    PlayerInventory {
        gold: 12500,
        gems: 3400,
        vault_progress: 55.5,
        wc_common: 42,
        wc_uncommon: 31,
        wc_rare: 8,
        wc_mythic: 3,
        wc_track_position: 7,
        draft_tokens: 1,
        sealed_tokens: 0,
        boosters: vec![
            BoosterStack {
                collation_id: 100001,
                count: 6,
                set_code: Some("MKM".to_string()),
            },
            BoosterStack {
                collation_id: 100002,
                count: 3,
                set_code: Some("OTJ".to_string()),
            },
        ],
        total_boosters: 9,
        custom_tokens: {
            let mut m = HashMap::new();
            m.insert("ORB_123".to_string(), 450);
            m.insert("DRAFT_TOKEN".to_string(), 1);
            m
        },
        user_deck_count: 12,
        precon_deck_count: 5,
    }
}

fn make_collection() -> HashMap<i32, i32> {
    let mut c = HashMap::new();
    c.insert(71001, 4);
    c.insert(71002, 2);
    c.insert(71003, 1);
    c.insert(71004, 3);
    c
}

fn make_cosmetics() -> PlayerCosmetics {
    PlayerCosmetics {
        art_styles: vec![
            crate::models::inventory::CosmeticArtStyle {
                grp_id: 80001,
                art_id: 1,
            },
        ],
        avatars: vec!["avatar_001".to_string(), "avatar_002".to_string()],
        sleeves: vec!["sleeve_001".to_string()],
        pets: vec![],
        emotes: vec!["emote_001".to_string()],
        titles: vec![],
        other: HashMap::new(),
    }
}

fn make_mastery() -> MasteryState {
    MasteryState {
        raw: serde_json::json!({}),
        node_count: 50,
        milestone_count: 10,
        milestones_completed: 6,
        current_level: 19,
        max_level: 80,
        current_xp: 750,
        has_premium: true,
        nodes_completed: 35,
    }
}

fn make_grants() -> Vec<GrantedCard> {
    vec![
        GrantedCard {
            grp_id: 71001,
            set_code: "MKM".to_string(),
            card_added: true,
            gems_compensation: 0,
            vault_progress: 0,
        },
        GrantedCard {
            grp_id: 71002,
            set_code: "MKM".to_string(),
            card_added: false,
            gems_compensation: 20,
            vault_progress: 1,
        },
        GrantedCard {
            grp_id: 71005,
            set_code: "OTJ".to_string(),
            card_added: true,
            gems_compensation: 0,
            vault_progress: 0,
        },
    ]
}

// --- Migration tests ---

#[test]
fn migration_creates_all_tables() {
    let db = HistoryDb::open_in_memory().unwrap();

    let tables: Vec<String> = db
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let expected = vec![
        "booster_entries",
        "card_grants",
        "collection_entries",
        "collection_snapshots",
        "cosmetic_snapshots",
        "custom_token_entries",
        "economy_snapshots",
        "mastery_snapshots",
        "schema_version",
    ];
    for t in &expected {
        assert!(tables.contains(&t.to_string()), "missing table: {}", t);
    }
}

#[test]
fn migration_is_idempotent() {
    let db = HistoryDb::open_in_memory().unwrap();

    // Run migrations again — should be a no-op
    let result = db.run_migrations();
    assert!(result.is_ok(), "re-running migrations should not fail");

    // Version should still be 1
    let version: i64 = db
        .conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn migration_records_version() {
    let db = HistoryDb::open_in_memory().unwrap();

    let version: i64 = db
        .conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
}

// --- Economy snapshot tests ---

#[test]
fn save_economy_snapshot_returns_id() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("startup", &inv).unwrap();
    assert!(id > 0, "snapshot ID should be positive");
}

#[test]
fn save_economy_snapshot_stores_all_fields() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("session_start", &inv).unwrap();

    // Query raw row
    let (gold, gems, vault, wc_r, trigger): (i64, i64, f64, i32, String) = db
        .conn
        .query_row(
            "SELECT gold, gems, vault_progress, wc_rare, trigger_kind FROM economy_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();

    assert_eq!(gold, 12500);
    assert_eq!(gems, 3400);
    assert!((vault - 55.5).abs() < 0.01);
    assert_eq!(wc_r, 8);
    assert_eq!(trigger, "session_start");
}

#[test]
fn save_economy_snapshot_stores_boosters() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("startup", &inv).unwrap();

    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM booster_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "should have 2 booster entries");

    // Verify specific booster
    let (set_code, cnt): (String, i32) = db
        .conn
        .query_row(
            "SELECT set_code, count FROM booster_entries WHERE snapshot_id = ?1 AND collation_id = 100001",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(set_code, "MKM");
    assert_eq!(cnt, 6);
}

#[test]
fn save_economy_snapshot_stores_custom_tokens() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("startup", &inv).unwrap();

    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM custom_token_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "should have 2 custom token entries");
}

#[test]
fn economy_snapshot_query_returns_saved_data() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    db.save_economy_snapshot("startup", &inv).unwrap();

    let rows = db.get_economy_history(None, None, 10).unwrap();
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.gold, 12500);
    assert_eq!(row.gems, 3400);
    assert_eq!(row.wc_rare, 8);
    assert_eq!(row.draft_tokens, 1);
    assert_eq!(row.trigger, "startup");

    // Boosters JSON should contain MKM and OTJ
    assert!(row.boosters_json.contains("MKM"), "boosters_json should contain MKM");
    assert!(row.boosters_json.contains("OTJ"), "boosters_json should contain OTJ");

    // Tokens JSON should contain our custom tokens
    assert!(row.tokens_json.contains("ORB_123"), "tokens_json should contain ORB_123");
}

#[test]
fn economy_snapshot_ordering() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id1 = db.save_economy_snapshot("startup", &inv).unwrap();
    let id2 = db.save_economy_snapshot("session_start", &inv).unwrap();

    let rows = db.get_economy_history(None, None, 10).unwrap();
    assert_eq!(rows.len(), 2);
    // Newest first
    assert_eq!(rows[0].id, id2);
    assert_eq!(rows[1].id, id1);
}

#[test]
fn economy_snapshot_limit() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    for _ in 0..5 {
        db.save_economy_snapshot("startup", &inv).unwrap();
    }

    let rows = db.get_economy_history(None, None, 3).unwrap();
    assert_eq!(rows.len(), 3);
}

// --- Collection snapshot tests ---

#[test]
fn save_collection_snapshot_returns_id() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, Some(0.95))
        .unwrap();
    assert!(id > 0);
}

#[test]
fn save_collection_snapshot_stores_entries() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, Some(0.95))
        .unwrap();

    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM collection_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 4);
}

#[test]
fn save_collection_snapshot_stores_metadata() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, Some(0.95))
        .unwrap();

    let (unique, total, score): (i32, i32, f64) = db
        .conn
        .query_row(
            "SELECT unique_cards, total_copies, scan_score FROM collection_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(unique, 4);
    assert_eq!(total, 10); // 4+2+1+3
    assert!((score - 0.95).abs() < 0.01);
}

#[test]
fn collection_dedup_skips_identical() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id1 = db
        .save_collection_snapshot("startup", &coll, None)
        .unwrap();
    assert!(id1 > 0);

    // Same collection again — should be skipped
    let id2 = db
        .save_collection_snapshot("session_start", &coll, None)
        .unwrap();
    assert_eq!(id2, -1, "identical collection should return -1 (skipped)");

    // Only one snapshot in the DB
    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM collection_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn collection_dedup_allows_different() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll1 = make_collection();

    let id1 = db
        .save_collection_snapshot("startup", &coll1, None)
        .unwrap();
    assert!(id1 > 0);

    // Different collection
    let mut coll2 = coll1.clone();
    coll2.insert(71005, 1); // new card
    let id2 = db
        .save_collection_snapshot("startup", &coll2, None)
        .unwrap();
    assert!(id2 > 0);
    assert_ne!(id1, id2);
}

#[test]
fn collection_snapshot_detail_returns_card_map() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, None)
        .unwrap();

    let detail = db.get_collection_snapshot_detail(id).unwrap();
    assert!(detail.is_some());
    let map = detail.unwrap();
    assert_eq!(map.len(), 4);
    assert_eq!(map[&71001], 4);
    assert_eq!(map[&71003], 1);
}

#[test]
fn collection_snapshot_detail_nonexistent_returns_none() {
    let db = HistoryDb::open_in_memory().unwrap();

    let detail = db.get_collection_snapshot_detail(999).unwrap();
    assert!(detail.is_none());
}

#[test]
fn collection_snapshot_summary_query() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    db.save_collection_snapshot("startup", &coll, Some(0.9))
        .unwrap();

    let rows = db.get_collection_snapshots(None, None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].unique_cards, 4);
    assert_eq!(rows[0].total_copies, 10);
    assert!((rows[0].scan_score.unwrap() - 0.9).abs() < 0.01);
}

// --- Cosmetics snapshot tests ---

#[test]
fn save_cosmetic_snapshot_stores_counts() {
    let db = HistoryDb::open_in_memory().unwrap();
    let cosm = make_cosmetics();

    let id = db.save_cosmetic_snapshot("startup", &cosm).unwrap();
    assert!(id > 0);

    let (art, avatars, sleeves, emotes): (i32, i32, i32, i32) = db
        .conn
        .query_row(
            "SELECT art_styles, avatars, sleeves, emotes FROM cosmetic_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(art, 1);
    assert_eq!(avatars, 2);
    assert_eq!(sleeves, 1);
    assert_eq!(emotes, 1);
}

#[test]
fn save_cosmetic_snapshot_with_other_categories() {
    let db = HistoryDb::open_in_memory().unwrap();
    let mut cosm = make_cosmetics();
    cosm.other.insert(
        "Backdrops".to_string(),
        vec![serde_json::json!("backdrop_001")],
    );

    let id = db.save_cosmetic_snapshot("startup", &cosm).unwrap();

    let extras: Option<String> = db
        .conn
        .query_row(
            "SELECT extras FROM cosmetic_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(extras.is_some());
    assert!(extras.unwrap().contains("Backdrops"));
}

// --- Mastery snapshot tests ---

#[test]
fn save_mastery_snapshot_stores_fields() {
    let db = HistoryDb::open_in_memory().unwrap();
    let mastery = make_mastery();

    let id = db.save_mastery_snapshot("startup", &mastery).unwrap();
    assert!(id > 0);

    let (level, max, xp, premium): (i32, i32, i32, i32) = db
        .conn
        .query_row(
            "SELECT current_level, max_level, current_xp, has_premium FROM mastery_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(level, 19);
    assert_eq!(max, 80);
    assert_eq!(xp, 750);
    assert_eq!(premium, 1);
}

// --- Card grants tests ---

#[test]
fn log_card_grants_stores_entries() {
    let db = HistoryDb::open_in_memory().unwrap();
    let grants = make_grants();

    let count = db.log_card_grants("BoosterOpen", &grants).unwrap();
    assert_eq!(count, 3);

    let stored: i32 = db
        .conn
        .query_row("SELECT COUNT(*) FROM card_grants", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, 3);
}

#[test]
fn log_card_grants_empty_is_noop() {
    let db = HistoryDb::open_in_memory().unwrap();

    let count = db.log_card_grants("BoosterOpen", &[]).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn log_card_grants_stores_compensation_fields() {
    let db = HistoryDb::open_in_memory().unwrap();
    let grants = make_grants();

    db.log_card_grants("BoosterOpen", &grants).unwrap();

    // The second grant was a 5th copy: card_added=false, gems_compensation=20
    let (added, gems, vault): (i32, i32, i32) = db
        .conn
        .query_row(
            "SELECT card_added, gems_compensation, vault_progress FROM card_grants WHERE grp_id = 71002",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(added, 0);
    assert_eq!(gems, 20);
    assert_eq!(vault, 1);
}

#[test]
fn card_grants_query_returns_data() {
    let db = HistoryDb::open_in_memory().unwrap();
    let grants = make_grants();

    db.log_card_grants("BoosterOpen", &grants).unwrap();

    let rows = db.get_card_grants(None, None, 10, None).unwrap();
    assert_eq!(rows.len(), 3);

    // Check bool conversion
    let added_card = rows.iter().find(|r| r.grp_id == 71001).unwrap();
    assert!(added_card.card_added);

    let dupe_card = rows.iter().find(|r| r.grp_id == 71002).unwrap();
    assert!(!dupe_card.card_added);
    assert_eq!(dupe_card.gems_compensation, 20);
}

#[test]
fn card_grants_filter_by_grp_id() {
    let db = HistoryDb::open_in_memory().unwrap();
    let grants = make_grants();

    db.log_card_grants("BoosterOpen", &grants).unwrap();

    let rows = db.get_card_grants(None, None, 10, Some(71005)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].set_code, "OTJ");
}

#[test]
fn card_grants_source_stored() {
    let db = HistoryDb::open_in_memory().unwrap();
    let grants = make_grants();

    db.log_card_grants("EventReward", &grants).unwrap();

    let rows = db.get_card_grants(None, None, 10, None).unwrap();
    for row in &rows {
        assert_eq!(row.source, "EventReward");
    }
}

// --- Date range filtering tests ---

#[test]
fn economy_date_range_filtering() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    // Insert with explicit timestamps via raw SQL to control timing
    let now = epoch_seconds();
    db.conn
        .execute(
            "INSERT INTO economy_snapshots (timestamp, trigger_kind, gold, gems, vault_progress,
             wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
             draft_tokens, sealed_tokens, total_boosters)
             VALUES (?1, 'old', ?2, ?3, 0.0, 0, 0, 0, 0, 0, 0, 0, 0)",
            rusqlite::params![now - 86400, inv.gold, inv.gems],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO economy_snapshots (timestamp, trigger_kind, gold, gems, vault_progress,
             wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
             draft_tokens, sealed_tokens, total_boosters)
             VALUES (?1, 'new', ?2, ?3, 0.0, 0, 0, 0, 0, 0, 0, 0, 0)",
            rusqlite::params![now, inv.gold, inv.gems],
        )
        .unwrap();

    // Query with from_ts that excludes the old one
    let rows = db
        .get_economy_history(Some(now - 100), None, 10)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].trigger, "new");

    // Query with to_ts that excludes the new one
    let rows = db
        .get_economy_history(None, Some(now - 100), 10)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].trigger, "old");

    // Query with range that includes both
    let rows = db
        .get_economy_history(Some(now - 86500), Some(now + 100), 10)
        .unwrap();
    assert_eq!(rows.len(), 2);
}

// --- Cascade delete tests ---

#[test]
fn economy_cascade_deletes_children() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("startup", &inv).unwrap();

    // Verify children exist
    let booster_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM booster_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(booster_count > 0);

    // Delete parent
    db.conn
        .execute(
            "DELETE FROM economy_snapshots WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap();

    // Children should be gone
    let booster_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM booster_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(booster_count, 0, "cascade delete should remove booster_entries");

    let token_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM custom_token_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(token_count, 0, "cascade delete should remove custom_token_entries");
}

#[test]
fn collection_cascade_deletes_entries() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, None)
        .unwrap();

    db.conn
        .execute(
            "DELETE FROM collection_snapshots WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap();

    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM collection_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "cascade delete should remove collection_entries");
}

// --- Pruning tests ---

#[test]
fn prune_removes_old_card_grants() {
    let db = HistoryDb::open_in_memory().unwrap();
    let now = epoch_seconds();

    // Insert a grant 200 days ago
    db.conn
        .execute(
            "INSERT INTO card_grants (timestamp, grp_id, set_code, source, card_added) VALUES (?1, 71001, 'MKM', 'test', 1)",
            rusqlite::params![now - 200 * 86400],
        )
        .unwrap();

    // Insert a grant today
    db.conn
        .execute(
            "INSERT INTO card_grants (timestamp, grp_id, set_code, source, card_added) VALUES (?1, 71002, 'MKM', 'test', 1)",
            rusqlite::params![now],
        )
        .unwrap();

    let pruned = db.prune(30, 180).unwrap();
    assert!(pruned >= 1, "should prune at least the old grant");

    let count: i32 = db
        .conn
        .query_row("SELECT COUNT(*) FROM card_grants", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "only the recent grant should remain");
}

#[test]
fn prune_keeps_recent_snapshots() {
    let db = HistoryDb::open_in_memory().unwrap();
    let now = epoch_seconds();

    // Insert 3 economy snapshots from today
    for i in 0..3 {
        db.conn
            .execute(
                "INSERT INTO economy_snapshots (timestamp, trigger_kind, gold, gems, vault_progress,
                 wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
                 draft_tokens, sealed_tokens, total_boosters)
                 VALUES (?1, 'test', 0, 0, 0.0, 0, 0, 0, 0, 0, 0, 0, 0)",
                rusqlite::params![now - i * 3600],
            )
            .unwrap();
    }

    let pruned = db.prune(30, 180).unwrap();
    assert_eq!(pruned, 0, "recent snapshots should not be pruned");

    let count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM economy_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
}

#[test]
fn prune_thins_weekly_range() {
    let db = HistoryDb::open_in_memory().unwrap();
    let now = epoch_seconds();

    // Insert multiple snapshots in the 30-180 day range (one per day for a week,
    // starting 60 days ago)
    for day in 0..7 {
        db.conn
            .execute(
                "INSERT INTO economy_snapshots (timestamp, trigger_kind, gold, gems, vault_progress,
                 wc_common, wc_uncommon, wc_rare, wc_mythic, wc_track_position,
                 draft_tokens, sealed_tokens, total_boosters)
                 VALUES (?1, 'test', 0, 0, 0.0, 0, 0, 0, 0, 0, 0, 0, 0)",
                rusqlite::params![now - (60 + day) * 86400],
            )
            .unwrap();
    }

    let before_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM economy_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_count, 7);

    let pruned = db.prune(30, 180).unwrap();
    assert!(pruned > 0, "should prune some snapshots in the weekly range");

    let after_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM economy_snapshots",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        after_count < before_count,
        "should have fewer snapshots after pruning (was {}, now {})",
        before_count,
        after_count
    );
    assert!(after_count >= 1, "should keep at least one per week");
}

// --- File-based open + corrupt recovery tests ---

#[test]
fn open_creates_file_database() {
    let dir = std::env::temp_dir().join("mtga_hub_test_open");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test_history.db");
    let _ = std::fs::remove_file(&path); // clean slate

    let db = HistoryDb::open(&path).unwrap();
    let inv = make_inventory();
    let id = db.save_economy_snapshot("test", &inv).unwrap();
    assert!(id > 0);

    // Verify file exists
    assert!(path.exists(), "database file should exist on disk");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn open_recovers_corrupt_database() {
    let dir = std::env::temp_dir().join("mtga_hub_test_corrupt");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("corrupt_history.db");

    // Write garbage to simulate corruption
    std::fs::write(&path, b"this is not a valid sqlite database").unwrap();

    // Open should recover by renaming corrupt file and creating fresh
    let db = HistoryDb::open(&path);
    assert!(db.is_ok(), "should recover from corrupt database");

    // Verify fresh database works
    let db = db.unwrap();
    let inv = make_inventory();
    let id = db.save_economy_snapshot("test", &inv).unwrap();
    assert!(id > 0);

    // Verify corrupt backup exists
    let backup = dir.join("corrupt_history.db.corrupt.bak");
    assert!(backup.exists(), "corrupt backup should exist");

    // Clean up
    drop(db);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_dir(&dir);
}

// --- Transaction integrity tests ---

#[test]
fn economy_snapshot_is_atomic() {
    let db = HistoryDb::open_in_memory().unwrap();
    let inv = make_inventory();

    let id = db.save_economy_snapshot("startup", &inv).unwrap();

    // All three tables should have consistent data
    let snap_exists: bool = db
        .conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM economy_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    let boosters_exist: bool = db
        .conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM booster_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    let tokens_exist: bool = db
        .conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM custom_token_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();

    assert!(snap_exists);
    assert!(boosters_exist);
    assert!(tokens_exist);
}

#[test]
fn collection_snapshot_is_atomic() {
    let db = HistoryDb::open_in_memory().unwrap();
    let coll = make_collection();

    let id = db
        .save_collection_snapshot("startup", &coll, None)
        .unwrap();

    let snap_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM collection_snapshots WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();
    let entry_count: i32 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM collection_entries WHERE snapshot_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(snap_count, 1);
    assert_eq!(entry_count, 4);
}

// --- Content hash determinism ---

#[test]
fn collection_hash_is_order_independent() {
    let db = HistoryDb::open_in_memory().unwrap();

    // Insert with one ordering
    let mut coll1 = HashMap::new();
    coll1.insert(1, 4);
    coll1.insert(2, 3);
    coll1.insert(3, 2);
    let id1 = db
        .save_collection_snapshot("a", &coll1, None)
        .unwrap();

    // Capture hash before clearing
    let hash1: i64 = db
        .conn
        .query_row(
            "SELECT content_hash FROM collection_snapshots WHERE id = ?1",
            rusqlite::params![id1],
            |row| row.get(0),
        )
        .unwrap();

    // Clear the snapshot so dedup doesn't skip
    db.conn
        .execute("DELETE FROM collection_entries", [])
        .unwrap();
    db.conn
        .execute("DELETE FROM collection_snapshots", [])
        .unwrap();

    // Insert with different insertion order but same data
    let mut coll2 = HashMap::new();
    coll2.insert(3, 2);
    coll2.insert(1, 4);
    coll2.insert(2, 3);
    let id2 = db
        .save_collection_snapshot("b", &coll2, None)
        .unwrap();

    let hash2: i64 = db
        .conn
        .query_row(
            "SELECT content_hash FROM collection_snapshots WHERE id = ?1",
            rusqlite::params![id2],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hash1, hash2, "hash should be order-independent");
}

// --- Multiple snapshot types coexist ---

#[test]
fn all_snapshot_types_coexist() {
    let db = HistoryDb::open_in_memory().unwrap();

    let econ_id = db
        .save_economy_snapshot("startup", &make_inventory())
        .unwrap();
    let coll_id = db
        .save_collection_snapshot("startup", &make_collection(), None)
        .unwrap();
    let cosm_id = db
        .save_cosmetic_snapshot("startup", &make_cosmetics())
        .unwrap();
    let mast_id = db
        .save_mastery_snapshot("startup", &make_mastery())
        .unwrap();
    let grant_count = db
        .log_card_grants("BoosterOpen", &make_grants())
        .unwrap();

    assert!(econ_id > 0);
    assert!(coll_id > 0);
    assert!(cosm_id > 0);
    assert!(mast_id > 0);
    assert_eq!(grant_count, 3);

    // Each type has its own ID sequence
    let econ_rows = db.get_economy_history(None, None, 10).unwrap();
    let coll_rows = db.get_collection_snapshots(None, None, 10).unwrap();
    let grant_rows = db.get_card_grants(None, None, 10, None).unwrap();

    assert_eq!(econ_rows.len(), 1);
    assert_eq!(coll_rows.len(), 1);
    assert_eq!(grant_rows.len(), 3);
}
