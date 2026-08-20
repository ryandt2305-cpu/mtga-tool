// Inventory scalar scanning — finds ClientPlayerInventory in MTGA.exe memory.
//
// Uses known-accurate StartHook values (gold, wcCommon, etc.) as search anchors.
// For each occurrence of the primary value, checks whether the other known values
// appear at their exact expected offsets in a 40-byte struct layout.
//
// Pure functions — no state, no side effects. Orchestration lives in mod.rs.

use crate::models::MemoryInventoryScalars;

use super::process::{MemRegion, ProcessHandle};

// --- Struct layout constants ---
// Matches .NET ClientPlayerInventory field order from DLL decompilation.

const INVENTORY_STRUCT_SIZE: usize = 40;
const OFFSET_WC_COMMON: usize = 0;
const OFFSET_WC_UNCOMMON: usize = 4;
const OFFSET_WC_RARE: usize = 8;
const OFFSET_WC_MYTHIC: usize = 12;
const OFFSET_GOLD: usize = 16;
const OFFSET_GEMS: usize = 20;
const OFFSET_WC_TRACK: usize = 24;
// +28 is padding (alignment for f64)
const OFFSET_VAULT: usize = 32;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB scan chunks

/// Known values from StartHook to use as search anchors.
///
/// At least 3 non-zero values are required for a reliable scan.
/// Fields that are MISSING from StartHook (gems, wcRare) must be None.
pub struct InventorySearchParams {
    pub gold: Option<i32>,
    pub wc_common: Option<i32>,
    pub wc_uncommon: Option<i32>,
    pub wc_mythic: Option<i32>,
    pub wc_track_position: Option<i32>,
}

/// A known field with its offset and value, used during struct matching.
struct KnownField {
    offset: usize,
    value: i32,
}

impl InventorySearchParams {
    /// Build the list of known fields and validate we have enough anchors.
    fn known_fields(&self) -> Result<Vec<KnownField>, String> {
        let mut fields = Vec::new();

        if let Some(v) = self.gold {
            if v != 0 {
                fields.push(KnownField {
                    offset: OFFSET_GOLD,
                    value: v,
                });
            }
        }
        if let Some(v) = self.wc_common {
            if v != 0 {
                fields.push(KnownField {
                    offset: OFFSET_WC_COMMON,
                    value: v,
                });
            }
        }
        if let Some(v) = self.wc_uncommon {
            if v != 0 {
                fields.push(KnownField {
                    offset: OFFSET_WC_UNCOMMON,
                    value: v,
                });
            }
        }
        if let Some(v) = self.wc_mythic {
            if v != 0 {
                fields.push(KnownField {
                    offset: OFFSET_WC_MYTHIC,
                    value: v,
                });
            }
        }
        if let Some(v) = self.wc_track_position {
            if v != 0 {
                fields.push(KnownField {
                    offset: OFFSET_WC_TRACK,
                    value: v,
                });
            }
        }

        if fields.len() < 3 {
            return Err(format!(
                "Need at least 3 non-zero known values, got {}",
                fields.len()
            ));
        }

        Ok(fields)
    }

    /// Pick the best primary search value — largest absolute value for fewest false positives.
    fn primary_field(fields: &[KnownField]) -> &KnownField {
        fields
            .iter()
            .max_by_key(|f| f.value.unsigned_abs())
            .expect("known_fields guarantees non-empty")
    }
}

