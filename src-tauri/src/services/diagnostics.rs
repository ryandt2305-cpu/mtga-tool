// Diagnostic code registry and emit helpers.
//
// Every error path in the backend gets a stable diagnostic code.
// User sees: friendly message + code. Developer searches codebase for the code.
// All codes documented in docs/diagnostic-codes.md.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::events;

/// A single diagnostic entry stored in the ring buffer.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticEntry {
    pub code: String,
    pub level: String,
    pub message: String,
    pub timestamp_epoch_s: u64,
}

/// In-memory ring buffer of recent diagnostic entries (capacity 200).
///
/// Managed as `Mutex<DiagnosticLog>` via Tauri state. Queryable from Tauri
/// commands and the debug HTTP server.
pub struct DiagnosticLog {
    entries: VecDeque<DiagnosticEntry>,
    capacity: usize,
}

impl DiagnosticLog {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(200),
            capacity: 200,
        }
    }

    pub fn push(&mut self, entry: DiagnosticEntry) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Return the most recent `limit` entries in chronological order (oldest first).
    pub fn recent(&self, limit: usize) -> Vec<DiagnosticEntry> {
        let skip = self.entries.len().saturating_sub(limit);
        self.entries.iter().skip(skip).cloned().collect()
    }
}

/// A diagnostic code with its static identifier.
pub struct DiagnosticCode {
    pub code: &'static str,
}

// --- CardDbService codes (CDB-*) ---

/// MTGA install directory not found at any known path.
pub const CDB_001: DiagnosticCode = DiagnosticCode { code: "CDB-001" };

/// No Raw_CardDatabase_*.mtga file found in install directory.
pub const CDB_002: DiagnosticCode = DiagnosticCode { code: "CDB-002" };

/// Failed to open SQLite connection to card database.
pub const CDB_003: DiagnosticCode = DiagnosticCode { code: "CDB-003" };

/// Schema validation failed — required tables or columns missing.
pub const CDB_004: DiagnosticCode = DiagnosticCode { code: "CDB-004" };

/// SQL query failed when loading card data.
pub const CDB_005: DiagnosticCode = DiagnosticCode { code: "CDB-005" };

/// Card database loaded with zero cards (unexpected).
pub const CDB_006: DiagnosticCode = DiagnosticCode { code: "CDB-006" };

/// Override path provided but file does not exist.
pub const CDB_007: DiagnosticCode = DiagnosticCode { code: "CDB-007" };

// --- MemoryService codes (MEM-*) ---

/// MTGA.exe process not found / not running.
pub const MEM_001: DiagnosticCode = DiagnosticCode { code: "MEM-001" };

/// Failed to open process handle (permissions?).
pub const MEM_002: DiagnosticCode = DiagnosticCode { code: "MEM-002" };

/// Failed to enumerate memory regions.
pub const MEM_003: DiagnosticCode = DiagnosticCode { code: "MEM-003" };

/// No collection data found in memory.
pub const MEM_004: DiagnosticCode = DiagnosticCode { code: "MEM-004" };

/// Card database not loaded — prerequisite for scanning.
pub const MEM_005: DiagnosticCode = DiagnosticCode { code: "MEM-005" };

/// ReadProcessMemory failed for a chunk (continues scanning).
pub const MEM_006: DiagnosticCode = DiagnosticCode { code: "MEM-006" };

/// Zero readable memory regions found.
pub const MEM_007: DiagnosticCode = DiagnosticCode { code: "MEM-007" };

/// Collection watch started.
pub const MEM_008: DiagnosticCode = DiagnosticCode { code: "MEM-008" };

/// Quick rescan failed — cached address invalid, attempting full scan fallback.
pub const MEM_009: DiagnosticCode = DiagnosticCode { code: "MEM-009" };

/// Inventory scalar scan started.
pub const MEM_010: DiagnosticCode = DiagnosticCode { code: "MEM-010" };

/// Inventory scan found no matching struct in memory.
pub const MEM_011: DiagnosticCode = DiagnosticCode { code: "MEM-011" };

/// Inventory re-read failed — cached address stale.
pub const MEM_012: DiagnosticCode = DiagnosticCode { code: "MEM-012" };

/// Inventory scan requires StartHook data (need known values as search anchors).
pub const MEM_013: DiagnosticCode = DiagnosticCode { code: "MEM-013" };

/// Inventory validation failed — implausible values at address.
pub const MEM_014: DiagnosticCode = DiagnosticCode { code: "MEM-014" };

/// Combined scan started (info).
pub const MEM_015: DiagnosticCode = DiagnosticCode { code: "MEM-015" };

/// Combined scan — collection phase complete (info).
pub const MEM_016: DiagnosticCode = DiagnosticCode { code: "MEM-016" };

/// Combined scan — inventory phase complete (info).
pub const MEM_017: DiagnosticCode = DiagnosticCode { code: "MEM-017" };

/// Scan cancelled by user (info).
pub const MEM_018: DiagnosticCode = DiagnosticCode { code: "MEM-018" };

/// Scan already in progress (warning).
pub const MEM_019: DiagnosticCode = DiagnosticCode { code: "MEM-019" };

