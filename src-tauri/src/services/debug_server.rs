// Localhost-only HTTP debug server for agent access to app state.
//
// Exposes read endpoints for all backend state and action endpoints for scans
// and watcher control. Binds to 127.0.0.1:9876 (configurable via
// MTGA_HUB_DEBUG_PORT env var). Falls back gracefully if port is in use.

use std::sync::Mutex;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::commands::{self, AppInfo, AppStatus, DiagnosticsExport, ServiceStatus};
use crate::models::{EventCourse, MasteryState, PlayerInventory};
use crate::services::card_db::CardDb;
use crate::services::diagnostics::{DiagnosticEntry, DiagnosticLog};
use crate::services::feed_db::FeedDb;
use crate::services::history_db::{EconomySnapshotRow, HistoryDb, SnapshotSummary};
use crate::services::log::schema::{DriftEntry, SchemaObserver};
use crate::services::log::LogService;
use crate::services::memory::MemoryService;

#[derive(Clone)]
struct ServerState {
    app: tauri::AppHandle,
}

/// Start the debug HTTP server. Blocks forever; intended to be spawned.
pub async fn start(app: tauri::AppHandle) {
    let port: u16 = std::env::var("MTGA_HUB_DEBUG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9876);

    let state = ServerState { app };

    let router = Router::new()
        // Read endpoints
        .route("/api/dump", get(handle_dump))
        .route("/api/status", get(handle_status))
        .route("/api/inventory", get(handle_inventory))
        .route("/api/history/economy", get(handle_economy_history))
        .route("/api/history/grants", get(handle_card_grants))
        .route("/api/history/collection", get(handle_collection_snapshots))
        .route("/api/feed", get(handle_feed))
        .route("/api/events", get(handle_events))
        .route("/api/diagnostics", get(handle_diagnostics))
        .route("/api/diagnostics/recent", get(handle_diagnostics_recent))
        // Action endpoints
        .route("/api/snapshot", post(handle_snapshot))
        .route("/api/scan", post(handle_scan))
        .route("/api/log/start", post(handle_log_start))
        .route("/api/log/stop", post(handle_log_stop))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            log::warn!(
                "DBG-001: Debug server failed to bind to {} — {}. \
                 App continues without debug server.",
                addr,
                e
            );
            return;
        }
    };

    log::info!("Debug server listening on http://{}", addr);

    if let Err(e) = axum::serve(listener, router).await {
        log::error!("DBG-002: Debug server stopped — {}", e);
    }
}

// ---------------------------------------------------------------------------
// Query parameter structs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct HistoryParams {
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<i32>,
}

#[derive(Deserialize)]
struct GrantParams {
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<i32>,
    grp_id: Option<i32>,
}

#[derive(Deserialize)]
struct FeedParams {
    limit: Option<i32>,
    before: Option<i64>,
}

#[derive(Deserialize)]
struct LimitParam {
    limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Error helper
// ---------------------------------------------------------------------------

fn err_json(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg })),
    )
}

// ---------------------------------------------------------------------------
// GET /api/dump
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DumpResponse {
    app: AppInfo,
    timestamp: u64,
    services: Vec<ServiceStatus>,
    status: AppStatus,
    inventory: Option<PlayerInventory>,
    collection_summary: Option<CollectionSummary>,
    event_courses: Vec<EventCourse>,
    schema_drift: Vec<DriftEntry>,
    recent_diagnostics: Vec<DiagnosticEntry>,
    latest_economy_snapshot: Option<EconomySnapshotRow>,
    economy_snapshot_count: usize,
    mastery: Option<MasteryState>,
}

#[derive(Serialize)]
struct CollectionSummary {
    unique: usize,
    total: usize,
}

