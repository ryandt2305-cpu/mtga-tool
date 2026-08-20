// Watch loop — polls a cached memory address for collection/inventory changes.
//
// Runs on a dedicated background thread spawned by MemoryService::start_watch().
// Falls back to a full scan if the cached address becomes invalid.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::models::MemoryInventoryScalars;
use crate::services::diagnostics::{self, MEM_009, MEM_012, MEM_020, MEM_021};

use super::inventory;
use super::process;
use super::scanner;
use super::MemoryService;

/// Poll interval for collection watch mode.
pub const WATCH_POLL_INTERVAL_MS: u64 = 3000;

/// Number of consecutive unchanged polls before spawning a background verification scan.
/// At 3s poll interval, 10 polls ≈ 30 seconds of no detected changes.
const STALENESS_THRESHOLD: u32 = 10;

/// Result from a background verification scan.
enum VerificationResult {
    /// Scan found the collection at a (possibly different) address.
    Found {
        address: usize,
        entry_count: usize,
        collection: HashMap<i32, i32>,
        /// The memory region containing the found address (for home region tracking).
        found_region: Option<(usize, usize)>,
    },
    /// Full scan completed but found no valid collection.
    NotFound,
}

/// Background watch loop — runs on a dedicated thread.
///
/// Polls the cached memory address every WATCH_POLL_INTERVAL_MS,
/// diffs against the previous collection, and emits events for changes.
/// Falls back to a full scan if the cached address becomes invalid.
pub fn watch_loop(
    app: AppHandle,
    stop_flag: Arc<AtomicBool>,
    mut address: usize,
    mut entry_count: usize,
    mut last_collection: HashMap<i32, i32>,
    valid_ids: Arc<HashSet<i32>>,
    non_collectible_ids: Arc<HashSet<i32>>,
    anchor_ids: Arc<HashSet<i32>>,
    inventory_address: Option<usize>,
    mut last_inventory: Option<MemoryInventoryScalars>,
) {
    // Open process handle once for the watch session
    let pid = match process::find_process("MTGA.exe") {
        Ok(Some(pid)) => pid,
        Ok(None) | Err(_) => {
            let _ = app.emit(
                events::memory::WATCH_STOPPED,
                &events::WatchStoppedPayload {
                    reason: "MTGA.exe not running".to_string(),
                },
            );
            restart_process_poller(&app);
            return;
        }
    };

    let handle = match process::ProcessHandle::open(pid) {
        Ok(h) => h,
        Err(e) => {
            let _ = app.emit(
                events::memory::WATCH_STOPPED,
                &events::WatchStoppedPayload {
                    reason: format!("Failed to open process: {}", e),
                },
            );
            return;
        }
    };

    let poll_duration = std::time::Duration::from_millis(WATCH_POLL_INTERVAL_MS);

    // Determine which memory region contains the collection address.
    // Verification scans check this region first (fast path) before falling
    // back to a full scan. Updated whenever the collection is relocated.
    let mut home_region: Option<(usize, usize)> = handle
        .enumerate_regions()
        .ok()
        .and_then(|regions| {
            regions.into_iter().find(|r| {
                address >= r.base && address < r.base + r.size
            })
        })
        .map(|r| (r.base, r.size));

    if let Some((base, size)) = home_region {
        log::debug!(
            "Home region for {:#x}: base={:#x} size={:#x}",
            address, base, size,
        );
    }

    // Staleness detection: track consecutive polls with no collection diff.
    // After STALENESS_THRESHOLD unchanged polls, spawn a background verification
    // scan to check if the cached address is stale (GC relocated the dictionary).
    let mut polls_without_change: u32 = 0;
    let mut verify_rx: Option<mpsc::Receiver<VerificationResult>> = None;

    while !stop_flag.load(Ordering::Relaxed) {
        std::thread::sleep(poll_duration);

        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        // --- Check for background verification results ---
        if let Some(ref rx) = verify_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    VerificationResult::Found {
                        address: new_addr,
                        entry_count: new_count,
                        collection: verified_collection,
                        found_region,
                    } => {
                        if verified_collection != last_collection {
                            // Cached address was stale — adopt the new address
                            diagnostics::emit_info(
                                &app,
                                &MEM_021,
                                &format!(
                                    "Stale address detected: {:#x} → {:#x} ({} cards)",
                                    address,
                                    new_addr,
                                    verified_collection.len(),
                                ),
                            );

                            let diff =
                                scanner::diff_collections(&last_collection, &verified_collection);
                            if !diff.added.is_empty()
                                || !diff.increased.is_empty()
                                || !diff.removed.is_empty()
                            {
                                let _ = app.emit(
                                    events::memory::COLLECTION_CHANGED,
                                    &events::CollectionChangedPayload {
                                        added: diff.added,
                                        increased: diff.increased,
                                        removed: diff.removed,
                                        scan_duration_ms: 0,
                                    },
                                );
                            }

                            address = new_addr;
                            entry_count = new_count;
                            last_collection = verified_collection;
                            home_region = found_region;
                        } else {
                            log::debug!(
                                "Verification scan confirmed address {:#x} is current",
                                address,
                            );
                        }
                        polls_without_change = 0;
                    }
                    VerificationResult::NotFound => {
                        log::warn!("Verification scan found no collection — will retry later");
                        polls_without_change = 0;
                    }
                }
                verify_rx = None;
            }
        }

        let start = Instant::now();

        match scanner::rescan_block(&handle, address, entry_count, &valid_ids, &non_collectible_ids)
        {
            Ok(new_collection) => {
                let diff = scanner::diff_collections(&last_collection, &new_collection);

                let has_changes = !diff.added.is_empty()
                    || !diff.increased.is_empty()
                    || !diff.removed.is_empty();

                if has_changes {
                    let scan_duration_ms = start.elapsed().as_millis() as u64;

                    log::info!(
                        "Collection changed: +{} new, {} increased, -{} removed ({}ms)",
                        diff.added.len(),
                        diff.increased.len(),
                        diff.removed.len(),
                        scan_duration_ms,
                    );

                    let _ = app.emit(
                        events::memory::COLLECTION_CHANGED,
                        &events::CollectionChangedPayload {
                            added: diff.added,
                            increased: diff.increased,
                            removed: diff.removed,
                            scan_duration_ms,
                        },
                    );

                    polls_without_change = 0;
                } else {
                    polls_without_change += 1;

                    // Spawn background verification if stale and no scan already running
                    if polls_without_change >= STALENESS_THRESHOLD && verify_rx.is_none() {
                        diagnostics::emit_info(
                            &app,
                            &MEM_020,
                            &format!(
                                "No collection changes for {} polls (~{}s), spawning verification scan",
                                polls_without_change,
                                polls_without_change as u64 * WATCH_POLL_INTERVAL_MS / 1000,
                            ),
                        );

                        let vi = valid_ids.clone();
                        let ai = anchor_ids.clone();
                        let nci = non_collectible_ids.clone();
                        let sf = stop_flag.clone();
                        let hr = home_region;
                        let (tx, rx) = mpsc::channel();
                        verify_rx = Some(rx);

                        std::thread::Builder::new()
                            .name("mem-verify".into())
                            .spawn(move || {
                                let result = run_verification_scan(&vi, &ai, &nci, &sf, hr);
                                let _ = tx.send(result);
                            })
                            .ok();
                    }
                }

                last_collection = new_collection;

                // --- Inventory scalar re-read (piggybacks on same handle) ---
                if let Some(inv_addr) = inventory_address {
                    match inventory::read_inventory_at(&handle, inv_addr) {
                        Ok(current) => {
                            if let Some(ref prev) = last_inventory {
                                if !inventory_scalars_equal(prev, &current) {
                                    log::info!(
                                        "Inventory changed: gems {} → {}, wcRare {} → {}",
                                        prev.gems,
                                        current.gems,
                                        prev.wc_rare,
                                        current.wc_rare,
                                    );
                                    let _ = app.emit(
                                        events::memory::INVENTORY_CHANGED,
                                        &events::InventoryChangedPayload {
                                            previous: prev.clone(),
                                            current: current.clone(),
                                        },
                                    );
                                }
                            }
                            last_inventory = Some(current);
                        }
                        Err(e) => {
                            diagnostics::emit_warning(
                                &app,
                                &MEM_012,
                                &format!("Inventory re-read failed: {}", e),
                            );
                            // Keep retrying — address may be temporarily unreadable
                        }
                    }
                }
            }
            Err(e) => {
                // Cached address invalid — try full scan fallback
                diagnostics::emit_warning(
                    &app,
                    &MEM_009,
                    &format!("Quick rescan failed: {}. Attempting full scan fallback.", e),
                );

                match try_full_scan_fallback(
                    &app,
                    &valid_ids,
                    &anchor_ids,
                    &non_collectible_ids,
                ) {
                    Some((new_addr, new_count, new_collection, new_home)) => {
                        address = new_addr;
                        entry_count = new_count;
                        last_collection = new_collection;
                        home_region = new_home;
                        log::info!(
                            "Watch fallback: relocated collection at {:#x}",
                            new_addr
                        );
                    }
                    None => {
                        log::error!("Watch fallback: full scan also failed, stopping watch");
                        let _ = app.emit(
                            events::memory::WATCH_STOPPED,
                            &events::WatchStoppedPayload {
                                reason: "Collection address lost and full scan fallback failed"
                                    .to_string(),
                            },
                        );
                        return;
                    }
                }
            }
        }
    }

    let _ = app.emit(
        events::memory::WATCH_STOPPED,
        &events::WatchStoppedPayload {
            reason: "Stopped by user".to_string(),
        },
    );
}

