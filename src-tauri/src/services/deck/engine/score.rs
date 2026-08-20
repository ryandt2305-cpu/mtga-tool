use std::collections::{HashMap, HashSet};

use super::template::Template;
use super::types::{mask_to_letters, CommunityData, OracleCard, Role, WildcardBudget};
use crate::models::Rarity;

pub struct ScoreCtx<'a> {
    pub template: &'a Template,
    pub role_counts: &'a HashMap<Role, u32>,
    pub curve_counts: &'a [u32; 8],
    pub theme: &'a HashSet<String>,
    /// Commander subtypes — may be mixed case; scoring lowercases on demand.
    /// Prefer supplying `commander_subtypes_lower` in hot paths (fill/build).
    pub commander_subtypes: &'a [String],
    pub build_around_bonus: &'a HashMap<String, f32>,
    pub community: &'a CommunityData,
    pub deck_names: &'a HashSet<String>,
    pub owned_only: bool,
    pub budget: &'a WildcardBudget,
    /// Perf caches populated by `fill()`; None in test/one-off callers.
    /// When set, avoids per-candidate `to_lowercase` allocations that
    /// dominated build time on real-size pools.
    pub commander_subtypes_lower: Option<&'a [String]>,
    pub card_tokens: Option<&'a HashMap<String, HashSet<String>>>,
    pub tag_idf: Option<&'a HashMap<String, f32>>,
    /// Running WUBRG pip totals of nonland spells already in the deck.
    /// Empty (`[0;5]`) at deck start; consumed by `compute_pip_fit`.
    pub pip_counts: &'a [u32; 5],
    /// Colour identity mask (WUBRG) the deck can produce mana for.
    pub identity: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    pub total: f32,
    pub role: Role,
    pub role_need: f32,
    pub synergy: f32,
    pub power: f32,
    pub ownership: f32,
    pub curve_fit: f32,
    pub pip_fit: f32,
    pub reasons: Vec<String>,
    pub affordable: bool,
}

pub fn cmc_bucket(cmc: f32) -> usize {
    let b = cmc.max(0.0).floor() as usize;
    b.min(7)
}

/// EDHREC rank → 0..1. 500 → 1.0, 3k → 0.56, 10k → 0.27, ≥30k → 0.
/// Generic paper-EDH popularity; in `compute_power` it is scaled by
/// `RANK_ONLY_CAP` so it can never outrank a commander-specific signal.
pub fn rank_curve(rank: u32) -> f32 {
    let r = (rank.max(1)) as f32;
    clamp01(1.0 - (r / 500.0).ln() / (60.0f32).ln())
}

/// Ceiling for power derived from `edhrec_rank` alone (no community signal).
/// Calibrated 2026-08-19 against live EDHREC pages: the median on-page card
/// (15 % inclusion → `share_norm` 0.67) must beat every rank-only card, while
/// elite generic cards (rank ≤ 1000 → ≤ 0.50) still beat the 1 %-inclusion
/// tail of a page (0.44).
pub const RANK_ONLY_CAP: f32 = 0.6;
/// Ceiling for 17Lands GIH win rate: a Limited quality proxy, not a Brawl
/// signal, so it stays below the on-page median.
pub const GIH_CAP: f32 = 0.6;
/// Added once per *additional* community signal present on a card
/// (inclusion / staple / seed / GIH agreeing with each other).
pub const AGREE_BONUS: f32 = 0.05;

/// Deck-share signal (EDHREC inclusion, Archidekt staple share, …) → power.
/// Log-shaped with a 0.40 floor: being on a commander's page at all puts a
/// card in roughly the top 2–4 % of its identity pool.
/// 0 → 0.40, 0.05 → 0.54, 0.15 → 0.67, 0.30 → 0.78, 0.50 → 0.87, 0.90 → 0.98, 1 → 1.0
pub fn share_norm(x: f32) -> f32 {
    clamp01(0.40 + 0.60 * (1.0 + 20.0 * x.max(0.0)).ln() / (21.0f32).ln())
}

/// EDHREC `synergy` (observed −0.3..0.7; High-Synergy section ≥ 0.3) → 0..1.
pub fn synergy_norm(s: f32) -> f32 {
    clamp01(s / 0.4)
}

/// 17Lands GIH win rate → 0..GIH_CAP. 50 % → 0, 57.5 % → 0.3, ≥ 65 % → 0.6.
pub fn gih_norm(wr: f32) -> f32 {
    GIH_CAP * clamp01((wr - 0.50) / 0.15)
}

