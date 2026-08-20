// Core memory scanning algorithm for .NET Dictionary<int,int> detection.
//
// Port of Python's `memory_scanner.py` with optimized validation order,
// zero-copy entry parsing, and pre-allocated buffers.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use super::process::{MemRegion, ProcessHandle};
use crate::models::{CollectionDiff, CrossValidation};

// --- Constants (matching Python reference) ---

pub const ENTRY_SIZE: usize = 16; // sizeof(Dictionary.Entry) = 4 × i32
const MIN_VALID_ENTRIES: usize = 100;
const MAX_QUANTITY: i32 = 250;
const MAX_GAP: usize = 8;
const MIN_DENSITY: f64 = 0.4;
const CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB (Python uses 4MB)

// Scoring weights
const WEIGHT_COUNT: f64 = 1.0;
const WEIGHT_DENSITY: f64 = 500.0;
const WEIGHT_ANCHOR: f64 = 200.0;

/// A contiguous memory region containing dense collection-like data.
pub struct CandidateBlock {
    pub address: usize,
    pub entries: Vec<(i32, i32)>, // (card_id, quantity)
    pub total_entries: usize,
    pub anchor_hits: usize,
    pub anchor_total: usize,
}

impl CandidateBlock {
    pub fn density(&self) -> f64 {
        if self.total_entries == 0 {
            return 0.0;
        }
        self.entries.len() as f64 / self.total_entries as f64
    }

    pub fn score(&self) -> f64 {
        let anchor_ratio = if self.anchor_total > 0 {
            self.anchor_hits as f64 / self.anchor_total as f64
        } else {
            0.0
        };
        WEIGHT_COUNT * self.entries.len() as f64
            + WEIGHT_DENSITY * self.density()
            + WEIGHT_ANCHOR * anchor_ratio
    }
}

/// Validate a 16-byte block as a Dictionary<int,int> entry.
///
/// Optimized check order (cheapest first):
/// 1. Value range (single comparison, eliminates vast majority)
/// 2. Hash check (arithmetic only)
/// 3. Next index (arithmetic only)
/// 4. Valid ID lookup (HashSet, most expensive surviving check)
#[inline(always)]
fn validate_entry(
    hash_code: i32,
    next_idx: i32,
    key: i32,
    value: i32,
    valid_ids: &HashSet<i32>,
) -> bool {
    // Value range — eliminates most entries immediately
    if value < 1 || value > MAX_QUANTITY {
        return false;
    }
    // hashCode must equal key & 0x7FFFFFFF
    if hash_code != (key & 0x7FFF_FFFF) {
        return false;
    }
    // next should be -1 or a reasonable array index
    if next_idx != -1 && (next_idx < 0 || next_idx > 100_000) {
        return false;
    }
    // Valid card ID (most expensive check)
    valid_ids.contains(&key)
}

