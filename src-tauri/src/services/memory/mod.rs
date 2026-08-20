// MemoryService — scans MTGA.exe process memory for the card collection.
//
// Orchestrates the scan pipeline: find process → enumerate regions →
// scan for Dictionary<int,int> entries → score candidates → build collection.
// Emits progress and result events via Tauri's event system.

pub mod inventory;
pub mod pipeline;
pub mod process;
pub mod process_poller;
pub mod scanner;
mod watch;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::models::{MemoryInventoryScalars, ScanResult};
use crate::services::card_db::CardDb;
use crate::services::diagnostics::{
    self, MEM_001, MEM_002, MEM_003, MEM_007, MEM_008, MEM_010, MEM_011,
};
use crate::services::history_db::HistoryDb;
use crate::services::log::LogService;

/// Provenance of the currently-held collection data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ScanSource {
    /// Freshly scanned from the live MTGA.exe process.
    Live,
    /// Hydrated from history.db because MTGA was not running.
    HistoryDb,
}

/// Managed state holding the last scan result and watch mode state.
pub struct MemoryService {
    last_scan: Option<ScanResult>,
    /// Cached memory address of the collection dictionary block.
    cached_address: Option<usize>,
    /// Estimated number of entries at the cached address.
    cached_entry_count: Option<usize>,
    /// Last known collection snapshot (for diffing).
    last_collection: Option<HashMap<i32, i32>>,
    /// Provenance of `last_scan` / `last_collection`.
    scan_source: Option<ScanSource>,
    /// Stop flag for the watch thread.
    watch_stop_flag: Option<Arc<AtomicBool>>,
    /// Handle to the watch thread.
    watch_thread: Option<JoinHandle<()>>,
    /// Cached memory address of the ClientPlayerInventory struct.
    cached_inventory_address: Option<usize>,
    /// Last known inventory scalars (for change detection).
    last_inventory_scalars: Option<MemoryInventoryScalars>,
    /// Whether a background scan is currently in progress.
    scan_in_progress: Arc<AtomicBool>,
    /// Cancel flag for the current scan.
    scan_cancel_flag: Arc<AtomicBool>,
    /// Stop flag for the MTGA.exe process poller. Also used to detect whether
    /// a poller is still running: the poller sets this to `true` before exit.
    poller_stop_flag: Option<Arc<AtomicBool>>,
    /// Handle to the process poller thread.
    poller_thread: Option<JoinHandle<()>>,
}

impl MemoryService {
    pub fn new() -> Self {
        Self {
            last_scan: None,
            cached_address: None,
            cached_entry_count: None,
            last_collection: None,
            scan_source: None,
            watch_stop_flag: None,
            watch_thread: None,
            cached_inventory_address: None,
            last_inventory_scalars: None,
            scan_in_progress: Arc::new(AtomicBool::new(false)),
            scan_cancel_flag: Arc::new(AtomicBool::new(false)),
            poller_stop_flag: None,
            poller_thread: None,
        }
    }

    /// Whether a successful full scan has been performed.
    pub fn has_scan(&self) -> bool {
        self.last_scan.is_some()
    }

    /// Provenance of the currently-held collection data (live scan vs. offline hydration).
    pub fn scan_source(&self) -> Option<ScanSource> {
        self.scan_source
    }

    /// Get the last scanned collection (for frontend hydration).
    pub fn last_collection(&self) -> Option<&HashMap<i32, i32>> {
        self.last_collection.as_ref()
    }

    /// Summary stats for AppStatus hydration: (has_scan, unique_cards, total_copies).
    pub fn last_scan_stats(&self) -> (bool, usize, usize) {
        match &self.last_scan {
            Some(scan) => {
                let total: usize = scan.collection.values().map(|&v| v as usize).sum();
                (true, scan.collection.len(), total)
            }
            None => (false, 0, 0),
        }
    }

    /// Whether the collection watch is currently running.
    pub fn is_watching(&self) -> bool {
        self.watch_stop_flag
            .as_ref()
            .map_or(false, |f| !f.load(Ordering::Relaxed))
    }

    /// Get the last scanned inventory scalars (for merge_with_memory).
    pub fn last_inventory_scalars(&self) -> Option<&MemoryInventoryScalars> {
        self.last_inventory_scalars.as_ref()
    }

    /// Whether a background scan is currently in progress.
    pub fn is_scan_in_progress(&self) -> bool {
        self.scan_in_progress.load(Ordering::Relaxed)
    }

    /// Set the scan-in-progress flag.
    pub fn set_scan_in_progress(&self, v: bool) {
        self.scan_in_progress.store(v, Ordering::Relaxed);
    }

