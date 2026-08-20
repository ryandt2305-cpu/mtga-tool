use super::test_fixtures::gen_oracles;
use super::*;

fn base_input(oracles: Vec<OracleCard>) -> EngineInput {
    EngineInput {
        oracles,
        wildcards_owned: WildcardBudget {
            common: 100,
            uncommon: 100,
            rare: 100,
            mythic: 100,
        },
        community: CommunityData::default(),
    }
}

fn base_req(commander_grp: i32) -> BuildRequest {
    BuildRequest {
        format: Format::Brawl100,
        commander_grp: Some(commander_grp),
        ownership: OwnershipMode::BestDeck,
        playstyle: Playstyle::Midrange,
        color_subset: None,
        theme_tags: vec![],
        must_include: vec![],
        build_around: vec![],
        exclude: vec![],
        seed: None,
        template_overrides: None,
        use_17lands: false,
    }
}

#[test]
fn builds_full_brawl100_deck() {
    let input = base_input(gen_oracles());
    let req = base_req(1);
    let result = build(&input, &req).unwrap();
    let total: i32 = result.slots.iter().map(|s| s.needed_qty).sum();
    assert_eq!(total, 99, "sum of needed_qty should be 99");
    assert_eq!(result.stats.land_count, 37);

    let mut seen: HashSet<String> = HashSet::new();
    for s in &result.slots {
        if !s.oracle_id.starts_with("basic-") {
            assert!(seen.insert(s.oracle_id.clone()), "dup {}", s.oracle_id);
        }
    }
    for s in &result.slots {
        let idx = input
            .oracles
            .iter()
            .position(|c| c.oracle_id == s.oracle_id)
            .unwrap();
        let card = &input.oracles[idx];
        assert!(
            is_subset(card.color_identity, W | G),
            "{} outside identity",
            s.name
        );
    }
    assert!(result.slots.iter().all(|s| s.oracle_id != "test-commander"));
}

#[test]
fn owned_only_never_exceeds_budget() {
    let input = base_input(gen_oracles());
    let mut req = base_req(1);
    req.ownership = OwnershipMode::OwnedOnly {
        wc_budget: WildcardBudget {
            common: 0,
            uncommon: 0,
            rare: 1,
            mythic: 0,
        },
    };
    let result = build(&input, &req).unwrap();
    assert!(
        result.stats.wildcard_cost.rare <= 1,
        "rare cost {} exceeded budget 1",
        result.stats.wildcard_cost.rare
    );
    assert_eq!(result.stats.wildcard_cost.common, 0);
    assert_eq!(result.stats.wildcard_cost.uncommon, 0);
    assert_eq!(result.stats.wildcard_cost.mythic, 0);
}

#[test]
fn best_deck_may_include_unowned() {
    let input = base_input(gen_oracles());
    let req = base_req(1);
    let result = build(&input, &req).unwrap();
    assert!(
        result.stats.owned < result.stats.total,
        "owned {} == total {}",
        result.stats.owned,
        result.stats.total
    );
}

#[test]
fn must_include_and_exclude_respected() {
    let input = base_input(gen_oracles());
    // find a grp for a Ramp W|G card and a Draw W card
    let must_grp = input
        .oracles
        .iter()
        .find(|c| c.oracle_id == "card-ramp-2")
        .and_then(|c| c.printings.first().map(|p| p.grp_id))
        .unwrap();
    let excl_grp = input
        .oracles
        .iter()
        .find(|c| c.oracle_id == "card-draw-0")
        .and_then(|c| c.printings.first().map(|p| p.grp_id))
        .unwrap();
    let mut req = base_req(1);
    req.must_include = vec![must_grp];
    req.exclude = vec![excl_grp];
    let result = build(&input, &req).unwrap();
    assert!(
        result.slots.iter().any(|s| s.grp_id == must_grp),
        "must_include grp missing"
    );
    assert!(
        result.slots.iter().all(|s| s.grp_id != excl_grp),
        "exclude grp present"
    );
}

