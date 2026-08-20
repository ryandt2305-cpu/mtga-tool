// CardDbService — load MTGA's SQLite card database, validate schema, cache cards.
//
// Port of Python's `card_database.py`. Key divergences from Python:
// - Rarity is a typed enum, not a string
// - Name cleanup regex compiled once via OnceLock, not per-call
// - SchemaGuard validates schema before query (Python doesn't validate)
// - Error paths produce diagnostic codes (Python raises exceptions)
//
// Pure functions (`resolve_db_path`, `load_cards_from_path`) are extracted so
// the compare binary can use them without spinning up a Tauri runtime.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use regex::Regex;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::models::{CardInfo, Rarity};
use crate::services::diagnostics::{self, CDB_001, CDB_002, CDB_003, CDB_004, CDB_005, CDB_006, CDB_007};
use crate::services::mtga_install;
use crate::services::schema_guard;

// ── Error type ──────────────────────────────────────────────────────

/// Errors from pure card DB operations (no Tauri dependency).
#[derive(Debug)]
pub enum CardDbError {
    InstallNotFound(Vec<String>),
    DatabaseNotFound(String),
    OverrideNotFound(String),
    ConnectionFailed(String),
    SchemaInvalid { missing_tables: Vec<String> },
    QueryFailed(String),
}

impl fmt::Display for CardDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstallNotFound(checked) => write!(
                f,
                "MTGA install not found. Checked: {}",
                checked.join(", ")
            ),
            Self::DatabaseNotFound(dir) => {
                write!(f, "No Raw_CardDatabase_*.mtga found in {}", dir)
            }
            Self::OverrideNotFound(path) => {
                write!(f, "Override path does not exist: {}", path)
            }
            Self::ConnectionFailed(e) => write!(f, "Failed to open card database: {}", e),
            Self::SchemaInvalid { missing_tables } => write!(
                f,
                "Card database schema invalid — missing tables: {}",
                missing_tables.join(", ")
            ),
            Self::QueryFailed(e) => write!(f, "Card query failed: {}", e),
        }
    }
}

// ── Load result ─────────────────────────────────────────────────────

/// Successful result from `load_cards_from_path` — everything the caller
/// needs to populate state or serialize output.
pub struct LoadResult {
    pub cards: HashMap<i32, CardInfo>,
    pub valid_ids: HashSet<i32>,
    pub db_path: String,
    pub schema_warnings: Vec<(String, String)>,  // (table, column)
    pub extra_tables: Vec<String>,
}

// ── Pure functions (no Tauri dependency) ─────────────────────────────

/// Compiled regex for stripping MTGA markup from card names.
/// Matches `<sprite="...">`, `<nobr>`, `</nobr>`, and any other XML-style tags.
fn markup_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("markup regex is valid"))
}

/// Strip MTGA markup tags from localized card names.
fn clean_name(raw: &str) -> String {
    markup_regex().replace_all(raw, "").into_owned()
}

/// Parse CMC from MTGA's OldSchoolManaText format.
/// "o2oW" = {2}{W} → 3, "oXoGoG" = {X}{G}{G} → 2, "" → 0
fn parse_cmc(mana_text: &str) -> i32 {
    if mana_text.is_empty() {
        return 0;
    }
    let mut cmc = 0;
    for symbol in mana_text.split('o').filter(|s| !s.is_empty()) {
        if let Ok(n) = symbol.parse::<i32>() {
            cmc += n; // generic mana (e.g., "2" → +2)
        } else if symbol == "X" || symbol == "Y" || symbol == "Z" {
            // variable costs don't contribute to CMC
        } else {
            cmc += 1; // colored symbol or hybrid (e.g., "W" → +1)
        }
    }
    cmc
}

/// Parse power or toughness from TEXT column.
/// "2" → Some(2), "*" → None, "X" → None, "" → None
fn parse_stat(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

/// Parse MTGA's comma-separated integer ID lists ("1,3" → [1,3]); ignores blanks and non-numbers.
pub(crate) fn parse_id_list(s: &str) -> Vec<i32> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<i32>().ok())
        .collect()
}

