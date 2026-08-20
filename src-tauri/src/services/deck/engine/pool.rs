use std::collections::{HashMap, HashSet};

use super::types::{is_subset, mask_from_str, Format, OracleCard};

pub struct PoolCtx<'a> {
    pub format: Format,
    pub identity: u8,
    pub commander_oracle: &'a str,
    pub exclude_grp: &'a HashSet<i32>,
    pub exclude_oracle: &'a HashSet<String>,
}

/// Indices into `oracles` that are legal, on-arena (have ≥1 printing), within identity,
/// not the commander, and not in either exclusion set.
pub fn candidate_pool(oracles: &[OracleCard], ctx: &PoolCtx) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, card) in oracles.iter().enumerate() {
        if card.printings.is_empty() {
            continue;
        }
        if !card.legal_in(ctx.format) {
            continue;
        }
        if !is_subset(card.color_identity, ctx.identity) {
            continue;
        }
        if !card.oracle_id.is_empty() && card.oracle_id == ctx.commander_oracle {
            continue;
        }
        if !card.oracle_id.is_empty() && ctx.exclude_oracle.contains(&card.oracle_id) {
            continue;
        }
        if card.printings.iter().any(|p| ctx.exclude_grp.contains(&p.grp_id)) {
            continue;
        }
        out.push(i);
    }
    out
}

/// Resolve grp_ids → oracle indices. Later printings overwrite earlier ones only if
/// they belong to a different oracle (a grp_id is expected to map to exactly one oracle).
pub fn index_by_grp(oracles: &[OracleCard]) -> HashMap<i32, usize> {
    let mut out = HashMap::new();
    for (i, card) in oracles.iter().enumerate() {
        for p in &card.printings {
            out.insert(p.grp_id, i);
        }
    }
    out
}

/// Effective identity: commander identity ∩ color_subset (if subset given).
/// Returns Err("DECK-009: …") if subset is not a subset of the commander's identity.
pub fn effective_identity(commander_identity: u8, color_subset: Option<&str>) -> Result<u8, String> {
    let Some(sub) = color_subset else {
        return Ok(commander_identity);
    };
    let sub_mask = mask_from_str(sub);
    if !is_subset(sub_mask, commander_identity) {
        return Err(format!(
            "DECK-009: color_subset {sub:?} is not within commander identity"
        ));
    }
    Ok(sub_mask & commander_identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Rarity;
    use crate::services::deck::engine::types::{sample_card, Printing, G, R, U, W};

    fn card_with(grp: i32, oracle_id: &str, name: &str) -> OracleCard {
        let mut c = sample_card();
        c.oracle_id = oracle_id.into();
        c.name = name.into();
        c.name_lower = name.to_lowercase();
        c.printings = vec![Printing {
            grp_id: grp,
            set_code: "x".into(),
            collector_number: "1".into(),
            rarity: Rarity::Common,
            owned_qty: 1,
        }];
        c
    }

    fn ctx<'a>(
        identity: u8,
        commander_oracle: &'a str,
        exclude_grp: &'a HashSet<i32>,
        exclude_oracle: &'a HashSet<String>,
    ) -> PoolCtx<'a> {
        PoolCtx {
            format: Format::Brawl100,
            identity,
            commander_oracle,
            exclude_grp,
            exclude_oracle,
        }
    }

    #[test]
    fn outside_identity_excluded() {
        let mut in_id = card_with(1, "in", "In");
        in_id.color_identity = G;
        let mut out_id = card_with(2, "out", "Out");
        out_id.color_identity = R;
        let excl_g = HashSet::new();
        let excl_o = HashSet::new();
        let c = ctx(G, "", &excl_g, &excl_o);
        let pool = candidate_pool(&[in_id, out_id], &c);
        assert_eq!(pool, vec![0]);
    }

    #[test]
    fn not_legal_in_format_excluded() {
        let mut a = card_with(1, "a", "A");
        a.color_identity = G;
        a.legal_standardbrawl = false;
        let excl_g = HashSet::new();
        let excl_o = HashSet::new();
        let mut c = ctx(G, "", &excl_g, &excl_o);
        c.format = Format::StandardBrawl60;
        assert!(candidate_pool(&[a], &c).is_empty());
    }

    #[test]
    fn no_printings_excluded() {
        let mut a = card_with(1, "a", "A");
        a.color_identity = G;
        a.printings.clear();
        let excl_g = HashSet::new();
        let excl_o = HashSet::new();
        let c = ctx(G, "", &excl_g, &excl_o);
        assert!(candidate_pool(&[a], &c).is_empty());
    }

    #[test]
    fn commander_excluded_by_oracle_id() {
        let mut cmdr = card_with(1, "cmdr-oid", "Cmdr");
        cmdr.color_identity = G;
        let excl_g = HashSet::new();
        let excl_o = HashSet::new();
        let c = ctx(G, "cmdr-oid", &excl_g, &excl_o);
        assert!(candidate_pool(&[cmdr], &c).is_empty());
    }

    #[test]
    fn excluded_grp_removes_card() {
        let mut a = card_with(42, "a", "A");
        a.color_identity = G;
        let mut excl_g = HashSet::new();
        excl_g.insert(42);
        let excl_o = HashSet::new();
        let c = ctx(G, "", &excl_g, &excl_o);
        assert!(candidate_pool(&[a], &c).is_empty());
    }

    #[test]
    fn effective_identity_intersects() {
        assert_eq!(effective_identity(W | U | G, Some("WG")).unwrap(), W | G);
    }

    #[test]
    fn effective_identity_subset_violation_errors() {
        let err = effective_identity(G, Some("R")).unwrap_err();
        assert!(err.contains("DECK-009"));
    }

    #[test]
    fn effective_identity_none_returns_commander() {
        assert_eq!(effective_identity(W | G, None).unwrap(), W | G);
    }

    #[test]
    fn index_by_grp_maps_all_printings() {
        let mut a = card_with(1, "a", "A");
        a.color_identity = G;
        a.printings.push(Printing {
            grp_id: 2,
            set_code: "y".into(),
            collector_number: "2".into(),
            rarity: Rarity::Rare,
            owned_qty: 0,
        });
        let idx = index_by_grp(&[a]);
        assert_eq!(idx.get(&1), Some(&0));
        assert_eq!(idx.get(&2), Some(&0));
    }
}