#[test]
fn must_include_nonbasic_lands_do_not_overshoot_deck_size() {
    let input = base_input(gen_oracles());
    // Pin 3 nonbasic lands (grp 106, 107, 108 are the first three utility G lands).
    let land_grps: Vec<i32> = input
        .oracles
        .iter()
        .filter(|c| c.is_land && !c.is_basic_land && is_subset(c.color_identity, W | G))
        .take(3)
        .map(|c| c.printings[0].grp_id)
        .collect();
    assert_eq!(land_grps.len(), 3, "fixture should provide 3 W|G lands");
    let mut req = base_req(1);
    req.must_include = land_grps.clone();
    let result = build(&input, &req).unwrap();
    let total: i32 = result.slots.iter().map(|s| s.needed_qty).sum();
    assert_eq!(total, 99, "sum of needed_qty must stay 99 when lands are pinned");
    assert_eq!(result.stats.land_count, 37, "land_count must stay 37");
    for g in &land_grps {
        assert!(
            result.slots.iter().any(|s| s.grp_id == *g),
            "pinned land grp {g} must appear in slots"
        );
    }
}

#[test]
fn must_include_basic_land_does_not_duplicate_slot() {
    let input = base_input(gen_oracles());
    // grp 100 is Plains (basic land).
    let plains_grp: i32 = 100;
    let mut req = base_req(1);
    req.must_include = vec![plains_grp];
    let result = build(&input, &req).unwrap();
    let total: i32 = result.slots.iter().map(|s| s.needed_qty).sum();
    assert_eq!(total, 99, "sum of needed_qty must stay 99 when a basic is pinned");
    let plains_slots: Vec<&Slot> =
        result.slots.iter().filter(|s| s.grp_id == plains_grp).collect();
    assert!(
        plains_slots.len() <= 1,
        "Plains must appear at most once in slots, found {}",
        plains_slots.len()
    );
}

#[test]
fn standardbrawl_uses_59_and_only_sb_legal() {
    let input = base_input(gen_oracles());
    let mut req = base_req(1);
    req.format = Format::StandardBrawl60;
    let result = build(&input, &req).unwrap();
    let total: i32 = result.slots.iter().map(|s| s.needed_qty).sum();
    assert_eq!(total, 59);
    for s in &result.slots {
        let card = input
            .oracles
            .iter()
            .find(|c| c.oracle_id == s.oracle_id)
            .unwrap();
        assert!(
            card.legal_standardbrawl,
            "{} not standardbrawl-legal",
            s.name
        );
    }
}

#[test]
fn deterministic() {
    let input = base_input(gen_oracles());
    let req = base_req(1);
    let a = build(&input, &req).unwrap();
    let b = build(&input, &req).unwrap();
    assert_eq!(a, b);
}

#[test]
fn invalid_commander_is_deck_009() {
    let input = base_input(gen_oracles());
    // grp 100 is a basic (Plains) — not commander-eligible.
    let mut req = base_req(100);
    let err = build(&input, &req).unwrap_err();
    assert_eq!(err.code(), "DECK-009");

    // Nonexistent grp
    req.commander_grp = Some(9_999_999);
    let err = build(&input, &req).unwrap_err();
    assert_eq!(err.code(), "DECK-009");
}

#[test]
fn alternatives_are_same_role_unused() {
    let input = base_input(gen_oracles());
    let req = base_req(1);
    let result = build(&input, &req).unwrap();
    let chosen_grps: HashSet<i32> = result.slots.iter().map(|s| s.grp_id).collect();
    for s in &result.slots {
        if s.role == Role::Land {
            continue;
        }
        for alt in &s.alternatives {
            assert!(
                !chosen_grps.contains(&alt.grp_id),
                "alt {} already in deck",
                alt.name
            );
            let card = input
                .oracles
                .iter()
                .find(|c| {
                    c.printings.iter().any(|p| p.grp_id == alt.grp_id)
                })
                .expect("alt card in oracles");
            assert!(
                card.roles.contains(&s.role),
                "alt {} role mismatch",
                alt.name
            );
        }
    }
}

