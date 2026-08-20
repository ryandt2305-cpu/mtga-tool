// StartHook and InventoryChange parsing.
//
// Port of Python's `inventory_parser.py` and relevant parts of `log_parser.py`.
// Pure functions — no state, no I/O, fully testable.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tauri::AppHandle;

use crate::models::inventory::{
    BoosterStack, CosmeticArtStyle, CosmeticsChange, GrantedCard, InventoryChange,
    PlayerCosmetics, PlayerInventory, KNOWN_COSMETIC_CATEGORIES,
};
use crate::models::{EventCourse, MasteryState};
use crate::services::diagnostics::{self, LOG_014};
use crate::services::log::parser;

/// Parse the StartHook JSON blob into inventory, non-collectible IDs, and anchor IDs.
///
/// The StartHook is the large (~228KB) JSON line containing InventoryInfo,
/// DeckSummaries, Decks, and CardMetadataInfo. It appears near the start of
/// each Player.log session.
///
/// Returns `(PlayerInventory, non_collectible_ids, anchor_ids)`.
pub fn parse_start_hook(
    data: &Value,
) -> Result<(PlayerInventory, HashSet<i32>, HashSet<i32>), String> {
    let inv = data
        .get("InventoryInfo")
        .ok_or_else(|| "StartHook missing InventoryInfo".to_string())?;

    // --- Boosters ---
    let boosters: Vec<BoosterStack> = inv
        .get("Boosters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|b| {
                    Some(BoosterStack {
                        collation_id: b.get("CollationId")?.as_i64()? as i32,
                        count: b.get("Count")?.as_i64()? as i32,
                        set_code: b.get("SetCode").and_then(|v| v.as_str()).map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let total_boosters: i32 = boosters.iter().map(|b| b.count).sum();

    // --- Deck counts ---
    let summaries_list = data
        .get("DeckSummaries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut user_deck_count: i32 = 0;
    let mut precon_deck_count: i32 = 0;
    for s in &summaries_list {
        if is_precon(s) {
            precon_deck_count += 1;
        } else {
            user_deck_count += 1;
        }
    }

    // --- Custom tokens ---
    let custom_tokens: HashMap<String, i32> = inv
        .get("CustomTokens")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_i64()? as i32)))
                .collect()
        })
        .unwrap_or_default();

    // --- Draft/Sealed tokens (may be top-level or in CustomTokens) ---
    let draft_tokens = inv
        .get("DraftTokens")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .filter(|&v| v != 0)
        .or_else(|| custom_tokens.get("DraftToken").copied())
        .unwrap_or(0);

    let sealed_tokens = inv
        .get("SealedTokens")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .filter(|&v| v != 0)
        .or_else(|| custom_tokens.get("SealedToken").copied())
        .unwrap_or(0);

    let inventory = PlayerInventory {
        gold: inv.get("Gold").and_then(|v| v.as_i64()).unwrap_or(0),
        gems: inv.get("Gems").and_then(|v| v.as_i64()).unwrap_or(0),
        vault_progress: inv
            .get("TotalVaultProgress")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            / 10.0,
        wc_common: get_i32(inv, "WildCardCommons"),
        wc_uncommon: get_i32(inv, "WildCardUnCommons"),
        wc_rare: get_i32(inv, "WildCardRares"),
        wc_mythic: get_i32(inv, "WildCardMythics"),
        wc_track_position: get_i32(inv, "wcTrackPosition"),
        draft_tokens,
        sealed_tokens,
        boosters,
        total_boosters,
        custom_tokens,
        user_deck_count,
        precon_deck_count,
    };

    // --- Non-collectible IDs ---
    let non_collectible_ids: HashSet<i32> = data
        .get("CardMetadataInfo")
        .and_then(|v| v.get("NonCollectibleCardList"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64().map(|n| n as i32)).collect())
        .unwrap_or_default();

    // --- Anchor IDs (card IDs from user-created decks) ---
    let summaries_by_id: HashMap<String, &Value> = summaries_list
        .iter()
        .filter_map(|s| {
            let id = s.get("DeckId")?.as_str()?.to_string();
            Some((id, s))
        })
        .collect();

    let mut anchor_ids: HashSet<i32> = HashSet::new();
    if let Some(decks) = data.get("Decks").and_then(|v| v.as_object()) {
        for (deck_id, deck) in decks {
            let summary = summaries_by_id.get(deck_id).copied();
            if summary.map_or(false, is_precon) {
                continue;
            }
            extract_deck_card_ids(deck, &mut anchor_ids);
        }
    }

    Ok((inventory, non_collectible_ids, anchor_ids))
}

