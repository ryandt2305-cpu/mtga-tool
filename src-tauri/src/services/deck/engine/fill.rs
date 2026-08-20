use std::collections::{HashMap, HashSet};

use super::score::{cmc_bucket, score_card, ScoreCtx, Scored};
use super::template::Template;
use super::types::*;

pub use super::local_search::LOCAL_SEARCH_ITERS;
pub const BASIC_NAMES: [&str; 6] = ["Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"];

pub struct FillPlan<'a> {
    pub template: &'a Template,
    pub identity: u8,
    pub commander_idx: usize,
    pub pool: Vec<usize>,
    pub must_include: Vec<usize>,
    pub seed_order: Vec<usize>,
    pub build_around: HashMap<String, f32>,
    pub theme: HashSet<String>,
    pub commander_subtypes: Vec<String>,
    /// Pre-lowercased commander subtypes — perf cache for `compute_synergy`.
    pub commander_subtypes_lower: Vec<String>,
    /// `oracle_id → word-boundary token set` for every pool card (plus
    /// commander / must_include / seed). Built once by `build()` so subtype
    /// matching in `compute_synergy` is a HashSet lookup, not a substring scan.
    pub card_tokens: HashMap<String, HashSet<String>>,
    /// Inverse document frequency per tag over the pool. `compute_synergy`
    /// weights theme overlap by these so rare tags outweigh generic ones.
    pub tag_idf: HashMap<String, f32>,
    /// name_lower → almost-complete combo info (see types::ComboCompletion).
    pub combo_completions: HashMap<String, ComboCompletion>,
    pub owned_only: bool,
    pub budget: WildcardBudget,
}

pub struct FillOutcome {
    pub chosen: Vec<(usize, Role, Scored)>,
    pub basics: Vec<(usize, u32)>,
    pub warnings: Vec<Warning>,
    pub budget_left: WildcardBudget,
}

pub(super) fn make_ctx<'a>(
    plan: &'a FillPlan<'a>,
    community: &'a CommunityData,
    role_counts: &'a HashMap<Role, u32>,
    curve_counts: &'a [u32; 8],
    pip_counts: &'a [u32; 5],
    deck_names: &'a HashSet<String>,
    budget: &'a WildcardBudget,
) -> ScoreCtx<'a> {
    ScoreCtx {
        template: plan.template,
        role_counts,
        curve_counts,
        theme: &plan.theme,
        commander_subtypes: &plan.commander_subtypes,
        commander_subtypes_lower: Some(&plan.commander_subtypes_lower),
        card_tokens: Some(&plan.card_tokens),
        tag_idf: Some(&plan.tag_idf),
        build_around_bonus: &plan.build_around,
        community,
        deck_names,
        owned_only: plan.owned_only,
        budget,
        pip_counts,
        identity: plan.identity,
        combo_completions: Some(&plan.combo_completions),
    }
}

pub(super) struct FillState {
    pub chosen: Vec<(usize, Role, Scored)>,
    pub used: HashSet<usize>,
    pub role_counts: HashMap<Role, u32>,
    pub curve_counts: [u32; 8],
    pub pip_counts: [u32; 5],
    pub deck_names: HashSet<String>,
    pub budget: WildcardBudget,
    pub warnings: Vec<Warning>,
}

impl FillState {
    pub(super) fn add(
        &mut self,
        oracles: &[OracleCard],
        community: &CommunityData,
        plan: &FillPlan,
        idx: usize,
        forced: bool,
    ) -> bool {
        let card = &oracles[idx];
        let ctx = make_ctx(
            plan,
            community,
            &self.role_counts,
            &self.curve_counts,
            &self.pip_counts,
            &self.deck_names,
            &self.budget,
        );
        let scored = score_card(card, &ctx);
        if !forced && !scored.affordable {
            return false;
        }
        if !card.is_owned() && plan.owned_only {
            if let Some(p) = card.best_printing() {
                if !self.budget.spend(&p.rarity) {
                    if forced {
                        self.warnings.push(Warning {
                            code: "DECK-010".into(),
                            message: format!(
                                "must-include over wildcard budget: {}",
                                card.name
                            ),
                        });
                    } else {
                        return false;
                    }
                }
            }
        }
        self.used.insert(idx);
        let role = scored.role;
        self.chosen.push((idx, role, scored));
        *self.role_counts.entry(role).or_insert(0) += 1;
        if !card.is_land {
            self.curve_counts[cmc_bucket(card.cmc)] += 1;
            let p = card.pips();
            for i in 0..5 {
                self.pip_counts[i] += p[i];
            }
        }
        self.deck_names.insert(card.name_lower.clone());
        true
    }

