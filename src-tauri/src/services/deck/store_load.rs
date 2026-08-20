// DeckStore::load_oracles — joins stored oracle rows + tags + roles + grp↔oracle
// with the live card DB and current collection to produce engine `OracleCard`s.
// Placed in its own file because `store.rs` is at the 750-line hard limit.

use std::collections::HashMap;

use crate::models::CardInfo;

use super::engine::roles::{theme_slugs, TagIndex};
use super::engine::types::{mask_from_str, OracleCard, Printing, Role};
use super::store::DeckStore;

impl DeckStore {
    /// Join stored oracle rows with the live card DB (`cards`) and the user's
    /// current collection (`collection`: grp_id → owned qty) into `OracleCard`s.
    ///
    /// Oracles with zero resolvable printings are still returned (seed name
    /// matching wants to warn about them); the engine's candidate pool filters
    /// them out.
    pub fn load_oracles(
        &self,
        cards: &HashMap<i32, CardInfo>,
        collection: &HashMap<i32, i32>,
    ) -> Result<Vec<OracleCard>, String> {
        let oracle_rows = self.load_oracle_rows()?;
        let role_map = self.load_roles()?;
        let taggings = self.load_taggings()?;
        let tag_rows = self.load_tag_rows()?;
        let grp_rows = self.load_grp_oracle()?;

        let tag_index = TagIndex::new(&tag_rows);

        let mut tags_by_oracle: HashMap<String, Vec<String>> = HashMap::new();
        for (oracle_id, tag_id) in taggings {
            tags_by_oracle.entry(oracle_id).or_default().push(tag_id);
        }

        let mut grps_by_oracle: HashMap<String, Vec<i32>> = HashMap::new();
        for (grp, oracle_id) in grp_rows {
            grps_by_oracle.entry(oracle_id).or_default().push(grp);
        }

        let mut out: Vec<OracleCard> = Vec::with_capacity(oracle_rows.len());
        for o in oracle_rows {
            let tag_ids = tags_by_oracle.remove(&o.oracle_id).unwrap_or_default();
            let theme = theme_slugs(&tag_index, &tag_ids);
            let roles: Vec<Role> = role_map
                .get(&o.oracle_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|s| Role::parse(&s))
                .collect();

            let grps = grps_by_oracle.remove(&o.oracle_id).unwrap_or_default();
            let mut primaries: Vec<Printing> = Vec::new();
            let mut alts: Vec<Printing> = Vec::new();
            let mut subtypes: Vec<String> = Vec::new();
            let mut primary_subtypes_set = false;
            for grp in grps {
                let card = match cards.get(&grp) {
                    Some(c) => c,
                    None => continue,
                };
                let owned_qty = collection.get(&grp).copied().unwrap_or(0);
                let printing = Printing {
                    grp_id: grp,
                    set_code: card.set_code.clone(),
                    collector_number: card.collector_number.clone(),
                    rarity: card.rarity.clone(),
                    owned_qty,
                };
                if card.is_primary_card {
                    if !primary_subtypes_set {
                        subtypes = card.subtypes.clone();
                        primary_subtypes_set = true;
                    }
                    primaries.push(printing);
                } else {
                    alts.push(printing);
                }
            }
            // Keep alt-art printings so owned alt-art still counts (plan §3.1).
            let printings = {
                let mut p = primaries;
                p.extend(alts);
                p
            };

            let front = o.type_line.split(" // ").next().unwrap_or(&o.type_line);
            let is_land = front.contains("Land");
            let is_basic_land = front.starts_with("Basic Land");
            let is_creature = front.contains("Creature");

            let keywords: Vec<String> = serde_json::from_str(&o.keywords).unwrap_or_default();

            out.push(OracleCard {
                oracle_id: o.oracle_id,
                name_lower: o.name_front_lower.clone(),
                name: o.name,
                cmc: o.cmc,
                mana_cost: o.mana_cost,
                color_identity: mask_from_str(&o.color_identity),
                type_line: o.type_line,
                keywords,
                oracle_text: o.oracle_text,
                edhrec_rank: o.edhrec_rank,
                legal_brawl: o.legal_brawl,
                legal_standardbrawl: o.legal_standardbrawl,
                is_commander_eligible: o.is_commander_eligible,
                power: o.power,
                roles,
                tags: theme,
                subtypes,
                is_land,
                is_basic_land,
                is_creature,
                printings,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CardInfo, Rarity};
    use crate::services::deck::engine::types::{B, G, U, W};
    use crate::services::deck::store::{DeckStore, IngestArena, IngestOracle, IngestTag};

    fn card(grp: i32, name: &str, set: &str, cn: &str, primary: bool) -> CardInfo {
        CardInfo {
            grp_id: grp,
            name: name.into(),
            set_code: set.into(),
            rarity: Rarity::Rare,
            collector_number: cn.into(),
            cmc: 2,
            power: None,
            toughness: None,
            deck_limit: 4,
            color_identity: vec!["G".into()],
            colors: vec!["G".into()],
            types: vec!["Creature".into()],
            subtypes: vec!["Elf".into(), "Druid".into()],
            supertypes: vec![],
            is_rebalanced: false,
            rebalanced_grp_id: None,
            is_primary_card: primary,
        }
    }

    fn oracle(id: &str, name: &str, type_line: &str, color_identity: &str) -> IngestOracle {
        IngestOracle {
            oracle_id: id.into(),
            name: name.into(),
            name_front_lower: name.to_lowercase(),
            cmc: 2.0,
            mana_cost: "{1}{G}".into(),
            color_identity: color_identity.into(),
            type_line: type_line.into(),
            keywords: r#"["Trample","Reach"]"#.into(),
            oracle_text: "".into(),
            edhrec_rank: Some(500),
            legal_brawl: true,
            legal_standardbrawl: false,
            is_commander_eligible: false,
            power: Some(2),
            layout: "normal".into(),
        }
    }

    #[test]
    fn load_oracles_joins_printings_roles_and_themes() {
        let mut db = DeckStore::open_in_memory().unwrap();
        // Two oracles: an elf creature (has printings) and a basic land (no printings in cards map)
        db.replace_bulk(
            &[
                oracle("o-elf", "Test Elf", "Creature — Elf", "G"),
                oracle("o-forest", "Forest", "Basic Land — Forest", "G"),
                oracle("o-ghost", "Ghost", "Instant", "WUB"),
            ],
            &[IngestArena {
                arena_id: 100,
                oracle_id: "o-elf".into(),
                set_code: "blb".into(),
                collector_number: "1".into(),
            }],
            &[
                IngestTag { tag_id: "t-ramp".into(), slug: "ramp".into(), label: "Ramp".into(), parent_id: None },
                IngestTag { tag_id: "t-elves".into(), slug: "typal-elf".into(), label: "Elves".into(), parent_id: None },
            ],
            &[
                ("o-elf".into(), "t-ramp".into()),
                ("o-elf".into(), "t-elves".into()),
            ],
        )
        .unwrap();
        db.replace_grp_oracle(&[
            (100, "o-elf".into(), "arena_id"),
            (101, "o-elf".into(), "name"),
            (200, "o-forest".into(), "arena_id"),
        ])
        .unwrap();
        db.replace_roles(&[
            ("o-elf".to_string(), "ramp".to_string()),
            ("o-elf".to_string(), "threat".to_string()),
        ])
        .unwrap();

        let mut cards = HashMap::new();
        cards.insert(100, card(100, "Test Elf", "blb", "1", true));
        cards.insert(101, card(101, "Test Elf", "blb", "1★", false)); // alt art
        cards.insert(200, {
            let mut c = card(200, "Forest", "blb", "270", true);
            c.subtypes = vec!["Forest".into()];
            c.types = vec!["Land".into()];
            c
        });
        // Note: o-ghost has no grp mapping → zero printings, must still appear
        let mut collection = HashMap::new();
        collection.insert(100, 3);
        collection.insert(200, 8);
        // grp 101 (alt art) not owned

        let oracles = db.load_oracles(&cards, &collection).unwrap();
        assert_eq!(oracles.len(), 3);

        let elf = oracles.iter().find(|o| o.oracle_id == "o-elf").unwrap();
        assert_eq!(elf.color_identity, G);
        assert!(elf.is_creature);
        assert!(!elf.is_land);
        assert!(!elf.is_basic_land);
        assert_eq!(elf.printings.len(), 2, "primary + alt-art both kept");
        // Primary comes first
        assert_eq!(elf.printings[0].grp_id, 100);
        assert_eq!(elf.printings[0].owned_qty, 3);
        assert_eq!(elf.printings[1].grp_id, 101);
        assert_eq!(elf.printings[1].owned_qty, 0);
        assert_eq!(elf.subtypes, vec!["Elf".to_string(), "Druid".to_string()]);
        assert_eq!(elf.keywords, vec!["Trample".to_string(), "Reach".to_string()]);
        assert!(elf.roles.contains(&Role::Ramp));
        assert!(elf.roles.contains(&Role::Threat));
        assert!(elf.tags.iter().any(|t| t == "typal-elf"), "theme_slugs keeps typal-elf");
        assert!(elf.tags.iter().any(|t| t == "ramp"), "ramp is not a META_ROOT — stays");

        let forest = oracles.iter().find(|o| o.oracle_id == "o-forest").unwrap();
        assert!(forest.is_land);
        assert!(forest.is_basic_land);
        assert_eq!(forest.printings.len(), 1);
        assert_eq!(forest.printings[0].owned_qty, 8);

        let ghost = oracles.iter().find(|o| o.oracle_id == "o-ghost").unwrap();
        assert_eq!(ghost.printings.len(), 0, "oracle with no printings still returned");
        assert_eq!(ghost.color_identity, W | U | B);
    }
}
