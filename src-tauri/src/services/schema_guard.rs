// SchemaGuard — validate MTGA card database schema before querying.
//
// Checks sqlite_master for expected tables and PRAGMA table_info for expected
// columns. Missing tables are fatal (can't proceed). Missing columns produce
// warnings (proceed with fallback). Extra/unknown tables are logged as
// informational for developer review.

use rusqlite::Connection;

/// Expected tables and their required columns.
const EXPECTED_SCHEMA: &[(&str, &[&str])] = &[
    (
        "Cards",
        &[
            "GrpId",
            "TitleId",
            "ExpansionCode",
            "Rarity",
            "CollectorNumber",
            "IsToken",
            "OldSchoolManaText",
            "Power",
            "Toughness",
            "AlternateDeckLimit",
            "Colors",
            "ColorIdentity",
            "Types",
            "Subtypes",
            "Supertypes",
            "IsRebalanced",
            "RebalancedCardGrpId",
            "IsPrimaryCard",
        ],
    ),
    ("Localizations_enUS", &["LocId", "Loc", "Formatted"]),
    ("Enums", &["Type", "Value", "LocId"]),
];

/// Known tables that are part of the card DB but not required for our queries.
const KNOWN_OPTIONAL_TABLES: &[&str] = &[
    "Versions",
    "Abilities",
    "AltPrintings",
    "AltToBasePrintings",
    "Prompts",
    "Localizations",
];

/// Result of schema validation.
#[derive(Debug)]
pub struct SchemaValidationResult {
    pub valid: bool,
    pub missing_tables: Vec<String>,
    pub missing_columns: Vec<(String, String)>, // (table, column)
    pub extra_tables: Vec<String>,
}

/// Validate the card database schema against expected tables and columns.
///
/// Returns a result describing what matched, what's missing, and what's new.
/// Missing tables → `valid = false` (can't proceed).
/// Missing columns → `valid = true` but warnings should be emitted.
/// Extra tables → informational only.
pub fn validate_card_db_schema(
    conn: &Connection,
) -> Result<SchemaValidationResult, rusqlite::Error> {
    // Get all table names from sqlite_master
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let actual_tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut missing_tables = Vec::new();
    let mut missing_columns = Vec::new();
    let mut extra_tables = Vec::new();

    // Check expected tables exist and have required columns
    for &(table, columns) in EXPECTED_SCHEMA {
        if !actual_tables.iter().any(|t| t == table) {
            missing_tables.push(table.to_string());
            continue;
        }

        // Get actual columns for this table
        let mut col_stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
        let actual_columns: Vec<String> = col_stmt
            .query_map([], |row| row.get::<_, String>(1))? // column 1 = name
            .filter_map(|r| r.ok())
            .collect();

        for &col in columns {
            if !actual_columns.iter().any(|c| c == col) {
                missing_columns.push((table.to_string(), col.to_string()));
            }
        }
    }

    // Find tables we don't expect (not in required or known-optional)
    let expected_names: Vec<&str> = EXPECTED_SCHEMA.iter().map(|&(name, _)| name).collect();
    for table in &actual_tables {
        if !expected_names.contains(&table.as_str())
            && !KNOWN_OPTIONAL_TABLES.contains(&table.as_str())
            && table != "sqlite_sequence"
        {
            extra_tables.push(table.clone());
        }
    }

    let valid = missing_tables.is_empty();

    Ok(SchemaValidationResult {
        valid,
        missing_tables,
        missing_columns,
        extra_tables,
    })
}