/// Map enum IDs to display names, skipping unknown IDs (SchemaGuard already warned).
pub(crate) fn resolve_enum_names(ids: &[i32], map: &HashMap<i32, String>) -> Vec<String> {
    ids.iter().filter_map(|id| map.get(id).cloned()).collect()
}

/// MTGA colour enum display name → single letter. Colorless/unknown → None.
pub(crate) fn color_letter(name: &str) -> Option<&'static str> {
    match name {
        "White" => Some("W"),
        "Blue" => Some("U"),
        "Black" => Some("B"),
        "Red" => Some("R"),
        "Green" => Some("G"),
        _ => None,
    }
}

/// Load `Enums` rows of one type into id → English display name.
pub(crate) fn load_enum_map(
    conn: &Connection,
    enum_type: &str,
) -> Result<HashMap<i32, String>, CardDbError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.Value, l.Loc FROM Enums e
             JOIN Localizations_enUS l ON e.LocId = l.LocId AND l.Formatted = 1
             WHERE e.Type = ?1",
        )
        .map_err(|e| CardDbError::QueryFailed(format!("Failed to prepare enum query ({}): {}", enum_type, e)))?;
    let rows = stmt
        .query_map([enum_type], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| CardDbError::QueryFailed(format!("Enum query failed ({}): {}", enum_type, e)))?;
    let mut map = HashMap::new();
    for r in rows {
        let (v, name) =
            r.map_err(|e| CardDbError::QueryFailed(format!("Enum row failed ({}): {}", enum_type, e)))?;
        map.insert(v, clean_name(&name));
    }
    Ok(map)
}

/// Find the Raw_CardDatabase_*.mtga file inside the install directory.
fn find_card_database(install_dir: &Path) -> Option<PathBuf> {
    let raw_dir = install_dir.join("MTGA_Data").join("Downloads").join("Raw");
    if !raw_dir.is_dir() {
        return None;
    }

    let pattern = raw_dir.join("Raw_CardDatabase_*.mtga");
    let pattern_str = pattern.to_string_lossy();

    // Use glob-style matching via read_dir + filter
    if let Ok(entries) = std::fs::read_dir(&raw_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("Raw_CardDatabase_") && name_str.ends_with(".mtga") {
                return Some(entry.path());
            }
        }
    }

    // Log the pattern we searched for debugging
    log::debug!("No card database found matching {}", pattern_str);
    None
}

/// Resolve the card database file path.
///
/// If `path_override` is provided, validates it exists and returns it.
/// Otherwise, searches known MTGA install locations.
pub fn resolve_db_path(path_override: Option<&str>) -> Result<PathBuf, CardDbError> {
    match path_override {
        Some(override_path) => {
            let path = PathBuf::from(override_path);
            if !path.exists() {
                return Err(CardDbError::OverrideNotFound(override_path.to_string()));
            }
            Ok(path)
        }
        None => {
            let install_dir =
                mtga_install::discover().map_err(CardDbError::InstallNotFound)?;

            find_card_database(&install_dir).ok_or_else(|| {
                CardDbError::DatabaseNotFound(
                    install_dir
                        .join("MTGA_Data/Downloads/Raw")
                        .display()
                        .to_string(),
                )
            })
        }
    }
}