    /// Undo an `add`: remove chosen[ci], decrement role/curve/pip counts,
    /// drop the name from deck_names, refund budget if the card was unowned
    /// under owned_only. Used by local search to try a swap and revert.
    pub(super) fn remove_at(&mut self, oracles: &[OracleCard], plan: &FillPlan, ci: usize) {
        let (idx, role, _) = self.chosen.remove(ci);
        let card = &oracles[idx];
        self.used.remove(&idx);
        if let Some(v) = self.role_counts.get_mut(&role) {
            *v = v.saturating_sub(1);
        }
        if !card.is_land {
            let b = cmc_bucket(card.cmc);
            self.curve_counts[b] = self.curve_counts[b].saturating_sub(1);
            let p = card.pips();
            for i in 0..5 {
                self.pip_counts[i] = self.pip_counts[i].saturating_sub(p[i]);
            }
        }
        self.deck_names.remove(&card.name_lower);
        if plan.owned_only && !card.is_owned() {
            if let Some(p) = card.best_printing() {
                self.budget.add(&p.rarity);
            }
        }
    }
}

pub(super) fn all_roles_at_max(
    card: &OracleCard,
    template: &Template,
    role_counts: &HashMap<Role, u32>,
) -> bool {
    if card.roles.is_empty() {
        return false;
    }
    card.roles.iter().all(|r| {
        let mx = template.max(*r);
        role_counts.get(r).copied().unwrap_or(0) >= mx
    })
}

fn nonbasic_land_bonus(card: &OracleCard, identity: u8) -> f32 {
    let text = card.oracle_text.to_lowercase();
    if text.contains("any color") {
        return 0.2 * mask_to_letters(identity).len() as f32;
    }
    let mut n = 0u32;
    for c in mask_to_letters(identity) {
        let needle = format!("add {{{}}}", c.to_ascii_lowercase());
        if text.contains(&needle) {
            n += 1;
        }
    }
    0.2 * n as f32
}

