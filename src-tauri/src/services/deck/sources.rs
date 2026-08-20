use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::engine::types::{CommunityData, CommunitySignal, Format};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Edhrec,
    ArchidektList,
    ArchidektDeck,
    Spellbook,
    SeventeenLands,
}

impl SourceKind {
    pub fn host(self) -> &'static str {
        match self {
            SourceKind::Edhrec => "edhrec",
            SourceKind::ArchidektList | SourceKind::ArchidektDeck => "archidekt",
            SourceKind::Spellbook => "spellbook",
            SourceKind::SeventeenLands => "17lands",
        }
    }

    pub fn ttl_secs(self) -> i64 {
        match self {
            SourceKind::Edhrec => 7 * 86_400,
            SourceKind::ArchidektList | SourceKind::ArchidektDeck => 86_400,
            SourceKind::Spellbook => 7 * 86_400,
            SourceKind::SeventeenLands => 30 * 86_400,
        }
    }
}

pub const SOURCE_DAILY_CAP: u32 = 200;
pub const HOST_SPACING_MS: u64 = 250;
pub const DEGRADE_AFTER_FAILURES: u32 = 3;

use super::store::CacheHit;

pub use super::sources_archidekt::*;

/// Decision produced by [`decide_fetch`] — pure summary of "should we hit the network?".
#[derive(Debug, Clone, PartialEq)]
pub enum FetchDecision {
    /// Fresh cache entry present; return its body without any network call.
    UseCache(String),
    /// No fresh cache; caller should attempt a network fetch. `stale` is the last
    /// cached body (if any) to fall back to on network failure.
    Fetch { stale: Option<String> },
    /// Blocked by degrade-after-failures or daily-cap. Return `stale` if any,
    /// else surface `code` (DECK-007 host degraded / DECK-008 daily cap reached).
    Blocked { stale: Option<String>, code: &'static str },
}

/// Pure policy: decides whether to serve cache, fetch, or block for a source hit.
/// Kept free of I/O so it can be unit-tested exhaustively.
pub fn decide_fetch(fresh: Option<CacheHit>, failures: u32, hits: u32) -> FetchDecision {
    if let Some(hit) = fresh.as_ref() {
        if hit.fresh {
            return FetchDecision::UseCache(hit.body.clone());
        }
    }
    let stale = fresh.and_then(|h| if h.fresh { None } else { Some(h.body) });
    if failures >= DEGRADE_AFTER_FAILURES {
        return FetchDecision::Blocked { stale, code: "DECK-007" };
    }
    if hits >= SOURCE_DAILY_CAP {
        return FetchDecision::Blocked { stale, code: "DECK-008" };
    }
    FetchDecision::Fetch { stale }
}

pub fn edhrec_slug(name: &str) -> String {
    let front = name.split(" // ").next().unwrap_or(name);
    let mut out = String::with_capacity(front.len());
    let mut prev_dash = true;
    for ch in front.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
        } else if lower == ' ' || lower == '-' || lower == '_' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

pub fn edhrec_url(name: &str) -> String {
    format!(
        "https://json.edhrec.com/pages/commanders/{}.json",
        edhrec_slug(name)
    )
}

pub fn spellbook_url(format: Format, page: u32) -> String {
    let q = match format {
        Format::Brawl100 => "legal:brawl",
        Format::StandardBrawl60 => "legal:standardBrawl",
    };
    format!(
        "https://backend.commanderspellbook.com/variants/?q={}&limit=100&page={}",
        q, page
    )
}

pub fn seventeenlands_url(set_code: &str) -> String {
    format!(
        "https://www.17lands.com/card_ratings/data?expansion={}&format=PremierDraft",
        set_code
    )
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EdhrecCard {
    pub name: String,
    pub inclusion: f32,
    pub synergy: f32,
    pub section: String,
}

#[derive(Debug, Deserialize)]
struct EdhrecRoot {
    container: EdhrecContainer,
}
#[derive(Debug, Deserialize)]
struct EdhrecContainer {
    json_dict: EdhrecJsonDict,
}
#[derive(Debug, Deserialize)]
struct EdhrecJsonDict {
    cardlists: Vec<EdhrecList>,
}
#[derive(Debug, Deserialize)]
struct EdhrecList {
    header: String,
    cardviews: Vec<EdhrecCardview>,
}
#[derive(Debug, Deserialize)]
struct EdhrecCardview {
    name: String,
    #[serde(default)]
    num_decks: Option<u32>,
    #[serde(default)]
    potential_decks: Option<u32>,
    #[serde(default)]
    synergy: Option<f32>,
}

pub fn parse_edhrec(body: &str) -> Result<Vec<EdhrecCard>, String> {
    let root: EdhrecRoot = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for list in root.container.json_dict.cardlists {
        for cv in list.cardviews {
            let inclusion = match (cv.num_decks, cv.potential_decks) {
                (Some(n), Some(p)) if p > 0 => (n as f32) / (p as f32),
                _ => 0.0,
            };
            out.push(EdhrecCard {
                name: cv.name,
                inclusion,
                synergy: cv.synergy.unwrap_or(0.0),
                section: list.header.clone(),
            });
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct SpellbookRoot {
    #[serde(default)]
    next: Option<String>,
    results: Vec<SpellbookVariant>,
}
#[derive(Debug, Deserialize)]
struct SpellbookVariant {
    uses: Vec<SpellbookUse>,
}
#[derive(Debug, Deserialize)]
struct SpellbookUse {
    card: SpellbookCard,
}
#[derive(Debug, Deserialize)]
struct SpellbookCard {
    name: String,
}

pub fn parse_spellbook(body: &str) -> Result<(Vec<Vec<String>>, bool), String> {
    let root: SpellbookRoot = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let has_next = root.next.is_some();
    let combos = root
        .results
        .into_iter()
        .map(|v| {
            v.uses
                .into_iter()
                .map(|u| u.card.name.to_lowercase())
                .collect()
        })
        .collect();
    Ok((combos, has_next))
}

#[derive(Debug, Deserialize)]
struct SeventeenLandsRow {
    mtga_id: i32,
    ever_drawn_win_rate: f32,
    game_count: u32,
}

pub fn parse_seventeenlands(body: &str) -> Result<Vec<(i32, f32)>, String> {
    let rows: Vec<SeventeenLandsRow> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter(|r| r.game_count >= 500)
        .map(|r| (r.mtga_id, r.ever_drawn_win_rate))
        .collect())
}

pub fn assemble_community(
    edhrec: Option<&[EdhrecCard]>,
    seed: Option<&SeedDeckDetail>,
    combos: &[Vec<String>],
    gih: &HashMap<String, f32>,
    staples: Option<&StaplesAggregate>,
    identity: u8,
) -> CommunityData {
    let mut by_name: HashMap<String, CommunitySignal> = HashMap::new();

    if let Some(cards) = edhrec {
        for c in cards {
            let entry = by_name.entry(c.name.to_lowercase()).or_default();
            entry.inclusion = Some(c.inclusion);
            entry.synergy = Some(c.synergy);
        }
    }

    let mut seed_names = Vec::new();
    if let Some(det) = seed {
        let commander_lower: Vec<String> = det
            .commander_names
            .iter()
            .map(|n| n.to_lowercase())
            .collect();
        for (name, _qty) in &det.cards {
            let lower = name.to_lowercase();
            if commander_lower.contains(&lower) {
                continue;
            }
            let entry = by_name.entry(lower.clone()).or_default();
            entry.seed_freq = Some(1.0);
            seed_names.push(name.clone());
        }
    }

    for (name_lower, wr) in gih {
        let entry = by_name.entry(name_lower.clone()).or_default();
        entry.gih_wr = Some(*wr);
    }

    if let Some(agg) = staples {
        for (name_lower, share) in staple_shares(agg, identity) {
            by_name.entry(name_lower).or_default().staple = Some(share);
        }
    }

    let combos_vec = combos.to_vec();
    // Precompute name -> combo indices so score_card doesn't scan the full
    // combo list for every card (was the dominant scoring hot spot).
    let mut combos_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, combo) in combos_vec.iter().enumerate() {
        for name in combo {
            combos_by_name.entry(name.clone()).or_default().push(i);
        }
    }

    CommunityData {
        by_name,
        combos: combos_vec,
        seed_names,
        available: edhrec.is_some() || seed.is_some() || staples.is_some(),
        staples_available: staples.is_some(),
        combos_by_name,
    }
}