#[test]
fn staples_signal_lifts_known_card() {
    use super::types::CommunitySignal;
    let mut oracles = gen_oracles();
    // Two identical unowned-agnostic Threat cards; give one a staple share.
    let base = oracles
        .iter()
        .position(|c| c.roles.contains(&Role::Threat) && !c.is_land)
        .unwrap();
    let mut a = oracles[base].clone();
    a.oracle_id = "staple-a".into();
    a.name = "Zzz Staple".into();
    a.name_lower = "zzz staple".into();
    a.printings[0].grp_id = 90_001;
    let mut b = a.clone();
    b.oracle_id = "staple-b".into();
    b.name = "Zzz Filler".into();
    b.name_lower = "zzz filler".into();
    b.printings[0].grp_id = 90_002;
    a.edhrec_rank = None;
    b.edhrec_rank = None;
    oracles.push(a);
    oracles.push(b);
    let mut input = base_input(oracles);
    input.community.by_name.insert(
        "zzz staple".into(),
        CommunitySignal {
            staple: Some(0.95),
            ..Default::default()
        },
    );
    let req = base_req(1);
    let result = build(&input, &req).unwrap();
    let has_staple = result.slots.iter().any(|s| s.name == "Zzz Staple");
    let has_filler = result.slots.iter().any(|s| s.name == "Zzz Filler");
    assert!(has_staple || !has_filler, "staple must not lose to identical filler");
}

#[test]
fn make_plan_flags_almost_complete_combos() {
    let oracles = gen_oracles();
    let mut community = CommunityData::default();
    // Combo A: two owned WG-legal pieces + one unowned piece → flagged.
    // Combo B: contains an out-of-identity piece ("counter 03" is U) → skipped.
    // Combo C: two pieces missing / out-of-identity → skipped.
    community.combos = vec![
        vec!["removal 00".into(), "draw 02".into(), "payoff 01".into()],
        vec!["removal 00".into(), "counter 03".into()],
        vec!["removal 00".into(), "payoff 01".into(), "payoff 03".into()],
    ];
    let input = EngineInput {
        oracles,
        wildcards_owned: WildcardBudget { common: 9, uncommon: 9, rare: 9, mythic: 9 },
        community,
    };
    let req = BuildRequest {
        format: Format::Brawl100,
        commander_grp: Some(1),
        ownership: OwnershipMode::OwnedOnly {
            wc_budget: WildcardBudget { common: 9, uncommon: 9, rare: 9, mythic: 9 },
        },
        playstyle: Playstyle::Midrange,
        color_subset: None,
        theme_tags: vec![],
        must_include: vec![],
        build_around: vec![],
        exclude: vec![],
        seed: None,
        template_overrides: None,
        use_17lands: false,
    };
    let template = resolve_template(&input, &req).unwrap();
    let plan = make_plan(&input, &req, &template).unwrap();

    let entry = plan
        .combo_completions
        .get("payoff 01")
        .expect("combo A flagged");
    assert_eq!(entry.have, 2);
    assert_eq!(entry.size, 3);
    assert!((entry.bonus - 0.5).abs() < 1e-5);
    assert!(!plan.combo_completions.contains_key("counter 03"));
    assert!(!plan.combo_completions.contains_key("payoff 03"));
}

#[test]
fn template_infeasible_warns() {
    // Trim to a tiny pool: commander + basics + a handful of cards. Aggro SB60.
    let full = gen_oracles();
    let mut trimmed: Vec<OracleCard> = Vec::new();
    for c in &full {
        if c.oracle_id == "test-commander" || c.is_basic_land {
            trimmed.push(c.clone());
        }
    }
    // Add 10 non-land W/G cards that are sb-legal.
    for c in full.iter() {
        if trimmed.len() >= 6 + 10 {
            break;
        }
        if !c.is_land
            && c.legal_standardbrawl
            && is_subset(c.color_identity, W | G)
            && c.oracle_id != "test-commander"
        {
            trimmed.push(c.clone());
        }
    }
    let mut input = base_input(trimmed);
    input.wildcards_owned = WildcardBudget {
        common: 100,
        uncommon: 100,
        rare: 100,
        mythic: 100,
    };
    let mut req = base_req(1);
    req.format = Format::StandardBrawl60;
    req.playstyle = Playstyle::Aggro;
    let result = build(&input, &req).unwrap();
    assert!(
        result.warnings.iter().any(|w| w.code == "DECK-010"),
        "expected DECK-010 warning; got {:?}",
        result.warnings
    );
}
