// Deck-level local search on top of `fill_core`. Given a filled state,
// try up to LOCAL_SEARCH_ITERS removal→replacement swaps that improve a
// deck-level objective. Deterministic (every sort ties by name).
//
// Objective = sum of chosen nonland card totals
//           − role-shortfall penalty
//           − curve-distance penalty
//           − pip-concentration penalty over identity colours
//           + combos-completed bonus.

use std::collections::HashSet;

use super::fill::{all_roles_at_max, make_ctx, FillPlan, FillState};
use super::score::score_card;
use super::types::*;

pub const LOCAL_SEARCH_ITERS: usize = 200;
pub const REMOVAL_CANDIDATES: usize = 8;
pub const SWAP_CANDIDATES: usize = 40;
pub const ACCEPT_DELTA: f32 = 0.02;

pub fn deck_objective(
    oracles: &[OracleCard],
    plan: &FillPlan,
    st: &FillState,
    community: &CommunityData,
) -> f32 {
    let template = plan.template;
    let identity = plan.identity;
    // Sum the current-state score of every chosen nonland card, not the score
    // stored when the card was first added — those get stale after each swap.
    // Recomputing keeps the objective consistent across the accept/revert loop.
    let ctx = make_ctx(
        plan,
        community,
        &st.role_counts,
        &st.curve_counts,
        &st.pip_counts,
        &st.deck_names,
        &st.budget,
    );
    let mut j: f32 = st
        .chosen
        .iter()
        .filter(|(i, _, _)| !oracles[*i].is_land)
        .map(|(i, _, _)| score_card(&oracles[*i], &ctx).total)
        .sum();
    // Role shortfall (sum of unmet minima, normalised).
    for r in Role::ALL {
        if r == Role::Land {
            continue;
        }
        let min = template.min(r) as f32;
        if min <= 0.0 {
            continue;
        }
        let c = st.role_counts.get(&r).copied().unwrap_or(0) as f32;
        j -= 0.5 * ((min - c).max(0.0) / min);
    }
    // Curve distance.
    let nonland = template.nonland() as f32;
    let mut dist = 0.0f32;
    for b in 0..8 {
        dist += (st.curve_counts[b] as f32 - template.curve[b] * nonland).abs();
    }
    j -= 0.5 * dist / nonland.max(1.0);
    // Pip concentration over identity colours.
    let letters = mask_to_letters(identity);
    let n = letters.len().max(1) as f32;
    let total: u32 = st.pip_counts.iter().sum();
    if total > 0 && letters.len() > 1 {
        for (i, ch) in ['W', 'U', 'B', 'R', 'G'].iter().enumerate() {
            if !letters.contains(ch) {
                continue;
            }
            let share = st.pip_counts[i] as f32 / total as f32;
            j -= 0.3 * (share - 1.0 / n - 0.2).max(0.0);
        }
    }
    // Combos completed.
    let mut done = 0u32;
    for combo in &community.combos {
        if combo.iter().all(|nm| st.deck_names.contains(nm)) {
            done += 1;
        }
    }
    j += 0.2 * done as f32;
    j
}

/// Pool ordering used by `improve` to pick swap candidates: cards scored
/// against an empty deck state (only power + synergy factors), best first,
/// ties broken by card name. Nonland only.
pub fn static_prescore(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
) -> Vec<(usize, f32)> {
    let rc = std::collections::HashMap::new();
    let cc = [0u32; 8];
    let pc = [0u32; 5];
    let dn: HashSet<String> = HashSet::new();
    let bg = plan.budget;
    let ctx = make_ctx(plan, community, &rc, &cc, &pc, &dn, &bg);
    let mut v: Vec<(usize, f32)> = plan
        .pool
        .iter()
        .copied()
        .filter(|&i| !oracles[i].is_land)
        .map(|i| {
            let s = score_card(&oracles[i], &ctx);
            (i, s.power + s.synergy)
        })
        .collect();
    v.sort_by(|a, b| {
        b.1
            .partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| oracles[a.0].name.cmp(&oracles[b.0].name))
    });
    v
}