/// Load cards from the SQLite database at `db_path`.
///
/// Opens read-only, validates schema via SchemaGuard, executes the card
/// query, and builds the card map. Returns a `LoadResult` with all data
/// needed by callers (both Tauri and standalone binary).
pub fn load_cards_from_path(db_path: &Path) -> Result<LoadResult, CardDbError> {
    let db_path_str = db_path.to_string_lossy().to_string();

    // Open SQLite connection (read-only)
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| CardDbError::ConnectionFailed(e.to_string()))?;

    // Schema validation
    let validation = schema_guard::validate_card_db_schema(&conn)
        .map_err(|e| CardDbError::QueryFailed(format!("Schema validation query failed: {}", e)))?;

    if !validation.valid {
        return Err(CardDbError::SchemaInvalid {
            missing_tables: validation.missing_tables,
        });
    }

    // Detect whether the DB has the extended columns; fall back to base SELECT if not.
    const EXT_COLS: &[&str] = &[
        "Colors", "ColorIdentity", "Types", "Subtypes", "Supertypes",
        "IsRebalanced", "RebalancedCardGrpId", "IsPrimaryCard",
    ];
    let has_ext = !validation
        .missing_columns
        .iter()
        .any(|(t, c)| t == "Cards" && EXT_COLS.contains(&c.as_str()));

    let (color_map, type_map, subtype_map, supertype_map) = if has_ext {
        (
            load_enum_map(&conn, "CardColor")?,
            load_enum_map(&conn, "CardType")?,
            load_enum_map(&conn, "SubType")?,
            load_enum_map(&conn, "SuperType")?,
        )
    } else {
        (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new())
    };

    const CARD_QUERY_BASE: &str =
        "SELECT c.GrpId, l.Loc, c.ExpansionCode, c.Rarity, c.CollectorNumber,
                c.OldSchoolManaText, c.Power, c.Toughness, c.AlternateDeckLimit
         FROM Cards c
         JOIN Localizations_enUS l ON c.TitleId = l.LocId AND l.Formatted = 1
         WHERE c.IsToken = 0";
    const CARD_QUERY_EXT: &str =
        "SELECT c.GrpId, l.Loc, c.ExpansionCode, c.Rarity, c.CollectorNumber,
                c.OldSchoolManaText, c.Power, c.Toughness, c.AlternateDeckLimit,
                c.Colors, c.ColorIdentity, c.Types, c.Subtypes, c.Supertypes,
                c.IsRebalanced, c.RebalancedCardGrpId, c.IsPrimaryCard
         FROM Cards c
         JOIN Localizations_enUS l ON c.TitleId = l.LocId AND l.Formatted = 1
         WHERE c.IsToken = 0";

    let query = if has_ext { CARD_QUERY_EXT } else { CARD_QUERY_BASE };
    let mut stmt = conn
        .prepare(query)
        .map_err(|e| CardDbError::QueryFailed(format!("Failed to prepare card query: {}", e)))?;

    let mut cards: HashMap<i32, CardInfo> = HashMap::new();
    let mut rows = stmt
        .query([])
        .map_err(|e| CardDbError::QueryFailed(e.to_string()))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| CardDbError::QueryFailed(format!("Failed to read card row: {}", e)))?
    {
        let grp_id: i32 = row.get(0).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let raw_name: String = row.get(1).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let set_code: String = row.get(2).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let rarity_int: i32 = row.get(3).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let collector_number: Option<String> = row.get(4).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let mana_text: Option<String> = row.get(5).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let power: Option<String> = row.get(6).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let toughness: Option<String> = row.get(7).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
        let alt_deck_limit: Option<i32> = row.get(8).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;

        let (colors, color_identity, types, subtypes, supertypes, is_rebalanced, rebalanced_grp_id, is_primary_card) =
            if has_ext {
                let colors_raw: Option<String> = row.get(9).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let identity_raw: Option<String> = row.get(10).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let types_raw: Option<String> = row.get(11).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let subtypes_raw: Option<String> = row.get(12).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let supertypes_raw: Option<String> = row.get(13).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let is_reb: Option<i32> = row.get(14).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let reb_grp: Option<i32> = row.get(15).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                let is_prim: Option<i32> = row.get(16).map_err(|e| CardDbError::QueryFailed(e.to_string()))?;
                (
                    resolve_enum_names(&parse_id_list(&colors_raw.unwrap_or_default()), &color_map)
                        .iter()
                        .filter_map(|n| color_letter(n))
                        .map(String::from)
                        .collect::<Vec<_>>(),
                    resolve_enum_names(&parse_id_list(&identity_raw.unwrap_or_default()), &color_map)
                        .iter()
                        .filter_map(|n| color_letter(n))
                        .map(String::from)
                        .collect::<Vec<_>>(),
                    resolve_enum_names(&parse_id_list(&types_raw.unwrap_or_default()), &type_map),
                    resolve_enum_names(&parse_id_list(&subtypes_raw.unwrap_or_default()), &subtype_map),
                    resolve_enum_names(&parse_id_list(&supertypes_raw.unwrap_or_default()), &supertype_map),
                    is_reb.unwrap_or(0) != 0,
                    reb_grp.filter(|g| *g != 0),
                    is_prim.unwrap_or(1) != 0,
                )
            } else {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), false, None, true)
            };

        cards.insert(
            grp_id,
            CardInfo {
                grp_id,
                name: clean_name(&raw_name),
                set_code,
                rarity: Rarity::from_i32(rarity_int),
                collector_number: collector_number.unwrap_or_default(),
                cmc: parse_cmc(&mana_text.unwrap_or_default()),
                power: parse_stat(&power.unwrap_or_default()),
                toughness: parse_stat(&toughness.unwrap_or_default()),
                deck_limit: alt_deck_limit.unwrap_or(0),
                colors,
                color_identity,
                types,
                subtypes,
                supertypes,
                is_rebalanced,
                rebalanced_grp_id,
                is_primary_card,
            },
        );
    }

    let valid_ids: HashSet<i32> = cards.keys().copied().collect();

    Ok(LoadResult {
        cards,
        valid_ids,
        db_path: db_path_str,
        schema_warnings: validation.missing_columns,
        extra_tables: validation.extra_tables,
    })
}

