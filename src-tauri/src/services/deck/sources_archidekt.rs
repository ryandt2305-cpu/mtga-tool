// Archidekt source: URL builders, list/deck payload parsers, and the
// per-format staples aggregate. Split out of `sources.rs` (2026-08-19) so that
// file stays under the 500-line soft limit; everything here is re-exported via
// `pub use super::sources_archidekt::*;` in `sources.rs`, so call sites keep
// importing from `services::deck::sources`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::engine::types::Format;

/// Archidekt deck search. `size=` is a real server-side filter (verified live
/// 2026-08-19: `deckFormat=20&size=60` → only 60-card decks), so the list only
/// carries decks of the format's exact size. `pageSize` is ignored by the
/// server (always 60 per page) but kept for compatibility.
pub fn archidekt_list_url(format: Format, page: u32) -> String {
    let df = match format {
        Format::Brawl100 => 20,
        Format::StandardBrawl60 => 13,
    };
    let size = format.deck_size() + 1;
    format!(
        "https://archidekt.com/api/decks/v3/?deckFormat={}&orderBy=-viewCount&size={}&pageSize=20&page={}",
        df, size, page
    )
}

pub fn archidekt_deck_url(id: i64) -> String {
    format!("https://archidekt.com/api/decks/{}/", id)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeedDeckSummary {
    pub id: i64,
    pub name: String,
    pub size: u32,
    pub view_count: u64,
    pub colors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ArchidektListRoot {
    #[serde(default)]
    next: Option<String>,
    results: Vec<ArchidektListResult>,
}
#[derive(Debug, Deserialize)]
struct ArchidektListResult {
    id: i64,
    name: String,
    size: u32,
    #[serde(rename = "viewCount")]
    view_count: u64,
    #[serde(default, deserialize_with = "de_colors")]
    colors: Vec<String>,
}

/// Archidekt's v3 list returns `colors` as a pip-count map
/// (`{"W":0,"U":0,"B":0,"R":58,"G":0}`, verified live 2026-08-19); older
/// payloads / fixtures used a list of letters. Accept both; anything else
/// (null, unexpected shape) → empty, never a parse failure.
#[derive(Deserialize)]
#[serde(untagged)]
enum ColorsField {
    Map(HashMap<String, u32>),
    List(Vec<String>),
    Other(serde::de::IgnoredAny),
}

fn de_colors<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match ColorsField::deserialize(d)? {
        ColorsField::Map(m) => ['W', 'U', 'B', 'R', 'G']
            .iter()
            .filter(|c| m.get(&c.to_string()).copied().unwrap_or(0) > 0)
            .map(|c| c.to_string())
            .collect(),
        ColorsField::List(v) => v,
        ColorsField::Other(_) => Vec::new(),
    })
}