/// Last-resort power when no community signal and no rank exist.
pub fn rarity_floor(r: &Rarity) -> f32 {
    match r {
        Rarity::Mythic => 0.15,
        Rarity::Rare => 0.12,
        Rarity::Uncommon => 0.08,
        _ => 0.05,
    }
}

fn rarity_word(r: &Rarity) -> &'static str {
    match r {
        Rarity::Common => "common",
        Rarity::Uncommon => "uncommon",
        Rarity::Rare => "rare",
        Rarity::Mythic => "mythic",
        Rarity::Special => "special",
        Rarity::BasicLand => "basic land",
        Rarity::Unknown(_) => "unknown",
    }
}

fn clamp01(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}

/// Word-boundary tokens from a card's type_line + oracle_text.
/// Lowercased, ASCII-alphanumeric splits. Adds naive plural/species
/// stems (elves→elf, faeries→faery, goblins→goblin) so subtype
/// matches like "Elves" find commanders declaring subtype "Elf".
pub fn tokenize(type_line: &str, oracle_text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let text = format!("{} {}", type_line, oracle_text).to_lowercase();
    for tok in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if tok.is_empty() {
            continue;
        }
        out.insert(tok.to_string());
        if tok.len() > 3 {
            if let Some(stem) = tok.strip_suffix("ves") {
                out.insert(format!("{stem}f"));
            }
            if let Some(stem) = tok.strip_suffix("ies") {
                out.insert(format!("{stem}y"));
            }
            if let Some(stem) = tok.strip_suffix('s') {
                out.insert(stem.to_string());
            }
        }
    }
    out
}

fn compute_role(card: &OracleCard, ctx: &ScoreCtx) -> (Role, f32) {
    if card.is_land {
        return (Role::Land, 0.0);
    }
    if card.roles.is_empty() {
        return (Role::Utility, 0.0);
    }

    let mut best: Option<(Role, f32)> = None;
    let mut all_at_or_over_max = true;
    for r in Role::ALL {
        if r == Role::Land || !card.roles.contains(&r) {
            continue;
        }
        let min = ctx.template.min(r) as f32;
        let max = ctx.template.max(r);
        let count = ctx.role_counts.get(&r).copied().unwrap_or(0);
        if count < max {
            all_at_or_over_max = false;
        }
        let need = ((min - count as f32).max(0.0)) / min.max(1.0);
        match best {
            None => best = Some((r, need)),
            Some((_, cur)) => {
                if need > cur {
                    best = Some((r, need));
                }
            }
        }
    }

    if all_at_or_over_max {
        let first = Role::ALL
            .iter()
            .copied()
            .find(|r| card.roles.contains(r))
            .unwrap_or(Role::Utility);
        return (first, 0.0);
    }

    best.unwrap_or((Role::Utility, 0.0))
}