// ── Managed state (Tauri-dependent) ─────────────────────────────────

/// Managed state holding the loaded card database.
pub struct CardDb {
    // Arc so callers on the hot path (deck build every ~30s) can hand out a
    // cheap ref-counted snapshot instead of cloning a ~30K-entry HashMap.
    cards: Option<Arc<HashMap<i32, CardInfo>>>,
    valid_ids: Option<HashSet<i32>>,
    db_path: Option<String>,
}

impl CardDb {
    pub fn new() -> Self {
        Self {
            cards: None,
            valid_ids: None,
            db_path: None,
        }
    }

    /// Get a card by GrpId.
    pub fn get_card(&self, grp_id: i32) -> Option<&CardInfo> {
        self.cards.as_ref()?.get(&grp_id)
    }

    /// Check if a GrpId is a valid (non-token) card.
    pub fn is_valid_id(&self, grp_id: i32) -> bool {
        self.valid_ids
            .as_ref()
            .map_or(false, |ids| ids.contains(&grp_id))
    }

    /// Get the full set of valid card IDs (for memory scanner).
    pub fn valid_ids(&self) -> Option<&HashSet<i32>> {
        self.valid_ids.as_ref()
    }

    /// Get the full card map (for frontend bulk transfer). Callers that need
    /// to hold the snapshot beyond the lock should prefer `all_cards_arc()`
    /// which is O(1) instead of cloning ~30K entries.
    pub fn all_cards(&self) -> Option<&HashMap<i32, CardInfo>> {
        self.cards.as_deref()
    }

    /// Get an Arc snapshot of the full card map. Cheap clone (refcount bump).
    /// Snapshot is stable across the lifetime of the returned Arc even if the
    /// DB is later reloaded.
    pub fn all_cards_arc(&self) -> Option<Arc<HashMap<i32, CardInfo>>> {
        self.cards.clone()
    }

    /// Number of cards loaded.
    pub fn card_count(&self) -> usize {
        self.cards.as_ref().map_or(0, |c| c.len())
    }

