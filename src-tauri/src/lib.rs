mod commands;
mod commands_deck;
mod commands_deck_community;
mod events;
pub mod models;
pub mod services;

use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

use services::card_db::CardDb;
use services::deck::DeckService;
use services::diagnostics::{emit_error, DiagnosticLog, DECK_001};
use services::feed_db::FeedDb;
use services::history_db::HistoryDb;
use services::image_cache::ImageCacheService;
use services::log::schema::SchemaObserver;
use services::log::LogService;
use services::memory::MemoryService;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new(DiagnosticLog::new()))
        .manage(Mutex::new(CardDb::new()))
        .manage(Mutex::new(MemoryService::new()))
        .manage(Mutex::new(LogService::new()))
        .setup(|app| {
            // Set window icon (needed for dev mode — bundle.icon only applies to packaged builds)
            if let Some(window) = app.get_webview_window("main") {
                let icon_bytes = include_bytes!("../icons/icon.ico");
                match tauri::image::Image::from_bytes(icon_bytes) {
                    Ok(icon) => { let _ = window.set_icon(icon); }
                    Err(e) => log::warn!("Failed to set window icon: {}", e),
                }
            }

            // Initialize FeedDb before the startup thread — schema creation is ~ms,
            // guarantees DB is ready before any event handler could attempt an insert.
            let app_data = app.path().app_data_dir()
                .map_err(|e| format!("FEED-001: {}", e))?;
            std::fs::create_dir_all(&app_data)
                .map_err(|e| format!("FEED-002: {}", e))?;
            let feed_db = FeedDb::open(app_data.join("feed.db"))
                .map_err(|e| { log::error!("{}", e); e })?;
            app.manage(Mutex::new(feed_db));

            // Initialize HistoryDb — persistent history snapshots
            let history_db = HistoryDb::open(app_data.join("history.db"))
                .map_err(|e| { log::error!("{}", e); e })?;
            app.manage(Mutex::new(history_db));

            // Initialize ImageCacheService — disk-cached card images
            let image_cache = ImageCacheService::new(app_data.join("card_images"))
                .map_err(|e| { log::error!("{}", e); e })?;
            app.manage(Arc::new(image_cache));

            // Initialize SchemaObserver — log event field validation
            let schema_observer = SchemaObserver::new(&app_data);
            app.manage(Mutex::new(schema_observer));

            // Initialize DeckService — deck builder store + ingest orchestration.
            // On failure, emit DECK-001 and do not manage — commands guard with
            // `try_state` and return a clear error.
            match DeckService::new(&app_data) {
                Ok(deck_service) => {
                    app.manage(Arc::new(deck_service));
                }
                Err(e) => {
                    log::error!("{}", e);
                    emit_error(&app.handle(), &DECK_001, &e);
                }
            }

            // Spawn the debug HTTP server (localhost-only, non-blocking)
            let debug_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                services::debug_server::start(debug_handle).await;
            });

            let handle = app.handle().clone();

            std::thread::spawn(move || {
                // Phase A: Independent data loading (parallel)
                // CardDb and LogService are independent — neither depends on the other.
                let handle_a = handle.clone();
                let thread_card_db = std::thread::Builder::new()
                    .name("startup-carddb".to_string())
                    .spawn(move || {
                        let state = handle_a.state::<Mutex<CardDb>>();
                        let mut db = state.lock().expect("CardDb lock poisoned during setup");
                        match db.load(&handle_a, None) {
                            Ok(count) => {
                                log::info!("Startup: loaded {} cards", count);
                                let _ = handle_a.emit(
                                    events::app::CARD_DB_READY,
                                    &events::CardDbReadyPayload { card_count: count },
                                );
                                true
                            }
                            Err(e) => {
                                log::error!("Startup: card DB failed — {}", e);
                                false
                            }
                        }
                    })
                    .expect("Failed to spawn carddb thread");

                let handle_b = handle.clone();
                let thread_log = std::thread::Builder::new()
                    .name("startup-log".to_string())
                    .spawn(move || {
                        let state = handle_b.state::<Mutex<LogService>>();
                        let mut log_svc =
                            state.lock().expect("LogService lock poisoned during setup");
                        match log_svc.start(&handle_b) {
                            Ok(path) => {
                                log::info!("Startup: log watcher started on {}", path);
                                let has_inventory = log_svc.current_inventory().is_some();
                                let _ = handle_b.emit(
                                    events::app::LOG_READY,
                                    &events::LogReadyPayload { has_inventory },
                                );
                            }
                            Err(e) => log::warn!("Startup: log watcher skipped — {}", e),
                        }
                    })
                    .expect("Failed to spawn log thread");

                // Wait for both to complete
                let card_db_ok = thread_card_db.join().unwrap_or(false);
                let _ = thread_log.join();

                // Phase A.5: Deck builder ingest — non-blocking, fire-and-forget so
                // Phase B (memory scan) is not delayed by the ~31 MB Scryfall download.
                if card_db_ok {
                    if let Some(deck) = handle.try_state::<Arc<DeckService>>() {
                        let cards_snapshot: Option<std::collections::HashMap<i32, models::CardInfo>> = {
                            let db = handle.state::<Mutex<CardDb>>();
                            let db = db.lock().expect("CardDb lock poisoned during setup");
                            db.all_cards().cloned()
                        };
                        if let Some(cards) = cards_snapshot {
                            let deck = deck.inner().clone();
                            let handle_deck = handle.clone();
                            tauri::async_runtime::spawn(async move {
                                match deck.ensure_bulk(&handle_deck, cards, false).await {
                                    Ok(changed) => log::info!(
                                        "DeckService: ensure_bulk complete (changed={})",
                                        changed
                                    ),
                                    Err(e) => log::warn!("DeckService: ensure_bulk failed — {}", e),
                                }
                            });
                        }
                    }
                }

                // Phase B: Memory scanning (needs results from Phase A)
                // Uses the shared helper so the poller runs identical orchestration.
                if card_db_ok {
                    match services::memory::run_scan_and_start_watch(&handle) {
                        Ok(true) => log::info!("Startup: live scan complete, watch active"),
                        Ok(false) => log::info!(
                            "Startup: live scan produced no collection (MTGA likely not running)"
                        ),
                        Err(e) => log::warn!("Startup: combined scan skipped — {}", e),
                    }
                }

                // Offline hydration — if the live scan didn't populate a collection
                // (MTGA not running or scan failed), fall back to the last snapshot
                // from history.db so the frontend has data to display.
                let offline_hydration: Option<(i64, usize, usize)> = {
                    let mem_state = handle.state::<Mutex<MemoryService>>();
                    let mut memory = mem_state
                        .lock()
                        .expect("MemoryService lock poisoned during setup");
                    if memory.has_scan() {
                        None
                    } else if let Ok(db) = handle.state::<Mutex<HistoryDb>>().try_lock() {
                        match memory.hydrate_from_history_db(&db) {
                            Ok(Some(meta)) => {
                                log::info!(
                                    "Startup: hydrated collection from history.db ({} unique, {} total, ts={})",
                                    meta.1, meta.2, meta.0
                                );
                                Some(meta)
                            }
                            Ok(None) => {
                                log::info!("Startup: no offline collection snapshot available");
                                None
                            }
                            Err(e) => {
                                log::warn!("Startup: offline hydration failed — {}", e);
                                None
                            }
                        }
                    } else {
                        log::warn!("Startup: could not lock HistoryDb for offline hydration");
                        None
                    }
                };

                if let Some((snapshot_timestamp, unique_cards, total_copies)) = offline_hydration {
                    let _ = handle.emit(
                        events::memory::OFFLINE_HYDRATED,
                        &events::OfflineHydratedPayload {
                            snapshot_timestamp,
                            unique_cards,
                            total_copies,
                        },
                    );
                }

                // If the live scan didn't establish the watch (MTGA closed,
                // partial scan, offline hydration only), start the process
                // poller so we auto-reconnect when MTGA launches.
                {
                    let mem_state = handle.state::<Mutex<MemoryService>>();
                    let mut memory = mem_state
                        .lock()
                        .expect("MemoryService lock poisoned during setup");
                    if !memory.is_watching() {
                        memory.start_process_poller(&handle);
                    }
                }

                // Emit final startup_complete event
                let (has_collection, has_inventory) = {
                    let mem_state = handle.state::<Mutex<MemoryService>>();
                    let memory =
                        mem_state.lock().expect("MemoryService lock poisoned during setup");
                    (
                        memory.has_scan(),
                        memory.last_inventory_scalars().is_some(),
                    )
                };
                let _ = handle.emit(
                    events::app::STARTUP_COMPLETE,
                    &events::StartupCompletePayload {
                        has_collection,
                        has_inventory,
                    },
                );
                log::info!(
                    "Startup complete (collection={}, inventory={})",
                    has_collection,
                    has_inventory
                );

                // --- Startup snapshots (non-fatal, fire-and-forget) ---
                if let Ok(db) = handle.state::<Mutex<HistoryDb>>().try_lock() {
                    // Economy snapshot from log inventory
                    if let Ok(log) = handle.state::<Mutex<LogService>>().try_lock() {
                        if let Some(inv) = log.current_inventory() {
                            match db.save_economy_snapshot("startup", &inv) {
                                Ok(id) => log::info!("Startup: economy snapshot saved (id={})", id),
                                Err(e) => log::warn!("{}", e),
                            }
                        }

                        // Cosmetics snapshot
                        let cosmetics = log.current_cosmetics();
                        if !cosmetics.art_styles.is_empty()
                            || !cosmetics.avatars.is_empty()
                            || !cosmetics.sleeves.is_empty()
                        {
                            match db.save_cosmetic_snapshot("startup", &cosmetics) {
                                Ok(id) => log::info!("Startup: cosmetics snapshot saved (id={})", id),
                                Err(e) => log::warn!("{}", e),
                            }
                        }

                        // Mastery snapshot
                        if let Some(mastery) = log.current_mastery() {
                            match db.save_mastery_snapshot("startup", &mastery) {
                                Ok(id) => log::info!("Startup: mastery snapshot saved (id={})", id),
                                Err(e) => log::warn!("{}", e),
                            }
                        }
                    }

                    // Collection snapshot from memory scan
                    if let Ok(memory) = handle.state::<Mutex<MemoryService>>().try_lock() {
                        if let Some(collection) = memory.last_collection() {
                            match db.save_collection_snapshot("startup", collection, None) {
                                Ok(id) if id >= 0 => {
                                    log::info!("Startup: collection snapshot saved (id={})", id)
                                }
                                Ok(_) => log::info!("HST-009: Collection snapshot skipped (identical)"),
                                Err(e) => log::warn!("{}", e),
                            }
                        }
                    }

                    // Prune old snapshots
                    match db.prune(30, 180) {
                        Ok(0) => {}
                        Ok(n) => log::info!("HST-010: Pruned {} old snapshot(s)", n),
                        Err(e) => log::warn!("{}", e),
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_status,
            commands::load_card_db,
            commands::get_card,
            commands::get_all_cards,
            commands::get_collection,
            commands::scan_collection,
            commands::start_log_watcher,
            commands::stop_log_watcher,
            commands::get_inventory,
            commands::get_log_status,
            commands::get_event_courses,
            commands::get_cosmetics,
            commands::get_mastery,
            commands::scan_inventory,
            commands::get_merged_inventory,
            commands::cancel_scan,
            commands::start_collection_watch,
            commands::stop_collection_watch,
            commands::get_service_statuses,
            commands::export_diagnostics,
            commands::get_recent_diagnostics,
            commands::insert_feed_entry,
            commands::get_feed_entries,
            commands::get_feed_sessions,
            commands::get_economy_history,
            commands::get_collection_snapshots,
            commands::get_collection_snapshot_detail,
            commands::get_card_grants,
            commands::take_snapshot,
            commands::get_image_cache_status,
            commands::sync_image_cache,
            commands::get_log_schema_drift,
            commands_deck::deck_get_status,
            commands_deck::deck_refresh_bulk,
            commands_deck::deck_get_templates,
            commands_deck::deck_get_commander_candidates,
            commands_deck::deck_get_seed_decks,
            commands_deck::deck_get_seed_deck_detail,
            commands_deck::deck_score_seed,
            commands_deck::deck_get_commander_community,
            commands_deck::deck_prefetch_community,
            commands_deck::deck_build,
            commands_deck::deck_rescore,
            commands_deck::deck_save,
            commands_deck::deck_list,
            commands_deck::deck_delete,
            commands_deck::deck_export_arena,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Save schema observations on app exit
                if let Ok(mut observer) = app.state::<Mutex<SchemaObserver>>().try_lock() {
                    observer.save();
                    log::info!("Schema observations saved on exit");
                }
            }
        });
}