fn compute_synergy(card: &OracleCard, ctx: &ScoreCtx) -> f32 {
    // Theme score: IDF-weighted overlap when idf is available, else Jaccard over theme.
    let mut theme_score = 0.0f32;
    if !ctx.theme.is_empty() {
        match ctx.tag_idf {
            Some(idf) => {
                let denom: f32 = ctx
                    .theme
                    .iter()
                    .map(|t| idf.get(t).copied().unwrap_or(1.0))
                    .sum();
                let num: f32 = card
                    .tags
                    .iter()
                    .filter(|t| ctx.theme.contains(*t))
                    .map(|t| idf.get(t).copied().unwrap_or(1.0))
                    .sum();
                if denom > 0.0 {
                    theme_score = clamp01(num / denom);
                }
            }
            None => {
                let hits = card.tags.iter().filter(|t| ctx.theme.contains(*t)).count();
                theme_score = hits as f32 / ctx.theme.len() as f32;
            }
        }
    }

    // Subtype: word-boundary token match. Use per-fill cache if present.
    let mut subtype_hit = 0.0f32;
    if !ctx.commander_subtypes.is_empty() {
        let owned_tokens;
        let tokens: &HashSet<String> = match ctx.card_tokens.and_then(|m| m.get(&card.oracle_id)) {
            Some(t) => t,
            None => {
                owned_tokens = tokenize(&card.type_line, &card.oracle_text);
                &owned_tokens
            }
        };
        let hit = match ctx.commander_subtypes_lower {
            Some(lowered) => lowered.iter().any(|s| !s.is_empty() && tokens.contains(s)),
            None => ctx
                .commander_subtypes
                .iter()
                .map(|s| s.to_lowercase())
                .any(|s| !s.is_empty() && tokens.contains(&s)),
        };
        if hit {
            subtype_hit = 1.0;
        }
    }

    // EDHREC synergy normalised to 0..1 (0.4 saturates) so commander-specific
    // synergy carries real weight; negative synergy (generic staples) → 0.
    let community_synergy = ctx
        .community
        .by_name
        .get(&card.name_lower)
        .and_then(|s| s.synergy)
        .map(synergy_norm)
        .unwrap_or(0.0);

    // O(1) lookup via combos_by_name; falls back to full scan only if the
    // index wasn't populated (test fixtures).
    let mut combo_hit = 0.0f32;
    if !ctx.community.combos_by_name.is_empty() || ctx.community.combos.is_empty() {
        if let Some(indices) = ctx.community.combos_by_name.get(&card.name_lower) {
            for &ci in indices {
                let combo = &ctx.community.combos[ci];
                if combo
                    .iter()
                    .any(|n| n != &card.name_lower && ctx.deck_names.contains(n))
                {
                    combo_hit = 1.0;
                    break;
                }
            }
        }
    } else {
        for combo in &ctx.community.combos {
            if combo.iter().any(|n| n == &card.name_lower)
                && combo.iter().any(|n| n != &card.name_lower && ctx.deck_names.contains(n))
            {
                combo_hit = 1.0;
                break;
            }
        }
    }

    let build_around = ctx
        .build_around_bonus
        .get(&card.oracle_id)
        .copied()
        .unwrap_or(0.0);

    clamp01(
        0.4 * theme_score
            + 0.3 * subtype_hit
            + 0.6 * community_synergy
            + 0.5 * combo_hit
            + build_around,
    )
}

/// Power = max over the normalised signals that exist for the card, plus a
/// small bonus when several community signals agree. Every signal is mapped
/// onto one comparable scale first (see `share_norm` / `gih_norm` /
/// `RANK_ONLY_CAP`) — the pre-2026-08-19 version returned raw shares (median
/// on-page EDHREC inclusion ≈ 0.15) next to an uncapped rank curve (rank 1000
/// → 0.83), which inverted the ranking for most commander-specific cards.
fn compute_power(card: &OracleCard, ctx: &ScoreCtx) -> f32 {
    let mut best: Option<f32> = None;
    let mut community_signals: u32 = 0;
    let mut upd = |v: f32, counts: bool| {
        best = Some(best.map_or(v, |b: f32| b.max(v)));
        if counts {
            community_signals += 1;
        }
    };
    if let Some(sig) = ctx.community.by_name.get(&card.name_lower) {
        if let Some(v) = sig.staple {
            upd(share_norm(v), true);
        }
        if let Some(v) = sig.inclusion {
            upd(share_norm(v), true);
        }
        if let Some(v) = sig.seed_freq {
            upd(clamp01(v), true);
        }
        if let Some(v) = sig.gih_wr {
            upd(gih_norm(v), true);
        }
    }
    if let Some(rank) = card.edhrec_rank {
        upd(RANK_ONLY_CAP * rank_curve(rank), false);
    }
    match best {
        Some(p) => clamp01(p + AGREE_BONUS * community_signals.saturating_sub(1) as f32),
        None => match card.best_printing() {
            Some(p) => rarity_floor(&p.rarity),
            None => 0.0,
        },
    }
}

fn compute_ownership(card: &OracleCard, ctx: &ScoreCtx) -> (f32, bool) {
    if card.is_owned() {
        return (1.0, true);
    }
    if !ctx.owned_only {
        return (0.0, true);
    }
    let rarity = match card.best_printing() {
        Some(p) => p.rarity.clone(),
        None => return (-10.0, false),
    };
    if ctx.budget.get(&rarity) == 0 {
        return (-10.0, false);
    }
    let cost = match rarity {
        Rarity::Common => -0.1,
        Rarity::Uncommon => -0.2,
        Rarity::Rare => -0.6,
        Rarity::Mythic => -0.9,
        _ => 0.0,
    };
    (cost, true)
}

fn compute_curve_fit(card: &OracleCard, ctx: &ScoreCtx) -> f32 {
    if card.is_land {
        return 1.0;
    }
    let b = cmc_bucket(card.cmc);
    let target = ctx.template.curve[b] * ctx.template.nonland() as f32;
    let count = ctx.curve_counts[b] as f32;
    if count < target {
        1.0
    } else {
        (1.0 - (count - target) / target.max(1.0)).max(0.0)
    }
}

