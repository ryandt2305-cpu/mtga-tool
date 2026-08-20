use std::collections::HashMap;

use super::score::{cmc_bucket, Scored};
use super::template::Template;
use super::types::*;

pub fn find_basic(oracles: &[OracleCard], name: &str) -> Option<usize> {
    oracles
        .iter()
        .position(|c| c.is_basic_land && c.name == name)
}

fn color_to_basic(c: char) -> &'static str {
    match c {
        'W' => "Plains",
        'U' => "Island",
        'B' => "Swamp",
        'R' => "Mountain",
        'G' => "Forest",
        _ => "",
    }
}

fn color_pip_index(c: char) -> usize {
    match c {
        'W' => 0,
        'U' => 1,
        'B' => 2,
        'R' => 3,
        'G' => 4,
        _ => 0,
    }
}

pub fn compute_basics(
    oracles: &[OracleCard],
    identity: u8,
    chosen: &[(usize, Role, Scored)],
    remaining: u32,
) -> (Vec<(usize, u32)>, Vec<Warning>) {
    let mut warnings: Vec<Warning> = Vec::new();
    let mut out: Vec<(usize, u32)> = Vec::new();

    if remaining == 0 {
        return (out, warnings);
    }

    if identity == 0 {
        match find_basic(oracles, "Wastes") {
            Some(idx) => out.push((idx, remaining)),
            None => warnings.push(Warning {
                code: "DECK-004".into(),
                message: "basic land 'Wastes' not in oracle set".into(),
            }),
        }
        return (out, warnings);
    }

    let letters = mask_to_letters(identity);
    let n_colors = letters.len() as u32;

    let mut pips_per_color = [0u32; 5];
    for (idx, _, _) in chosen {
        let card = &oracles[*idx];
        if card.is_land {
            continue;
        }
        let p = card.pips();
        for i in 0..5 {
            pips_per_color[i] += p[i];
        }
    }

    let ident_pips: Vec<(char, u32)> = letters
        .iter()
        .map(|&c| (c, pips_per_color[color_pip_index(c)]))
        .collect();
    let sum: u32 = ident_pips.iter().map(|(_, p)| *p).sum();

    let mut counts: Vec<(char, u32)>;
    if sum == 0 {
        let base = remaining / n_colors;
        let extra = remaining % n_colors;
        counts = ident_pips
            .iter()
            .enumerate()
            .map(|(i, &(c, _))| (c, base + if (i as u32) < extra { 1 } else { 0 }))
            .collect();
    } else {
        let floats: Vec<(char, f32)> = ident_pips
            .iter()
            .map(|&(c, p)| (c, remaining as f32 * (p as f32 / sum as f32)))
            .collect();
        let base: Vec<(char, u32)> = floats.iter().map(|&(c, f)| (c, f.floor() as u32)).collect();
        let used: u32 = base.iter().map(|(_, n)| *n).sum();
        let mut extra = remaining.saturating_sub(used);
        let mut rem: Vec<f32> = floats
            .iter()
            .zip(base.iter())
            .map(|(&(_, f), &(_, b))| f - b as f32)
            .collect();
        counts = base.clone();

        if remaining >= n_colors {
            for i in 0..counts.len() {
                if counts[i].1 == 0 && extra > 0 {
                    counts[i].1 = 1;
                    extra -= 1;
                    rem[i] = -1.0;
                }
            }
        }
        while extra > 0 {
            let mut best_i = 0usize;
            let mut best_r = f32::NEG_INFINITY;
            for (i, r) in rem.iter().enumerate() {
                if *r > best_r {
                    best_r = *r;
                    best_i = i;
                }
            }
            if best_r == f32::NEG_INFINITY {
                break;
            }
            counts[best_i].1 += 1;
            rem[best_i] = f32::NEG_INFINITY;
            extra -= 1;
        }
    }

    for (c, n) in counts {
        if n == 0 {
            continue;
        }
        let name = color_to_basic(c);
        if name.is_empty() {
            continue;
        }
        match find_basic(oracles, name) {
            Some(idx) => out.push((idx, n)),
            None => warnings.push(Warning {
                code: "DECK-004".into(),
                message: format!("basic land '{name}' not in oracle set"),
            }),
        }
    }

    (out, warnings)
}

pub fn compute_stats(
    oracles: &[OracleCard],
    template: &Template,
    chosen: &[(usize, Role, Scored)],
    basics: &[(usize, u32)],
) -> DeckStats {
    let mut role_counts: HashMap<Role, u32> = HashMap::new();
    let mut curve = [0u32; 8];
    let mut pips = [0u32; 5];
    let mut owned: u32 = 0;
    let mut total: u32 = 0;
    let mut wildcard_cost = WildcardBudget::default();
    let mut land_count: u32 = 0;

    for (idx, role, _) in chosen {
        let card = &oracles[*idx];
        *role_counts.entry(*role).or_insert(0) += 1;
        if !card.is_land {
            curve[cmc_bucket(card.cmc)] += 1;
        } else {
            land_count += 1;
        }
        let p = card.pips();
        for i in 0..5 {
            pips[i] += p[i];
        }
        total += 1;
        if card.is_owned() {
            owned += 1;
        } else if let Some(bp) = card.best_printing() {
            wildcard_cost.add(&bp.rarity);
        }
    }
    for (bidx, qty) in basics {
        *role_counts.entry(Role::Land).or_insert(0) += *qty;
        land_count += *qty;
        total += *qty;
        owned += *qty;
        let _ = bidx;
    }

    let mut role_targets: HashMap<Role, (u32, u32)> = HashMap::new();
    for r in Role::ALL {
        role_targets.insert(r, (template.min(r), template.max(r)));
    }

    DeckStats {
        curve,
        pips,
        role_counts,
        role_targets,
        owned,
        total,
        wildcard_cost,
        land_count,
    }
}
