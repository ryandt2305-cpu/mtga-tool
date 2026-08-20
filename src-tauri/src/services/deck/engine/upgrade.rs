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
pub const UPGRADE_SUGGESTIONS: usize = 15;

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

/// Rank the unowned candidates that did NOT make the deck: what the next
/// wildcards should buy. gain = candidate score with the ownership term
/// removed (so an empty budget doesn't poison the ranking) minus the score
/// of the card it would displace. Ties break by Archidekt staple share
/// (cross-deck reusability), then name.
pub fn craft_suggestions(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
    st: &FillState,
    n: usize,
) -> Vec<CraftSuggestion> {
    let anchored: HashSet<usize> = plan.must_include.iter().copied().collect();
    let w = &plan.template.weights;

    // Displacement targets: weakest stored total per role + global weakest.
    let mut weakest_by_role: std::collections::HashMap<Role, (usize, f32)> =
        std::collections::HashMap::new();
    let mut weakest_global: Option<(usize, f32)> = None;
    for (i, role, s) in &st.chosen {
        if oracles[*i].is_land || anchored.contains(i) {
            continue;
        }
        let e = weakest_by_role.entry(*role).or_insert((*i, s.total));
        if s.total < e.1 {
            *e = (*i, s.total);
        }
        if weakest_global.map_or(true, |(_, t)| s.total < t) {
            weakest_global = Some((*i, s.total));
        }
    }

    let ctx = make_ctx(
        plan,
        community,
        &st.role_counts,
        &st.curve_counts,
        &st.pip_counts,
        &st.deck_names,
        &st.budget,
    );

    let mut out: Vec<(CraftSuggestion, f32)> = Vec::new();
    for &pidx in &plan.pool {
        if st.used.contains(&pidx) {
            continue;
        }
        let card = &oracles[pidx];
        if card.is_land || card.is_owned() {
            continue;
        }
        let Some(bp) = card.best_printing() else {
            continue;
        };
        let s = score_card(card, &ctx);
        // Strip the ownership component so gain ranks card quality, not
        // whether the budget happens to be empty right now.
        let total_adj = s.total - w.ownership * s.ownership;
        let displaced = weakest_by_role.get(&s.role).copied().or(weakest_global);
        let Some((d_idx, d_total)) = displaced else {
            continue;
        };
        let gain = total_adj - d_total;
        if gain <= 0.0 {
            continue;
        }
        let mut reasons = s.reasons.clone();
        let min = plan.template.min(s.role);
        if st.role_counts.get(&s.role).copied().unwrap_or(0) < min {
            reasons.push(format!("fills {} gap", s.role.label()));
        }
        let staple = community
            .by_name
            .get(&card.name_lower)
            .and_then(|sig| sig.staple)
            .unwrap_or(0.0);
        out.push((
            CraftSuggestion {
                grp_id: bp.grp_id,
                oracle_id: card.oracle_id.clone(),
                name: card.name.clone(),
                rarity: bp.rarity.clone(),
                gain,
                affordable: st.budget.get(&bp.rarity) > 0,
                replaces_name: Some(oracles[d_idx].name.clone()),
                reasons,
            },
            staple,
        ));
    }
    out.sort_by(|a, b| {
        b.0.gain
            .partial_cmp(&a.0.gain)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.0.name.cmp(&b.0.name))
    });
    out.into_iter().take(n).map(|(s, _)| s).collect()
}
