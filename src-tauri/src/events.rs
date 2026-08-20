// All backend event types. Each service emits typed events through Tauri's
// event system. Adding a new event = one variant here + one emit call in the
// service. Frontend subscribes via @tauri-apps/api/event.

use serde::Serialize;

/// App-level events — emitted by the main startup sequence.
pub mod app {
    pub const STARTUP_COMPLETE: &str = "app:startup_complete";
    pub const CARD_DB_READY: &str = "app:card_db_ready";
    pub const LOG_READY: &str = "app:log_ready";
    pub const SCAN_STARTING: &str = "app:scan_starting";
}

/// CardDbService events — emitted when card database is loaded or fails.
pub mod card_db {
    pub const LOADED: &str = "card_db:loaded";
    pub const ERROR: &str = "card_db:error";
}

/// DiagnosticService events — structured diagnostic logging.
pub mod diagnostics {
    pub const LOGGED: &str = "diagnostics:logged";
}

/// SchemaGuard events — schema validation warnings.
pub mod schema_guard {
    pub const WARNING: &str = "schema_guard:warning";
}

/// MemoryService events — process discovery, collection scanning, and watch mode.
pub mod memory {
    pub const PROCESS_FOUND: &str = "memory:process_found";
    pub const SCAN_STARTED: &str = "memory:scan_started";
    pub const SCAN_PROGRESS: &str = "memory:scan_progress";
    pub const COLLECTION_SCANNED: &str = "memory:collection_scanned";
    pub const SCAN_ERROR: &str = "memory:scan_error";
    pub const COLLECTION_CHANGED: &str = "memory:collection_changed";
    pub const WATCH_STARTED: &str = "memory:watch_started";
    pub const WATCH_STOPPED: &str = "memory:watch_stopped";
    pub const INVENTORY_SCANNED: &str = "memory:inventory_scanned";
    pub const INVENTORY_CHANGED: &str = "memory:inventory_changed";
    pub const OFFLINE_HYDRATED: &str = "memory:offline_hydrated";
}

/// LogService events — log tailing, session detection, inventory changes.
pub mod log {
    pub const WATCHER_STARTED: &str = "log:watcher_started";
    pub const WATCHER_STOPPED: &str = "log:watcher_stopped";
    pub const SESSION_STARTED: &str = "log:session_started";
    pub const INVENTORY_UPDATED: &str = "log:inventory_updated";
    pub const CARDS_GRANTED: &str = "log:cards_granted";
    pub const LOG_EVENT: &str = "log:event";
    pub const EVENTS_UPDATED: &str = "log:events_updated";
    pub const COSMETICS_UPDATED: &str = "log:cosmetics_updated";
    pub const MASTERY_UPDATED: &str = "log:mastery_updated";
    pub const MATCH_STARTING: &str = "log:match_starting";
    pub const MATCH_RESULT: &str = "log:match_result";
    pub const ACHIEVEMENTS_UPDATED: &str = "log:achievements_updated";
}

/// HistoryDb events — snapshot persistence notifications.
pub mod history {
    pub const SNAPSHOT_SAVED: &str = "history:snapshot_saved";
}

/// ImageCacheService events — background image download progress.
pub mod image_cache {
    pub const PROGRESS: &str = "image_cache:progress";
    pub const COMPLETE: &str = "image_cache:complete";
}

/// DeckService events — bulk ingest lifecycle and community source refreshes.
pub mod deck {
    pub const INGEST_PROGRESS: &str = "deck:ingest_progress";
    pub const INGEST_COMPLETE: &str = "deck:ingest_complete";
    pub const INGEST_ERROR: &str = "deck:ingest_error";
    pub const SOURCES_UPDATED: &str = "deck:sources_updated";
}

// --- Payload structs ---

#[derive(Debug, Clone, Serialize)]
pub struct StartupCompletePayload {
    pub has_collection: bool,
    pub has_inventory: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardDbReadyPayload {
    pub card_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogReadyPayload {
    pub has_inventory: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanStartingPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct CardDbLoadedPayload {
    pub card_count: usize,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardDbErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticPayload {
    pub code: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaWarningPayload {
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct ScanErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessFoundPayload {
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgressPayload {
    pub current_region: usize,
    pub total_regions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionScannedPayload {
    pub unique_cards: usize,
    pub total_copies: usize,
    pub candidates_found: usize,
    pub scan_duration_ms: u64,
}

// --- Watch mode payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct CollectionChangedPayload {
    pub added: Vec<(i32, i32)>,
    pub increased: Vec<(i32, i32, i32)>,
    pub removed: Vec<(i32, i32)>,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchStartedPayload {
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchStoppedPayload {
    pub reason: String,
}

// --- Inventory scanning payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct InventoryScannedPayload {
    pub scalars: crate::models::MemoryInventoryScalars,
    pub candidates_found: usize,
    pub address_hex: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InventoryChangedPayload {
    pub previous: crate::models::MemoryInventoryScalars,
    pub current: crate::models::MemoryInventoryScalars,
}

#[derive(Debug, Clone, Serialize)]
pub struct OfflineHydratedPayload {
    pub snapshot_timestamp: i64,
    pub unique_cards: usize,
    pub total_copies: usize,
}

// --- LogService payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct WatcherStartedPayload {
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatcherStoppedPayload {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStartedPayload {
    pub inventory: crate::models::PlayerInventory,
    pub is_refresh: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchStartingPayload {
    pub game_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchResultPayload {
    pub result: crate::models::MatchResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct AchievementsUpdatedPayload {
    pub achievements: serde_json::Value,
    pub is_refresh: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InventoryUpdatedPayload {
    pub changes: Vec<crate::models::InventoryChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardsGrantedPayload {
    pub cards: Vec<crate::models::GrantedCard>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEventPayload {
    pub event_type: String,
    pub summary: String,
    pub raw_size: usize,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventsUpdatedPayload {
    pub courses: Vec<crate::models::EventCourse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CosmeticsUpdatedPayload {
    pub cosmetics: crate::models::PlayerCosmetics,
}

#[derive(Debug, Clone, Serialize)]
pub struct MasteryUpdatedPayload {
    pub state: crate::models::MasteryState,
}

// --- ImageCacheService payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct ImageCacheProgressPayload {
    pub completed: usize,
    pub total: usize,
    pub new_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageCacheCompletePayload {
    pub total_downloaded: usize,
    pub total_skipped: usize,
    pub total_failed: usize,
}

// --- HistoryDb payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotSavedPayload {
    pub snapshot_type: String,
    pub snapshot_id: i64,
    pub trigger: String,
}

// --- DeckService payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct IngestProgressPayload {
    pub stage: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestCompletePayload {
    pub changed: bool,
    pub oracles: u64,
    pub unresolved_grp: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourcesUpdatedPayload {
    pub host: String,
    pub key: String,
}
