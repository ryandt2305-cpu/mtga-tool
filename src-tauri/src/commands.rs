use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

use tauri::{Emitter, Manager};

use crate::events;
use crate::models::{CardInfo, EventCourse, MasteryState, MemoryInventoryScalars, PlayerCosmetics, PlayerInventory};
use crate::services::card_db::CardDb;
use crate::services::diagnostics::{self, DiagnosticEntry, DiagnosticLog, MEM_019};
use crate::services::feed_db::{FeedDb, FeedEntryInput, FeedEntryRow, SessionSummary};
use crate::services::history_db::{
    CardGrantRow, CollectionSnapshotSummary, EconomySnapshotRow, HistoryDb, SnapshotSummary,
};
use crate::services::image_cache::{ImageCacheService, ImageCacheStatus, ImageSyncEntry};
use crate::services::log::schema::{DriftEntry, SchemaObserver};
use crate::services::log::LogService;
use crate::services::log::LogStatus;
use crate::services::memory::{self, pipeline, MemoryService};

// --- Service registry types ---

#[derive(Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub state: String,
    pub detail: String,
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct DiagnosticsExport {
    pub app_version: String,
    pub services: Vec<ServiceStatus>,
    pub timestamp_epoch_s: u64,
}

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        name: "MTGA Hub".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Current backend state snapshot for frontend hydration on startup.
/// Solves the race condition where events fire before the webview subscribes.
#[derive(Serialize)]
pub struct AppStatus {
    pub card_db_loaded: bool,
    pub card_db_card_count: usize,
    pub card_db_path: Option<String>,
    pub mtga_pid: Option<u32>,
    pub log_watcher_running: bool,
    pub log_has_inventory: bool,
    pub has_collection: bool,
    pub collection_unique: usize,
    pub collection_total: usize,
    pub is_watching: bool,
    pub is_scanning: bool,
}

#[tauri::command]
pub fn get_status(
    card_db_state: State<'_, Mutex<CardDb>>,
    log_state: State<'_, Mutex<LogService>>,
    memory_state: State<'_, Mutex<MemoryService>>,
) -> AppStatus {
    let db = card_db_state.lock().expect("CardDb lock poisoned");
    let mtga_pid = memory::process::find_process("MTGA.exe")
        .ok()
        .flatten();
    let log = log_state.lock().expect("LogService lock poisoned");
    // try_lock: don't block the frontend if a scan is holding the mutex
    let (has_collection, collection_unique, collection_total, is_watching, is_scanning) =
        match memory_state.try_lock() {
            Ok(memory) => {
                let (has, unique, total) = memory.last_scan_stats();
                (has, unique, total, memory.is_watching(), memory.is_scan_in_progress())
            }
            Err(_) => (false, 0, 0, false, true), // if mutex busy, likely scanning
        };
    AppStatus {
        card_db_loaded: db.is_loaded(),
        card_db_card_count: db.card_count(),
        card_db_path: db.db_path(),
        mtga_pid,
        log_watcher_running: log.is_running(),
        log_has_inventory: log.current_inventory().is_some(),
        has_collection,
        collection_unique,
        collection_total,
        is_watching,
        is_scanning,
    }
}

/// Load the card database from MTGA's install directory or an override path.
///
/// Returns the number of cards loaded on success, or an error string.
#[tauri::command]
pub fn load_card_db(
    app: tauri::AppHandle,
    state: State<'_, Mutex<CardDb>>,
    path_override: Option<String>,
) -> Result<usize, String> {
    let mut db = state.lock().map_err(|e| format!("Lock poisoned: {}", e))?;
    db.load(&app, path_override.as_deref())
}

/// Look up a single card by GrpId.
///
/// Returns None if the database isn't loaded or the ID doesn't exist.
#[tauri::command]
pub fn get_card(grp_id: i32, state: State<'_, Mutex<CardDb>>) -> Option<CardInfo> {
    let db = state.lock().ok()?;
    db.get_card(grp_id).cloned()
}