/// Scan a single memory region for runs of valid Dictionary entries.
///
/// Reads in CHUNK_SIZE chunks using a pre-allocated buffer.
/// Tracks runs with gap tolerance (MAX_GAP consecutive invalid entries).
fn scan_region(
    handle: &ProcessHandle,
    region: &MemRegion,
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    buffer: &mut Vec<u8>,
) -> Vec<CandidateBlock> {
    let mut candidates = Vec::new();
    let mut offset: usize = 0;

    while offset < region.size {
        let read_size = std::cmp::min(CHUNK_SIZE, region.size - offset);

        // Ensure buffer is large enough
        if buffer.len() < read_size {
            buffer.resize(read_size, 0);
        }

        let bytes_read = match handle.read_memory(region.base + offset, &mut buffer[..read_size]) {
            Ok(n) => n,
            Err(_) => {
                // MEM-006: ReadProcessMemory failed for chunk, skip and continue
                offset += read_size;
                continue;
            }
        };

        if bytes_read < ENTRY_SIZE {
            offset += read_size;
            continue;
        }

        let num_entries = bytes_read / ENTRY_SIZE;
        let mut current_run: Vec<(i32, i32)> = Vec::new();
        let mut run_start: usize = 0;
        let mut gap: usize = 0;
        let mut total_in_run: usize = 0;

        for i in 0..num_entries {
            let pos = i * ENTRY_SIZE;

            // Zero-copy parsing: i32::from_le_bytes on byte slices
            let hash_code =
                i32::from_le_bytes(buffer[pos..pos + 4].try_into().unwrap());
            let next_idx =
                i32::from_le_bytes(buffer[pos + 4..pos + 8].try_into().unwrap());
            let key =
                i32::from_le_bytes(buffer[pos + 8..pos + 12].try_into().unwrap());
            let value =
                i32::from_le_bytes(buffer[pos + 12..pos + 16].try_into().unwrap());

            if validate_entry(hash_code, next_idx, key, value, valid_ids) {
                if gap > 0 {
                    total_in_run += gap;
                    gap = 0;
                }
                current_run.push((key, value));
                total_in_run += 1;
            } else {
                gap += 1;
                if gap > MAX_GAP {
                    // Run ended — evaluate
                    finalize_run(
                        &mut candidates,
                        &mut current_run,
                        total_in_run,
                        region.base + offset + run_start * ENTRY_SIZE,
                        anchor_ids,
                    );
                    total_in_run = 0;
                    gap = 0;
                    run_start = i + 1;
                }
            }
        }

        // Handle run extending to end of chunk
        finalize_run(
            &mut candidates,
            &mut current_run,
            total_in_run,
            region.base + offset + run_start * ENTRY_SIZE,
            anchor_ids,
        );

        offset += read_size;
    }

    candidates
}

/// Finalize a run into a CandidateBlock if it meets thresholds.
/// Clears the run buffer for reuse.
fn finalize_run(
    candidates: &mut Vec<CandidateBlock>,
    run: &mut Vec<(i32, i32)>,
    total_entries: usize,
    address: usize,
    anchor_ids: &HashSet<i32>,
) {
    if run.len() >= MIN_VALID_ENTRIES {
        let total = if total_entries > 0 {
            total_entries
        } else {
            run.len()
        };
        let density = run.len() as f64 / total as f64;
        if density >= MIN_DENSITY {
            let anchor_hits = if anchor_ids.is_empty() {
                0
            } else {
                let found_ids: HashSet<i32> = run.iter().map(|(k, _)| *k).collect();
                found_ids.intersection(anchor_ids).count()
            };

            candidates.push(CandidateBlock {
                address,
                entries: std::mem::take(run),
                total_entries: total,
                anchor_hits,
                anchor_total: anchor_ids.len(),
            });
            return;
        }
    }
    run.clear();
}

/// Minimum anchor match ratio to trigger early termination.
/// 0.8 = 80% of anchor IDs found in the candidate. Set conservatively to
/// avoid false positives from partial dictionary fragments.
const EARLY_TERM_ANCHOR_RATIO: f64 = 0.8;

/// Minimum anchor set size for early termination to activate.
/// With fewer anchors, a high ratio could be coincidental.
const EARLY_TERM_MIN_ANCHORS: usize = 5;

