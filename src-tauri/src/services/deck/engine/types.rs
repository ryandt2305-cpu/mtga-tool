use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::Rarity;

pub const W: u8 = 1;
pub const U: u8 = 2;
pub const B: u8 = 4;
pub const R: u8 = 8;
pub const G: u8 = 16;

fn letter_mask(c: char) -> u8 {
    match c {
        'W' | 'w' => W,
        'U' | 'u' => U,
        'B' | 'b' => B,
        'R' | 'r' => R,
        'G' | 'g' => G,
        _ => 0,
    }
}

pub fn mask_from_letters<S: AsRef<str>>(letters: &[S]) -> u8 {
    let mut m = 0u8;
    for s in letters {
        for c in s.as_ref().chars() {
            m |= letter_mask(c);
        }
    }
    m
}

pub fn mask_from_str(s: &str) -> u8 {
    let mut m = 0u8;
    for c in s.chars() {
        m |= letter_mask(c);
    }
    m
}

pub fn mask_to_letters(mask: u8) -> Vec<char> {
    let mut v = Vec::new();
    if mask & W != 0 { v.push('W'); }
    if mask & U != 0 { v.push('U'); }
    if mask & B != 0 { v.push('B'); }
    if mask & R != 0 { v.push('R'); }
    if mask & G != 0 { v.push('G'); }
    v
}

pub fn is_subset(inner: u8, outer: u8) -> bool {
    inner & !outer == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Land,
    Ramp,
    Draw,
    Removal,
    Wipe,
    Counter,
    Recursion,
    Tutor,
    Protection,
    Threat,
    Finisher,
    Payoff,
    Utility,
}

impl Role {
    pub const ALL: [Role; 13] = [
        Role::Land, Role::Ramp, Role::Draw, Role::Removal, Role::Wipe, Role::Counter,
        Role::Recursion, Role::Tutor, Role::Protection, Role::Threat, Role::Finisher,
        Role::Payoff, Role::Utility,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Land => "land",
            Role::Ramp => "ramp",
            Role::Draw => "draw",
            Role::Removal => "removal",
            Role::Wipe => "wipe",
            Role::Counter => "counter",
            Role::Recursion => "recursion",
            Role::Tutor => "tutor",
            Role::Protection => "protection",
            Role::Threat => "threat",
            Role::Finisher => "finisher",
            Role::Payoff => "payoff",
            Role::Utility => "utility",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.iter().copied().find(|r| r.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Land => "Lands",
            Role::Ramp => "Ramp",
            Role::Draw => "Card Draw",
            Role::Removal => "Removal",
            Role::Wipe => "Board Wipes",
            Role::Counter => "Counterspells",
            Role::Recursion => "Recursion",
            Role::Tutor => "Tutors",
            Role::Protection => "Protection",
            Role::Threat => "Threats",
            Role::Finisher => "Finishers",
            Role::Payoff => "Synergy Payoffs",
            Role::Utility => "Utility",
        }
    }
}

// Serde names are set explicitly (not via rename_all = "snake_case") because
// snake_case would render StandardBrawl60 as "standard_brawl60", which does
// not match the frontend's Format union or Format::as_str(). Keep these in
// lock-step with `src/lib/deckTypes.ts::Format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    #[serde(rename = "brawl100")]
    Brawl100,
    #[serde(rename = "standardbrawl60")]
    StandardBrawl60,
}

