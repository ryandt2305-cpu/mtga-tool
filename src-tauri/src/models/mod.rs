// Shared data types: card, inventory, collection, settings.

pub mod inventory;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use inventory::{
    BoosterStack, CosmeticArtStyle, CosmeticsChange, DeckCard, DeckContents, GrantedCard,
    InventoryChange, MemoryInventoryScalars, PlayerCosmetics, PlayerInventory,
};

/// An active event course (draft, sealed, constructed event, etc.).
///
/// Parsed from the `Courses` array in `EventGetCoursesV2` log events.
/// Win/loss data may be absent for events that don't track records
/// (e.g., Jump In, bot draft without match play).
#[derive(Debug, Clone, Serialize)]
pub struct EventCourse {
    pub course_id: String,
    pub internal_event_name: String,
    pub current_module: String,
    pub wins: Option<i32>,
    pub losses: Option<i32>,
    pub max_wins: Option<i32>,
    pub max_losses: Option<i32>,
}

/// Mastery/campaign graph state. Stores raw JSON for inspector + extracted level info.
///
/// The graph system serves multiple purposes: mastery pass (level track) AND
/// NPE/campaign progression. Level track fields are zero when the graph is
/// a campaign graph rather than a mastery pass.
#[derive(Debug, Clone, Serialize)]
pub struct MasteryState {
    pub raw: serde_json::Value,
    pub node_count: usize,
    pub milestone_count: usize,
    /// How many milestones have been completed (true values in MilestoneStates)
    pub milestones_completed: usize,
    /// Highest completed level in the track (e.g., 19 if Levels 1-19 are complete).
    /// Zero when the graph is not a mastery pass level track.
    pub current_level: u32,
    /// Max level seen in the track data. Zero when no level track present.
    pub max_level: u32,
    /// XP progress towards the next level (from ProgressNodeState.CurrentProgress)
    pub current_xp: u32,
    /// Whether the player has the premium mastery pass
    pub has_premium: bool,
    /// How many NodeStates have Status: "Completed"
    pub nodes_completed: usize,
}

/// Result of a completed match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub match_id: String,
    pub event_id: String,
    pub opponent_name: String,
    pub result: MatchOutcome,
    pub completed_reason: String,
    pub game_mode: Option<String>,
}

/// Win/loss/draw outcome of a match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchOutcome {
    Victory,
    Defeat,
    Draw,
    Unknown,
}

/// In-progress match tracking (not serialized to frontend).
pub struct ActiveMatchState {
    pub match_id: String,
    pub event_id: String,
    pub our_team_id: i64,
    pub opponent_name: String,
}

/// Rarity of a card in MTGA.
///
/// Integer values match the Rarity column in the Cards table
/// of MTGA's SQLite card database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    Special,
    BasicLand,
    Common,
    Uncommon,
    Rare,
    Mythic,
    Unknown(i32),
}

impl Rarity {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => Rarity::Special,
            1 => Rarity::BasicLand,
            2 => Rarity::Common,
            3 => Rarity::Uncommon,
            4 => Rarity::Rare,
            5 => Rarity::Mythic,
            other => Rarity::Unknown(other),
        }
    }
}

/// Core card data from MTGA's SQLite card database.
///
/// Mirrors Python's `CardInfo` dataclass in `card_database.py`.
/// Divergence: `rarity` is a typed enum instead of a string.
/// `cmc` is derived from `OldSchoolManaText`, `power`/`toughness` from TEXT columns.
#[derive(Debug, Clone, Serialize)]
pub struct CardInfo {
    pub grp_id: i32,
    pub name: String,
    pub set_code: String,
    pub rarity: Rarity,
    pub collector_number: String,
    pub cmc: i32,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub deck_limit: i32,
    /// Colour identity letters "W","U","B","R","G" (from Cards.ColorIdentity via Enums.CardColor).
    pub color_identity: Vec<String>,
    /// Colours letters (from Cards.Colors).
    pub colors: Vec<String>,
    /// Card types, e.g. "Creature", "Land" (Enums.CardType).
    pub types: Vec<String>,
    /// Subtypes, e.g. "Elf", "Warrior" (Enums.SubType).
    pub subtypes: Vec<String>,
    /// Supertypes, e.g. "Legendary", "Basic" (Enums.SuperType).
    pub supertypes: Vec<String>,
    pub is_rebalanced: bool,
    pub rebalanced_grp_id: Option<i32>,
    pub is_primary_card: bool,
}

/// Result of a memory scan for the card collection.
#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub collection: HashMap<i32, i32>,
    pub candidate_count: usize,
    pub cross_validation: Option<CrossValidation>,
    pub scan_duration_ms: u64,
}

/// Diff between two collection snapshots.
#[derive(Debug, Clone, Serialize)]
pub struct CollectionDiff {
    /// Cards that didn't exist before: (card_id, new_quantity)
    pub added: Vec<(i32, i32)>,
    /// Cards whose quantity increased: (card_id, old_qty, new_qty)
    pub increased: Vec<(i32, i32, i32)>,
    /// Cards that disappeared: (card_id, old_quantity)
    pub removed: Vec<(i32, i32)>,
}

/// Cross-validation between the top two memory copies.
#[derive(Debug, Clone, Serialize)]
pub struct CrossValidation {
    pub block_a_unique: usize,
    pub block_b_unique: usize,
    pub shared_ids: usize,
    pub quantity_matches: usize,
    pub quantity_mismatches: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
}
