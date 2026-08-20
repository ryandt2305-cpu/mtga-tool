// Scan pipeline — free functions that run without holding the MemoryService mutex.
//
// run_scan() owns the full collection scan lifecycle:
// find process → open handle → enumerate regions → scan → return results.
// Callers hold the mutex only briefly to store results after completion.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use tauri::{AppHandle, Emitter};

use crate::events;
use crate::models::{CrossValidation, MemoryInventoryScalars};
use crate::services::diagnostics::{
    self, MEM_001, MEM_002, MEM_003, MEM_004, MEM_005, MEM_007, MEM_015, MEM_016, MEM_017,
};

use super::inventory;
use super::process;
use super::scanner;

/// Output from a collection scan — all data needed to update MemoryService state.
pub struct ScanOutput {
    pub collection: HashMap<i32, i32>,
    pub best_address: usize,
    pub best_entry_count: usize,
    pub candidate_count: usize,
    pub cross_validation: Option<CrossValidation>,
    pub scan_duration_ms: u64,
}

/// Pure function — no mutex, no &self. Takes owned data, returns results.
///
/// Runs the full collection scan pipeline: find process → open handle →
/// enumerate regions → scan all regions → score candidates → build collection.
pub fn run_scan(
    app: &AppHandle,
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    non_collectible_ids: &HashSet<i32>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<ScanOutput, String> {
    // Prerequisite: card database must be loaded
    if valid_ids.is_empty() {
        let msg = "Card database not loaded — cannot scan without valid card IDs";
        diagnostics::emit_error(app, &MEM_005, msg);
        return Err(msg.to_string());
    }

    let start = Instant::now();

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

    let _ = app.emit(
        events::memory::PROCESS_FOUND,
        &events::ProcessFoundPayload { pid },
    );

    // Step 3: Enumerate memory regions
    let regions = handle.enumerate_regions().map_err(|e| {
        let msg = format!("Failed to enumerate memory regions: {}", e);
        diagnostics::emit_error(app, &MEM_003, &msg);
        msg
    })?;

    if regions.is_empty() {
        let msg = "Zero readable memory regions found in MTGA.exe";
        diagnostics::emit_error(app, &MEM_007, msg);
        return Err(msg.to_string());
    }

    log::info!(
        "MTGA.exe (PID {}) — {} readable memory regions",
        pid,
        regions.len()
    );

    // Build scan_ids: valid_ids minus non_collectible (matches Python behavior)
    let scan_ids: HashSet<i32> = if non_collectible_ids.is_empty() {
        valid_ids.clone()
    } else {
        valid_ids.difference(non_collectible_ids).copied().collect()
    };

    // Step 4: Scan all regions
    let app_clone = app.clone();
    let candidates = scanner::scan_all_regions(
        &handle,
        &regions,
        &scan_ids,
        anchor_ids,
        Some(&|current, total| {
            let _ = app_clone.emit(
                events::memory::SCAN_PROGRESS,
                &events::ScanProgressPayload {
                    current_region: current,
                    total_regions: total,
                },
            );
        }),
        cancel_flag,
        0,
    );

    if candidates.is_empty() {
        let msg = "No collection data found in MTGA memory. \
                   Make sure your collection is loaded in the game client \
                   (open the Decks or Collection tab).";
        diagnostics::emit_error(app, &MEM_004, msg);
        return Err(msg.to_string());
    }

    let best = &candidates[0];
    log::info!(
        "Found {} candidate blocks. Best: {} entries, density={:.1}%, score={:.0}",
        candidates.len(),
        best.entries.len(),
        best.density() * 100.0,
        best.score(),
    );

    // Step 5: Build collection from best candidate
    let collection = scanner::build_collection(best, non_collectible_ids);

    // Step 6: Cross-validate with runner-up
    let cross_validation = if candidates.len() >= 2 {
        let col_b = scanner::build_collection(&candidates[1], non_collectible_ids);
        Some(scanner::cross_validate(&collection, &col_b))
    } else {
        None
    };

    let scan_duration_ms = start.elapsed().as_millis() as u64;

    let total_copies: usize = collection.values().map(|&v| v as usize).sum();

    let _ = app.emit(
        events::memory::COLLECTION_SCANNED,
        &events::CollectionScannedPayload {
            unique_cards: collection.len(),
            total_copies,
            candidates_found: candidates.len(),
            scan_duration_ms,
        },
    );

    log::info!(
        "Collection scan complete: {} unique cards, {} total copies in {}ms",
        collection.len(),
        total_copies,
        scan_duration_ms,
    );

    Ok(ScanOutput {
        best_address: best.address,
        best_entry_count: best.total_entries,
        candidate_count: candidates.len(),
        collection,
        cross_validation,
        scan_duration_ms,
    })
}

/// Output from a combined collection + inventory scan.
pub struct CombinedScanOutput {
    pub collection: Option<ScanOutput>,
    pub inventory: Option<(usize, MemoryInventoryScalars)>,
}

/// Single process open + region enum, runs both collection and inventory scans.
///
/// Avoids the duplicate find_process → open → enumerate_regions that happens
/// when collection and inventory scans run independently during startup.
pub fn run_combined_scan(
    app: &AppHandle,
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    non_collectible_ids: &HashSet<i32>,
    inventory_params: Option<&inventory::InventorySearchParams>,
    cancel_flag: Option<&AtomicBool>,
) -> Result<CombinedScanOutput, String> {
    diagnostics::emit_info(app, &MEM_015, "Combined scan started");

    // Step 1: Find MTGA.exe (once)
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

    // Step 2: Open process handle (once)
    let handle = process::ProcessHandle::open(pid).map_err(|e| {
        let msg = format!("Failed to open MTGA.exe (PID {}): {}", pid, e);
        diagnostics::emit_error(app, &MEM_002, &msg);
        msg
    })?;

    let _ = app.emit(
        events::memory::PROCESS_FOUND,
        &events::ProcessFoundPayload { pid },
    );

    // Step 3: Enumerate memory regions (once)
    let regions = handle.enumerate_regions().map_err(|e| {
        let msg = format!("Failed to enumerate memory regions: {}", e);
        diagnostics::emit_error(app, &MEM_003, &msg);
        msg
    })?;

    if regions.is_empty() {
        let msg = "Zero readable memory regions found in MTGA.exe";
        diagnostics::emit_error(app, &MEM_007, msg);
        return Err(msg.to_string());
    }

    log::info!(
        "MTGA.exe (PID {}) — {} readable memory regions",
        pid,
        regions.len()
    );

    // Step 4: Collection scan
    let collection_output = if !valid_ids.is_empty() {
        let start = Instant::now();

        let scan_ids: HashSet<i32> = if non_collectible_ids.is_empty() {
            valid_ids.clone()
        } else {
            valid_ids.difference(non_collectible_ids).copied().collect()
        };

        let app_clone = app.clone();
        let candidates = scanner::scan_all_regions(
            &handle,
            &regions,
            &scan_ids,
            anchor_ids,
            Some(&|current, total| {
                let _ = app_clone.emit(
                    events::memory::SCAN_PROGRESS,
                    &events::ScanProgressPayload {
                        current_region: current,
                        total_regions: total,
                    },
                );
            }),
            cancel_flag,
            0,
        );

        if candidates.is_empty() {
            log::warn!("Combined scan: no collection data found in memory");
            None
        } else {
            let best = &candidates[0];
            let collection = scanner::build_collection(best, non_collectible_ids);
            let cross_validation = if candidates.len() >= 2 {
                let col_b = scanner::build_collection(&candidates[1], non_collectible_ids);
                Some(scanner::cross_validate(&collection, &col_b))
            } else {
                None
            };

            let scan_duration_ms = start.elapsed().as_millis() as u64;
            let total_copies: usize = collection.values().map(|&v| v as usize).sum();

            let _ = app.emit(
                events::memory::COLLECTION_SCANNED,
                &events::CollectionScannedPayload {
                    unique_cards: collection.len(),
                    total_copies,
                    candidates_found: candidates.len(),
                    scan_duration_ms,
                },
            );

            diagnostics::emit_info(
                app,
                &MEM_016,
                &format!(
                    "Collection phase complete: {} unique cards in {}ms",
                    collection.len(),
                    scan_duration_ms
                ),
            );

            Some(ScanOutput {
                best_address: best.address,
                best_entry_count: best.total_entries,
                candidate_count: candidates.len(),
                collection,
                cross_validation,
                scan_duration_ms,
            })
        }
    } else {
        None
    };

    // Step 5: Inventory scan (reuses same handle + regions)
    let inventory_output = if let Some(params) = inventory_params {
        match inventory::scan_for_inventory(&handle, &regions, params) {
            Ok((addr, scalars)) => {
                let _ = app.emit(
                    events::memory::INVENTORY_SCANNED,
                    &events::InventoryScannedPayload {
                        scalars: scalars.clone(),
                        candidates_found: 1,
                        address_hex: format!("{:#x}", addr),
                    },
                );
                diagnostics::emit_info(
                    app,
                    &MEM_017,
                    &format!(
                        "Inventory phase complete: gems={}, wcRare={}",
                        scalars.gems, scalars.wc_rare
                    ),
                );
                Some((addr, scalars))
            }
            Err(e) => {
                log::warn!("Combined scan: inventory scan failed — {}", e);
                None
            }
        }
    } else {
        None
    };

    Ok(CombinedScanOutput {
        collection: collection_output,
        inventory: inventory_output,
    })
}