pub fn improve(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
    st: &mut FillState,
    prescore: &[(usize, f32)],
) {
    let anchored: HashSet<usize> = plan.must_include.iter().copied().collect();
    let seeded: HashSet<usize> = plan.seed_order.iter().copied().collect();
    for _ in 0..LOCAL_SEARCH_ITERS {
        let j0 = deck_objective(oracles, plan, st, community);
        // Bottom-K removal candidates by total, ascending, ties by name.
        // Store oracle indices (not chosen positions) — every remove_at + add
        // reorders `chosen`, so a position captured now is stale next
        // iteration. Positions get resolved fresh inside the loop.
        let mut cands: Vec<(usize, f32, String)> = st
            .chosen
            .iter()
            .filter(|(i, _, _)| {
                !oracles[*i].is_land && !anchored.contains(i) && !seeded.contains(i)
            })
            .map(|(i, _, s)| (*i, s.total, oracles[*i].name.clone()))
            .collect();
        cands.sort_by(|a, b| {
            a.1
                .partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });
        cands.truncate(REMOVAL_CANDIDATES);
        let mut accepted = false;
        for (removed_idx, _, _) in cands {
            let Some(ci) = st.chosen.iter().position(|(i, _, _)| *i == removed_idx) else {
                continue;
            };
            st.remove_at(oracles, plan, ci);

            // Best replacement among top SWAP_CANDIDATES by prescore.
            let mut best: Option<(usize, f32)> = None;
            let mut best_name = String::new();
            let mut tried = 0usize;
            for &(pidx, _) in prescore {
                if tried >= SWAP_CANDIDATES {
                    break;
                }
                if pidx == removed_idx || st.used.contains(&pidx) {
                    continue;
                }
                let card = &oracles[pidx];
                if all_roles_at_max(card, plan.template, &st.role_counts) {
                    continue;
                }
                tried += 1;
                let ctx = make_ctx(
                    plan,
                    community,
                    &st.role_counts,
                    &st.curve_counts,
                    &st.pip_counts,
                    &st.deck_names,
                    &st.budget,
                );
                let s = score_card(card, &ctx);
                if !s.affordable {
                    continue;
                }
                if best.map_or(true, |(_, t)| {
                    s.total > t || (s.total == t && card.name < best_name)
                }) {
                    best = Some((pidx, s.total));
                    best_name = card.name.clone();
                }
            }
            let Some((new_idx, _)) = best else {
                // No replacement found for this slot — restore original and try next.
                st.add(oracles, community, plan, removed_idx, true);
                continue;
            };
            st.add(oracles, community, plan, new_idx, false);
            let j1 = deck_objective(oracles, plan, st, community);
            if j1 > j0 + ACCEPT_DELTA {
                debug_assert!(j1 >= j0);
                accepted = true;
                break;
            }
            // Revert the swap.
            let new_ci = st
                .chosen
                .iter()
                .position(|(i, _, _)| *i == new_idx)
                .expect("just added");
            st.remove_at(oracles, plan, new_ci);
            st.add(oracles, community, plan, removed_idx, true);
        }
        if !accepted {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::deck::engine::test_fixtures::gen_oracles;
    use crate::services::deck::engine::{
        make_plan, resolve_template, BuildRequest, EngineInput, OwnershipMode, Playstyle,
    };

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
    fn improve_never_lowers_objective() {
        let input = base_input(gen_oracles());
        let req = base_req(1);
        let template = resolve_template(&input, &req).unwrap();
        let plan = make_plan(&input, &req, &template).unwrap();
        let (mut st, prescore) = crate::services::deck::engine::fill::fill_state_without_search(
            &input.oracles,
            &input.community,
            &plan,
        );
        let j0 = deck_objective(&input.oracles, &plan, &st, &input.community);
        improve(&input.oracles, &input.community, &plan, &mut st, &prescore);
        let j1 = deck_objective(&input.oracles, &plan, &st, &input.community);
        assert!(j1 >= j0 - 1e-4);
    }
}
