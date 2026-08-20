// Pure parsing for Scryfall bulk data + grp↔oracle join. No I/O; caller
// provides bytes and the joined-in card DB view. See Task 4 of the deck plan.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};

use serde::Deserialize;

use crate::models::CardInfo;

pub use super::store::{IngestArena, IngestOracle, IngestTag};

#[derive(Debug, Clone)]
pub struct ParsedOracles {
    pub oracles: Vec<IngestOracle>,
    pub arena: Vec<IngestArena>,
    pub skipped: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedTags {
    pub tags: Vec<IngestTag>,
    pub taggings: Vec<(String, String)>, // (oracle_id, tag_id)
    pub anomalies: u64,
}

#[derive(Debug, Clone)]
pub struct JoinResult {
    pub rows: Vec<(i32, String, &'static str)>,
    pub unresolved: Vec<i32>,
}

// --- Scryfall row shapes (only fields we consume) ---

#[derive(Deserialize)]
struct ScryFace {
    #[allow(dead_code)]
    name: Option<String>,
    oracle_text: Option<String>,
    power: Option<String>,
}

#[derive(Deserialize)]
struct ScryCard {
    oracle_id: Option<String>,
    name: String,
    arena_id: Option<i32>,
    set: Option<String>,
    collector_number: Option<String>,
    #[serde(default)]
    cmc: f32,
    mana_cost: Option<String>,
    type_line: Option<String>,
    oracle_text: Option<String>,
    #[serde(default)]
    color_identity: Vec<String>,
    #[serde(default)]
    keywords: Vec<String>,
    power: Option<String>,
    layout: Option<String>,
    edhrec_rank: Option<u32>,
    #[serde(default)]
    legalities: HashMap<String, String>,
    card_faces: Option<Vec<ScryFace>>,
}

const WUBRG: [char; 5] = ['W', 'U', 'B', 'R', 'G'];

pub fn identity_string(letters: &[String]) -> String {
    WUBRG
        .iter()
        .filter(|c| letters.iter().any(|l| l.chars().next() == Some(**c)))
        .collect()
}

pub fn normalize_name(name: &str) -> String {
    name.split(" // ")
        .next()
        .unwrap_or(name)
        .trim()
        .to_lowercase()
}

pub fn is_commander_eligible(type_line: &str, oracle_text: &str) -> bool {
    let front = type_line.split(" // ").next().unwrap_or(type_line);
    let legendary = front.contains("Legendary");
    (legendary && (front.contains("Creature") || front.contains("Planeswalker")))
        || oracle_text.contains("can be your commander")
}

fn parse_power(p: Option<&String>) -> Option<i32> {
    p.and_then(|s| s.trim().parse::<i32>().ok())
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

// Scryfall migrated the `oracle_cards` bulk to gzipped JSONL (only
// `jsonl_download_uri` is exposed); older fixtures/tests still supply a raw
// JSON array. Gzip magic (0x1f 0x8b) picks the branch.
pub fn parse_oracle_cards(bytes: &[u8]) -> Result<ParsedOracles, String> {
    let mut out = ParsedOracles {
        oracles: Vec::new(),
        arena: Vec::new(),
        skipped: 0,
    };
    let is_gzip = bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b;
    if is_gzip {
        let dec = flate2::read::GzDecoder::new(bytes);
        let reader = BufReader::new(dec);
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| {
                format!("DECK-004: oracle_cards read failed at line {}: {}", i + 1, e)
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let c: ScryCard = serde_json::from_str(&line).map_err(|e| {
                format!(
                    "DECK-004: oracle_cards parse failed at line {}: {}",
                    i + 1,
                    e
                )
            })?;
            process_scry_card(c, &mut out);
        }
    } else {
        let cards: Vec<ScryCard> = serde_json::from_slice(bytes)
            .map_err(|e| format!("DECK-004: oracle_cards parse failed: {}", e))?;
        out.oracles.reserve(cards.len());
        for c in cards {
            process_scry_card(c, &mut out);
        }
    }
    Ok(out)
}

fn process_scry_card(c: ScryCard, out: &mut ParsedOracles) {
    let layout = c.layout.clone().unwrap_or_default();
    let oracle_id = match c.oracle_id {
        Some(id) => id,
        None => {
            out.skipped += 1;
            return;
        }
    };
    if layout == "token"
        || layout == "double_faced_token"
        || layout == "emblem"
        || layout == "art_series"
    {
        out.skipped += 1;
        return;
    }
    let type_line = c.type_line.clone().unwrap_or_default();
    let root_oracle = nonempty(c.oracle_text.clone());
    let oracle_text = root_oracle.unwrap_or_else(|| {
        c.card_faces
            .as_ref()
            .map(|faces| {
                faces
                    .iter()
                    .filter_map(|f| f.oracle_text.clone())
                    .collect::<Vec<_>>()
                    .join("\n//\n")
            })
            .unwrap_or_default()
    });
    let power = parse_power(c.power.as_ref()).or_else(|| {
        c.card_faces
            .as_ref()
            .and_then(|f| f.iter().find_map(|x| parse_power(x.power.as_ref())))
    });
    if let Some(aid) = c.arena_id {
        out.arena.push(IngestArena {
            arena_id: aid,
            oracle_id: oracle_id.clone(),
            set_code: c.set.clone().unwrap_or_default(),
            collector_number: c.collector_number.clone().unwrap_or_default(),
        });
    }
    out.oracles.push(IngestOracle {
        name_front_lower: normalize_name(&c.name),
        name: c.name,
        oracle_id,
        cmc: c.cmc,
        mana_cost: c.mana_cost.unwrap_or_default(),
        color_identity: identity_string(&c.color_identity),
        is_commander_eligible: is_commander_eligible(&type_line, &oracle_text),
        type_line,
        keywords: serde_json::to_string(&c.keywords).unwrap_or_else(|_| "[]".into()),
        oracle_text,
        edhrec_rank: c.edhrec_rank,
        legal_brawl: c
            .legalities
            .get("brawl")
            .map(|s| s == "legal")
            .unwrap_or(false),
        legal_standardbrawl: c
            .legalities
            .get("standardbrawl")
            .map(|s| s == "legal")
            .unwrap_or(false),
        power,
        layout,
    });
}

// --- Tags ---

#[derive(Deserialize)]
struct TagLine {
    id: String,
    slug: String,
    label: Option<String>,
    #[serde(default)]
    parent_ids: Vec<String>,
    #[serde(default)]
    taggings: Vec<Tagging>,
}

#[derive(Deserialize)]
struct Tagging {
    oracle_id: String,
}

pub fn parse_oracle_tags_jsonl(gz_bytes: &[u8]) -> Result<ParsedTags, String> {
    let dec = flate2::read::GzDecoder::new(gz_bytes);
    let reader = BufReader::new(dec);
    let mut raw: Vec<TagLine> = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line
            .map_err(|e| format!("DECK-004: oracle_tags read failed at line {}: {}", i + 1, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let t: TagLine = serde_json::from_str(&line).map_err(|e| {
            format!("DECK-004: oracle_tags parse failed at line {}: {}", i + 1, e)
        })?;
        raw.push(t);
    }
    let ids: std::collections::HashSet<String> = raw.iter().map(|t| t.id.clone()).collect();
    let mut out = ParsedTags {
        tags: Vec::new(),
        taggings: Vec::new(),
        anomalies: 0,
    };
    for t in raw {
        let parent = t.parent_ids.first().cloned();
        if let Some(p) = &parent {
            if !ids.contains(p) {
                out.anomalies += 1;
            }
        }
        for g in &t.taggings {
            out.taggings.push((g.oracle_id.clone(), t.id.clone()));
        }
        out.tags.push(IngestTag {
            tag_id: t.id,
            slug: t.slug.clone(),
            label: t.label.unwrap_or(t.slug),
            parent_id: parent,
        });
    }
    Ok(out)
}

pub fn join_grp_to_oracle(
    cards: &HashMap<i32, CardInfo>,
    arena_map: &HashMap<i32, String>,
    name_map: &HashMap<String, String>,
) -> JoinResult {
    let mut rows = Vec::with_capacity(cards.len());
    let mut unresolved = Vec::new();
    for (grp, card) in cards {
        if let Some(o) = arena_map.get(grp) {
            rows.push((*grp, o.clone(), "arena_id"));
            continue;
        }
        if let Some(o) = name_map.get(&normalize_name(&card.name)) {
            rows.push((*grp, o.clone(), "name"));
            continue;
        }
        unresolved.push(*grp);
    }
    unresolved.sort();
    JoinResult { rows, unresolved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const CARDS: &[u8] =
        include_bytes!("../../../tests/fixtures/deck/oracle_cards_sample.json");
    const TAGS: &str = include_str!("../../../tests/fixtures/deck/oracle_tags_sample.jsonl");

    fn gz(s: &str) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(s.as_bytes()).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn parses_oracle_cards_and_skips_tokens() {
        let p = parse_oracle_cards(CARDS).unwrap();
        assert_eq!(p.oracles.len(), 5);
        assert_eq!(p.skipped, 1);
        let baylen = p
            .oracles
            .iter()
            .find(|o| o.oracle_id == "o-baylen")
            .unwrap();
        assert_eq!(baylen.color_identity, "WRG");
        assert!(
            baylen.legal_brawl && baylen.legal_standardbrawl && baylen.is_commander_eligible
        );
        assert_eq!(baylen.power, Some(1));
        assert_eq!(
            p.arena,
            vec![IngestArena {
                arena_id: 91741,
                oracle_id: "o-baylen".into(),
                set_code: "blb".into(),
                collector_number: "207".into()
            }]
        );
        let fable = p.oracles.iter().find(|o| o.oracle_id == "o-fable").unwrap();
        assert_eq!(fable.name_front_lower, "fable of the mirror-breaker");
        assert_eq!(fable.power, Some(2));
        let teferi = p
            .oracles
            .iter()
            .find(|o| o.oracle_id == "o-teferi")
            .unwrap();
        assert!(teferi.is_commander_eligible);
        let forest = p
            .oracles
            .iter()
            .find(|o| o.oracle_id == "o-forest")
            .unwrap();
        assert!(!forest.is_commander_eligible);
    }

    #[test]
    fn parses_tags_with_hierarchy_and_counts_anomalies() {
        let p = parse_oracle_tags_jsonl(&gz(TAGS)).unwrap();
        assert_eq!(p.tags.len(), 3);
        assert_eq!(p.taggings.len(), 2);
        let lr = p.tags.iter().find(|t| t.slug == "land-ramp").unwrap();
        assert_eq!(lr.parent_id.as_deref(), Some("t-ramp"));
        assert_eq!(p.anomalies, 1);
    }

    #[test]
    fn normalize_name_takes_front_face() {
        assert_eq!(
            normalize_name("Fable of the Mirror-Breaker // Reflection of Kiki-Jiki"),
            "fable of the mirror-breaker"
        );
        assert_eq!(normalize_name("  Cultivate "), "cultivate");
    }

    #[test]
    fn commander_eligibility_rules() {
        assert!(is_commander_eligible("Legendary Creature — Elf", ""));
        assert!(is_commander_eligible("Legendary Planeswalker — Teferi", ""));
        assert!(!is_commander_eligible("Creature — Elf", ""));
        assert!(!is_commander_eligible("Legendary Artifact", ""));
        assert!(is_commander_eligible(
            "Legendary Artifact — Vehicle",
            "This card can be your commander."
        ));
    }

    #[test]
    fn join_prefers_arena_id_then_name() {
        use crate::models::{CardInfo, Rarity};
        let mk = |grp: i32, name: &str| CardInfo {
            grp_id: grp,
            name: name.into(),
            set_code: "blb".into(),
            rarity: Rarity::Rare,
            collector_number: "1".into(),
            cmc: 0,
            power: None,
            toughness: None,
            deck_limit: 0,
            color_identity: vec![],
            colors: vec![],
            types: vec![],
            subtypes: vec![],
            supertypes: vec![],
            is_rebalanced: false,
            rebalanced_grp_id: None,
            is_primary_card: true,
        };
        let mut cards = HashMap::new();
        cards.insert(91741, mk(91741, "Baylen, the Haymaker"));
        cards.insert(5, mk(5, "Cultivate"));
        cards.insert(6, mk(6, "Fable of the Mirror-Breaker"));
        cards.insert(7, mk(7, "Not A Card"));
        let mut arena = HashMap::new();
        arena.insert(91741, "o-baylen".to_string());
        let mut names = HashMap::new();
        names.insert("cultivate".to_string(), "o-cultivate".to_string());
        names.insert(
            "fable of the mirror-breaker".to_string(),
            "o-fable".to_string(),
        );
        let j = join_grp_to_oracle(&cards, &arena, &names);
        assert!(j.rows.contains(&(91741, "o-baylen".to_string(), "arena_id")));
        assert!(j.rows.contains(&(5, "o-cultivate".to_string(), "name")));
        assert!(j.rows.contains(&(6, "o-fable".to_string(), "name")));
        assert_eq!(j.unresolved, vec![7]);
    }
}
