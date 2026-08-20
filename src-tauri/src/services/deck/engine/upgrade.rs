// Budgeted upgrade pass for OwnedOnly builds. After the owned-first skeleton
// (fill_core with the budget deferred) and local search, this pass spends the
// reserved wildcard budget on the swaps that most improve `deck_objective`.
// Mirrors local_search::improve with three deltas: the replacement pool is
// unowned cards only, the budget is live (spend on add, refund on revert),
// and seeded cards are displaceable — only must_include stays anchored.

use std::collections::HashSet;

use super::fill::{all_roles_at_max, make_ctx, FillPlan, FillState};
use super::local_search::{deck_objective, static_prescore};
use super::score::score_card;
use super::types::*;

pub const UPGRADE_ACCEPT_DELTA: f32 = 0.02;
pub const UPGRADE_REMOVAL_CANDIDATES: usize = 8;
pub const UPGRADE_SWAP_CANDIDATES: usize = 80;
pub const UPGRADE_MAX_ITERS: usize = 200;

pub fn run(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
    st: &mut FillState,
    reserved: WildcardBudget,
) {
    st.budget = reserved;
    let anchored: HashSet<usize> = plan.must_include.iter().copied().collect();
    // Unowned candidates ranked by empty-deck power+synergy. The combo-
    // completion bonus (compute_synergy) is deck-state independent, so
    // missing combo pieces rank high here even with low generic power.
    let prescore: Vec<(usize, f32)> = static_prescore(oracles, community, plan)
        .into_iter()
        .filter(|(i, _)| !oracles[*i].is_owned())
        .collect();

    for _ in 0..UPGRADE_MAX_ITERS {
        let j0 = deck_objective(oracles, plan, st, community);
        // Bottom-K removal candidates by stored total (owned skeleton cards;
        // seeds are fair game — only must_include is anchored).
        let mut cands: Vec<(usize, f32, String)> = st
            .chosen
            .iter()
            .filter(|(i, _, _)| !oracles[*i].is_land && !anchored.contains(i))
            .map(|(i, _, s)| (*i, s.total, oracles[*i].name.clone()))
            .collect();
        cands.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });
        cands.truncate(UPGRADE_REMOVAL_CANDIDATES);

        let mut accepted = false;
        for (removed_idx, _, _) in cands {
            let Some(ci) = st.chosen.iter().position(|(i, _, _)| *i == removed_idx) else {
                continue;
            };
            st.remove_at(oracles, plan, ci);

            let mut best: Option<(usize, f32)> = None;
            let mut best_name = String::new();
            let mut tried = 0usize;
            for &(pidx, _) in &prescore {
                if tried >= UPGRADE_SWAP_CANDIDATES {
                    break;
                }
                if pidx == removed_idx || st.used.contains(&pidx) {
                    continue;
                }
                let card = &oracles[pidx];
                if card.is_land || all_roles_at_max(card, plan.template, &st.role_counts) {
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
                st.add(oracles, community, plan, removed_idx, true);
                continue;
            };
            st.add(oracles, community, plan, new_idx, false);
            let j1 = deck_objective(oracles, plan, st, community);
            if j1 > j0 + UPGRADE_ACCEPT_DELTA {
                accepted = true;
                break;
            }
            // Revert: remove_at refunds the wildcard spent by add().
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