impl Format {
    pub fn deck_size(self) -> u32 {
        match self {
            Format::Brawl100 => 99,
            Format::StandardBrawl60 => 59,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Format::Brawl100 => "brawl100",
            Format::StandardBrawl60 => "standardbrawl60",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Playstyle {
    Aggro,
    Midrange,
    Control,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WildcardBudget {
    #[serde(default)] pub common: u32,
    #[serde(default)] pub uncommon: u32,
    #[serde(default)] pub rare: u32,
    #[serde(default)] pub mythic: u32,
}

impl WildcardBudget {
    pub fn get(&self, r: &Rarity) -> u32 {
        match r {
            Rarity::Common => self.common,
            Rarity::Uncommon => self.uncommon,
            Rarity::Rare => self.rare,
            Rarity::Mythic => self.mythic,
            Rarity::Special | Rarity::BasicLand | Rarity::Unknown(_) => u32::MAX,
        }
    }

    pub fn spend(&mut self, r: &Rarity) -> bool {
        match r {
            Rarity::Common => {
                if self.common == 0 { return false; }
                self.common -= 1; true
            }
            Rarity::Uncommon => {
                if self.uncommon == 0 { return false; }
                self.uncommon -= 1; true
            }
            Rarity::Rare => {
                if self.rare == 0 { return false; }
                self.rare -= 1; true
            }
            Rarity::Mythic => {
                if self.mythic == 0 { return false; }
                self.mythic -= 1; true
            }
            Rarity::Special | Rarity::BasicLand | Rarity::Unknown(_) => true,
        }
    }

    pub fn add(&mut self, r: &Rarity) {
        match r {
            Rarity::Common => self.common += 1,
            Rarity::Uncommon => self.uncommon += 1,
            Rarity::Rare => self.rare += 1,
            Rarity::Mythic => self.mythic += 1,
            Rarity::Special | Rarity::BasicLand | Rarity::Unknown(_) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum OwnershipMode {
    OwnedOnly { wc_budget: WildcardBudget },
    BestDeck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Seed {
    Edhrec,
    Archidekt { deck_id: i64 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TemplateOverrides {
    #[serde(default)] pub lands: Option<u32>,
    #[serde(default)] pub role_min: HashMap<Role, u32>,
    #[serde(default)] pub role_max: HashMap<Role, u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildAround {
    pub grp_id: i32,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildRequest {
    pub format: Format,
    pub commander_grp: Option<i32>,
    pub ownership: OwnershipMode,
    pub playstyle: Playstyle,
    #[serde(default)] pub color_subset: Option<String>,
    #[serde(default)] pub theme_tags: Vec<String>,
    #[serde(default)] pub must_include: Vec<i32>,
    #[serde(default)] pub build_around: Vec<BuildAround>,
    #[serde(default)] pub exclude: Vec<i32>,
    #[serde(default)] pub seed: Option<Seed>,
    #[serde(default)] pub template_overrides: Option<TemplateOverrides>,
    #[serde(default)] pub use_17lands: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Printing {
    pub grp_id: i32,
    pub set_code: String,
    pub collector_number: String,
    pub rarity: Rarity,
    pub owned_qty: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OracleCard {
    pub oracle_id: String,
    pub name: String,
    pub name_lower: String,
    pub cmc: f32,
    pub mana_cost: String,
    pub color_identity: u8,
    pub type_line: String,
    pub keywords: Vec<String>,
    pub oracle_text: String,
    pub edhrec_rank: Option<u32>,
    pub legal_brawl: bool,
    pub legal_standardbrawl: bool,
    pub is_commander_eligible: bool,
    pub power: Option<i32>,
    pub roles: Vec<Role>,
    pub tags: Vec<String>,
    pub subtypes: Vec<String>,
    pub is_land: bool,
    pub is_basic_land: bool,
    pub is_creature: bool,
    pub printings: Vec<Printing>,
}

fn rarity_rank(r: &Rarity) -> u8 {
    match r {
        Rarity::Common => 0,
        Rarity::Uncommon => 1,
        Rarity::Rare => 2,
        Rarity::Mythic => 3,
        Rarity::BasicLand | Rarity::Special | Rarity::Unknown(_) => 0,
    }
}

impl OracleCard {
    pub fn is_owned(&self) -> bool {
        self.printings.iter().any(|p| p.owned_qty > 0)
    }

    pub fn best_printing(&self) -> Option<&Printing> {
        if let Some(p) = self
            .printings
            .iter()
            .filter(|p| p.owned_qty > 0)
            .max_by_key(|p| p.owned_qty)
        {
            return Some(p);
        }
        if let Some(p) = self.printings.iter().min_by_key(|p| rarity_rank(&p.rarity)) {
            return Some(p);
        }
        self.printings.first()
    }

    pub fn has_role(&self, r: Role) -> bool {
        self.roles.contains(&r)
    }

    pub fn legal_in(&self, f: Format) -> bool {
        match f {
            Format::Brawl100 => self.legal_brawl,
            Format::StandardBrawl60 => self.legal_standardbrawl,
        }
    }

    pub fn pips(&self) -> [u32; 5] {
        let mut out = [0u32; 5];
        let bytes = self.mana_cost.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'}' {
                    end += 1;
                }
                if end <= bytes.len() {
                    let sym = &self.mana_cost[start..end];
                    for c in sym.chars() {
                        match c {
                            'W' | 'w' => out[0] += 1,
                            'U' | 'u' => out[1] += 1,
                            'B' | 'b' => out[2] += 1,
                            'R' | 'r' => out[3] += 1,
                            'G' | 'g' => out[4] += 1,
                            _ => {}
                        }
                    }
                }
                i = end + 1;
            } else {
                i += 1;
            }
        }
        out
    }
}

/// Almost-complete combo: the user owns every piece of a known combo except
/// this card. Computed once per build in `make_plan`; consumed by
/// `compute_synergy` so the missing piece carries value from the first
/// scoring pass instead of depending on fill order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComboCompletion {
    /// Additive synergy term (capped; see score::COMBO_COMPLETION_CAP).
    pub bonus: f32,
    /// Pieces already owned / guaranteed in deck.
    pub have: u32,
    /// Total pieces in the combo.
    pub size: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommunitySignal {
    pub inclusion: Option<f32>,
    pub synergy: Option<f32>,
    pub seed_freq: Option<f32>,
    pub gih_wr: Option<f32>,
    pub staple: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct CommunityData {
    pub by_name: HashMap<String, CommunitySignal>,
    pub combos: Vec<Vec<String>>,
    pub seed_names: Vec<String>,
    pub available: bool,
    pub staples_available: bool,
    /// Perf index: card_name_lower -> indices in `combos` that contain it.
    /// Populated once by `assemble_community`; scoring uses it to avoid a full
    /// pass over `combos` on every score_card call (the previous hot spot when
    /// pool*slots*combos = ~180M iterations dominated build time).
    pub combos_by_name: HashMap<String, Vec<usize>>,
}

pub struct EngineInput {
    pub oracles: Vec<OracleCard>,
    pub wildcards_owned: WildcardBudget,
    pub community: CommunityData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Alternative {
    pub grp_id: i32,
    pub name: String,
    pub owned: bool,
    pub wildcard_rarity: Option<Rarity>,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub grp_id: i32,
    pub oracle_id: String,
    pub name: String,
    pub role: Role,
    pub cmc: f32,
    pub owned_qty: i32,
    pub needed_qty: i32,
    pub wildcard_rarity: Option<Rarity>,
    pub score: f32,
    pub reasons: Vec<String>,
    pub anchored: bool,
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DeckStats {
    pub curve: [u32; 8],
    pub pips: [u32; 5],
    pub role_counts: HashMap<Role, u32>,
    pub role_targets: HashMap<Role, (u32, u32)>,
    pub owned: u32,
    pub total: u32,
    pub wildcard_cost: WildcardBudget,
    pub land_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildResult {
    pub format: Format,
    pub commander: Slot,
    pub slots: Vec<Slot>,
    pub stats: DeckStats,
    pub warnings: Vec<Warning>,
}

#[cfg(test)]
pub(crate) fn sample_card() -> OracleCard {
    OracleCard {
        oracle_id: String::new(),
        name: "Sample".into(),
        name_lower: "sample".into(),
        cmc: 0.0,
        mana_cost: String::new(),
        color_identity: G,
        type_line: String::new(),
        keywords: Vec::new(),
        oracle_text: String::new(),
        edhrec_rank: None,
        legal_brawl: true,
        legal_standardbrawl: true,
        is_commander_eligible: false,
        power: None,
        roles: Vec::new(),
        tags: Vec::new(),
        subtypes: Vec::new(),
        is_land: false,
        is_basic_land: false,
        is_creature: false,
        printings: vec![Printing {
            grp_id: 1000,
            set_code: String::new(),
            collector_number: String::new(),
            rarity: Rarity::Common,
            owned_qty: 1,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(mask_from_str("WUG"), W | U | G);
        assert_eq!(mask_from_letters(&["G", "R"]), G | R);
        assert_eq!(mask_to_letters(W | G), vec!['W', 'G']);
        assert!(is_subset(G, W | G));
        assert!(!is_subset(R, W | G));
        assert!(is_subset(0, 0));
    }

    #[test]
    fn budget_spend() {
        let mut b = WildcardBudget { rare: 1, ..Default::default() };
        assert!(b.spend(&Rarity::Rare));
        assert!(!b.spend(&Rarity::Rare));
        assert!(b.spend(&Rarity::BasicLand));
    }

    #[test]
    fn pips_count_hybrid_both() {
        let mut c = sample_card();
        c.mana_cost = "{1}{W/U}{G}{G}".into();
        assert_eq!(c.pips(), [1, 1, 0, 0, 2]);
    }

    #[test]
    fn format_deserializes_frontend_strings() {
        // Frontend sends "brawl100" and "standardbrawl60" (no underscore between
        // the words and digits). Regression: serde snake_case for
        // StandardBrawl60 previously produced "standard_brawl60", which failed
        // to deserialize the string the UI actually sends.
        let a: Format = serde_json::from_str("\"brawl100\"").expect("brawl100");
        assert_eq!(a, Format::Brawl100);
        let b: Format = serde_json::from_str("\"standardbrawl60\"").expect("standardbrawl60");
        assert_eq!(b, Format::StandardBrawl60);
    }

    #[test]
    fn best_printing_prefers_owned_then_cheapest() {
        let mut c = sample_card();
        c.printings = vec![
            Printing { grp_id: 1, set_code: "a".into(), collector_number: "1".into(), rarity: Rarity::Mythic, owned_qty: 0 },
            Printing { grp_id: 2, set_code: "b".into(), collector_number: "2".into(), rarity: Rarity::Rare, owned_qty: 0 },
        ];
        assert_eq!(c.best_printing().unwrap().grp_id, 2);
        c.printings[0].owned_qty = 1;
        assert_eq!(c.best_printing().unwrap().grp_id, 1);
    }

    #[test]
    fn role_roundtrip() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.as_str()), Some(r));
        }
    }
}