/// Scan all memory regions using priority-band ordering.
///
/// Regions are sorted into three priority bands based on size:
/// - **Band 0 (4–64 MB, ascending)**: Mono/Boehm GC heap segments where the
///   Dictionary<int,int> entries array lives. Ascending order within the band
///   ensures the smallest GC segments (typically 16 MB) are checked first.
/// - **Band 1 (64–128 MB, descending)**: Larger GC segments or edge cases.
/// - **Band 2 (>128 MB and <4 MB, descending)**: Unity native allocator pools
///   (textures, assets) and tiny regions — almost never contain managed objects.
///
/// Combined with early termination (anchor-based or first-candidate), this
/// typically finds the collection in the first few regions (<1 second) instead
/// of wasting time on 100+ MB native texture pools.
///
/// ReadProcessMemory is kernel-serialized — parallelism creates thread
/// contention and floods the target process. Sequential scanning avoids this.
///
/// `yield_ms` controls the inter-region sleep duration. Pass 0 for no sleep.
pub fn scan_all_regions(
    handle: &ProcessHandle,
    regions: &[MemRegion],
    valid_ids: &HashSet<i32>,
    anchor_ids: &HashSet<i32>,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    cancel_flag: Option<&AtomicBool>,
    yield_ms: u64,
) -> Vec<CandidateBlock> {
    // Priority-band sort: GC sweet spot first (4–64 MB ascending),
    // then larger GC (64–128 MB descending), then everything else.
    let mut sorted_indices: Vec<usize> = (0..regions.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let sa = regions[a].size;
        let sb = regions[b].size;

        fn priority(size: usize) -> u8 {
            const MB: usize = 1024 * 1024;
            if size >= 4 * MB && size <= 64 * MB { 0 }      // GC sweet spot
            else if size > 64 * MB && size <= 128 * MB { 1 } // larger GC
            else { 2 }                                        // native / tiny
        }

        let pa = priority(sa);
        let pb = priority(sb);

        match pa.cmp(&pb) {
            std::cmp::Ordering::Equal => {
                if pa == 0 {
                    sa.cmp(&sb) // ascending within primary band
                } else {
                    sb.cmp(&sa) // descending in other bands
                }
            }
            other => other,
        }
    });

    let total = regions.len();

    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut all_candidates: Vec<CandidateBlock> = Vec::new();
    let can_early_term = anchor_ids.len() >= EARLY_TERM_MIN_ANCHORS;

    for (i, &region_idx) in sorted_indices.iter().enumerate() {
        // Check cancellation between regions
        if let Some(flag) = cancel_flag {
            if flag.load(Ordering::Relaxed) {
                log::info!("Scan cancelled after {}/{} regions", i, total);
                break;
            }
        }

        let candidates = scan_region(handle, &regions[region_idx], valid_ids, anchor_ids, &mut buffer);

        if !candidates.is_empty() {
            log::info!(
                "  Region #{} ({:.1} MB @ {:#x}): {} candidates",
                i + 1,
                regions[region_idx].size as f64 / (1024.0 * 1024.0),
                regions[region_idx].base,
                candidates.len(),
            );
        }

        all_candidates.extend(candidates);

        if let Some(cb) = &progress {
            cb(i + 1, total);
        }

        // Priority 1: anchor-based termination (high confidence).
        // If we have a candidate matching ≥80% of anchor IDs, it's the
        // collection dictionary — no need to scan remaining regions.
        if can_early_term {
            if let Some(best) = all_candidates.last() {
                if best.anchor_total > 0 {
                    let ratio = best.anchor_hits as f64 / best.anchor_total as f64;
                    if ratio >= EARLY_TERM_ANCHOR_RATIO && best.density() >= MIN_DENSITY {
                        log::info!(
                            "Early termination (anchors) after {}/{} regions: {} entries, \
                             {}/{} anchors ({:.0}%), density={:.1}%",
                            i + 1,
                            total,
                            best.entries.len(),
                            best.anchor_hits,
                            best.anchor_total,
                            ratio * 100.0,
                            best.density() * 100.0,
                        );
                        break;
                    }
                }
            }
        }

        // Priority 2: first-candidate termination (fallback when no anchors).
        // Validation is strict enough (≥100 entries matching known card IDs with
        // correct hash codes at ≥40% density) that any candidate passing all
        // checks is genuine — false positives are near-impossible.
        if !can_early_term && !all_candidates.is_empty() {
            log::info!(
                "Early termination (no anchors) after {}/{} regions: {} entries, density={:.1}%",
                i + 1,
                total,
                all_candidates.last().unwrap().entries.len(),
                all_candidates.last().unwrap().density() * 100.0,
            );
            break;
        }

        if yield_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(yield_ms));
        }
    }

    all_candidates.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_candidates
}

/// Build a deduplicated collection from a candidate block.
///
/// Takes max quantity per card_id (same dedup strategy as Python).
/// Excludes non-collectible IDs.
pub fn build_collection(
    block: &CandidateBlock,
    non_collectible_ids: &HashSet<i32>,
) -> HashMap<i32, i32> {
    let mut collection: HashMap<i32, i32> = HashMap::with_capacity(block.entries.len());
    for &(card_id, quantity) in &block.entries {
        if !non_collectible_ids.contains(&card_id) {
            let entry = collection.entry(card_id).or_insert(0);
            *entry = (*entry).max(quantity);
        }
    }
    collection
}