/// Parse an InventoryUpdate event into a list of changes.
pub fn parse_inventory_changes(data: &Value) -> Result<Vec<InventoryChange>, String> {
    let changes = data
        .get("InventoryInfo")
        .and_then(|v| v.get("Changes"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| "InventoryUpdate missing InventoryInfo.Changes".to_string())?;

    let mut result = Vec::with_capacity(changes.len());
    for change in changes {
        let source = change
            .get("Source")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let granted_cards: Vec<GrantedCard> = change
            .get("GrantedCards")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        Some(GrantedCard {
                            grp_id: c.get("GrpId")?.as_i64()? as i32,
                            set_code: c
                                .get("SetCode")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            card_added: c
                                .get("CardAdded")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true),
                            gems_compensation: get_i32(c, "GemsCompensation"),
                            vault_progress: get_i32(c, "VaultProgress"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let booster_changes: Vec<BoosterStack> = change
            .get("Boosters")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| {
                        Some(BoosterStack {
                            collation_id: b.get("CollationId")?.as_i64()? as i32,
                            count: b.get("Count")?.as_i64()? as i32,
                            set_code: b.get("SetCode").and_then(|v| v.as_str()).map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse cosmetics from this change entry
        let cosmetics = parse_cosmetics_change(change);

        // Parse custom token deltas from this change entry.
        // JSON field name: "CustomTokens" (matches StartHook pattern where
        // InventoryGold→"Gold"). Falls back to "InventoryCustomTokens" (C# property name).
        let custom_tokens_delta: HashMap<String, i32> = change
            .get("CustomTokens")
            .or_else(|| change.get("InventoryCustomTokens"))
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_i64()? as i32)))
                    .collect()
            })
            .unwrap_or_default();

        result.push(InventoryChange {
            source,
            granted_cards,
            gold_delta: change.get("Gold").and_then(|v| v.as_i64()).unwrap_or(0),
            gems_delta: change.get("Gems").and_then(|v| v.as_i64()).unwrap_or(0),
            wc_common_delta: get_i32(change, "WildCardCommons"),
            wc_uncommon_delta: get_i32(change, "WildCardUnCommons"),
            wc_rare_delta: get_i32(change, "WildCardRares"),
            wc_mythic_delta: get_i32(change, "WildCardMythics"),
            wc_track_position_delta: get_i32(change, "wcTrackPosition"),
            vault_progress_delta: get_i32(change, "TotalVaultProgress"),
            xp_delta: get_i32(change, "SetMasteryProgress"),
            boosters: booster_changes,
            cosmetics,
            custom_tokens_delta,
            event_name: None, // Set by handle_inventory_update from courses cache
        });
    }

    Ok(result)
}

/// Check if a deck summary represents a precon deck.
/// Same logic as Python: `?=?` in name or `Loc/` in name or `Precon` in description.
pub fn is_precon(summary: &Value) -> bool {
    let name = summary
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let desc = summary
        .get("Description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    name.contains("?=?") || name.contains("Loc/") || desc.contains("Precon")
}

/// Extract card IDs from a deck's zones into the given set.
fn extract_deck_card_ids(deck: &Value, ids: &mut HashSet<i32>) {
    for zone in &["MainDeck", "Sideboard", "CommandZone", "Companions"] {
        if let Some(entries) = deck.get(*zone).and_then(|v| v.as_array()) {
            for entry in entries {
                let cid = entry.get("cardId").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                if cid != 0 {
                    ids.insert(cid);
                }
            }
        }
    }
}

/// Parse a CourseList event (`EventGetCoursesV2`) into active event courses.
///
/// The payload has a top-level `Courses` array. Each entry contains:
/// - `CourseId`, `InternalEventName`, `CurrentModule`
/// - Win/loss in root, `CourseDeckSummary`, or `ModulePayload`
pub fn parse_course_list(data: &Value) -> Result<Vec<EventCourse>, String> {
    let courses = data
        .get("Courses")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "CourseList missing Courses array".to_string())?;

    let mut result = Vec::with_capacity(courses.len());
    for course in courses {
        let course_id = course
            .get("CourseId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let event_name = course
            .get("InternalEventName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let module = course
            .get("CurrentModule")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if course_id.is_empty() && event_name.is_empty() {
            continue;
        }

        let (wins, losses) = parser::find_win_loss(course);
        let (max_wins, max_losses) = parser::find_max_win_loss(course);

        result.push(EventCourse {
            course_id,
            internal_event_name: event_name,
            current_module: module,
            wins: wins.map(|v| v as i32),
            losses: losses.map(|v| v as i32),
            max_wins: max_wins.map(|v| v as i32),
            max_losses: max_losses.map(|v| v as i32),
        });
    }

    Ok(result)
}

/// Parse the Cosmetics object from InventoryInfo in StartHook.
///
/// Extracts known categories into typed fields. Unknown categories go into `other`
/// and emit a discovery diagnostic (SchemaGuard principle).
pub fn parse_cosmetics(cosmetics_obj: &Value, app: Option<&AppHandle>) -> PlayerCosmetics {
    let obj = match cosmetics_obj.as_object() {
        Some(o) => o,
        None => return PlayerCosmetics::default(),
    };

    let mut result = PlayerCosmetics::default();

    for (key, value) in obj {
        match key.as_str() {
            "ArtStyles" => {
                result.art_styles = parse_art_styles(value);
            }
            "Avatars" => {
                result.avatars = parse_cosmetic_entries(value);
            }
            "Sleeves" => {
                result.sleeves = parse_cosmetic_entries(value);
            }
            "Pets" => {
                result.pets = parse_cosmetic_entries(value);
            }
            "Emotes" => {
                result.emotes = parse_cosmetic_entries(value);
            }
            "Titles" => {
                result.titles = parse_cosmetic_entries(value);
            }
            _ => {
                // Unknown category — capture in `other` and emit discovery info
                if let Some(app) = app {
                    diagnostics::emit_info(
                        app,
                        &LOG_014,
                        &format!("New cosmetic category discovered: {}", key),
                    );
                }
                if let Some(arr) = value.as_array() {
                    result.other.insert(key.clone(), arr.clone());
                } else {
                    result.other.insert(key.clone(), vec![value.clone()]);
                }
            }
        }
    }

    result
}

/// Parse cosmetics from a single InventoryChange entry.
///
/// InventoryChange can contain cosmetic grants as arrays at the top level
/// (same keys as the Cosmetics object in StartHook).
pub fn parse_cosmetics_change(change: &Value) -> CosmeticsChange {
    let mut result = CosmeticsChange::default();

    let obj = match change.as_object() {
        Some(o) => o,
        None => return result,
    };

    // Check for cosmetic-related keys in the change
    if let Some(v) = obj.get("ArtStyles") {
        result.art_styles = parse_art_styles(v);
    }
    if let Some(v) = obj.get("Avatars") {
        result.avatars = parse_cosmetic_entries(v);
    }
    if let Some(v) = obj.get("Sleeves") {
        result.sleeves = parse_cosmetic_entries(v);
    }
    if let Some(v) = obj.get("Pets") {
        result.pets = parse_cosmetic_entries(v);
    }
    if let Some(v) = obj.get("Emotes") {
        result.emotes = parse_cosmetic_entries(v);
    }
    if let Some(v) = obj.get("Titles") {
        result.titles = parse_cosmetic_entries(v);
    }

    // Capture unknown cosmetic-looking arrays
    for (key, value) in obj {
        if KNOWN_COSMETIC_CATEGORIES.contains(&key.as_str()) {
            continue;
        }
        // Skip non-cosmetic known keys
        if matches!(
            key.as_str(),
            "Source" | "Gold" | "Gems" | "GrantedCards" | "Boosters"
                | "WildCardCommons" | "WildCardUnCommons" | "WildCardRares"
                | "WildCardMythics" | "TotalVaultProgress" | "SetMasteryProgress"
                | "CollationId" | "Count" | "SetCode"
                | "CustomTokens" | "InventoryCustomTokens"
                | "wcTrackPosition"
        ) {
            continue;
        }
        // Heuristic: if it's an array, it might be a new cosmetic category
        if let Some(arr) = value.as_array() {
            if !arr.is_empty() {
                result.other.insert(key.clone(), arr.clone());
            }
        }
    }

    result
}

/// Parse a ProgressUpdate event into MasteryState.
///
/// The ProgressUpdate payload contains `NodeStates` and `MilestoneStates`.
pub fn parse_progress_update(data: &Value) -> Result<MasteryState, String> {
    // ProgressUpdate can have the data at top level or nested
    let node_states = data.get("NodeStates")
        .or_else(|| data.get("InventoryInfo").and_then(|v| v.get("NodeStates")));
    let milestone_states = data.get("MilestoneStates")
        .or_else(|| data.get("InventoryInfo").and_then(|v| v.get("MilestoneStates")));

    let (node_count, nodes_completed) = count_nodes(node_states);
    let (milestone_count, milestones_completed) = count_milestones(milestone_states);

    if node_count == 0 && milestone_count == 0 {
        return Err("ProgressUpdate: no NodeStates or MilestoneStates found".to_string());
    }

    let level_info = extract_level_track(node_states);

    Ok(MasteryState {
        raw: data.clone(),
        node_count,
        milestone_count,
        milestones_completed,
        current_level: level_info.current_level,
        max_level: level_info.max_level,
        current_xp: level_info.current_xp,
        has_premium: level_info.has_premium,
        nodes_completed,
    })
}

/// Extract mastery/graph state from StartHook's UpdatedGraphs field.
pub fn parse_updated_graphs(data: &Value) -> Option<MasteryState> {
    let graphs = data.get("UpdatedGraphs")
        .or_else(|| data.get("InventoryInfo").and_then(|v| v.get("UpdatedGraphs")))?;

    let node_states = graphs.get("NodeStates");
    let milestone_states = graphs.get("MilestoneStates");

    let (node_count, nodes_completed) = count_nodes(node_states);
    let (milestone_count, milestones_completed) = count_milestones(milestone_states);

    if node_count == 0 && milestone_count == 0 {
        return None;
    }

    let level_info = extract_level_track(node_states);

    Some(MasteryState {
        raw: graphs.clone(),
        node_count,
        milestone_count,
        milestones_completed,
        current_level: level_info.current_level,
        max_level: level_info.max_level,
        current_xp: level_info.current_xp,
        has_premium: level_info.has_premium,
        nodes_completed,
    })
}

/// Parse art styles array: each entry has {GrpId, ArtId} or is a simple int.
fn parse_art_styles(value: &Value) -> Vec<CosmeticArtStyle> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    arr.iter()
        .filter_map(|item| {
            if let Some(obj) = item.as_object() {
                let grp_id = obj.get("GrpId").or(obj.get("grpId"))
                    .and_then(|v| v.as_i64())? as i32;
                let art_id = obj.get("ArtId").or(obj.get("artId"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                Some(CosmeticArtStyle { grp_id, art_id })
            } else if let Some(id) = item.as_i64() {
                Some(CosmeticArtStyle { grp_id: id as i32, art_id: 0 })
            } else {
                None
            }
        })
        .collect()
}

/// Extract string IDs from a cosmetics array that may contain strings OR objects.
///
/// CosmeticsClient (StartHook) uses typed entries (CosmeticsAvatarEntry, etc.)
/// which serialize as objects. InventoryChange uses plain strings.
/// This handles both formats:
/// - Strings: used directly
/// - Objects: tries "Type", "Id", "Name" fields, then first string field
/// - Numbers: converted to string
fn parse_cosmetic_entries(value: &Value) -> Vec<String> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    if arr.is_empty() {
        return Vec::new();
    }

    // Log first entry structure for debugging unknown formats
    if let Some(first) = arr.first() {
        if first.is_object() {
            log::debug!("Cosmetic entry format (first item): {}", first);
        }
    }

    arr.iter()
        .filter_map(|item| {
            // Simple string
            if let Some(s) = item.as_str() {
                return Some(s.to_string());
            }
            // Number (some IDs are numeric)
            if let Some(n) = item.as_i64() {
                return Some(n.to_string());
            }
            // Object: try common ID field names
            if let Some(obj) = item.as_object() {
                // Try known field names in priority order
                for key in &["Type", "Id", "Name", "type", "id", "name"] {
                    if let Some(val) = obj.get(*key) {
                        if let Some(s) = val.as_str() {
                            return Some(s.to_string());
                        }
                        if let Some(n) = val.as_i64() {
                            return Some(n.to_string());
                        }
                    }
                }
                // Fallback: use first string value in the object
                for (_k, v) in obj {
                    if let Some(s) = v.as_str() {
                        return Some(s.to_string());
                    }
                }
                // Last resort: stringify the whole object so we at least count it
                return Some(item.to_string());
            }
            None
        })
        .collect()
}

/// Helper: get an i32 from a JSON value field, defaulting to 0.
fn get_i32(obj: &Value, key: &str) -> i32 {
    obj.get(key).and_then(|v| v.as_i64()).unwrap_or(0) as i32
}

/// Extracted level track info from NodeStates.
struct LevelTrackInfo {
    current_level: u32,
    max_level: u32,
    current_xp: u32,
    has_premium: bool,
}

/// Count total and completed nodes from NodeStates.
/// Nodes are objects with a `"Status"` field; completed ones have `"Status": "Completed"`.
fn count_nodes(node_states: Option<&Value>) -> (usize, usize) {
    match node_states.and_then(|v| v.as_object()) {
        Some(obj) => {
            let total = obj.len();
            let completed = obj.values()
                .filter(|v| v.get("Status").and_then(|s| s.as_str()) == Some("Completed"))
                .count();
            (total, completed)
        }
        None => {
            let total = node_states.and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            (total, 0)
        }
    }
}

/// Count total and completed milestones from MilestoneStates.
/// Milestones can be boolean values (`true` = completed) or objects.
fn count_milestones(milestone_states: Option<&Value>) -> (usize, usize) {
    match milestone_states.and_then(|v| v.as_object()) {
        Some(obj) => {
            let total = obj.len();
            let completed = obj.values()
                .filter(|v| {
                    // Boolean format: true = completed
                    if let Some(b) = v.as_bool() {
                        return b;
                    }
                    // Object format: check Status field
                    v.get("Status").and_then(|s| s.as_str()) == Some("Completed")
                })
                .count();
            (total, completed)
        }
        None => {
            let total = milestone_states.and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            (total, 0)
        }
    }
}

/// Parse the level track from NodeStates to extract meaningful mastery info.
///
/// Node naming convention: `LevelTrack_Level_N` for level gates,
/// `LevelTrack_Level_N_Reward` for reward nodes.
/// Completed levels have `"Status": "Completed"`.
/// Current level has `"Status": "Available"` with `ProgressNodeState.CurrentProgress`.
/// Premium detected from `TierRewardNodeState.CurrentTiers` containing "premium".
fn extract_level_track(node_states: Option<&Value>) -> LevelTrackInfo {
    let nodes = match node_states.and_then(|v| v.as_object()) {
        Some(n) => n,
        None => return LevelTrackInfo { current_level: 0, max_level: 0, current_xp: 0, has_premium: false },
    };

    let mut max_level: u32 = 0;
    let mut highest_completed: u32 = 0;
    let mut current_xp: u32 = 0;
    let mut has_premium = false;

    for (key, value) in nodes {
        // Match "LevelTrack_Level_N" (not "_Reward" suffix)
        if let Some(level_num) = parse_level_number(key) {
            if level_num > max_level {
                max_level = level_num;
            }

            let status = value.get("Status").and_then(|v| v.as_str()).unwrap_or("");

            if status == "Completed" && level_num > highest_completed {
                highest_completed = level_num;
            }

            // Check for XP progress on available (current) level
            if status == "Available" {
                if let Some(progress) = value.get("ProgressNodeState")
                    .and_then(|p| p.get("CurrentProgress"))
                    .and_then(|v| v.as_u64())
                {
                    current_xp = progress as u32;
                }
            }
        }

        // Check reward nodes for premium pass
        if key.ends_with("_Reward") {
            if let Some(tier_state) = value.get("TierRewardNodeState") {
                if let Some(tiers) = tier_state.get("CurrentTiers").and_then(|v| v.as_array()) {
                    for tier in tiers {
                        if tier.as_str() == Some("premium") {
                            has_premium = true;
                        }
                    }
                }
            }
        }
    }

    LevelTrackInfo {
        current_level: highest_completed,
        max_level,
        current_xp,
        has_premium,
    }
}

/// Extract level number from a node key like "LevelTrack_Level_12".
/// Returns None for reward nodes ("LevelTrack_Level_12_Reward") and non-level nodes.
fn parse_level_number(key: &str) -> Option<u32> {
    if !key.starts_with("LevelTrack_Level_") {
        return None;
    }
    // Skip if it's a reward node
    if key.ends_with("_Reward") {
        return None;
    }
    let suffix = &key["LevelTrack_Level_".len()..];
    suffix.parse::<u32>().ok()
}