/// Staleness threshold reached — spawning background verification scan.
pub const MEM_020: DiagnosticCode = DiagnosticCode { code: "MEM-020" };

/// Stale address detected — relocated collection via verification scan.
pub const MEM_021: DiagnosticCode = DiagnosticCode { code: "MEM-021" };

/// Process poller started — background thread watching for MTGA.exe.
pub const MEM_022: DiagnosticCode = DiagnosticCode { code: "MEM-022" };

/// Process poller detected MTGA.exe — triggering combined scan.
pub const MEM_023: DiagnosticCode = DiagnosticCode { code: "MEM-023" };

// --- SchemaGuard codes (SCH-*) ---

/// Required table missing from card database.
pub const SCH_001: DiagnosticCode = DiagnosticCode { code: "SCH-001" };

/// Required column missing from an expected table.
pub const SCH_002: DiagnosticCode = DiagnosticCode { code: "SCH-002" };

/// Unexpected extra tables found (informational, not an error).
pub const SCH_003: DiagnosticCode = DiagnosticCode { code: "SCH-003" };

/// Failed to query sqlite_master for schema inspection.
pub const SCH_004: DiagnosticCode = DiagnosticCode { code: "SCH-004" };

/// Failed to query PRAGMA table_info for column inspection.
pub const SCH_005: DiagnosticCode = DiagnosticCode { code: "SCH-005" };