/// Scan process memory for the ClientPlayerInventory struct.
///
/// Returns (base_address, scalars) on success. Searches all provided regions
/// for the primary known value, then validates the full struct layout.
pub fn scan_for_inventory(
    handle: &ProcessHandle,
    regions: &[MemRegion],
    params: &InventorySearchParams,
) -> Result<(usize, MemoryInventoryScalars), String> {
    let known_fields = params.known_fields()?;
    let primary = InventorySearchParams::primary_field(&known_fields);

    let primary_bytes = primary.value.to_le_bytes();

    let mut best_address: Option<usize> = None;
    let mut best_score: usize = 0;
    let mut best_scalars: Option<MemoryInventoryScalars> = None;
    let mut total_candidates: usize = 0;

    let mut chunk_buf = vec![0u8; CHUNK_SIZE];

    for region in regions {
        // Skip regions too small to hold the struct
        if region.size < INVENTORY_STRUCT_SIZE {
            continue;
        }

        let mut offset = 0;
        while offset < region.size {
            let read_size = std::cmp::min(CHUNK_SIZE, region.size - offset);
            let buf = &mut chunk_buf[..read_size];

            if handle.read_memory(region.base + offset, buf).is_err() {
                offset += read_size;
                continue;
            }

            // Scan for 4-byte LE occurrences of the primary value
            let max_pos = if read_size >= 4 { read_size - 3 } else { 0 };
            let mut pos = 0;
            while pos < max_pos {
                if buf[pos] == primary_bytes[0]
                    && buf[pos + 1] == primary_bytes[1]
                    && buf[pos + 2] == primary_bytes[2]
                    && buf[pos + 3] == primary_bytes[3]
                {
                    // Compute hypothetical struct base address
                    let hit_absolute = region.base + offset + pos;
                    if hit_absolute < primary.offset {
                        pos += 1;
                        continue;
                    }
                    let base_addr = hit_absolute - primary.offset;

                    // Try to read the full 40-byte struct
                    let mut struct_buf = [0u8; INVENTORY_STRUCT_SIZE];
                    if handle.read_memory(base_addr, &mut struct_buf).is_ok() {
                        let score = score_candidate(&struct_buf, &known_fields);

                        if score >= 3 {
                            total_candidates += 1;

                            if let Some(scalars) = parse_struct(&struct_buf) {
                                if validate_plausibility(&scalars) && score > best_score {
                                    best_score = score;
                                    best_address = Some(base_addr);
                                    best_scalars = Some(scalars);
                                }
                            }
                        }
                    }
                }
                pos += 1;
            }

            offset += read_size;
        }
    }

    match (best_address, best_scalars) {
        (Some(addr), Some(scalars)) => {
            log::info!(
                "Inventory scan: found struct at {:#x} (score={}, candidates={})",
                addr,
                best_score,
                total_candidates,
            );
            Ok((addr, scalars))
        }
        _ => Err(format!(
            "No matching ClientPlayerInventory struct found ({} partial candidates)",
            total_candidates,
        )),
    }
}

/// Read the inventory struct from a previously cached address.
///
/// Returns the parsed scalars if the read succeeds and values are plausible.
pub fn read_inventory_at(
    handle: &ProcessHandle,
    address: usize,
) -> Result<MemoryInventoryScalars, String> {
    let mut buf = [0u8; INVENTORY_STRUCT_SIZE];
    handle.read_memory(address, &mut buf)?;

    let scalars =
        parse_struct(&buf).ok_or_else(|| "Failed to parse inventory struct bytes".to_string())?;

    if !validate_plausibility(&scalars) {
        return Err(format!(
            "Implausible values at {:#x}: gold={}, gems={}, vault={:.1}",
            address, scalars.gold, scalars.gems, scalars.vault_progress,
        ));
    }

    Ok(scalars)
}

/// Count how many known fields match at their expected offsets.
fn score_candidate(buf: &[u8; INVENTORY_STRUCT_SIZE], known_fields: &[KnownField]) -> usize {
    let mut score = 0;
    for field in known_fields {
        if field.offset + 4 <= INVENTORY_STRUCT_SIZE {
            let actual = i32::from_le_bytes([
                buf[field.offset],
                buf[field.offset + 1],
                buf[field.offset + 2],
                buf[field.offset + 3],
            ]);
            if actual == field.value {
                score += 1;
            }
        }
    }
    score
}

/// Parse a 40-byte buffer into MemoryInventoryScalars.
fn parse_struct(buf: &[u8; INVENTORY_STRUCT_SIZE]) -> Option<MemoryInventoryScalars> {
    let read_i32 = |offset: usize| -> i32 {
        i32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
    };

    let vault_bytes: [u8; 8] = buf[OFFSET_VAULT..OFFSET_VAULT + 8].try_into().ok()?;
    let vault_progress = f64::from_le_bytes(vault_bytes);

    Some(MemoryInventoryScalars {
        wc_common: read_i32(OFFSET_WC_COMMON),
        wc_uncommon: read_i32(OFFSET_WC_UNCOMMON),
        wc_rare: read_i32(OFFSET_WC_RARE),
        wc_mythic: read_i32(OFFSET_WC_MYTHIC),
        gold: read_i32(OFFSET_GOLD),
        gems: read_i32(OFFSET_GEMS),
        wc_track_position: read_i32(OFFSET_WC_TRACK),
        vault_progress,
    })
}

/// Sanity-check that parsed values are within plausible game ranges.
fn validate_plausibility(s: &MemoryInventoryScalars) -> bool {
    s.wc_common >= 0
        && s.wc_uncommon >= 0
        && s.wc_rare >= 0
        && s.wc_mythic >= 0
        && s.gold >= 0
        && s.gems >= 0
        && s.wc_track_position >= 0
        && s.vault_progress >= 0.0
        && s.vault_progress <= 200.0
}