    /// Get a clone of the scan_in_progress Arc for use outside the mutex.
    pub fn scan_in_progress_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.scan_in_progress)
    }

    /// Get a clone of the cancel flag Arc for use outside the mutex.
    pub fn scan_cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.scan_cancel_flag)
    }

    /// Request cancellation of the current scan.
    pub fn cancel_scan(&self) {
        self.scan_cancel_flag.store(true, Ordering::Relaxed);
    }

    /// Reset the cancel flag (call before starting a new scan).
    pub fn reset_cancel_flag(&self) {
        self.scan_cancel_flag.store(false, Ordering::Relaxed);
    }

    /// Scan MTGA.exe memory for the ClientPlayerInventory struct.
    ///
    /// Uses known values from StartHook as search anchors. Reuses existing
    /// process discovery and region enumeration from the collection scanner.
    pub fn scan_inventory(
        &mut self,
        app: &AppHandle,
        search_params: inventory::InventorySearchParams,
    ) -> Result<MemoryInventoryScalars, String> {
        diagnostics::emit_info(app, &MEM_010, "Inventory scalar scan started");

        // Step 1: Find MTGA.exe
        let pid = process::find_process("MTGA.exe")
            .map_err(|e| {
                let msg = format!("Failed to enumerate processes: {}", e);
                diagnostics::emit_error(app, &MEM_001, &msg);
                msg
            })?
            .ok_or_else(|| {
                let msg = "MTGA.exe is not running";
                diagnostics::emit_error(app, &MEM_001, msg);
                msg.to_string()
            })?;

        // Step 2: Open process handle
        let handle = process::ProcessHandle::open(pid).map_err(|e| {
            let msg = format!("Failed to open MTGA.exe (PID {}): {}", pid, e);
            diagnostics::emit_error(app, &MEM_002, &msg);
            msg
        })?;

        // Step 3: Enumerate memory regions
        let regions = handle.enumerate_regions().map_err(|e| {
            let msg = format!("Failed to enumerate memory regions: {}", e);
            diagnostics::emit_error(app, &MEM_003, &msg);
            msg
        })?;

        if regions.is_empty() {
            let msg = "Zero readable memory regions found";
            diagnostics::emit_error(app, &MEM_007, msg);
            return Err(msg.to_string());
        }

        // Step 4: Scan for the inventory struct
        let (address, scalars) =
            inventory::scan_for_inventory(&handle, &regions, &search_params).map_err(|e| {
                diagnostics::emit_error(app, &MEM_011, &e);
                e
            })?;

        // Cache results
        self.cached_inventory_address = Some(address);
        self.last_inventory_scalars = Some(scalars.clone());

        let _ = app.emit(
            events::memory::INVENTORY_SCANNED,
            &events::InventoryScannedPayload {
                scalars: scalars.clone(),
                candidates_found: 1, // best candidate selected
                address_hex: format!("{:#x}", address),
            },
        );

        log::info!(
            "Inventory scalars found: gems={}, wcRare={}, gold={}, vault={:.1}",
            scalars.gems,
            scalars.wc_rare,
            scalars.gold,
            scalars.vault_progress,
        );

        Ok(scalars)
    }

    /// Start the collection watch loop.
    ///
    /// Requires a prior successful full scan (needs cached address).
    /// Spawns a background thread that polls the cached address every 3 seconds.
    pub fn start_watch(
        &mut self,
        app: &AppHandle,
        valid_ids: Arc<HashSet<i32>>,
        non_collectible_ids: Arc<HashSet<i32>>,
        anchor_ids: Arc<HashSet<i32>>,
    ) -> Result<(), String> {
        if self.is_watching() {
            return Err("Collection watch is already running".to_string());
        }

        let address = self
            .cached_address
            .ok_or("No cached address — run a full scan first")?;
        let entry_count = self
            .cached_entry_count
            .ok_or("No cached entry count — run a full scan first")?;
        let last_collection = self
            .last_collection
            .clone()
            .ok_or("No cached collection — run a full scan first")?;

        let inventory_address = self.cached_inventory_address;
        let last_inventory = self.last_inventory_scalars.clone();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let app_clone = app.clone();

        let handle = std::thread::Builder::new()
            .name("collection-watch".to_string())
            .spawn(move || {
                watch::watch_loop(
                    app_clone,
                    stop_flag_clone,
                    address,
                    entry_count,
                    last_collection,
                    valid_ids,
                    non_collectible_ids,
                    anchor_ids,
                    inventory_address,
                    last_inventory,
                );
            })
            .map_err(|e| format!("Failed to spawn watch thread: {}", e))?;

        self.watch_stop_flag = Some(stop_flag);
        self.watch_thread = Some(handle);

        diagnostics::emit_info(app, &MEM_008, "Collection watch started");
        let _ = app.emit(
            events::memory::WATCH_STARTED,
            &events::WatchStartedPayload {
                poll_interval_ms: watch::WATCH_POLL_INTERVAL_MS,
            },
        );

        Ok(())
    }

    /// Stop the collection watch loop.
    pub fn stop_watch(&mut self) -> Result<(), String> {
        if let Some(flag) = self.watch_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.watch_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Store results from a completed scan into MemoryService state.
    ///
    /// Brief lock scope — call this after run_scan() completes.
    pub fn store_scan_output(&mut self, output: pipeline::ScanOutput) -> ScanResult {
        self.cached_address = Some(output.best_address);
        self.cached_entry_count = Some(output.best_entry_count);
        self.last_collection = Some(output.collection.clone());
        self.scan_source = Some(ScanSource::Live);

        let result = ScanResult {
            collection: output.collection,
            candidate_count: output.candidate_count,
            cross_validation: output.cross_validation,
            scan_duration_ms: output.scan_duration_ms,
        };

        self.last_scan = Some(result.clone());
        result
    }

    /// Populate collection state from the most recent history.db snapshot.
    ///
    /// Used at startup when MTGA.exe is not running so the frontend can still
    /// display the last-known collection. Does NOT set a memory address or start
    /// the watch loop — those require a live process. Returns the snapshot metadata
    /// on success, or `Ok(None)` if history.db has no collection snapshots yet.
    ///
    /// Only hydrates if there is no live scan already loaded — a Live scan always
    /// wins over persisted data.
    pub fn hydrate_from_history_db(
        &mut self,
        db: &HistoryDb,
    ) -> Result<Option<(i64, usize, usize)>, String> {
        if self.last_scan.is_some() {
            return Ok(None);
        }

        let summaries = db.get_collection_snapshots(None, None, 1)?;
        let summary = match summaries.into_iter().next() {
            Some(s) => s,
            None => return Ok(None),
        };

        let collection = match db.get_collection_snapshot_detail(summary.id)? {
            Some(c) => c,
            None => return Ok(None),
        };

        let unique = collection.len();
        let total: usize = collection.values().map(|&v| v as usize).sum();

        self.last_collection = Some(collection.clone());
        self.last_scan = Some(ScanResult {
            collection,
            candidate_count: 0,
            cross_validation: None,
            scan_duration_ms: 0,
        });
        self.scan_source = Some(ScanSource::HistoryDb);

        Ok(Some((summary.timestamp, unique, total)))
    }

    /// Store inventory scan results.
    pub fn store_inventory_output(&mut self, address: usize, scalars: MemoryInventoryScalars) {
        self.cached_inventory_address = Some(address);
        self.last_inventory_scalars = Some(scalars);
    }

    /// Run a full memory scan for the card collection.
    ///
    /// Delegates to pipeline::run_scan() (free function, no mutex needed during scan).
    /// Stores results with a brief lock after completion.
    pub fn scan(
        &mut self,
        app: &AppHandle,
        valid_ids: &HashSet<i32>,
        anchor_ids: &HashSet<i32>,
        non_collectible_ids: &HashSet<i32>,
    ) -> Result<ScanResult, String> {
        let output = pipeline::run_scan(app, valid_ids, anchor_ids, non_collectible_ids, None)?;
        Ok(self.store_scan_output(output))
    }

    /// Whether the MTGA.exe process poller is currently running.
    pub fn is_process_poller_running(&self) -> bool {
        self.poller_stop_flag
            .as_ref()
            .map_or(false, |f| !f.load(Ordering::Relaxed))
    }

    /// Start the background poller that watches for MTGA.exe to launch.
    ///
    /// No-op if a poller is already running. If a previous poller has finished
    /// (its stop flag is `true`), its thread handle is joined before spawning
    /// a fresh one so we don't leak `JoinHandle`s across multiple game launches.
    pub fn start_process_poller(&mut self, app: &AppHandle) {
        if self.is_process_poller_running() {
            log::debug!("Process poller already running — start_process_poller ignored");
            return;
        }

        // Join the previous (already-finished) thread if present.
        if let Some(handle) = self.poller_thread.take() {
            let _ = handle.join();
        }
        self.poller_stop_flag = None;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let app_clone = app.clone();

        match std::thread::Builder::new()
            .name("mtga-process-poller".to_string())
            .spawn(move || {
                process_poller::poll_loop(app_clone, stop_flag_clone);
            }) {
            Ok(handle) => {
                self.poller_stop_flag = Some(stop_flag);
                self.poller_thread = Some(handle);
            }
            Err(e) => {
                log::warn!("Failed to spawn process poller thread: {}", e);
            }
        }
    }

    /// Stop the process poller if running.
    pub fn stop_process_poller(&mut self) {
        if let Some(flag) = self.poller_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.poller_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Run the combined collection + inventory scan and, on success, start the
/// collection watch loop. Shared between the startup task and the process
/// poller so both use identical scan orchestration.
///
/// Returns:
/// - `Ok(true)`  — collection was found and stored (watch started or already
///   running). The caller can consider the "connect to MTGA" job done.
/// - `Ok(false)` — nothing to do this attempt (scan already in progress, card
///   DB not loaded, or the scan completed but produced no collection).
/// - `Err(msg)`  — the scan pipeline itself failed (e.g., MTGA quit between
///   the caller's detection and the pipeline's own `find_process`).
pub fn run_scan_and_start_watch(app: &AppHandle) -> Result<bool, String> {
    // Guard: skip if a scan is already in progress (manual user rescan, or
    // the previous poller tick still finishing). This also flips the flag on
    // for us in the same critical section so a concurrent scan_collection
    // command sees is_scan_in_progress() == true.
    {
        let mem_state = app.state::<Mutex<MemoryService>>();
        let memory = mem_state
            .lock()
            .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;
        if memory.is_scan_in_progress() {
            return Ok(false);
        }
        memory.set_scan_in_progress(true);
        memory.reset_cancel_flag();
    }

    // Extract inputs from CardDb + LogService (owned clones — no mutex held
    // during the scan itself).
    let valid_ids: Option<HashSet<i32>> = {
        let db_state = app.state::<Mutex<CardDb>>();
        let db = db_state
            .lock()
            .map_err(|e| format!("CardDb lock poisoned: {}", e))?;
        db.valid_ids().cloned()
    };

    let Some(valid_ids) = valid_ids else {
        // Card DB not loaded yet — reset flag and bail.
        clear_scan_in_progress(app);
        return Ok(false);
    };

    let (anchor_ids, non_collectible_ids, inventory_params) = {
        let log_state = app.state::<Mutex<LogService>>();
        let log = log_state
            .lock()
            .map_err(|e| format!("LogService lock poisoned: {}", e))?;
        let params = log.current_inventory().map(|inv| {
            inventory::InventorySearchParams {
                gold: Some(inv.gold as i32),
                wc_common: Some(inv.wc_common),
                wc_uncommon: Some(inv.wc_uncommon),
                wc_mythic: Some(inv.wc_mythic),
                wc_track_position: Some(inv.wc_track_position),
            }
        });
        (log.anchor_ids(), log.non_collectible_ids(), params)
    };

    let _ = app.emit(events::app::SCAN_STARTING, &events::ScanStartingPayload {});

    let scan_result = pipeline::run_combined_scan(
        app,
        &valid_ids,
        &anchor_ids,
        &non_collectible_ids,
        inventory_params.as_ref(),
        None,
    );

    // Always clear the scan_in_progress flag before returning.
    clear_scan_in_progress(app);

    let output = scan_result?;

    let mut collection_found = false;
    {
        let mem_state = app.state::<Mutex<MemoryService>>();
        let mut memory = mem_state
            .lock()
            .map_err(|e| format!("MemoryService lock poisoned: {}", e))?;

        if let Some(scan_output) = output.collection {
            log::info!(
                "Combined scan: {} unique cards stored",
                scan_output.collection.len()
            );
            memory.store_scan_output(scan_output);
            collection_found = true;

            if !memory.is_watching() {
                match memory.start_watch(
                    app,
                    Arc::new(valid_ids),
                    Arc::new(non_collectible_ids),
                    Arc::new(anchor_ids),
                ) {
                    Ok(()) => log::info!("Combined scan: collection watch started"),
                    Err(e) => log::warn!("Combined scan: collection watch failed — {}", e),
                }
            }
        }

        if let Some((addr, scalars)) = output.inventory {
            log::info!(
                "Combined scan: inventory scalars found (gems={}, wcRare={})",
                scalars.gems,
                scalars.wc_rare,
            );
            memory.store_inventory_output(addr, scalars);
        }
    }

    Ok(collection_found)
}

fn clear_scan_in_progress(app: &AppHandle) {
    if let Ok(memory) = app.state::<Mutex<MemoryService>>().lock() {
        memory.set_scan_in_progress(false);
    }
}
