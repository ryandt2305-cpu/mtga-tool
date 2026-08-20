// Process poller — background thread that watches for MTGA.exe launching.
//
// Spawned when the app starts without MTGA running (or when the collection
// watch exits because MTGA quit). Polls every POLL_INTERVAL_MS; on detection
// it runs the same combined-scan pipeline the startup path uses and, on
// success, exits (the watch loop takes over from there).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

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
