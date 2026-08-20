// Inventory data types extracted from Player.log's StartHook response.
//
// These mirror the Python reference in `inventory_parser.py` and `log_parser.py`,
// with Rust-typed fields instead of Python dataclasses.

use std::collections::HashMap;

use serde::Serialize;

/// A stack of boosters for a single collation (set).
#[derive(Debug, Clone, Serialize)]
pub struct BoosterStack {
    pub collation_id: i32,
    pub count: i32,
    pub set_code: Option<String>,
}

/// Full inventory state from the StartHook response.
///
/// Extracted from the InventoryInfo + DeckSummaries keys in the StartHook JSON blob.
/// Divergence from Python: `set_code` on BoosterStack (Python doesn't extract it).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerInventory {
    pub gold: i64,
    pub gems: i64,
    pub vault_progress: f64,
    pub wc_common: i32,
    pub wc_uncommon: i32,
    pub wc_rare: i32,
    pub wc_mythic: i32,
    pub wc_track_position: i32,
    pub draft_tokens: i32,
    pub sealed_tokens: i32,
    pub boosters: Vec<BoosterStack>,
    pub total_boosters: i32,
    pub custom_tokens: HashMap<String, i32>,
    pub user_deck_count: i32,
    pub precon_deck_count: i32,
}

/// Scalar fields read directly from MTGA.exe's ClientPlayerInventory object.
///
/// These bypass Player.log entirely — gems and wcRare are MISSING from StartHook
/// (Wizards removed them from serialization). Memory scanning is the only way to
/// get these values.
///
/// Layout: 40 bytes, fixed offsets, matches .NET ClientPlayerInventory struct.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryInventoryScalars {
    pub wc_common: i32,
    pub wc_uncommon: i32,
    pub wc_rare: i32,
    pub wc_mythic: i32,
    pub gold: i32,
    pub gems: i32,
    pub wc_track_position: i32,
    pub vault_progress: f64,
}

impl PlayerInventory {
    /// Merge memory-scanned scalars into this log-derived inventory.
    ///
    /// Memory values overwrite all scalar fields (they're more current).
    /// Log-only fields (boosters, tokens, decks) are preserved from self.
    pub fn merge_with_memory(&self, mem: &MemoryInventoryScalars) -> PlayerInventory {
        PlayerInventory {
            gold: mem.gold as i64,
            gems: mem.gems as i64,
            vault_progress: mem.vault_progress,
            wc_common: mem.wc_common,
            wc_uncommon: mem.wc_uncommon,
            wc_rare: mem.wc_rare,
            wc_mythic: mem.wc_mythic,
            wc_track_position: mem.wc_track_position,
            // Log-only fields preserved
            draft_tokens: self.draft_tokens,
            sealed_tokens: self.sealed_tokens,
            boosters: self.boosters.clone(),
            total_boosters: self.total_boosters,
            custom_tokens: self.custom_tokens.clone(),
            user_deck_count: self.user_deck_count,
            precon_deck_count: self.precon_deck_count,
        }
    }
}

/// A card granted via an inventory change (pack opening, reward, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct GrantedCard {
    pub grp_id: i32,
    pub set_code: String,
    pub card_added: bool,
    pub gems_compensation: i32,
    pub vault_progress: i32,
}

/// A single cosmetic art style entry (card art variant).
#[derive(Debug, Clone, Serialize)]
pub struct CosmeticArtStyle {
    pub grp_id: i32,
    pub art_id: i32,
}

/// Known cosmetic category keys in the StartHook InventoryInfo.Cosmetics object.
pub const KNOWN_COSMETIC_CATEGORIES: &[&str] =
    &["ArtStyles", "Avatars", "Sleeves", "Pets", "Emotes", "Titles"];

/// Full cosmetics state from StartHook. Known categories are typed; unknown ones
/// are captured in `other` for dynamic discovery (SchemaGuard principle).
#[derive(Debug, Clone, Serialize, Default)]
pub struct PlayerCosmetics {
    pub art_styles: Vec<CosmeticArtStyle>,
    pub avatars: Vec<String>,
    pub sleeves: Vec<String>,
    pub pets: Vec<String>,
    pub emotes: Vec<String>,
    pub titles: Vec<String>,
    /// Dynamic catch-all: any cosmetic category not in KNOWN_COSMETIC_CATEGORIES.
    /// Logged as discovery info when first seen. Rendered dynamically in UI.
    pub other: HashMap<String, Vec<serde_json::Value>>,
}

impl PlayerCosmetics {
    /// Merge incoming cosmetics change into this state (deduplication).
    pub fn merge_change(&mut self, change: &CosmeticsChange) {
        // Art styles: deduplicate by (grp_id, art_id)
        for style in &change.art_styles {
            if !self.art_styles.iter().any(|s| s.grp_id == style.grp_id && s.art_id == style.art_id) {
                self.art_styles.push(style.clone());
            }
        }
        // String categories: deduplicate by value
        merge_string_vec(&mut self.avatars, &change.avatars);
        merge_string_vec(&mut self.sleeves, &change.sleeves);
        merge_string_vec(&mut self.pets, &change.pets);
        merge_string_vec(&mut self.emotes, &change.emotes);
        merge_string_vec(&mut self.titles, &change.titles);
        // Other: merge by key, append new values
        for (key, values) in &change.other {
            let entry = self.other.entry(key.clone()).or_default();
            for v in values {
                if !entry.contains(v) {
                    entry.push(v.clone());
                }
            }
        }
    }
}

/// Cosmetics gained in a single InventoryChange entry.
#[derive(Debug, Clone, Serialize, Default)]
pub struct CosmeticsChange {
    pub art_styles: Vec<CosmeticArtStyle>,
    pub avatars: Vec<String>,
    pub sleeves: Vec<String>,
    pub pets: Vec<String>,
    pub emotes: Vec<String>,
    pub titles: Vec<String>,
    pub other: HashMap<String, Vec<serde_json::Value>>,
}

/// A single inventory mutation event.
///
/// Maps to one entry in the InventoryInfo.Changes array from ProcessInventoryUpdate.
#[derive(Debug, Clone, Serialize)]
pub struct InventoryChange {
    pub source: String,
    pub granted_cards: Vec<GrantedCard>,
    pub gold_delta: i64,
    pub gems_delta: i64,
    pub wc_common_delta: i32,
    pub wc_uncommon_delta: i32,
    pub wc_rare_delta: i32,
    pub wc_mythic_delta: i32,
    pub vault_progress_delta: i32,
    pub xp_delta: i32,
    pub boosters: Vec<BoosterStack>,
    pub cosmetics: CosmeticsChange,
    pub custom_tokens_delta: HashMap<String, i32>,
    pub wc_track_position_delta: i32,
    /// Event name derived from active courses (set by handle_inventory_update
    /// for event-related sources like EventPayEntry, EventReward, Draft, Sealed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
}

/// A single card entry in a deck zone (mainboard/sideboard).
#[derive(Debug, Clone, Serialize)]
pub struct DeckCard {
    pub grp_id: i32,
    pub quantity: i32,
}

/// Full deck contents: name, format, and card lists.
#[derive(Debug, Clone, Serialize)]
pub struct DeckContents {
    pub name: String,
    pub format: Option<String>,
    pub mainboard: Vec<DeckCard>,
    pub sideboard: Vec<DeckCard>,
}

/// Helper: merge new strings into existing vec, deduplicating.
fn merge_string_vec(existing: &mut Vec<String>, incoming: &[String]) {
    for s in incoming {
        if !existing.contains(s) {
            existing.push(s.clone());
        }
    }
}