pub fn parse_archidekt_list(body: &str) -> Result<(Vec<SeedDeckSummary>, bool), String> {
    let root: ArchidektListRoot = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let has_next = root.next.is_some();
    let items = root
        .results
        .into_iter()
        .map(|r| SeedDeckSummary {
            id: r.id,
            name: r.name,
            size: r.size,
            view_count: r.view_count,
            colors: r.colors,
        })
        .collect();
    Ok((items, has_next))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeedDeckDetail {
    pub id: i64,
    pub name: String,
    pub commander_names: Vec<String>,
    pub cards: Vec<(String, u32)>,
}

pub const STAPLES_DECK_COUNT: usize = 20;
pub const STAPLES_TTL_SECS: i64 = 7 * 86_400;
pub const STAPLES_MIN_DECKS: usize = 5;

pub fn staples_cache_key(format: Format) -> String {
    format!("archidekt:staples:{}", format.as_str())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StapleDeck {
    pub colors: String,
    pub cards: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaplesAggregate {
    pub built_at: i64,
    pub decks: Vec<StapleDeck>,
}

fn front_face_lower(name: &str) -> String {
    name.split(" // ").next().unwrap_or(name).trim().to_lowercase()
}

pub fn build_staples(
    built_at: i64,
    decks: &[(&SeedDeckSummary, &SeedDeckDetail)],
) -> StaplesAggregate {
    use std::collections::BTreeSet;
    let mut out = Vec::with_capacity(decks.len());
    for (summary, detail) in decks {
        let mut letters: Vec<char> = summary
            .colors
            .iter()
            .flat_map(|s| s.chars())
            .map(|c| c.to_ascii_uppercase())
            .filter(|c| matches!(c, 'W' | 'U' | 'B' | 'R' | 'G'))
            .collect();
        letters.sort_unstable();
        letters.dedup();
        let commanders: BTreeSet<String> = detail
            .commander_names
            .iter()
            .map(|n| front_face_lower(n))
            .collect();
        let cards: BTreeSet<String> = detail
            .cards
            .iter()
            .map(|(n, _)| front_face_lower(n))
            .filter(|n| !n.is_empty() && !commanders.contains(n))
            .collect();
        out.push(StapleDeck {
            colors: letters.into_iter().collect(),
            cards: cards.into_iter().collect(),
        });
    }
    StaplesAggregate { built_at, decks: out }
}

pub fn staple_shares(agg: &StaplesAggregate, identity: u8) -> HashMap<String, f32> {
    use super::engine::types::{is_subset, mask_from_str};
    if agg.decks.is_empty() {
        return HashMap::new();
    }
    let subset: Vec<&StapleDeck> = agg
        .decks
        .iter()
        .filter(|d| is_subset(mask_from_str(&d.colors), identity))
        .collect();
    let pool: Vec<&StapleDeck> = if subset.len() >= STAPLES_MIN_DECKS {
        subset
    } else {
        agg.decks.iter().collect()
    };
    let n = pool.len() as f32;
    let mut counts: HashMap<String, u32> = HashMap::new();
    for d in &pool {
        for c in &d.cards {
            *counts.entry(c.clone()).or_insert(0) += 1;
        }
    }
    counts.into_iter().map(|(k, v)| (k, v as f32 / n)).collect()
}

#[derive(Debug, Deserialize)]
struct ArchidektDeckRoot {
    id: i64,
    name: String,
    /// Deck-level category definitions. `includedInDeck=false` marks
    /// Maybeboard-style categories whose cards are not part of the 99.
    #[serde(default)]
    categories: Vec<ArchidektDeckCategory>,
    cards: Vec<ArchidektDeckCard>,
}
#[derive(Debug, Deserialize)]
struct ArchidektDeckCategory {
    name: String,
    #[serde(rename = "includedInDeck", default = "default_true")]
    included_in_deck: bool,
}
fn default_true() -> bool {
    true
}
#[derive(Debug, Deserialize)]
struct ArchidektDeckCard {
    quantity: u32,
    #[serde(default)]
    categories: Vec<String>,
    card: ArchidektDeckCardInner,
}
#[derive(Debug, Deserialize)]
struct ArchidektDeckCardInner {
    #[serde(rename = "oracleCard")]
    oracle_card: ArchidektDeckOracle,
}
#[derive(Debug, Deserialize)]
struct ArchidektDeckOracle {
    name: String,
}

pub fn parse_archidekt_deck(body: &str) -> Result<SeedDeckDetail, String> {
    let root: ArchidektDeckRoot = serde_json::from_str(body).map_err(|e| e.to_string())?;
    // Cards outside the 99: any deck-level category flagged includedInDeck=false,
    // plus Maybeboard/Sideboard by name (Archidekt defaults Sideboard to
    // includedInDeck=true, but it is never part of a Brawl 99). Also covers
    // payloads that omit the deck-level `categories` list entirely.
    let mut excluded: std::collections::HashSet<String> =
        ["maybeboard".to_string(), "sideboard".to_string()].into_iter().collect();
    for cat in &root.categories {
        if !cat.included_in_deck {
            excluded.insert(cat.name.to_lowercase());
        }
    }
    let mut commander_names = Vec::new();
    let mut cards = Vec::new();
    for c in root.cards {
        let is_commander = c
            .categories
            .iter()
            .any(|cat| cat.eq_ignore_ascii_case("Commander"));
        let name = c.card.oracle_card.name;
        if is_commander {
            commander_names.push(name);
            continue;
        }
        if c
            .categories
            .iter()
            .any(|cat| excluded.contains(&cat.to_lowercase()))
        {
            continue;
        }
        cards.push((name, c.quantity));
    }
    Ok(SeedDeckDetail {
        id: root.id,
        name: root.name,
        commander_names,
        cards,
    })
}
