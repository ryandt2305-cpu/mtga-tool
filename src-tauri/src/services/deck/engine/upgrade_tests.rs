#![cfg(test)]

use super::fill::fill_core;
use super::local_search::{deck_objective, improve, static_prescore};
use super::test_fixtures::gen_oracles;
use super::types::*;
use super::upgrade;
use super::{make_plan, resolve_template, BuildRequest, EngineInput, OwnershipMode, Playstyle};

fn base_req(budget: WildcardBudget) -> BuildRequest {
    BuildRequest {
        format: Format::Brawl100,
        commander_grp: Some(1),
        ownership: OwnershipMode::OwnedOnly { wc_budget: budget },
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

fn base_input(community: CommunityData, budget: WildcardBudget) -> EngineInput {
    EngineInput {
        oracles: gen_oracles(),
        wildcards_owned: budget,
        community,
    }
}

/// Run skeleton + improve + upgrade; return the final state.
fn run_pipeline(input: &EngineInput, req: &BuildRequest) -> super::fill::FillState {
    let template = resolve_template(input, req).unwrap();
    let plan = make_plan(input, req, &template).unwrap();
    let (mut st, _b, _n, reserved) = fill_core(&input.oracles, &input.community, &plan);
    let prescore = static_prescore(&input.oracles, &input.community, &plan);
    improve(&input.oracles, &input.community, &plan, &mut st, &prescore);
    upgrade::run(&input.oracles, &input.community, &plan, &mut st, reserved);
    st
}

#[test]
fn upgrade_never_exceeds_budget() {
    let budget = WildcardBudget { common: 2, uncommon: 2, rare: 1, mythic: 0 };
    let input = base_input(CommunityData::default(), budget);
    let req = base_req(budget);
    let st = run_pipeline(&input, &req);
    let mut spent = WildcardBudget::default();
    for (idx, _, _) in &st.chosen {
        let card = &input.oracles[*idx];
        if !card.is_owned() {
            spent.add(&card.best_printing().unwrap().rarity);
        }
    }
    assert!(spent.common <= 2 && spent.uncommon <= 2 && spent.rare <= 1 && spent.mythic == 0);
}

#[test]
fn upgrade_is_deterministic() {
    let budget = WildcardBudget { common: 3, uncommon: 3, rare: 2, mythic: 1 };
    let input = base_input(CommunityData::default(), budget);
    let req = base_req(budget);
    let st1 = run_pipeline(&input, &req);
    let st2 = run_pipeline(&input, &req);
    let names = |st: &super::fill::FillState| {
        let mut v: Vec<String> = st
            .chosen
            .iter()
            .map(|(i, _, _)| input.oracles[*i].name.clone())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&st1), names(&st2));
}

#[test]
fn upgrade_never_lowers_objective() {
    let budget = WildcardBudget { common: 3, uncommon: 3, rare: 2, mythic: 1 };
    let input = base_input(CommunityData::default(), budget);
    let req = base_req(budget);
    let template = resolve_template(&input, &req).unwrap();
    let plan = make_plan(&input, &req, &template).unwrap();
    let (mut st, _b, _n, reserved) = fill_core(&input.oracles, &input.community, &plan);
    let prescore = static_prescore(&input.oracles, &input.community, &plan);
    improve(&input.oracles, &input.community, &plan, &mut st, &prescore);
    let j0 = deck_objective(&input.oracles, &plan, &st, &input.community);
    upgrade::run(&input.oracles, &input.community, &plan, &mut st, reserved);
    let j1 = deck_objective(&input.oracles, &plan, &st, &input.community);
    assert!(j1 >= j0 - 1e-4);
}

#[test]
fn upgrade_targets_missing_combo_piece_over_generic_staple() {
    // The user owns 2/3 of a combo (both forced into the deck via
    // must_include); the missing piece "payoff 01" (unowned, uncommon, G)
    // must win the single uncommon wildcard over a popular generic card
    // "threat 07" (unowned, uncommon, W|G) that has EDHREC inclusion.
    let oracles = gen_oracles();
    let grp_of = |name: &str| -> i32 {
        oracles
            .iter()
            .find(|c| c.name_lower == name)
            .unwrap()
            .printings[0]
            .grp_id
    };
    let removal_grp = grp_of("removal 00");
    let draw_grp = grp_of("draw 02");
    let mut community = CommunityData::default();
    community.combos = vec![vec![
        "removal 00".into(),
        "draw 02".into(),
        "payoff 01".into(),
    ]];
    community.by_name.insert(
        "threat 07".into(),
        CommunitySignal {
            inclusion: Some(0.15),
            ..Default::default()
        },
    );
    community.available = true;
    // Rebuild the combos_by_name index the way assemble_community does.
    for (i, combo) in community.combos.clone().iter().enumerate() {
        for n in combo {
            community
                .combos_by_name
                .entry(n.clone())
                .or_default()
                .push(i);
        }
    }
    let budget = WildcardBudget { common: 0, uncommon: 1, rare: 0, mythic: 0 };
    let input = EngineInput {
        oracles,
        wildcards_owned: budget,
        community,
    };
    let mut req = base_req(budget);
    req.must_include = vec![removal_grp, draw_grp];
    let st = run_pipeline(&input, &req);
    let chosen_names: Vec<&str> = st
        .chosen
        .iter()
        .map(|(i, _, _)| input.oracles[*i].name_lower.as_str())
        .collect();
    assert!(
        chosen_names.contains(&"payoff 01"),
        "combo piece must be crafted"
    );
    assert!(
        !chosen_names.contains(&"threat 07"),
        "budget must not go to the generic staple"
    );
}

#[test]
fn craft_suggestions_ranked_unowned_and_capped() {
    let budget = WildcardBudget { common: 1, uncommon: 1, rare: 0, mythic: 0 };
    let input = base_input(CommunityData::default(), budget);
    let req = base_req(budget);
    let template = resolve_template(&input, &req).unwrap();
    let plan = make_plan(&input, &req, &template).unwrap();
    let (mut st, _b, _n, reserved) = fill_core(&input.oracles, &input.community, &plan);
    let prescore = static_prescore(&input.oracles, &input.community, &plan);
    improve(&input.oracles, &input.community, &plan, &mut st, &prescore);
    upgrade::run(&input.oracles, &input.community, &plan, &mut st, reserved);
    let sugg = upgrade::craft_suggestions(
        &input.oracles,
        &input.community,
        &plan,
        &st,
        upgrade::UPGRADE_SUGGESTIONS,
    );
    assert!(sugg.len() <= upgrade::UPGRADE_SUGGESTIONS);
    assert!(!sugg.is_empty(), "fixture has many unowned upgrades");
    for w in sugg.windows(2) {
        assert!(w[0].gain >= w[1].gain, "must be sorted by gain desc");
    }
    for s in &sugg {
        let card = input
            .oracles
            .iter()
            .find(|c| c.oracle_id == s.oracle_id)
            .unwrap();
        assert!(!card.is_owned(), "owned cards must not be suggested");
        assert!(s.gain > 0.0);
        // affordable mirrors the post-run budget.
        assert_eq!(s.affordable, st.budget.get(&s.rarity) > 0);
    }
}

#[test]
fn build_result_carries_suggestions_and_budget_left() {
    let budget = WildcardBudget { common: 2, uncommon: 2, rare: 1, mythic: 0 };
    let input = base_input(CommunityData::default(), budget);
    let req = base_req(budget);
    let result = super::build(&input, &req).unwrap();
    // OwnedOnly build: suggestions populated (fixture has unowned upgrades)
    // and budget_left never exceeds the request.
    assert!(!result.craft_suggestions.is_empty());
    assert!(result.budget_left.common <= 2 && result.budget_left.rare <= 1);
    // BestDeck build: no suggestions.
    let mut req2 = base_req(budget);
    req2.ownership = OwnershipMode::BestDeck;
    let result2 = super::build(&input, &req2).unwrap();
    assert!(result2.craft_suggestions.is_empty());
}