async fn handle_dump(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let app_info = commands::get_app_info();

    let db_state = app.state::<Mutex<CardDb>>();
    let db = db_state.lock().expect("CardDb lock");
    let log_state = app.state::<Mutex<LogService>>();
    let log = log_state.lock().expect("Log lock");
    let mem_state = app.state::<Mutex<MemoryService>>();
    let memory = mem_state.try_lock().ok();

    let services = commands::build_service_statuses(&db, &log, memory.as_deref());
    let status = build_app_status(&db, &log, memory.as_deref());

    let inventory = log.current_inventory();
    let event_courses = log.current_courses();
    let mastery = log.current_mastery();

    let collection_summary = memory.as_deref().and_then(|m| {
        let (has, unique, total) = m.last_scan_stats();
        if has {
            Some(CollectionSummary { unique, total })
        } else {
            None
        }
    });

    // Drop service locks before acquiring other locks
    drop(db);
    drop(log);
    drop(memory);

    let schema_state = app.state::<Mutex<SchemaObserver>>();
    let schema_drift = match schema_state.try_lock() {
        Ok(obs) => obs.drift_report(),
        Err(_) => Vec::new(),
    };

    let dlog_state = app.state::<Mutex<DiagnosticLog>>();
    let recent_diagnostics = match dlog_state.try_lock() {
        Ok(dlog) => dlog.recent(20),
        Err(_) => Vec::new(),
    };

    let hdb_state = app.state::<Mutex<HistoryDb>>();
    let (latest_economy_snapshot, economy_snapshot_count) = match hdb_state.try_lock() {
        Ok(hdb) => {
            let latest = hdb
                .get_economy_history(None, None, 1)
                .ok()
                .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });
            let count = hdb
                .get_economy_history(None, None, 10_000)
                .map(|v| v.len())
                .unwrap_or(0);
            (latest, count)
        }
        Err(_) => (None, 0),
    };

    Json(DumpResponse {
        app: app_info,
        timestamp: now,
        services,
        status,
        inventory,
        collection_summary,
        event_courses,
        schema_drift,
        recent_diagnostics,
        latest_economy_snapshot,
        economy_snapshot_count,
        mastery,
    })
}

// ---------------------------------------------------------------------------
// GET /api/status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StatusResponse {
    app: AppInfo,
    status: AppStatus,
    services: Vec<ServiceStatus>,
}

async fn handle_status(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let db_state = app.state::<Mutex<CardDb>>();
    let db = db_state.lock().expect("CardDb lock");
    let log_state = app.state::<Mutex<LogService>>();
    let log = log_state.lock().expect("Log lock");
    let mem_state = app.state::<Mutex<MemoryService>>();
    let memory = mem_state.try_lock().ok();

    Json(StatusResponse {
        app: commands::get_app_info(),
        status: build_app_status(&db, &log, memory.as_deref()),
        services: commands::build_service_statuses(&db, &log, memory.as_deref()),
    })
}

// ---------------------------------------------------------------------------
// GET /api/inventory
// ---------------------------------------------------------------------------

async fn handle_inventory(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let log_state = app.state::<Mutex<LogService>>();
    let log = log_state.lock().expect("Log lock");
    let inv = log.current_inventory();
    let mem_state = app.state::<Mutex<MemoryService>>();
    let memory = mem_state.try_lock().ok();

    let merged =
        inv.map(
            |base| match memory.as_deref().and_then(|m| m.last_inventory_scalars()) {
                Some(scalars) => base.merge_with_memory(scalars),
                None => base,
            },
        );

    Json(merged)
}

// ---------------------------------------------------------------------------
// GET /api/history/economy
// ---------------------------------------------------------------------------

async fn handle_economy_history(
    State(state): State<ServerState>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    let app = &state.app;
    let hdb_state = app.state::<Mutex<HistoryDb>>();
    let r = match hdb_state.try_lock() {
        Ok(db) => {
            match db.get_economy_history(params.from, params.to, params.limit.unwrap_or(100)) {
                Ok(rows) => Ok(Json(rows)),
                Err(e) => Err(err_json(&e)),
            }
        }
        Err(_) => Err(err_json("HistoryDb busy")),
    };
    r
}

// ---------------------------------------------------------------------------
// GET /api/history/grants
// ---------------------------------------------------------------------------

async fn handle_card_grants(
    State(state): State<ServerState>,
    Query(params): Query<GrantParams>,
) -> impl IntoResponse {
    let app = &state.app;
    let hdb_state = app.state::<Mutex<HistoryDb>>();
    let r = match hdb_state.try_lock() {
        Ok(db) => match db.get_card_grants(
            params.from,
            params.to,
            params.limit.unwrap_or(100),
            params.grp_id,
        ) {
            Ok(rows) => Ok(Json(rows)),
            Err(e) => Err(err_json(&e)),
        },
        Err(_) => Err(err_json("HistoryDb busy")),
    };
    r
}

// ---------------------------------------------------------------------------
// GET /api/history/collection
// ---------------------------------------------------------------------------

async fn handle_collection_snapshots(
    State(state): State<ServerState>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    let app = &state.app;
    let hdb_state = app.state::<Mutex<HistoryDb>>();
    let r = match hdb_state.try_lock() {
        Ok(db) => {
            match db.get_collection_snapshots(
                params.from,
                params.to,
                params.limit.unwrap_or(100),
            ) {
                Ok(rows) => Ok(Json(rows)),
                Err(e) => Err(err_json(&e)),
            }
        }
        Err(_) => Err(err_json("HistoryDb busy")),
    };
    r
}

