#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod score_tests;
#[cfg(test)]
pub(crate) mod test_fixtures;

pub mod commanders;
pub mod fill;
pub mod lands;
pub mod local_search;
pub mod pool;
pub mod roles;
pub mod score;
pub mod template;
pub mod types;

pub use types::*;

use std::collections::{HashMap, HashSet};

use fill::{alternatives_for, fill, FillPlan};
use lands::compute_stats;
use pool::{candidate_pool, effective_identity, index_by_grp, PoolCtx};
use template::template_for;

#[derive(Debug)]
pub enum BuildError {
    /// DECK-009
    InvalidRequest(String),
    /// DECK-004
    NoData(String),
}

impl BuildError {
    pub fn code(&self) -> &'static str {
        match self {
            BuildError::InvalidRequest(_) => "DECK-009",
            BuildError::NoData(_) => "DECK-004",
        }
    }
    pub fn message(&self) -> String {
        match self {
            BuildError::InvalidRequest(s) => s.clone(),
            BuildError::NoData(s) => s.clone(),
        }
    }
}

/// Validate the commander + color subset, and produce the format template.
/// Extracted so tests can construct a `FillPlan` without running `build()`.
pub(crate) fn resolve_template(
    input: &EngineInput,
    req: &BuildRequest,
) -> Result<template::Template, BuildError> {
    let oracles = &input.oracles;
    let grp_index = index_by_grp(oracles);

    let commander_grp = req
        .commander_grp
        .ok_or_else(|| BuildError::InvalidRequest("DECK-009: commander_grp required".into()))?;
    let commander_idx = *grp_index.get(&commander_grp).ok_or_else(|| {
        BuildError::InvalidRequest(format!("DECK-009: commander not found (grp {commander_grp})"))
    })?;
    let commander = &oracles[commander_idx];
    if !commander.is_commander_eligible {
        return Err(BuildError::InvalidRequest(format!(
            "DECK-009: commander not eligible: {}",
            commander.name
        )));
    }
    if !commander.legal_in(req.format) {
        return Err(BuildError::InvalidRequest(format!(
            "DECK-009: commander not legal in format: {}",
            commander.name
        )));
    }

    let identity = effective_identity(commander.color_identity, req.color_subset.as_deref())
        .map_err(BuildError::InvalidRequest)?;

    Ok(template_for(
        req.format,
        req.playstyle,
        identity,
        req.template_overrides.as_ref(),
    ))
}