/// Re-read a known memory address for the collection dictionary.
///
/// Used by the watch loop after a full scan has located the collection.
/// Reads a single contiguous block (no region enumeration, no parallelism)
/// and validates entries using the same logic as a full scan.
/// Returns the parsed collection, or an error if the block has moved/corrupted.
pub fn rescan_block(
    handle: &ProcessHandle,
    address: usize,
    estimated_entries: usize,
    valid_ids: &HashSet<i32>,
    non_collectible_ids: &HashSet<i32>,
) -> Result<HashMap<i32, i32>, String> {
    // 2x buffer for dictionary growth between polls
    let read_size = estimated_entries * 2 * ENTRY_SIZE;
    let mut buffer = vec![0u8; read_size];

    let bytes_read = handle.read_memory(address, &mut buffer)?;

    if bytes_read < ENTRY_SIZE {
        return Err("Read too few bytes from cached address".to_string());
    }

    let num_entries = bytes_read / ENTRY_SIZE;
    let mut collection: HashMap<i32, i32> = HashMap::with_capacity(estimated_entries);
    let mut valid_count = 0usize;

    // Build scan_ids: valid_ids minus non_collectible (same as full scan)
    let scan_ids: HashSet<i32> = if non_collectible_ids.is_empty() {
        valid_ids.clone()
    } else {
        valid_ids.difference(non_collectible_ids).copied().collect()
    };

    for i in 0..num_entries {
        let pos = i * ENTRY_SIZE;

        let hash_code = i32::from_le_bytes(buffer[pos..pos + 4].try_into().unwrap());
        let next_idx = i32::from_le_bytes(buffer[pos + 4..pos + 8].try_into().unwrap());
        let key = i32::from_le_bytes(buffer[pos + 8..pos + 12].try_into().unwrap());
        let value = i32::from_le_bytes(buffer[pos + 12..pos + 16].try_into().unwrap());

        if validate_entry(hash_code, next_idx, key, value, &scan_ids) {
            valid_count += 1;
            if !non_collectible_ids.contains(&key) {
                let entry = collection.entry(key).or_insert(0);
                *entry = (*entry).max(value);
            }
        }
    }

    if valid_count < MIN_VALID_ENTRIES {
        return Err(format!(
            "Only {} valid entries found (minimum {}), block likely moved",
            valid_count, MIN_VALID_ENTRIES,
        ));
    }

    Ok(collection)
}

/// Compute the diff between two collections.
///
/// Returns added, increased, and removed entries.
pub fn diff_collections(
    old: &HashMap<i32, i32>,
    new: &HashMap<i32, i32>,
) -> CollectionDiff {
    let mut added = Vec::new();
    let mut increased = Vec::new();
    let mut removed = Vec::new();

    // Check for new/increased cards
    for (&card_id, &new_qty) in new {
        match old.get(&card_id) {
            None => added.push((card_id, new_qty)),
            Some(&old_qty) if new_qty > old_qty => {
                increased.push((card_id, old_qty, new_qty));
            }
            _ => {}
        }
    }

    // Check for removed cards
    for (&card_id, &old_qty) in old {
        if !new.contains_key(&card_id) {
            removed.push((card_id, old_qty));
        }
    }

    CollectionDiff {
        added,
        increased,
        removed,
    }
}

/// Cross-validate two collection reads for consistency.
pub fn cross_validate(
    col_a: &HashMap<i32, i32>,
    col_b: &HashMap<i32, i32>,
) -> CrossValidation {
    let ids_a: HashSet<i32> = col_a.keys().copied().collect();
    let ids_b: HashSet<i32> = col_b.keys().copied().collect();
    let shared: HashSet<i32> = ids_a.intersection(&ids_b).copied().collect();

    let quantity_matches = shared
        .iter()
        .filter(|id| col_a[id] == col_b[id])
        .count();
    let quantity_mismatches = shared.len() - quantity_matches;

    CrossValidation {
        block_a_unique: ids_a.len(),
        block_b_unique: ids_b.len(),
        shared_ids: shared.len(),
        quantity_matches,
        quantity_mismatches,
        only_in_a: ids_a.difference(&ids_b).count(),
        only_in_b: ids_b.difference(&ids_a).count(),
    }
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
