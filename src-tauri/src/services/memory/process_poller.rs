// Process poller — background thread that watches for MTGA.exe launching.
//
// Spawned when the app starts without MTGA running (or when the collection
// watch exits because MTGA quit). Polls every POLL_INTERVAL_MS; on detection
// it runs the same combined-scan pipeline the startup path uses and, on
// success, exits (the watch loop takes over from there).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::services::card_db::CardDb;
use crate::services::diagnostics::{self, MEM_022, MEM_023};

use super::process;

/// How often to check for MTGA.exe.
pub const POLL_INTERVAL_MS: u64 = 5000;

/// Poll loop — runs on a dedicated background thread.
///
/// Exits as soon as one of these is true:
/// - `stop_flag` is set (external stop).
/// - MTGA.exe was detected and the combined scan populated a collection.
///
/// The stop flag is also set to `true` before returning so `MemoryService`
/// can tell (via `stop_flag.load()`) that the previous poller is finished
/// and a new one can be spawned.
pub fn poll_loop(app: AppHandle, stop_flag: Arc<AtomicBool>) {
    diagnostics::emit_info(&app, &MEM_022, "Process poller started");

    let interval = Duration::from_millis(POLL_INTERVAL_MS);

    // PID of the last card-DB load attempt, so a missing database logs its
    // CDB error once per game launch instead of every poll tick.
    let mut last_load_attempt_pid: Option<u32> = None;

    loop {
        // Sleep in small slices so an external stop is responsive.
        let mut slept = Duration::ZERO;
        while slept < interval {
            if stop_flag.load(Ordering::Relaxed) {
                stop_flag.store(true, Ordering::Relaxed);
                log::info!("Process poller: stopped externally");
                return;
            }
            let slice = Duration::from_millis(250).min(interval - slept);
            std::thread::sleep(slice);
            slept += slice;
        }

        match process::find_process("MTGA.exe") {
            Ok(Some(pid)) => {
                // If the startup card-DB load failed (e.g., non-default install
                // path with MTGA not yet running), retry now — install discovery
                // can resolve the path from the running process.
                ensure_card_db_loaded(&app, pid, &mut last_load_attempt_pid);

                diagnostics::emit_info(
                    &app,
                    &MEM_023,
                    &format!("MTGA.exe detected (PID {}) — running scan", pid),
                );

                match super::run_scan_and_start_watch(&app) {
                    Ok(true) => {
                        log::info!(
                            "Process poller: scan complete, watch active — exiting poller"
                        );
                        stop_flag.store(true, Ordering::Relaxed);
                        return;
                    }
                    Ok(false) => {
                        // Skip conditions: scan_in_progress, card DB not loaded,
                        // or the scan produced no collection. Retry next tick.
                        log::debug!(
                            "Process poller: scan produced no collection — will retry"
                        );
                    }
                    Err(e) => {
                        // Most likely: MTGA quit between our detection and the
                        // pipeline's own find_process. Just retry.
                        log::warn!("Process poller: scan attempt failed — {}", e);
                    }
                }
            }
            Ok(None) => {
                // Not running — silent, retry next tick.
            }
            Err(e) => {
                log::warn!("Process poller: process enumeration failed — {}", e);
            }
        }
    }
}

/// Retry the card-DB load if it isn't loaded yet, at most once per detected
/// MTGA.exe PID. `CardDb::load` emits its own diagnostics and the
/// `card_db:loaded` event; on success we also emit `app::CARD_DB_READY` for
/// parity with the startup path.
fn ensure_card_db_loaded(app: &AppHandle, pid: u32, last_attempt_pid: &mut Option<u32>) {
    let state = app.state::<Mutex<CardDb>>();

    {
        let Ok(db) = state.lock() else { return };
        if db.is_loaded() {
            return;
        }
    }

    if *last_attempt_pid == Some(pid) {
        return;
    }
    *last_attempt_pid = Some(pid);

    let Ok(mut db) = state.lock() else { return };
    if db.is_loaded() {
        return; // raced with the startup load thread
    }
    match db.load(app, None) {
        Ok(count) => {
            log::info!(
                "Process poller: card DB loaded after game launch ({} cards)",
                count
            );
            let _ = app.emit(
                events::app::CARD_DB_READY,
                &events::CardDbReadyPayload { card_count: count },
            );
        }
        Err(e) => log::warn!("Process poller: card DB load failed — {}", e),
    }
}