    /// Whether the database has been loaded.
    pub fn is_loaded(&self) -> bool {
        self.cards.is_some()
    }

    /// Path to the loaded database file, if any.
    pub fn db_path(&self) -> Option<String> {
        self.db_path.clone()
    }

    /// Load the card database. Resolves path, validates schema, executes query.
    ///
    /// Thin wrapper over `resolve_db_path` + `load_cards_from_path` that
    /// emits Tauri events and diagnostic codes on each outcome.
    pub fn load(&mut self, app: &AppHandle, path_override: Option<&str>) -> Result<usize, String> {
        // Step 1: Resolve database path
        let db_path = resolve_db_path(path_override).map_err(|e| {
            let diag = match &e {
                CardDbError::InstallNotFound(_) => &CDB_001,
                CardDbError::DatabaseNotFound(_) => &CDB_002,
                CardDbError::OverrideNotFound(_) => &CDB_007,
                _ => &CDB_003,
            };
            let msg = e.to_string();
            diagnostics::emit_error(app, diag, &msg);
            msg
        })?;

        log::info!("Loading card database from: {}", db_path.display());

        // Step 2: Load cards from resolved path
        let result = load_cards_from_path(&db_path).map_err(|e| {
            let diag = match &e {
                CardDbError::ConnectionFailed(_) => &CDB_003,
                CardDbError::SchemaInvalid { .. } => &CDB_004,
                CardDbError::QueryFailed(_) => &CDB_005,
                _ => &CDB_003,
            };
            let msg = e.to_string();
            diagnostics::emit_error(app, diag, &msg);
            msg
        })?;

        // Step 3: Emit schema warnings (non-fatal)
        for (table, col) in &result.schema_warnings {
            let msg = format!("Missing column {}.{} — some data may be unavailable", table, col);
            diagnostics::emit_warning(app, &diagnostics::SCH_002, &msg);
            let _ = app.emit(
                events::schema_guard::WARNING,
                &events::SchemaWarningPayload {
                    code: diagnostics::SCH_002.code.to_string(),
                    message: msg.clone(),
                    details: vec![format!("{}.{}", table, col)],
                },
            );
        }

        // Log extra tables as informational
        if !result.extra_tables.is_empty() {
            let msg = format!(
                "Unknown tables in card database: {}",
                result.extra_tables.join(", ")
            );
            diagnostics::emit_info(app, &diagnostics::SCH_003, &msg);
        }

        let count = result.cards.len();

        if count == 0 {
            let msg = "Card database loaded but contains zero cards".to_string();
            diagnostics::emit_warning(app, &CDB_006, &msg);
        }

        // Step 4: Store in state
        self.cards = Some(Arc::new(result.cards));
        self.valid_ids = Some(result.valid_ids);
        self.db_path = Some(result.db_path.clone());

        // Step 5: Emit success event
        let _ = app.emit(
            events::card_db::LOADED,
            &events::CardDbLoadedPayload {
                card_count: count,
                db_path: result.db_path,
            },
        );

        log::info!("Card database loaded: {} cards", count);
        Ok(count)
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn parse_id_list_handles_empty_and_csv() {
        assert_eq!(parse_id_list(""), Vec::<i32>::new());
        assert_eq!(parse_id_list("1,3"), vec![1, 3]);
        assert_eq!(parse_id_list(" 2 , x ,5"), vec![2, 5]);
    }

    #[test]
    fn resolve_enum_names_maps_known_and_skips_unknown() {
        let mut m = HashMap::new();
        m.insert(1, "White".to_string());
        m.insert(2, "Blue".to_string());
        assert_eq!(resolve_enum_names(&[2, 9, 1], &m), vec!["Blue".to_string(), "White".to_string()]);
    }

    #[test]
    fn color_letter_maps_mtga_names() {
        assert_eq!(color_letter("White"), Some("W"));
        assert_eq!(color_letter("Green"), Some("G"));
        assert_eq!(color_letter("Colorless"), None);
    }
}
