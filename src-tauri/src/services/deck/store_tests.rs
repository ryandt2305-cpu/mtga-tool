    use super::*;

    fn oracle(id: &str, name: &str) -> IngestOracle {
        IngestOracle {
            oracle_id: id.into(),
            name: name.into(),
            name_front_lower: name.to_lowercase(),
            cmc: 2.0,
            mana_cost: "{1}{G}".into(),
            color_identity: "G".into(),
            type_line: "Creature — Elf".into(),
            keywords: "[]".into(),
            oracle_text: "".into(),
            edhrec_rank: Some(100),
            legal_brawl: true,
            legal_standardbrawl: false,
            is_commander_eligible: false,
            power: Some(2),
            layout: "normal".into(),
        }
    }

    #[test]
    fn migrations_create_tables_and_are_idempotent() {
        let db = DeckStore::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
                 ('meta','oracle','oracle_arena','grp_oracle','tag','tagging',
                  'oracle_role','source_cache','source_hits','saved_deck')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 10);
    }

    #[test]
    fn meta_roundtrip() {
        let db = DeckStore::open_in_memory().unwrap();
        assert_eq!(db.get_meta("x").unwrap(), None);
        db.set_meta("x", "1").unwrap();
        db.set_meta("x", "2").unwrap();
        assert_eq!(db.get_meta("x").unwrap(), Some("2".into()));
    }

    #[test]
    fn replace_bulk_replaces_not_appends() {
        let mut db = DeckStore::open_in_memory().unwrap();
        db.replace_bulk(&[oracle("a", "A"), oracle("b", "B")], &[], &[], &[])
            .unwrap();
        db.replace_bulk(
            &[oracle("c", "C")],
            &[IngestArena {
                arena_id: 5,
                oracle_id: "c".into(),
                set_code: "blb".into(),
                collector_number: "1".into(),
            }],
            &[IngestTag {
                tag_id: "t1".into(),
                slug: "ramp".into(),
                label: "Ramp".into(),
                parent_id: None,
            }],
            &[("c".into(), "t1".into())],
        )
        .unwrap();
        assert_eq!(db.oracle_count().unwrap(), 1);
        assert_eq!(db.arena_id_map().unwrap().get(&5).unwrap(), "c");
        assert_eq!(db.name_map().unwrap().get("c").unwrap(), "c");
        assert_eq!(
            db.load_taggings().unwrap(),
            vec![("c".to_string(), "t1".to_string())]
        );
    }

    #[test]
    fn cache_ttl_and_hits() {
        let db = DeckStore::open_in_memory().unwrap();
        assert!(db.cache_get("edhrec", "k", 1000).unwrap().is_none());
        db.cache_put("edhrec", "k", "{}", 60, 1000).unwrap();
        let hit = db.cache_get("edhrec", "k", 1030).unwrap().unwrap();
        assert!(hit.fresh);
        let stale = db.cache_get("edhrec", "k", 2000).unwrap().unwrap();
        assert!(!stale.fresh);
        assert_eq!(db.hits_today("edhrec", "2026-08-16").unwrap(), 0);
        db.hit_increment("edhrec", "2026-08-16").unwrap();
        db.hit_increment("edhrec", "2026-08-16").unwrap();
        assert_eq!(db.hits_today("edhrec", "2026-08-16").unwrap(), 2);
        db.cache_put("x", "old", "{}", 10, 0).unwrap();
        // Both edhrec (put at t=1000, ttl=60 → expiry=1060) and x (put at t=0, ttl=10
        // → expiry=10) are past their expiry by ~40 d at now=40*86400, so
        // `fetched_at + ttl_secs < now - 30d` prunes both.
        assert_eq!(db.prune_cache(40 * 86_400).unwrap(), 2);
    }

    #[test]
    fn saved_deck_crud() {
        let db = DeckStore::open_in_memory().unwrap();
        let id = db
            .save_deck(&SavedDeckInput {
                id: None,
                name: "Elves".into(),
                format: "brawl100".into(),
                commander_grp: 1,
                request_json: "{}".into(),
                result_json: "{}".into(),
            })
            .unwrap();
        let again = db
            .save_deck(&SavedDeckInput {
                id: Some(id),
                name: "Elves v2".into(),
                format: "brawl100".into(),
                commander_grp: 1,
                request_json: "{}".into(),
                result_json: "{}".into(),
            })
            .unwrap();
        assert_eq!(id, again);
        assert_eq!(db.list_decks().unwrap().len(), 1);
        assert_eq!(db.get_deck(id).unwrap().unwrap().name, "Elves v2");
        db.delete_deck(id).unwrap();
        assert!(db.get_deck(id).unwrap().is_none());
    }
