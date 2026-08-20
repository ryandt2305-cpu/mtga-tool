#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::services::deck::engine::types::Format;
    use crate::services::deck::sources::*;
    use crate::services::deck::store::CacheHit;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/deck")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?}: {}", path, e))
    }

    fn hit(body: &str, fresh: bool) -> CacheHit {
        CacheHit { body: body.into(), fresh }
    }

    #[test]
    fn decide_fetch_returns_cache_when_fresh() {
        let d = decide_fetch(Some(hit("BODY", true)), 0, 0);
        assert_eq!(d, FetchDecision::UseCache("BODY".into()));
    }

    #[test]
    fn decide_fetch_returns_fetch_when_no_cache() {
        let d = decide_fetch(None, 0, 0);
        assert_eq!(d, FetchDecision::Fetch { stale: None });
    }

    #[test]
    fn decide_fetch_returns_fetch_with_stale_when_expired() {
        let d = decide_fetch(Some(hit("OLD", false)), 0, 0);
        assert_eq!(d, FetchDecision::Fetch { stale: Some("OLD".into()) });
    }

    #[test]
    fn decide_fetch_blocks_when_degraded() {
        let d = decide_fetch(Some(hit("OLD", false)), DEGRADE_AFTER_FAILURES, 0);
        assert_eq!(d, FetchDecision::Blocked { stale: Some("OLD".into()), code: "DECK-007" });
        let d = decide_fetch(None, DEGRADE_AFTER_FAILURES + 5, 0);
        assert_eq!(d, FetchDecision::Blocked { stale: None, code: "DECK-007" });
    }

    #[test]
    fn decide_fetch_blocks_when_capped() {
        let d = decide_fetch(Some(hit("OLD", false)), 0, SOURCE_DAILY_CAP);
        assert_eq!(d, FetchDecision::Blocked { stale: Some("OLD".into()), code: "DECK-008" });
    }

    #[test]
    fn decide_fetch_degrade_beats_cap_when_both_trip() {
        let d = decide_fetch(None, DEGRADE_AFTER_FAILURES, SOURCE_DAILY_CAP);
        assert_eq!(d, FetchDecision::Blocked { stale: None, code: "DECK-007" });
    }

    #[test]
    fn slug_front_face_only() {
        assert_eq!(
            edhrec_slug("Fable of the Mirror-Breaker // Reflection of Kiki-Jiki"),
            "fable-of-the-mirror-breaker"
        );
    }

    #[test]
    fn slug_strips_commas_and_apostrophes() {
        assert_eq!(edhrec_slug("Kaalia of the Vast"), "kaalia-of-the-vast");
        assert_eq!(edhrec_slug("Baylen, the Haymaker"), "baylen-the-haymaker");
        assert_eq!(edhrec_slug("Jhoira's Familiar"), "jhoiras-familiar");
    }

    #[test]
    fn url_builders_exact() {
        assert_eq!(
            edhrec_url("Baylen, the Haymaker"),
            "https://json.edhrec.com/pages/commanders/baylen-the-haymaker.json"
        );
        assert_eq!(
            archidekt_list_url(Format::Brawl100, 1),
            "https://archidekt.com/api/decks/v3/?deckFormat=20&orderBy=-viewCount&size=100&pageSize=20&page=1"
        );
        assert_eq!(
            archidekt_list_url(Format::StandardBrawl60, 2),
            "https://archidekt.com/api/decks/v3/?deckFormat=13&orderBy=-viewCount&size=60&pageSize=20&page=2"
        );
        assert_eq!(
            archidekt_deck_url(12345),
            "https://archidekt.com/api/decks/12345/"
        );
        assert_eq!(
            spellbook_url(Format::Brawl100, 1),
            "https://backend.commanderspellbook.com/variants/?q=legal:brawl&limit=100&page=1"
        );
        assert_eq!(
            spellbook_url(Format::StandardBrawl60, 3),
            "https://backend.commanderspellbook.com/variants/?q=legal:standardBrawl&limit=100&page=3"
        );
        assert_eq!(
            seventeenlands_url("BLB"),
            "https://www.17lands.com/card_ratings/data?expansion=BLB&format=PremierDraft"
        );
    }

    #[test]
    fn source_kind_host_and_ttl() {
        assert_eq!(SourceKind::Edhrec.host(), "edhrec");
        assert_eq!(SourceKind::ArchidektList.host(), "archidekt");
        assert_eq!(SourceKind::ArchidektDeck.host(), "archidekt");
        assert_eq!(SourceKind::Spellbook.host(), "spellbook");
        assert_eq!(SourceKind::SeventeenLands.host(), "17lands");
        assert_eq!(SourceKind::Edhrec.ttl_secs(), 7 * 86_400);
        assert_eq!(SourceKind::ArchidektList.ttl_secs(), 86_400);
        assert_eq!(SourceKind::SeventeenLands.ttl_secs(), 30 * 86_400);
    }

    #[test]
    fn parse_edhrec_fixture() {
        let cards = parse_edhrec(&fixture("edhrec_sample.json")).unwrap();
        assert_eq!(cards.len(), 3);
        let sol = cards.iter().find(|c| c.name == "Sol Ring").unwrap();
        assert!((sol.inclusion - 0.9).abs() < 1e-4);
        assert!((sol.synergy - 0.15).abs() < 1e-4);
        assert_eq!(sol.section, "Top Cards");
        let ramp = cards.iter().find(|c| c.name == "Rampant Growth").unwrap();
        assert!((ramp.inclusion - 0.2).abs() < 1e-4);
        assert_eq!(ramp.section, "Ramp");
    }

    #[test]
    fn parse_archidekt_list_fixture() {
        let (items, has_next) = parse_archidekt_list(&fixture("archidekt_list_sample.json")).unwrap();
        assert_eq!(items.len(), 2);
        assert!(has_next);
        assert_eq!(items[0].id, 12345);
        assert_eq!(items[0].name, "Baylen Tokens");
        assert_eq!(items[0].size, 100);
        assert_eq!(items[0].view_count, 5432);
        assert_eq!(items[0].colors, vec!["W", "R", "G"]);
        assert_eq!(items[1].colors, vec!["W", "U"]);
    }

    #[test]
    fn parse_archidekt_list_accepts_array_colors_and_missing() {
        // Older/alternate shape: colors as a list of letters; also absent entirely.
        let body = r#"{"count":2,"next":null,"results":[
            {"id":1,"name":"A","size":100,"viewCount":10,"colors":["G","R","W"]},
            {"id":2,"name":"B","size":100,"viewCount":5}
        ]}"#;
        let (items, has_next) = parse_archidekt_list(body).unwrap();
        assert!(!has_next);
        assert_eq!(items[0].colors, vec!["G", "R", "W"]);
        assert!(items[1].colors.is_empty());
    }

    #[test]
    fn parse_archidekt_deck_fixture() {
        let d = parse_archidekt_deck(&fixture("archidekt_deck_sample.json")).unwrap();
        assert_eq!(d.id, 12345);
        assert_eq!(d.commander_names, vec!["Baylen, the Haymaker".to_string()]);
        assert_eq!(d.cards.len(), 3);
        assert!(d.cards.iter().any(|(n, q)| n == "Forest" && *q == 30));
        assert!(d.cards.iter().any(|(n, q)| n == "Cultivate" && *q == 1));
        // Maybeboard (includedInDeck=false), Sideboard (by name even though
        // Archidekt flags it includedInDeck=true), and a custom category with
        // includedInDeck=false are all outside the 99.
        assert!(!d.cards.iter().any(|(n, _)| n == "Maybe Card"));
        assert!(!d.cards.iter().any(|(n, _)| n == "Side Card"));
        assert!(!d.cards.iter().any(|(n, _)| n == "Custom Excluded Card"));
    }

    #[test]
    fn parse_archidekt_deck_without_deck_categories_still_drops_maybeboard() {
        // Deck-level `categories` absent → fall back to name-based exclusion.
        let body = r#"{"id":7,"name":"x","cards":[
            {"quantity":1,"categories":["Commander"],"card":{"oracleCard":{"name":"Cmdr"}}},
            {"quantity":1,"categories":["Ramp"],"card":{"oracleCard":{"name":"Keep"}}},
            {"quantity":1,"categories":["maybeboard"],"card":{"oracleCard":{"name":"Drop"}}}
        ]}"#;
        let d = parse_archidekt_deck(body).unwrap();
        assert_eq!(d.commander_names, vec!["Cmdr".to_string()]);
        assert_eq!(d.cards, vec![("Keep".to_string(), 1)]);
    }

    #[test]
    fn parse_spellbook_fixture() {
        let (combos, has_next) = parse_spellbook(&fixture("spellbook_sample.json")).unwrap();
        assert!(!has_next);
        assert_eq!(combos.len(), 2);
        assert_eq!(combos[0], vec!["baylen, the haymaker", "scute swarm"]);
        assert_eq!(combos[1].len(), 3);
        assert!(combos[1].contains(&"teferi, hero of dominaria".to_string()));
    }

    #[test]
    fn parse_seventeenlands_filters_low_gamecount() {
        let rows = parse_seventeenlands(&fixture("seventeenlands_sample.json")).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|(id, _)| *id == 91001));
        assert!(rows.iter().any(|(id, _)| *id == 91002));
        assert!(!rows.iter().any(|(id, _)| *id == 91003));
    }

    #[test]
    fn assemble_merges_all_sources() {
        let edhrec = vec![
            EdhrecCard {
                name: "Sol Ring".into(),
                inclusion: 0.9,
                synergy: 0.15,
                section: "Top Cards".into(),
            },
            EdhrecCard {
                name: "Cultivate".into(),
                inclusion: 0.3,
                synergy: 0.08,
                section: "Top Cards".into(),
            },
        ];
        let seed = SeedDeckDetail {
            id: 1,
            name: "test".into(),
            commander_names: vec!["Baylen, the Haymaker".into()],
            cards: vec![
                ("Cultivate".into(), 1),
                ("Forest".into(), 30),
                ("Baylen, the Haymaker".into(), 1),
            ],
        };
        let combos = vec![vec!["baylen, the haymaker".into(), "scute swarm".into()]];
        let mut gih: HashMap<String, f32> = HashMap::new();
        gih.insert("sol ring".into(), 0.62);
        gih.insert("new card".into(), 0.55);

        let data = assemble_community(Some(&edhrec), Some(&seed), &combos, &gih, None, 0);
        assert!(data.available);
        assert_eq!(data.combos, combos);
        assert_eq!(data.seed_names, vec!["Cultivate", "Forest"]);

        let sol = data.by_name.get("sol ring").unwrap();
        assert_eq!(sol.inclusion, Some(0.9));
        assert_eq!(sol.synergy, Some(0.15));
        assert_eq!(sol.gih_wr, Some(0.62));

        let cult = data.by_name.get("cultivate").unwrap();
        assert_eq!(cult.inclusion, Some(0.3));
        assert_eq!(cult.seed_freq, Some(1.0));

        let forest = data.by_name.get("forest").unwrap();
        assert_eq!(forest.seed_freq, Some(1.0));
        assert!(forest.inclusion.is_none());

        assert!(data.by_name.get("baylen, the haymaker").is_none());

        let new_card = data.by_name.get("new card").unwrap();
        assert_eq!(new_card.gih_wr, Some(0.55));
    }

    #[test]
    fn assemble_not_available_without_edhrec_or_seed() {
        let data = assemble_community(None, None, &[], &HashMap::new(), None, 0);
        assert!(!data.available);
        assert!(data.by_name.is_empty());
    }

    fn summary(id: i64, colors: &[&str]) -> SeedDeckSummary {
        SeedDeckSummary {
            id,
            name: format!("d{id}"),
            size: 100,
            view_count: 1,
            colors: colors.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn detail(id: i64, commander: &str, cards: &[&str]) -> SeedDeckDetail {
        SeedDeckDetail {
            id,
            name: format!("d{id}"),
            commander_names: vec![commander.into()],
            cards: cards.iter().map(|c| (c.to_string(), 1)).collect(),
        }
    }

    #[test]
    fn build_staples_lowercases_dedupes_and_sorts_colors() {
        let s = summary(1, &["R", "G", "W"]);
        let d = detail(
            1,
            "Baylen",
            &[
                "Sol Ring",
                "sol ring",
                "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki",
                "Baylen",
            ],
        );
        let agg = build_staples(42, &[(&s, &d)]);
        assert_eq!(agg.built_at, 42);
        assert_eq!(agg.decks.len(), 1);
        assert_eq!(agg.decks[0].colors, "GRW");
        assert_eq!(
            agg.decks[0].cards,
            vec![
                "fable of the mirror-breaker".to_string(),
                "sol ring".to_string()
            ]
        );
    }

    #[test]
    fn staple_shares_uses_identity_subset_when_large_enough() {
        let mut decks = Vec::new();
        for _ in 0..6 {
            decks.push(StapleDeck {
                colors: "G".into(),
                cards: vec!["llanowar elves".into()],
            });
        }
        for _ in 0..4 {
            decks.push(StapleDeck {
                colors: "U".into(),
                cards: vec!["counterspell".into()],
            });
        }
        let agg = StaplesAggregate { built_at: 0, decks };
        let g = staple_shares(&agg, crate::services::deck::engine::types::G);
        assert!((g["llanowar elves"] - 1.0).abs() < 1e-6);
        assert!(g.get("counterspell").is_none());
        // Identity U has only 4 compatible decks (< STAPLES_MIN_DECKS) → falls back to all 10.
        let u = staple_shares(&agg, crate::services::deck::engine::types::U);
        assert!((u["counterspell"] - 0.4).abs() < 1e-6);
        assert!((u["llanowar elves"] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn assemble_community_carries_staple_share() {
        let agg = StaplesAggregate {
            built_at: 0,
            decks: (0..5)
                .map(|_| StapleDeck {
                    colors: "G".into(),
                    cards: vec!["sol ring".into()],
                })
                .collect(),
        };
        let c = assemble_community(
            None,
            None,
            &[],
            &HashMap::new(),
            Some(&agg),
            crate::services::deck::engine::types::G,
        );
        assert!(c.staples_available);
        assert_eq!(c.by_name["sol ring"].staple, Some(1.0));
    }
}