/// Sections 3–7: must_include, seed_order, greedy nonland, nonbasic lands,
/// basics. Returns the filled state, the basic-land slots, the number of
/// basics originally allocated (so `fill` can recompute basics after local
/// search shifts pip counts), and the budget reserved for the upgrade pass
/// (zero under BestDeck; the remaining wildcard budget under OwnedOnly).
pub(super) fn fill_core(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
) -> (FillState, Vec<(usize, u32)>, u32, WildcardBudget) {
    let mut st = FillState {
        chosen: Vec::new(),
        used: HashSet::new(),
        role_counts: HashMap::new(),
        curve_counts: [0u32; 8],
        pip_counts: [0u32; 5],
        deck_names: HashSet::new(),
        budget: plan.budget,
        warnings: Vec::new(),
    };

    let nonland_target = plan.template.nonland();
    let mut nonland_count: u32 = 0;
    // Nonbasic lands added via must_include consume from the same land budget as
    // sections 6/7 below — track so we don't overshoot the deck size.
    let mut must_include_nonbasic_lands: u32 = 0;

    // 3. must_include (forced)
    for &idx in &plan.must_include {
        let card = &oracles[idx];
        // Basics are owned by section 7 (compute_basics) — pinning one here
        // would double-add it (once in chosen with qty=1, once in basics with
        // qty=N) and overshoot deck size. Silently ignore with a warning.
        if card.is_basic_land {
            st.warnings.push(Warning {
                code: "DECK-010".into(),
                message: format!(
                    "basic land pinned in must_include ignored (auto-computed): {}",
                    card.name
                ),
            });
            continue;
        }
        let counts_as_nonland = !card.is_land;
        if counts_as_nonland && nonland_count >= nonland_target {
            break;
        }
        if st.add(oracles, community, plan, idx, true) {
            if counts_as_nonland {
                nonland_count += 1;
            } else {
                must_include_nonbasic_lands += 1;
            }
        }
    }

    // Owned-first skeleton: under OwnedOnly, reserve the remaining budget for
    // the upgrade pass (upgrade.rs). Sections 4-6 then build from owned cards
    // only — with st.budget zeroed, compute_ownership marks every unowned
    // card unaffordable. must_include above still spends real wildcards.
    let reserved = if plan.owned_only {
        std::mem::take(&mut st.budget)
    } else {
        WildcardBudget::default()
    };

    // 4. seed_order (not forced)
    let pool_set: HashSet<usize> = plan.pool.iter().copied().collect();
    for &idx in &plan.seed_order {
        if st.used.contains(&idx) {
            continue;
        }
        if !pool_set.contains(&idx) {
            continue;
        }
        let card = &oracles[idx];
        if card.is_land {
            continue;
        }
        if nonland_count >= nonland_target {
            break;
        }
        if st.add(oracles, community, plan, idx, false) {
            nonland_count += 1;
        }
    }

    // 5. Greedy nonland
    while nonland_count < nonland_target {
        let mut best_idx: Option<usize> = None;
        let mut best_total = f32::NEG_INFINITY;
        let mut best_name = String::new();
        for &idx in &plan.pool {
            if st.used.contains(&idx) {
                continue;
            }
            let card = &oracles[idx];
            if card.is_land {
                continue;
            }
            if all_roles_at_max(card, plan.template, &st.role_counts) {
                continue;
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
            let scored = score_card(card, &ctx);
            if !scored.affordable {
                continue;
            }
            let take = scored.total > best_total
                || (scored.total == best_total && card.name < best_name);
            if take {
                best_total = scored.total;
                best_name = card.name.clone();
                best_idx = Some(idx);
            }
        }
        match best_idx {
            Some(idx) => {
                if st.add(oracles, community, plan, idx, false) {
                    nonland_count += 1;
                } else {
                    break;
                }
            }
            None => {
                st.warnings.push(Warning {
                    code: "DECK-010".into(),
                    message: format!(
                        "could not fill {} nonland slots",
                        nonland_target - nonland_count
                    ),
                });
                break;
            }
        }
    }

    // 6. Nonbasic lands
    let lands_target = plan.template.lands;
    // Whatever must_include already placed counts against the caps below.
    let nonbasic_cap = plan
        .template
        .nonbasic_land_cap
        .saturating_sub(must_include_nonbasic_lands);
    let lands_target_remaining = lands_target.saturating_sub(must_include_nonbasic_lands);

    let mut land_cands: Vec<(usize, f32)> = Vec::new();
    for &idx in &plan.pool {
        if st.used.contains(&idx) {
            continue;
        }
        let card = &oracles[idx];
        if !card.is_land || card.is_basic_land {
            continue;
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
        let scored = score_card(card, &ctx);
        if plan.owned_only && !scored.affordable {
            continue;
        }
        let bonus = nonbasic_land_bonus(card, plan.identity);
        land_cands.push((idx, scored.total + bonus));
    }
    land_cands.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| oracles[a.0].name.cmp(&oracles[b.0].name))
    });

    let mut nonbasic_added: u32 = 0;
    for (idx, _) in land_cands {
        if nonbasic_added >= nonbasic_cap {
            break;
        }
        if nonbasic_added >= lands_target_remaining {
            break;
        }
        if st.add(oracles, community, plan, idx, false) {
            nonbasic_added += 1;
        }
    }

    // 7. Basics — fill the remainder of the land quota, subtracting both
    // must_include nonbasic lands and section-6 additions.
    let basics_needed = lands_target
        .saturating_sub(nonbasic_added)
        .saturating_sub(must_include_nonbasic_lands);
    let (basics, basic_warnings) =
        super::lands::compute_basics(oracles, plan.identity, &st.chosen, basics_needed);
    st.warnings.extend(basic_warnings);

    (st, basics, basics_needed, reserved)
}