/// Background verification scan — runs on its own thread.
///
/// Opens a fresh process handle and attempts to locate the collection.
/// Fast path: checks the home region first (GC compaction usually relocates
/// within the same heap segment). Falls back to a full scan if not found.
/// Both paths run with yield_ms=0 (no inter-region sleep) since this is
/// a background thread.
fn run_verification_scan(
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    non_collectible_ids: &HashSet<i32>,
    stop_flag: &AtomicBool,
    home_region: Option<(usize, usize)>,
) -> VerificationResult {
    let pid = match process::find_process("MTGA.exe") {
        Ok(Some(pid)) => pid,
        _ => return VerificationResult::NotFound,
    };

    let handle = match process::ProcessHandle::open(pid) {
        Ok(h) => h,
        Err(_) => return VerificationResult::NotFound,
    };

    let scan_ids: HashSet<i32> = if non_collectible_ids.is_empty() {
        valid_ids.clone()
    } else {
        valid_ids.difference(non_collectible_ids).copied().collect()
    };

    // Fast path: check home region first
    if let Some((base, size)) = home_region {
        let region = process::MemRegion { base, size };
        let candidates = scanner::scan_all_regions(
            &handle,
            &[region],
            &scan_ids,
            anchor_ids,
            None,
            Some(stop_flag),
            0,
        );
        if !candidates.is_empty() {
            let best = &candidates[0];
            let collection = scanner::build_collection(best, non_collectible_ids);
            log::debug!(
                "Verification fast path: found {} cards in home region {:#x}",
                collection.len(),
                base,
            );
            return VerificationResult::Found {
                address: best.address,
                entry_count: best.total_entries,
                collection,
                found_region: Some((base, size)),
            };
        }
        log::debug!("Verification fast path miss — falling back to full scan");
    }

    // Slow path: full scan (no sleep between regions)
    let regions = match handle.enumerate_regions() {
        Ok(r) if !r.is_empty() => r,
        _ => return VerificationResult::NotFound,
    };

    let candidates = scanner::scan_all_regions(
        &handle,
        &regions,
        &scan_ids,
        anchor_ids,
        None,
        Some(stop_flag),
        0,
    );

    if candidates.is_empty() {
        return VerificationResult::NotFound;
    }

    let best = &candidates[0];
    let collection = scanner::build_collection(best, non_collectible_ids);

    // Determine which region contains the found address for future fast paths
    let found_region = regions
        .iter()
        .find(|r| best.address >= r.base && best.address < r.base + r.size)
        .map(|r| (r.base, r.size));

    VerificationResult::Found {
        address: best.address,
        entry_count: best.total_entries,
        collection,
        found_region,
    }
}