/// Penalise a card whose colour pips exceed the deck's current source share
/// in that colour by more than a tolerance. Lands and colourless spells are
/// always a fit. When the deck is empty, the "source share" is uniform over
/// identity colours (1/n).
pub fn compute_pip_fit(card: &OracleCard, pip_counts: &[u32; 5], identity: u8) -> f32 {
    if card.is_land {
        return 1.0;
    }
    let p = card.pips();
    let total: u32 = p.iter().sum();
    if total == 0 {
        return 1.0;
    }
    let deck_total: u32 = pip_counts.iter().sum();
    let letters = mask_to_letters(identity);
    let n_colours = letters.len().max(1) as f32;
    let in_identity =
        |i: usize| letters.contains(&['W', 'U', 'B', 'R', 'G'][i]);
    let mut penalty = 0.0f32;
    for i in 0..5 {
        if p[i] == 0 {
            continue;
        }
        let card_share = p[i] as f32 / total as f32;
        let source_share = if deck_total > 0 {
            pip_counts[i] as f32 / deck_total as f32
        } else if in_identity(i) {
            1.0 / n_colours
        } else {
            0.0
        };
        penalty += (card_share - source_share - 0.15).max(0.0);
    }
    clamp01(1.0 - penalty)
}

pub fn score_card(card: &OracleCard, ctx: &ScoreCtx) -> Scored {
    let (role, role_need) = compute_role(card, ctx);
    let synergy = compute_synergy(card, ctx);
    let power = compute_power(card, ctx);
    let (ownership, affordable) = compute_ownership(card, ctx);
    let curve_fit = compute_curve_fit(card, ctx);
    let pip_fit = compute_pip_fit(card, ctx.pip_counts, ctx.identity);

    let w = &ctx.template.weights;
    let total = w.role * role_need
        + w.synergy * synergy
        + w.power * power
        + w.ownership * ownership
        + w.curve * curve_fit
        + w.pip * pip_fit;

    let mut reasons: Vec<String> = Vec::new();
    reasons.push(role.label().to_string());

    let best = card.best_printing();
    if card.is_owned() {
        reasons.push("owned".to_string());
    } else if ctx.owned_only {
        if let Some(p) = best {
            reasons.push(format!("needs {} wildcard", rarity_word(&p.rarity)));
        }
    }

    reasons.push("in identity".to_string());

    if let Some(sig) = ctx.community.by_name.get(&card.name_lower) {
        if let Some(inc) = sig.inclusion {
            reasons.push(format!("EDHREC {:.0}%", inc * 100.0));
        }
        if let Some(st) = sig.staple {
            if st >= 0.2 {
                reasons.push(format!("staple {:.0}%", st * 100.0));
            }
        }
        if let Some(wr) = sig.gih_wr {
            reasons.push(format!("GIH {:.0}%", wr * 100.0));
        }
    }

    let mut shared_count = 0;
    for t in &card.tags {
        if shared_count >= 2 {
            break;
        }
        if ctx.theme.contains(t) {
            reasons.push(format!("synergy: {}", t));
            shared_count += 1;
        }
    }

    // O(1) via combos_by_name; only iterate combos actually containing this
    // card. Full-scan fallback stays for callers that skip the index.
    if !ctx.community.combos_by_name.is_empty() || ctx.community.combos.is_empty() {
        if let Some(indices) = ctx.community.combos_by_name.get(&card.name_lower) {
            'outer: for &ci in indices {
                let combo = &ctx.community.combos[ci];
                for other in combo {
                    if other != &card.name_lower && ctx.deck_names.contains(other) {
                        reasons.push(format!("combo with {}", other));
                        break 'outer;
                    }
                }
            }
        }
    } else {
        'outer: for combo in &ctx.community.combos {
            if combo.iter().any(|n| n == &card.name_lower) {
                for other in combo {
                    if other != &card.name_lower && ctx.deck_names.contains(other) {
                        reasons.push(format!("combo with {}", other));
                        break 'outer;
                    }
                }
            }
        }
    }

    if ctx.build_around_bonus.contains_key(&card.oracle_id) {
        reasons.push("build-around".to_string());
    }

    Scored {
        total,
        role,
        role_need,
        synergy,
        power,
        ownership,
        curve_fit,
        pip_fit,
        reasons,
        affordable,
    }
}