/// Build a `FillPlan` from an input + request against a caller-owned
/// template. `build()` calls this after `resolve_template`; tests use it to
/// exercise `fill_core` / `local_search::improve` directly.
pub(crate) fn make_plan<'a>(
    input: &'a EngineInput,
    req: &BuildRequest,
    template: &'a template::Template,
) -> Result<FillPlan<'a>, BuildError> {
    let oracles = &input.oracles;
    let grp_index = index_by_grp(oracles);

    let commander_grp = req
        .commander_grp
        .ok_or_else(|| BuildError::InvalidRequest("DECK-009: commander_grp required".into()))?;
    let commander_idx = *grp_index.get(&commander_grp).ok_or_else(|| {
        BuildError::InvalidRequest(format!("DECK-009: commander not found (grp {commander_grp})"))
    })?;
    let commander = &oracles[commander_idx];
    let identity = effective_identity(commander.color_identity, req.color_subset.as_deref())
        .map_err(BuildError::InvalidRequest)?;

    let exclude_grp: HashSet<i32> = req.exclude.iter().copied().collect();
    let mut exclude_oracle: HashSet<String> = HashSet::new();
    exclude_oracle.insert(commander.oracle_id.clone());

    let pool = candidate_pool(
        oracles,
        &PoolCtx {
            format: req.format,
            identity,
            commander_oracle: &commander.oracle_id,
            exclude_grp: &exclude_grp,
            exclude_oracle: &exclude_oracle,
        },
    );

    let mut must_include: Vec<usize> = Vec::new();
    for &g in &req.must_include {
        let idx = *grp_index.get(&g).ok_or_else(|| {
            BuildError::InvalidRequest(format!("DECK-009: must_include grp not found: {g}"))
        })?;
        let c = &oracles[idx];
        if !is_subset(c.color_identity, identity) {
            return Err(BuildError::InvalidRequest(format!(
                "DECK-009: must_include outside identity: {}",
                c.name
            )));
        }
        if !c.legal_in(req.format) {
            return Err(BuildError::InvalidRequest(format!(
                "DECK-009: must_include not legal: {}",
                c.name
            )));
        }
        must_include.push(idx);
    }

    let mut build_around: HashMap<String, f32> = HashMap::new();
    let mut theme: HashSet<String> = commander.tags.iter().cloned().collect();
    for ba in &req.build_around {
        let idx = *grp_index.get(&ba.grp_id).ok_or_else(|| {
            BuildError::InvalidRequest(format!(
                "DECK-009: build_around grp not found: {}",
                ba.grp_id
            ))
        })?;
        let c = &oracles[idx];
        if !is_subset(c.color_identity, identity) {
            return Err(BuildError::InvalidRequest(format!(
                "DECK-009: build_around outside identity: {}",
                c.name
            )));
        }
        if !c.legal_in(req.format) {
            return Err(BuildError::InvalidRequest(format!(
                "DECK-009: build_around not legal: {}",
                c.name
            )));
        }
        build_around.insert(c.oracle_id.clone(), ba.weight);
        for t in &c.tags {
            theme.insert(t.clone());
        }
    }
    for t in &req.theme_tags {
        theme.insert(t.clone());
    }

    let commander_subtypes = commander.subtypes.clone();
    let commander_subtypes_lower: Vec<String> = commander_subtypes
        .iter()
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let name_lower_idx: HashMap<String, usize> = oracles
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name_lower.clone(), i))
        .collect();
    let mut seed_order: Vec<usize> = Vec::new();
    for name in &input.community.seed_names {
        if let Some(&idx) = name_lower_idx.get(name) {
            seed_order.push(idx);
        }
    }

    let (owned_only, budget) = match &req.ownership {
        OwnershipMode::OwnedOnly { wc_budget } => {
            let mut b = *wc_budget;
            b.common = b.common.min(input.wildcards_owned.common);
            b.uncommon = b.uncommon.min(input.wildcards_owned.uncommon);
            b.rare = b.rare.min(input.wildcards_owned.rare);
            b.mythic = b.mythic.min(input.wildcards_owned.mythic);
            (true, b)
        }
        OwnershipMode::BestDeck => (
            false,
            WildcardBudget {
                common: u32::MAX,
                uncommon: u32::MAX,
                rare: u32::MAX,
                mythic: u32::MAX,
            },
        ),
    };

    // Perf caches (see the score.rs / fill.rs docs).
    let mut card_tokens: HashMap<String, HashSet<String>> = HashMap::with_capacity(pool.len() + 8);
    let push_tokens = |idx: usize, m: &mut HashMap<String, HashSet<String>>| {
        let c = &oracles[idx];
        m.entry(c.oracle_id.clone())
            .or_insert_with(|| score::tokenize(&c.type_line, &c.oracle_text));
    };
    for &pidx in &pool {
        push_tokens(pidx, &mut card_tokens);
    }
    push_tokens(commander_idx, &mut card_tokens);
    for &idx in &must_include {
        push_tokens(idx, &mut card_tokens);
    }
    for &idx in &seed_order {
        push_tokens(idx, &mut card_tokens);
    }

    let mut df: HashMap<String, u32> = HashMap::new();
    for &pidx in &pool {
        for t in &oracles[pidx].tags {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let n = pool.len() as f32;
    let tag_idf: HashMap<String, f32> = df
        .into_iter()
        .map(|(t, d)| (t, ((n + 1.0) / (d as f32 + 1.0)).ln().max(0.0)))
        .collect();

    Ok(FillPlan {
        template,
        identity,
        commander_idx,
        pool,
        must_include,
        seed_order,
        build_around,
        theme,
        commander_subtypes,
        commander_subtypes_lower,
        card_tokens,
        tag_idf,
        owned_only,
        budget,
    })
}

pub fn build(input: &EngineInput, req: &BuildRequest) -> Result<BuildResult, BuildError> {
    let oracles = &input.oracles;
    let grp_index = index_by_grp(oracles);

    let template = resolve_template(input, req)?;
    let plan = make_plan(input, req, &template)?;

    // build() still owns the "seed missing from Arena" warning surface —
    // recompute seed_warnings here (make_plan silently drops missing seeds).
    let commander_grp = req.commander_grp.expect("checked by resolve_template");
    let commander_idx = *grp_index.get(&commander_grp).expect("checked");
    let commander = &oracles[commander_idx];
    let name_lower_idx: HashMap<String, usize> = oracles
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name_lower.clone(), i))
        .collect();
    let mut seed_warnings: Vec<Warning> = Vec::new();
    for name in &input.community.seed_names {
        if !name_lower_idx.contains_key(name) {
            seed_warnings.push(Warning {
                code: "DECK-010".into(),
                message: format!("seed card not on Arena: {name}"),
            });
        }
    }

    let outcome = fill(oracles, &input.community, &plan);
    let stats = compute_stats(oracles, &template, &outcome.chosen, &outcome.basics);

    let anchored_set: HashSet<usize> = plan.must_include.iter().copied().collect();

    let mut slots: Vec<Slot> = Vec::new();
    for (i, (idx, role, scored)) in outcome.chosen.iter().enumerate() {
        let card = &oracles[*idx];
        let bp = card.best_printing();
        let alts = if card.is_land {
            Vec::new()
        } else {
            alternatives_for(oracles, &input.community, &plan, &outcome.chosen, i, 5)
        };
        slots.push(Slot {
            grp_id: bp.map(|p| p.grp_id).unwrap_or(0),
            oracle_id: card.oracle_id.clone(),
            name: card.name.clone(),
            role: *role,
            cmc: card.cmc,
            owned_qty: bp.map(|p| p.owned_qty).unwrap_or(0),
            needed_qty: 1,
            wildcard_rarity: if card.is_owned() {
                None
            } else {
                bp.map(|p| p.rarity.clone())
            },
            score: scored.total,
            reasons: scored.reasons.clone(),
            anchored: anchored_set.contains(idx),
            alternatives: alts,
        });
    }
    for (bidx, qty) in &outcome.basics {
        let card = &oracles[*bidx];
        let bp = card.best_printing();
        slots.push(Slot {
            grp_id: bp.map(|p| p.grp_id).unwrap_or(0),
            oracle_id: card.oracle_id.clone(),
            name: card.name.clone(),
            role: Role::Land,
            cmc: card.cmc,
            owned_qty: *qty as i32,
            needed_qty: *qty as i32,
            wildcard_rarity: None,
            score: 0.0,
            reasons: vec!["Basic land".into()],
            anchored: false,
            alternatives: Vec::new(),
        });
    }
    slots.sort_by(|a, b| {
        let ra = Role::ALL.iter().position(|r| *r == a.role).unwrap_or(0);
        let rb = Role::ALL.iter().position(|r| *r == b.role).unwrap_or(0);
        ra.cmp(&rb)
            .then_with(|| {
                a.cmc
                    .partial_cmp(&b.cmc)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.name.cmp(&b.name))
    });

    let cmdr_bp = commander.best_printing();
    let commander_slot = Slot {
        grp_id: cmdr_bp.map(|p| p.grp_id).unwrap_or(0),
        oracle_id: commander.oracle_id.clone(),
        name: commander.name.clone(),
        role: Role::Threat,
        cmc: commander.cmc,
        owned_qty: cmdr_bp.map(|p| p.owned_qty).unwrap_or(0),
        needed_qty: 1,
        wildcard_rarity: if commander.is_owned() {
            None
        } else {
            cmdr_bp.map(|p| p.rarity.clone())
        },
        score: 0.0,
        reasons: vec!["Commander".into()],
        anchored: true,
        alternatives: Vec::new(),
    };

    let mut warnings = outcome.warnings;
    warnings.extend(seed_warnings);

    Ok(BuildResult {
        format: req.format,
        commander: commander_slot,
        slots,
        stats,
        warnings,
    })
}