/// Emit a diagnostic event to the frontend and push to the ring buffer.
pub fn emit_diagnostic(app: &AppHandle, code: &str, level: &str, message: &str) {
    let payload = events::DiagnosticPayload {
        code: code.to_string(),
        level: level.to_string(),
        message: message.to_string(),
    };
    let _ = app.emit(events::diagnostics::LOGGED, &payload);

    // Push to the in-memory ring buffer (if managed state is available).
    // try_state: DiagnosticLog may not be managed yet during very early startup.
    if let Some(state) = app.try_state::<Mutex<DiagnosticLog>>() {
        if let Ok(mut log) = state.lock() {
            log.push(DiagnosticEntry {
                code: code.to_string(),
                level: level.to_string(),
                message: message.to_string(),
                timestamp_epoch_s: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }

    // Also log to Rust's log framework for backend visibility
    match level {
        "error" => log::error!("[{}] {}", code, message),
        "warn" => log::warn!("[{}] {}", code, message),
        "info" => log::info!("[{}] {}", code, message),
        _ => log::debug!("[{}] {}", code, message),
    }
}

/// Convenience: emit an error-level diagnostic.
pub fn emit_error(app: &AppHandle, code: &DiagnosticCode, message: &str) {
    emit_diagnostic(app, code.code, "error", message);
}

/// Convenience: emit a warning-level diagnostic.
pub fn emit_warning(app: &AppHandle, code: &DiagnosticCode, message: &str) {
    emit_diagnostic(app, code.code, "warn", message);
}

/// Convenience: emit an info-level diagnostic.
pub fn emit_info(app: &AppHandle, code: &DiagnosticCode, message: &str) {
    emit_diagnostic(app, code.code, "info", message);
}

// --- LogService codes (LOG-*) ---

/// Player.log not found at expected path.
pub const LOG_001: DiagnosticCode = DiagnosticCode { code: "LOG-001" };

/// Failed to read Player.log.
pub const LOG_002: DiagnosticCode = DiagnosticCode { code: "LOG-002" };

/// File watcher setup failed.
pub const LOG_003: DiagnosticCode = DiagnosticCode { code: "LOG-003" };

/// JSON parse error in log line (non-fatal, skip line).
pub const LOG_004: DiagnosticCode = DiagnosticCode { code: "LOG-004" };

/// StartHook parsing failed — unexpected structure.
pub const LOG_005: DiagnosticCode = DiagnosticCode { code: "LOG-005" };

/// InventoryChange parsing failed — unexpected structure.
pub const LOG_006: DiagnosticCode = DiagnosticCode { code: "LOG-006" };

/// File rotation detected (game restarted).
pub const LOG_007: DiagnosticCode = DiagnosticCode { code: "LOG-007" };

/// Watcher thread stopped unexpectedly.
pub const LOG_008: DiagnosticCode = DiagnosticCode { code: "LOG-008" };

/// CourseList (EventGetCoursesV2) parse failed — unexpected structure.
pub const LOG_009: DiagnosticCode = DiagnosticCode { code: "LOG-009" };

/// Cosmetics parse failed in StartHook (informational — no Cosmetics object).
pub const LOG_010: DiagnosticCode = DiagnosticCode { code: "LOG-010" };

/// Cosmetics change parse failed in InventoryChange.
pub const LOG_011: DiagnosticCode = DiagnosticCode { code: "LOG-011" };

/// ProgressUpdate parse failed — unexpected structure.
pub const LOG_012: DiagnosticCode = DiagnosticCode { code: "LOG-012" };

/// UpdatedGraphs parse failed in StartHook.
pub const LOG_013: DiagnosticCode = DiagnosticCode { code: "LOG-013" };

/// New cosmetic category discovered (info level — schema discovery, not error).
pub const LOG_014: DiagnosticCode = DiagnosticCode { code: "LOG-014" };

/// Expected field disappeared from log event (≥80% historical presence).
pub const LOG_015: DiagnosticCode = DiagnosticCode { code: "LOG-015" };

/// Field type changed vs historical observation in log event.
pub const LOG_016: DiagnosticCode = DiagnosticCode { code: "LOG-016" };

/// New field discovered in log event (never seen before).
pub const LOG_017: DiagnosticCode = DiagnosticCode { code: "LOG-017" };

/// MatchGameRoomStateChanged parse warning — missing or unexpected field.
pub const LOG_018: DiagnosticCode = DiagnosticCode { code: "LOG-018" };

// --- FeedDb codes (FEED-*) ---

/// Failed to resolve app_data_dir for feed database.
pub const FEED_001: DiagnosticCode = DiagnosticCode { code: "FEED-001" };

/// Failed to create app data directory.
pub const FEED_002: DiagnosticCode = DiagnosticCode { code: "FEED-002" };

/// Failed to open SQLite connection for feed database.
pub const FEED_003: DiagnosticCode = DiagnosticCode { code: "FEED-003" };

/// Failed to create feed_entries table (schema init).
pub const FEED_004: DiagnosticCode = DiagnosticCode { code: "FEED-004" };

/// Failed to insert a feed entry.
pub const FEED_005: DiagnosticCode = DiagnosticCode { code: "FEED-005" };

/// Failed to query feed entries.
pub const FEED_006: DiagnosticCode = DiagnosticCode { code: "FEED-006" };

/// Failed to query feed sessions.
pub const FEED_007: DiagnosticCode = DiagnosticCode { code: "FEED-007" };

// --- HistoryDb codes (HST-*) ---

/// Failed to resolve app_data_dir for history database.
pub const HST_001: DiagnosticCode = DiagnosticCode { code: "HST-001" };

/// Failed to create app data directory.
pub const HST_002: DiagnosticCode = DiagnosticCode { code: "HST-002" };

/// Failed to open/create history database.
pub const HST_003: DiagnosticCode = DiagnosticCode { code: "HST-003" };

/// Schema migration failed.
pub const HST_004: DiagnosticCode = DiagnosticCode { code: "HST-004" };

/// Economy snapshot save failed.
pub const HST_005: DiagnosticCode = DiagnosticCode { code: "HST-005" };

/// Collection snapshot save failed.
pub const HST_006: DiagnosticCode = DiagnosticCode { code: "HST-006" };

/// Card grants log failed.
pub const HST_007: DiagnosticCode = DiagnosticCode { code: "HST-007" };

/// History query failed.
pub const HST_008: DiagnosticCode = DiagnosticCode { code: "HST-008" };

/// Collection snapshot skipped (dedup — identical to previous).
pub const HST_009: DiagnosticCode = DiagnosticCode { code: "HST-009" };

/// Database pruned (N snapshots removed).
pub const HST_010: DiagnosticCode = DiagnosticCode { code: "HST-010" };

/// Corrupt database detected, recreated.
pub const HST_011: DiagnosticCode = DiagnosticCode { code: "HST-011" };

// --- DeckService codes (DECK-*) ---

/// deckbuilder.db open/migrate failed.
pub const DECK_001: DiagnosticCode = DiagnosticCode { code: "DECK-001" };
/// Scryfall bulk metadata fetch failed.
pub const DECK_002: DiagnosticCode = DiagnosticCode { code: "DECK-002" };
/// Scryfall bulk download failed.
pub const DECK_003: DiagnosticCode = DiagnosticCode { code: "DECK-003" };
/// Bulk parse/field anomaly.
pub const DECK_004: DiagnosticCode = DiagnosticCode { code: "DECK-004" };
/// grp_id ↔ oracle unresolved (aggregated count).
pub const DECK_005: DiagnosticCode = DiagnosticCode { code: "DECK-005" };
/// Tag hierarchy anomaly (cycle / missing parent).
pub const DECK_006: DiagnosticCode = DiagnosticCode { code: "DECK-006" };
/// Community source fetch failed.
pub const DECK_007: DiagnosticCode = DiagnosticCode { code: "DECK-007" };
/// Community source daily cap reached.
pub const DECK_008: DiagnosticCode = DiagnosticCode { code: "DECK-008" };
/// Invalid build request.
pub const DECK_009: DiagnosticCode = DiagnosticCode { code: "DECK-009" };
/// Template infeasible — targets relaxed.
pub const DECK_010: DiagnosticCode = DiagnosticCode { code: "DECK-010" };
/// Saved deck read/write failed.
pub const DECK_011: DiagnosticCode = DiagnosticCode { code: "DECK-011" };
/// Ingest aborted, previous data retained.
pub const DECK_012: DiagnosticCode = DiagnosticCode { code: "DECK-012" };
/// Staples aggregate rebuilt (info).
pub const DECK_013: DiagnosticCode = DiagnosticCode { code: "DECK-013" };