pub fn fill(oracles: &[OracleCard], community: &CommunityData, plan: &FillPlan) -> FillOutcome {
    let (mut st, basics, basics_needed, reserved) = fill_core(oracles, community, plan);
    let prescore = super::local_search::static_prescore(oracles, community, plan);
    super::local_search::improve(oracles, community, plan, &mut st, &prescore);
    if plan.owned_only {
        super::upgrade::run(oracles, community, plan, &mut st, reserved);
    }
    // Recompute basics after local search — pip changes may have shifted the
    // colour balance, so a new basic-land breakdown better matches the deck.
    let (basics, basic_warnings) = if basics_needed > 0 {
        super::lands::compute_basics(oracles, plan.identity, &st.chosen, basics_needed)
    } else {
        (basics, Vec::new())
    };
    st.warnings.extend(basic_warnings);
    FillOutcome {
        chosen: st.chosen,
        basics,
        warnings: st.warnings,
        budget_left: st.budget,
    }
}

/// Test-only helper: run sections 3–7 without local search and return the
/// bare state plus the static pre-score. Lets `local_search` tests compare
/// deck objective before / after `improve`.
#[cfg(test)]
pub(super) fn fill_state_without_search(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
) -> (FillState, Vec<(usize, f32)>) {
    let (st, _basics, _needed, _reserved) = fill_core(oracles, community, plan);
    let prescore = super::local_search::static_prescore(oracles, community, plan);
    (st, prescore)
}

pub fn alternatives_for(
    oracles: &[OracleCard],
    community: &CommunityData,
    plan: &FillPlan,
    chosen: &[(usize, Role, Scored)],
    slot_idx: usize,
    n: usize,
) -> Vec<Alternative> {
    let (removed_idx, slot_role, _) = &chosen[slot_idx];
    let removed_card = &oracles[*removed_idx];
    let is_land_slot = removed_card.is_land;

    let mut rc: HashMap<Role, u32> = HashMap::new();
    let mut cc = [0u32; 8];
    let mut pc = [0u32; 5];
    let mut dn: HashSet<String> = HashSet::new();
    for (i, (cidx, role, _)) in chosen.iter().enumerate() {
        if i == slot_idx {
            continue;
        }
        *rc.entry(*role).or_insert(0) += 1;
        let card = &oracles[*cidx];
        if !card.is_land {
            cc[cmc_bucket(card.cmc)] += 1;
            let p = card.pips();
            for i in 0..5 {
                pc[i] += p[i];
            }
        }
        dn.insert(card.name_lower.clone());
    }

    let mut bg = plan.budget;
    if plan.owned_only {
        for (i, (cidx, _, _)) in chosen.iter().enumerate() {
            if i == slot_idx {
                continue;
            }
            let card = &oracles[*cidx];
            if !card.is_owned() {
                if let Some(p) = card.best_printing() {
                    let _ = bg.spend(&p.rarity);
                }
            }
        }
    }

    let used: HashSet<usize> = chosen
        .iter()
        .enumerate()
        .filter_map(|(i, (idx, _, _))| if i == slot_idx { None } else { Some(*idx) })
        .collect();

    let mut cands: Vec<(usize, Scored)> = Vec::new();
    for &pidx in &plan.pool {
        if pidx == *removed_idx || used.contains(&pidx) {
            continue;
        }
        let card = &oracles[pidx];
        if card.is_land != is_land_slot {
            continue;
        }
        if !card.has_role(*slot_role) {
            continue;
        }
        let ctx = make_ctx(plan, community, &rc, &cc, &pc, &dn, &bg);
        let s = score_card(card, &ctx);
        if !s.affordable {
            continue;
        }
        cands.push((pidx, s));
    }
    cands.sort_by(|a, b| {
        b.1.total
            .partial_cmp(&a.1.total)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| oracles[a.0].name.cmp(&oracles[b.0].name))
    });

    cands
        .into_iter()
        .take(n)
        .map(|(idx, s)| {
            let card = &oracles[idx];
            let bp = card.best_printing();
            Alternative {
                grp_id: bp.map(|p| p.grp_id).unwrap_or(0),
                name: card.name.clone(),
                owned: card.is_owned(),
                wildcard_rarity: if card.is_owned() {
                    None
                } else {
                    bp.map(|p| p.rarity.clone())
                },
                score: s.total,
                reasons: s.reasons,
            }
        })
        .collect()
}
