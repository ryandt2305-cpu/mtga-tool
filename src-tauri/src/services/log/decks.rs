// Deck card list parsing from StartHook's Decks object.
//
// Extracts per-deck card lists (mainboard + sideboard) for caching.
// Used to enrich DeckUpdate events with full card data.

use std::collections::HashMap;

use serde_json::Value;

use crate::models::inventory::{DeckCard, DeckContents};
use crate::services::log::inventory::is_precon;

/// Parse the Decks object from StartHook into a map of deck_id → DeckContents.
///
/// Skips precon decks (same logic as anchor ID extraction in inventory.rs).
/// `summaries` is the DeckSummaries array — used for precon filtering and
/// extracting name/format metadata.
pub fn parse_deck_map(data: &Value, summaries: &[Value]) -> HashMap<String, DeckContents> {
    let summaries_by_id: HashMap<&str, &Value> = summaries
        .iter()
        .filter_map(|s| {
            let id = s.get("DeckId")?.as_str()?;
            Some((id, s))
        })
        .collect();

    let decks = match data.get("Decks").and_then(|v| v.as_object()) {
        Some(d) => d,
        None => return HashMap::new(),
    };

    let mut result = HashMap::with_capacity(decks.len());

    for (deck_id, deck) in decks {
        let summary = summaries_by_id.get(deck_id.as_str()).copied();

        // Skip precon decks
        if summary.map_or(false, is_precon) {
            continue;
        }

        let name = summary
            .and_then(|s| s.get("Name"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Deck")
            .to_string();

        let format = summary
            .and_then(|s| s.get("Attributes"))
            .and_then(|v| v.get("Format"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let mainboard = extract_zone_cards(deck, &["MainDeck", "CommandZone", "Companions"]);
        let sideboard = extract_zone_cards(deck, &["Sideboard"]);

        result.insert(
            deck_id.clone(),
            DeckContents {
                name,
                format,
                mainboard,
                sideboard,
            },
        );
    }

    result
}

/// Extract card entries from one or more deck zones.
fn extract_zone_cards(deck: &Value, zones: &[&str]) -> Vec<DeckCard> {
    let mut cards = Vec::new();
    for zone in zones {
        if let Some(entries) = deck.get(*zone).and_then(|v| v.as_array()) {
            for entry in entries {
                let grp_id = entry.get("cardId").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let quantity = entry.get("quantity").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
                if grp_id != 0 {
                    cards.push(DeckCard { grp_id, quantity });
                }
            }
        }
    }
    cards
}