/// Attempt a full scan to relocate the collection after a rescan failure.
///
/// Returns (address, entry_count, collection, home_region) on success.
fn try_full_scan_fallback(
    app: &AppHandle,
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    non_collectible_ids: &HashSet<i32>,
) -> Option<(usize, usize, HashMap<i32, i32>, Option<(usize, usize)>)> {
    let pid = process::find_process("MTGA.exe").ok()??;
    let handle = process::ProcessHandle::open(pid).ok()?;
    let regions = handle.enumerate_regions().ok()?;

    if regions.is_empty() {
        return None;
    }

    let scan_ids: HashSet<i32> = if non_collectible_ids.is_empty() {
        valid_ids.clone()
    } else {
        valid_ids.difference(non_collectible_ids).copied().collect()
    };

    let candidates =
        scanner::scan_all_regions(&handle, &regions, &scan_ids, anchor_ids, None, None, 0);

    if candidates.is_empty() {
        return None;
    }

    let best = &candidates[0];
    let collection = scanner::build_collection(best, non_collectible_ids);

    // Determine home region for the new address
    let new_home = regions
        .iter()
        .find(|r| best.address >= r.base && best.address < r.base + r.size)
        .map(|r| (r.base, r.size));

    let total_copies: usize = collection.values().map(|&v| v as usize).sum();
    let _ = app.emit(
        events::memory::COLLECTION_SCANNED,
        &events::CollectionScannedPayload {
            unique_cards: collection.len(),
            total_copies,
            candidates_found: candidates.len(),
            scan_duration_ms: 0, // Not tracked for fallback
        },
    );

    Some((best.address, best.total_entries, collection, new_home))
}

/// Restart the MTGA.exe process poller so a future game launch is caught.
///
/// Called from the watch loop when it exits because MTGA isn't running —
/// the live scan cache stays populated (the frontend keeps showing the last
/// live state) and the poller will re-enter live mode automatically on the
/// next game launch.
fn restart_process_poller(app: &AppHandle) {
    let mem_state = app.state::<Mutex<MemoryService>>();
    match mem_state.lock() {
        Ok(mut memory) => memory.start_process_poller(app),
        Err(e) => log::warn!("Failed to restart process poller: {}", e),
    };
}

/// Compare two inventory scalar snapshots for equality.
pub fn inventory_scalars_equal(a: &MemoryInventoryScalars, b: &MemoryInventoryScalars) -> bool {
    a.wc_common == b.wc_common
        && a.wc_uncommon == b.wc_uncommon
        && a.wc_rare == b.wc_rare
        && a.wc_mythic == b.wc_mythic
        && a.gold == b.gold
        && a.gems == b.gems
        && a.wc_track_position == b.wc_track_position
        && a.vault_progress == b.vault_progress
}