// ---------------------------------------------------------------------------
// GET /api/feed
// ---------------------------------------------------------------------------

async fn handle_feed(
    State(state): State<ServerState>,
    Query(params): Query<FeedParams>,
) -> impl IntoResponse {
    let app = &state.app;
    let feed_state = app.state::<Mutex<FeedDb>>();
    let r = match feed_state.try_lock() {
        Ok(db) => match db.get_entries(params.limit.unwrap_or(50), params.before) {
            Ok(rows) => Ok(Json(rows)),
            Err(e) => Err(err_json(&e)),
        },
        Err(_) => Err(err_json("FeedDb busy")),
    };
    r
}

// ---------------------------------------------------------------------------
// GET /api/events
// ---------------------------------------------------------------------------

async fn handle_events(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let log_state = app.state::<Mutex<LogService>>();
    let log = log_state.lock().expect("Log lock");
    Json(log.current_courses())
}

// ---------------------------------------------------------------------------
// GET /api/diagnostics
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DiagnosticsResponse {
    #[serde(flatten)]
    export: DiagnosticsExport,
    recent: Vec<DiagnosticEntry>,
}

async fn handle_diagnostics(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;

    let db_state = app.state::<Mutex<CardDb>>();
    let db = db_state.lock().expect("CardDb lock");
    let log_state = app.state::<Mutex<LogService>>();
    let log = log_state.lock().expect("Log lock");
    let mem_state = app.state::<Mutex<MemoryService>>();
    let memory = mem_state.try_lock().ok();

    let export = DiagnosticsExport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        services: commands::build_service_statuses(&db, &log, memory.as_deref()),
        timestamp_epoch_s: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    drop(db);
    drop(log);
    drop(memory);

    let dlog_state = app.state::<Mutex<DiagnosticLog>>();
    let recent = match dlog_state.try_lock() {
        Ok(dlog) => dlog.recent(50),
        Err(_) => Vec::new(),
    };

    Json(DiagnosticsResponse { export, recent })
}

// ---------------------------------------------------------------------------
// GET /api/diagnostics/recent
// ---------------------------------------------------------------------------

async fn handle_diagnostics_recent(
    State(state): State<ServerState>,
    Query(params): Query<LimitParam>,
) -> impl IntoResponse {
    let app = &state.app;
    let dlog_state = app.state::<Mutex<DiagnosticLog>>();
    let r = match dlog_state.try_lock() {
        Ok(dlog) => Json(dlog.recent(params.limit.unwrap_or(50))),
        Err(_) => Json(Vec::new()),
    };
    r
}

// ---------------------------------------------------------------------------
// POST /api/snapshot
// ---------------------------------------------------------------------------

async fn handle_snapshot(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;

    let hdb_state = app.state::<Mutex<HistoryDb>>();
    let hdb = match hdb_state.try_lock() {
        Ok(db) => db,
        Err(_) => return Err(err_json("HistoryDb busy")),
    };
    let log_state = app.state::<Mutex<LogService>>();
    let log = match log_state.try_lock() {
        Ok(l) => l,
        Err(_) => return Err(err_json("LogService busy")),
    };

    let mut summary = SnapshotSummary {
        economy_id: None,
        collection_id: None,
        cosmetics_id: None,
        mastery_id: None,
    };

    if let Some(inv) = log.current_inventory() {
        if let Ok(id) = hdb.save_economy_snapshot("debug_api", &inv) {
            summary.economy_id = Some(id);
        }
    }

    let cosmetics = log.current_cosmetics();
    if let Ok(id) = hdb.save_cosmetic_snapshot("debug_api", &cosmetics) {
        summary.cosmetics_id = Some(id);
    }

    if let Some(mastery) = log.current_mastery() {
        if let Ok(id) = hdb.save_mastery_snapshot("debug_api", &mastery) {
            summary.mastery_id = Some(id);
        }
    }

    drop(log);

    let mem_state = app.state::<Mutex<MemoryService>>();
    if let Ok(memory) = mem_state.try_lock() {
        if let Some(collection) = memory.last_collection() {
            if let Ok(id) = hdb.save_collection_snapshot("debug_api", collection, None) {
                if id >= 0 {
                    summary.collection_id = Some(id);
                }
            }
        }
    }

    Ok(Json(summary))
}