/// Get the full card database for frontend bulk transfer.
///
/// Returns the complete grpId→CardInfo map (~2.4MB over IPC).
/// Frontend caches this for local filtering/searching.
#[tauri::command]
pub fn get_all_cards(state: State<'_, Mutex<CardDb>>) -> HashMap<i32, CardInfo> {
    let db = state.lock().expect("CardDb lock poisoned");
    db.all_cards().cloned().unwrap_or_default()
}

/// Get the cached collection from the last memory scan.
///
/// Returns None if no scan has been performed yet or if a scan is in progress.
#[tauri::command]
pub fn get_collection(state: State<'_, Mutex<MemoryService>>) -> Option<HashMap<i32, i32>> {
    let memory = state.try_lock().ok()?;
    memory.last_collection().cloned()
}

/// Scan MTGA.exe process memory for the card collection (non-blocking).
///
/// Returns immediately. The scan runs on a background thread and results
/// arrive via the `memory:collection_scanned` event. Fetch the collection
/// data with `get_collection` after receiving the event.
#[tauri::command]
pub fn scan_collection(
    app: tauri::AppHandle,
    memory_state: State<'_, Mutex<MemoryService>>,
    card_db_state: State<'_, Mutex<CardDb>>,
    log_state: State<'_, Mutex<LogService>>,
) -> Result<(), String> {
    // Brief lock: check not already scanning
    let (cancel_flag, in_progress_flag) = {
        let memory = memory_state
            .lock()
            .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
        if memory.is_scan_in_progress() {
            diagnostics::emit_warning(&app, &MEM_019, "Scan already in progress");
            return Err("A scan is already in progress".to_string());
        }
        (memory.scan_cancel_flag(), memory.scan_in_progress_flag())
    };

    // Brief lock: extract valid_ids
    let valid_ids = {
        let db = card_db_state
            .lock()
            .map_err(|e| format!("CardDb lock poisoned: {}", e))?;
        match db.valid_ids() {
            Some(ids) => ids.clone(),
            None => {
                return Err("Card database not loaded — load it before scanning".to_string());
            }
        }
    };

    // Brief lock: extract anchor + non_collectible
    let (anchor_ids, non_collectible_ids) = {
        let log = log_state
            .lock()
            .map_err(|e| format!("LogService lock poisoned: {}", e))?;
        (log.anchor_ids(), log.non_collectible_ids())
    };

    // Set scan_in_progress and reset cancel flag
    {
        let memory = memory_state
            .lock()
            .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
        memory.set_scan_in_progress(true);
        memory.reset_cancel_flag();
    }

    let _ = app.emit(
        events::memory::SCAN_STARTED,
        &events::ScanStartedPayload {},
    );

    // Spawn background thread — NO mutex held
    // Clone app first, then use it inside the thread to access state
    let app_bg = app;
    std::thread::Builder::new()
        .name("scan-collection".to_string())
        .spawn(move || {
            let result = pipeline::run_scan(
                &app_bg,
                &valid_ids,
                &anchor_ids,
                &non_collectible_ids,
                Some(&cancel_flag),
            );

            match result {
                Ok(output) => {
                    // Brief lock: store results
                    let mem_state = app_bg.state::<Mutex<MemoryService>>();
                    let mut memory = mem_state.lock().expect("MemoryService lock poisoned");
                    memory.store_scan_output(output);
                    memory.set_scan_in_progress(false);

                    // Auto-start watch
                    if !memory.is_watching() {
                        if let Err(e) = memory.start_watch(
                            &app_bg,
                            Arc::new(valid_ids),
                            Arc::new(non_collectible_ids),
                            Arc::new(anchor_ids),
                        ) {
                            log::warn!("Auto-start collection watch failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    in_progress_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                    let _ = app_bg.emit(
                        events::memory::SCAN_ERROR,
                        &events::ScanErrorPayload {
                            message: e.clone(),
                        },
                    );
                    log::error!("Background scan failed: {}", e);
                }
            }
        })
        .map_err(|e| {
            // Failed to spawn — reset flag
            if let Ok(memory) = memory_state.lock() {
                memory.set_scan_in_progress(false);
            }
            format!("Failed to spawn scan thread: {}", e)
        })?;

    Ok(()) // Returns immediately
}

/// Cancel a scan that is currently in progress.
#[tauri::command]
pub fn cancel_scan(memory_state: State<'_, Mutex<MemoryService>>) -> Result<(), String> {
    let memory = memory_state
        .lock()
        .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
    if memory.is_scan_in_progress() {
        memory.cancel_scan();
        log::info!("Scan cancellation requested");
    }
    Ok(())
}

// --- LogService commands ---

/// Start the log watcher. Returns the path to Player.log on success.
#[tauri::command]
pub fn start_log_watcher(
    app: tauri::AppHandle,
    state: State<'_, Mutex<LogService>>,
) -> Result<String, String> {
    let mut log = state
        .lock()
        .map_err(|e| format!("LogService lock poisoned: {}", e))?;
    log.start(&app)
}

/// Stop the log watcher.
#[tauri::command]
pub fn stop_log_watcher(state: State<'_, Mutex<LogService>>) -> Result<(), String> {
    let mut log = state
        .lock()
        .map_err(|e| format!("LogService lock poisoned: {}", e))?;
    log.stop();
    Ok(())
}

/// Get the current inventory snapshot from the log watcher.
#[tauri::command]
pub fn get_inventory(state: State<'_, Mutex<LogService>>) -> Option<PlayerInventory> {
    let log = state.lock().ok()?;
    log.current_inventory()
}

/// Get the log watcher status.
#[tauri::command]
pub fn get_log_status(state: State<'_, Mutex<LogService>>) -> LogStatus {
    let log = state.lock().expect("LogService lock poisoned");
    log.status()
}

/// Get the current active event courses from the log watcher.
#[tauri::command]
pub fn get_event_courses(state: State<'_, Mutex<LogService>>) -> Vec<EventCourse> {
    let log = state.lock().expect("LogService lock poisoned");
    log.current_courses()
}

/// Get the current cosmetics state from the log watcher.
#[tauri::command]
pub fn get_cosmetics(state: State<'_, Mutex<LogService>>) -> PlayerCosmetics {
    let log = state.lock().expect("LogService lock poisoned");
    log.current_cosmetics()
}

/// Get the current mastery state from the log watcher.
#[tauri::command]
pub fn get_mastery(state: State<'_, Mutex<LogService>>) -> Option<MasteryState> {
    let log = state.lock().expect("LogService lock poisoned");
    log.current_mastery()
}

/// Get the schema drift report from the current session's log event observations.
#[tauri::command]
pub fn get_log_schema_drift(
    state: State<'_, Mutex<SchemaObserver>>,
) -> Vec<DriftEntry> {
    match state.try_lock() {
        Ok(observer) => observer.drift_report(),
        Err(_) => Vec::new(),
    }
}

// --- Inventory scanning commands ---

/// Scan MTGA.exe memory for inventory scalars (gems, wcRare, etc.).
///
/// Requires LogService to have parsed StartHook (needs known values as search anchors).
#[tauri::command]
pub fn scan_inventory(
    app: tauri::AppHandle,
    memory_state: State<'_, Mutex<MemoryService>>,
    log_state: State<'_, Mutex<LogService>>,
) -> Result<MemoryInventoryScalars, String> {
    // Build search params from LogService's current inventory
    let search_params = {
        let log = log_state
            .lock()
            .map_err(|e| format!("LogService lock poisoned: {}", e))?;
        let inv = log
            .current_inventory()
            .ok_or("No inventory data from StartHook — wait for MTGA to finish loading")?;

        memory::inventory::InventorySearchParams {
            gold: Some(inv.gold as i32),
            wc_common: Some(inv.wc_common),
            wc_uncommon: Some(inv.wc_uncommon),
            wc_mythic: Some(inv.wc_mythic),
            wc_track_position: Some(inv.wc_track_position),
        }
    };

    let mut memory = memory_state
        .lock()
        .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
    memory.scan_inventory(&app, search_params)
}

/// Get the merged inventory: log-derived base with memory-scanned scalar overrides.
///
/// Returns the full PlayerInventory with correct gems/wcRare from memory scanning.
/// Falls back to log-only inventory if no memory scan has been performed.
#[tauri::command]
pub fn get_merged_inventory(
    log_state: State<'_, Mutex<LogService>>,
    memory_state: State<'_, Mutex<MemoryService>>,
) -> Option<PlayerInventory> {
    let log = log_state.lock().ok()?;
    let inv = log.current_inventory()?;

    let memory = memory_state.lock().ok()?;
    match memory.last_inventory_scalars() {
        Some(scalars) => Some(inv.merge_with_memory(scalars)),
        None => Some(inv),
    }
}

// --- Collection watch commands ---

/// Start polling the cached collection address for changes.
///
/// Requires a prior full scan (needs cached address). If no prior scan exists,
/// does a full scan first.
#[tauri::command]
pub fn start_collection_watch(
    app: tauri::AppHandle,
    memory_state: State<'_, Mutex<MemoryService>>,
    card_db_state: State<'_, Mutex<CardDb>>,
    log_state: State<'_, Mutex<LogService>>,
) -> Result<(), String> {
    // Extract valid_ids from card DB
    let valid_ids = {
        let db = card_db_state
            .lock()
            .map_err(|e| format!("CardDb lock poisoned: {}", e))?;
        match db.valid_ids() {
            Some(ids) => Arc::new(ids.clone()),
            None => return Err("Card database not loaded — load it before watching".to_string()),
        }
    };

    // Get anchor_ids and non_collectible_ids from LogService
    let (anchor_ids, non_collectible_ids) = {
        let log = log_state
            .lock()
            .map_err(|e| format!("LogService lock poisoned: {}", e))?;
        (Arc::new(log.anchor_ids()), Arc::new(log.non_collectible_ids()))
    };

    let mut memory = memory_state
        .lock()
        .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;

    // If no prior scan, do one now
    if !memory.has_scan() {
        memory.scan(&app, &valid_ids, &anchor_ids, &non_collectible_ids)?;
    }

    memory.start_watch(&app, valid_ids, non_collectible_ids, anchor_ids)
}

/// Stop the collection watch loop.
#[tauri::command]
pub fn stop_collection_watch(
    memory_state: State<'_, Mutex<MemoryService>>,
) -> Result<(), String> {
    let mut memory = memory_state
        .lock()
        .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
    memory.stop_watch()
}

// --- Service registry commands ---

pub fn build_service_statuses(
    card_db: &CardDb,
    log: &LogService,
    memory: Option<&MemoryService>,
) -> Vec<ServiceStatus> {
    let mut out = Vec::new();

    // Card Database
    out.push(ServiceStatus {
        id: "card_db".to_string(),
        name: "Card Database".to_string(),
        state: if card_db.is_loaded() { "ok" } else { "error" }.to_string(),
        detail: if card_db.is_loaded() {
            format!("{} cards loaded", card_db.card_count())
        } else {
            "Not loaded".to_string()
        },
        path: card_db.db_path(),
    });

    // Log Watcher
    let log_status = log.status();
    out.push(ServiceStatus {
        id: "log".to_string(),
        name: "Log Watcher".to_string(),
        state: if log_status.running { "active" } else { "idle" }.to_string(),
        detail: if !log_status.running {
            "Stopped".to_string()
        } else if log_status.has_inventory {
            "Watching \u{00b7} inventory parsed".to_string()
        } else {
            "Watching \u{00b7} waiting for session".to_string()
        },
        path: log_status.log_path,
    });

    // Memory Scanner + Collection Watch + MTGA Process
    match memory {
        Some(mem) => {
            let (has_scan, unique, total) = mem.last_scan_stats();
            let scanning = mem.is_scan_in_progress();

            out.push(ServiceStatus {
                id: "memory".to_string(),
                name: "Memory Scanner".to_string(),
                state: if scanning {
                    "scanning"
                } else if has_scan {
                    "ok"
                } else {
                    "idle"
                }
                .to_string(),
                detail: if scanning {
                    "Scanning\u{2026}".to_string()
                } else if has_scan {
                    format!("{} unique / {} total", unique, total)
                } else {
                    "No scan performed".to_string()
                },
                path: None,
            });

            out.push(ServiceStatus {
                id: "collection_watch".to_string(),
                name: "Collection Watch".to_string(),
                state: if mem.is_watching() { "active" } else { "idle" }.to_string(),
                detail: if mem.is_watching() {
                    "Polling for changes".to_string()
                } else {
                    "Not watching".to_string()
                },
                path: None,
            });
        }
        None => {
            // Mutex busy (likely scanning)
            out.push(ServiceStatus {
                id: "memory".to_string(),
                name: "Memory Scanner".to_string(),
                state: "scanning".to_string(),
                detail: "Scanning\u{2026}".to_string(),
                path: None,
            });
            out.push(ServiceStatus {
                id: "collection_watch".to_string(),
                name: "Collection Watch".to_string(),
                state: "idle".to_string(),
                detail: "Busy".to_string(),
                path: None,
            });
        }
    }

    // MTGA Process
    let mtga_pid = memory::process::find_process("MTGA.exe").ok().flatten();
    out.push(ServiceStatus {
        id: "process".to_string(),
        name: "MTGA Process".to_string(),
        state: if mtga_pid.is_some() { "ok" } else { "idle" }.to_string(),
        detail: match mtga_pid {
            Some(pid) => format!("PID {}", pid),
            None => "Not detected".to_string(),
        },
        path: None,
    });

    out
}

/// Get the current status of all backend services.
#[tauri::command]
pub fn get_service_statuses(
    card_db_state: State<'_, Mutex<CardDb>>,
    log_state: State<'_, Mutex<LogService>>,
    memory_state: State<'_, Mutex<MemoryService>>,
) -> Vec<ServiceStatus> {
    let db = card_db_state.lock().expect("CardDb lock poisoned");
    let log = log_state.lock().expect("LogService lock poisoned");
    let memory = memory_state.try_lock().ok();
    build_service_statuses(&db, &log, memory.as_deref())
}

/// Export all diagnostic state for clipboard copy.
#[tauri::command]
pub fn export_diagnostics(
    card_db_state: State<'_, Mutex<CardDb>>,
    log_state: State<'_, Mutex<LogService>>,
    memory_state: State<'_, Mutex<MemoryService>>,
) -> DiagnosticsExport {
    let db = card_db_state.lock().expect("CardDb lock poisoned");
    let log = log_state.lock().expect("LogService lock poisoned");
    let memory = memory_state.try_lock().ok();
    DiagnosticsExport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        services: build_service_statuses(&db, &log, memory.as_deref()),
        timestamp_epoch_s: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    }
}

/// Get recent diagnostic entries from the in-memory ring buffer.
#[tauri::command]
pub fn get_recent_diagnostics(
    limit: Option<usize>,
    state: State<'_, Mutex<DiagnosticLog>>,
) -> Vec<DiagnosticEntry> {
    match state.try_lock() {
        Ok(dlog) => dlog.recent(limit.unwrap_or(50)),
        Err(_) => Vec::new(),
    }
}

// --- FeedDb commands ---

/// Insert a feed entry into the persistent database. Returns the new row ID.
#[tauri::command]
pub fn insert_feed_entry(
    entry: FeedEntryInput,
    state: State<'_, Mutex<FeedDb>>,
) -> Result<i64, String> {
    let db = state.lock().map_err(|e| format!("FeedDb lock poisoned: {}", e))?;
    db.insert(&entry)
}

/// Query persisted feed entries with cursor-based pagination.
#[tauri::command]
pub fn get_feed_entries(
    limit: i32,
    before_id: Option<i64>,
    state: State<'_, Mutex<FeedDb>>,
) -> Result<Vec<FeedEntryRow>, String> {
    let db = state.lock().map_err(|e| format!("FeedDb lock poisoned: {}", e))?;
    db.get_entries(limit, before_id)
}

/// Get a summary of all sessions with feed entries.
#[tauri::command]
pub fn get_feed_sessions(
    state: State<'_, Mutex<FeedDb>>,
) -> Result<Vec<SessionSummary>, String> {
    let db = state.lock().map_err(|e| format!("FeedDb lock poisoned: {}", e))?;
    db.get_sessions()
}

// --- HistoryDb commands ---

/// Get economy snapshot history with optional date-range filter.
#[tauri::command]
pub fn get_economy_history(
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    limit: Option<i32>,
    state: State<'_, Mutex<HistoryDb>>,
) -> Result<Vec<EconomySnapshotRow>, String> {
    let db = state
        .lock()
        .map_err(|e| format!("HistoryDb lock poisoned: {}", e))?;
    db.get_economy_history(from_ts, to_ts, limit.unwrap_or(100))
}

/// Get collection snapshot summaries (metadata only, no card map).
#[tauri::command]
pub fn get_collection_snapshots(
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    limit: Option<i32>,
    state: State<'_, Mutex<HistoryDb>>,
) -> Result<Vec<CollectionSnapshotSummary>, String> {
    let db = state
        .lock()
        .map_err(|e| format!("HistoryDb lock poisoned: {}", e))?;
    db.get_collection_snapshots(from_ts, to_ts, limit.unwrap_or(100))
}

/// Get the full card map for a single collection snapshot (for diff/export).
#[tauri::command]
pub fn get_collection_snapshot_detail(
    id: i64,
    state: State<'_, Mutex<HistoryDb>>,
) -> Result<Option<HashMap<i32, i32>>, String> {
    let db = state
        .lock()
        .map_err(|e| format!("HistoryDb lock poisoned: {}", e))?;
    db.get_collection_snapshot_detail(id)
}

/// Get card grant log entries with optional date-range and card filter.
#[tauri::command]
pub fn get_card_grants(
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    limit: Option<i32>,
    grp_id_filter: Option<i32>,
    state: State<'_, Mutex<HistoryDb>>,
) -> Result<Vec<CardGrantRow>, String> {
    let db = state
        .lock()
        .map_err(|e| format!("HistoryDb lock poisoned: {}", e))?;
    db.get_card_grants(from_ts, to_ts, limit.unwrap_or(100), grp_id_filter)
}

/// Manually trigger snapshots of all currently available data.
#[tauri::command]
pub fn take_snapshot(
    history_state: State<'_, Mutex<HistoryDb>>,
    log_state: State<'_, Mutex<LogService>>,
    memory_state: State<'_, Mutex<MemoryService>>,
) -> Result<SnapshotSummary, String> {
    let db = history_state
        .lock()
        .map_err(|e| format!("HistoryDb lock poisoned: {}", e))?;
    let log = log_state
        .lock()
        .map_err(|e| format!("LogService lock poisoned: {}", e))?;

    let mut summary = SnapshotSummary {
        economy_id: None,
        collection_id: None,
        cosmetics_id: None,
        mastery_id: None,
    };

    // Economy
    if let Some(inv) = log.current_inventory() {
        match db.save_economy_snapshot("manual", &inv) {
            Ok(id) => summary.economy_id = Some(id),
            Err(e) => log::warn!("{}", e),
        }
    }

    // Cosmetics
    let cosmetics = log.current_cosmetics();
    match db.save_cosmetic_snapshot("manual", &cosmetics) {
        Ok(id) => summary.cosmetics_id = Some(id),
        Err(e) => log::warn!("{}", e),
    }

    // Mastery
    if let Some(mastery) = log.current_mastery() {
        match db.save_mastery_snapshot("manual", &mastery) {
            Ok(id) => summary.mastery_id = Some(id),
            Err(e) => log::warn!("{}", e),
        }
    }

    // Collection — try_lock to avoid blocking if scanning
    if let Ok(memory) = memory_state.try_lock() {
        if let Some(collection) = memory.last_collection() {
            match db.save_collection_snapshot("manual", collection, None) {
                Ok(id) if id >= 0 => summary.collection_id = Some(id),
                Ok(_) => {} // dedup skip
                Err(e) => log::warn!("{}", e),
            }
        }
    }

    Ok(summary)
}

// --- ImageCacheService commands ---

/// Get the current image cache status (cache dir path + list of cached filenames).
#[tauri::command]
pub fn get_image_cache_status(
    state: State<'_, Arc<ImageCacheService>>,
) -> ImageCacheStatus {
    state.status()
}

/// Queue a batch of card images for background download. Returns immediately.
#[tauri::command]
pub async fn sync_image_cache(
    app: tauri::AppHandle,
    state: State<'_, Arc<ImageCacheService>>,
    entries: Vec<ImageSyncEntry>,
) -> Result<(), String> {
    let svc = Arc::clone(&*state);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = svc.sync(&app, entries).await {
            log::error!("Image cache sync failed: {}", e);
        }
    });
    Ok(())
}