// ---------------------------------------------------------------------------
// POST /api/scan
// ---------------------------------------------------------------------------

async fn handle_scan(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;

    // Check if scan already in progress
    {
        let mem_state = app.state::<Mutex<MemoryService>>();
        let in_progress = mem_state
            .try_lock()
            .map(|m| m.is_scan_in_progress())
            .unwrap_or(false);
        if in_progress {
            return Err(err_json("Scan already in progress"));
        }
    }

    // Extract valid_ids
    let valid_ids = {
        let db_state = app.state::<Mutex<CardDb>>();
        let db = match db_state.try_lock() {
            Ok(db) => db,
            Err(_) => return Err(err_json("CardDb busy")),
        };
        match db.valid_ids() {
            Some(ids) => ids.clone(),
            None => return Err(err_json("Card database not loaded")),
        }
    };

    // Extract anchor + non_collectible
    let (anchor_ids, non_collectible_ids) = {
        let log_state = app.state::<Mutex<LogService>>();
        let log = match log_state.try_lock() {
            Ok(l) => l,
            Err(_) => return Err(err_json("LogService busy")),
        };
        (log.anchor_ids(), log.non_collectible_ids())
    };

    // Set in-progress flags
    {
        let mem_state = app.state::<Mutex<MemoryService>>();
        let memory = match mem_state.try_lock() {
            Ok(m) => m,
            Err(_) => return Err(err_json("MemoryService busy")),
        };
        memory.set_scan_in_progress(true);
        memory.reset_cancel_flag();
    }

    let app_bg = app.clone();
    std::thread::Builder::new()
        .name("debug-scan".to_string())
        .spawn(move || {
            let cancel_flag = {
                let m = app_bg.state::<Mutex<MemoryService>>();
                let memory = m.lock().expect("MemoryService lock");
                memory.scan_cancel_flag()
            };

            let result = crate::services::memory::pipeline::run_scan(
                &app_bg,
                &valid_ids,
                &anchor_ids,
                &non_collectible_ids,
                Some(&cancel_flag),
            );

            match result {
                Ok(output) => {
                    let mem_state = app_bg.state::<Mutex<MemoryService>>();
                    let mut memory = mem_state.lock().expect("MemoryService lock");
                    memory.store_scan_output(output);
                    memory.set_scan_in_progress(false);
                }
                Err(e) => {
                    let mem_state = app_bg.state::<Mutex<MemoryService>>();
                    if let Ok(memory) = mem_state.lock() {
                        memory.set_scan_in_progress(false);
                    }
                    log::error!("Debug API scan failed: {}", e);
                }
            }
        })
        .map_err(|e| err_json(&format!("Failed to spawn scan thread: {}", e)))?;

    Ok(Json(serde_json::json!({ "started": true })))
}

// ---------------------------------------------------------------------------
// POST /api/log/start
// ---------------------------------------------------------------------------

async fn handle_log_start(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let log_state = app.state::<Mutex<LogService>>();
    let mut log = match log_state.try_lock() {
        Ok(l) => l,
        Err(_) => return Err(err_json("LogService busy")),
    };
    match log.start(app) {
        Ok(path) => Ok(Json(serde_json::json!({ "path": path }))),
        Err(e) => Err(err_json(&e)),
    }
}

// ---------------------------------------------------------------------------
// POST /api/log/stop
// ---------------------------------------------------------------------------

async fn handle_log_stop(State(state): State<ServerState>) -> impl IntoResponse {
    let app = &state.app;
    let log_state = app.state::<Mutex<LogService>>();
    let mut log = match log_state.try_lock() {
        Ok(l) => l,
        Err(_) => return Err(err_json("LogService busy")),
    };
    log.stop();
    Ok::<_, (StatusCode, Json<serde_json::Value>)>(Json(
        serde_json::json!({ "stopped": true }),
    ))
}

// ---------------------------------------------------------------------------
// Shared helper: build AppStatus without Tauri State extractors
// ---------------------------------------------------------------------------

fn build_app_status(db: &CardDb, log: &LogService, memory: Option<&MemoryService>) -> AppStatus {
    let mtga_pid = crate::services::memory::process::find_process("MTGA.exe")
        .ok()
        .flatten();

    let (has_collection, collection_unique, collection_total, is_watching, is_scanning) =
        match memory {
            Some(mem) => {
                let (has, unique, total) = mem.last_scan_stats();
                (
                    has,
                    unique,
                    total,
                    mem.is_watching(),
                    mem.is_scan_in_progress(),
                )
            }
            None => (false, 0, 0, false, true),
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
