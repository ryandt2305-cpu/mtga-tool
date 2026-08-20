use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::template::template_for;
use super::types::{
    is_subset, mask_from_str, mask_to_letters, Format, OracleCard, Playstyle, Role,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommanderCandidate {
    pub grp_id: i32,
    pub oracle_id: String,
    pub name: String,
    pub color_identity: String,
    pub owned: bool,
    pub buildability: f32,
    pub popularity: f32,
    pub theme_match: f32,
    pub score: f32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CandidateRequest {
    pub format: Format,
    #[serde(default)]
    pub owned_only: bool,
    #[serde(default)]
    pub color_subset: Option<String>,
    #[serde(default)]
    pub theme_tags: Vec<String>,
    #[serde(default)]
    pub search: Option<String>,
    pub playstyle: Playstyle,
}

pub fn rank_commanders(
    oracles: &[OracleCard],
    req: &CandidateRequest,
    limit: usize,
) -> Vec<CommanderCandidate> {
    // Precompute avail[mask][role_idx] for the 32 identity masks.
    let mut avail: [[u32; Role::ALL.len()]; 32] = [[0; Role::ALL.len()]; 32];
    for card in oracles {
        if card.printings.is_empty() {
            continue;
        }
        if !card.legal_in(req.format) {
            continue;
        }
        if req.owned_only && !card.is_owned() {
            continue;
        }
        for (r_idx, role) in Role::ALL.iter().enumerate() {
            if !card.has_role(*role) {
                continue;
            }
            for mask in 0u8..32 {
                if is_subset(card.color_identity, mask) {
                    avail[mask as usize][r_idx] += 1;
                }
            }
        }
    }

    let color_subset_mask: Option<u8> = req.color_subset.as_deref().map(mask_from_str);
    let search_lc: Option<String> = req
        .search
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let theme_wanted: HashSet<&str> = req.theme_tags.iter().map(|s| s.as_str()).collect();

    let mut out: Vec<CommanderCandidate> = Vec::new();
    for card in oracles {
        if !card.is_commander_eligible {
            continue;
        }
        if card.printings.is_empty() {
            continue;
        }
        if !card.legal_in(req.format) {
            continue;
        }
        if req.owned_only && !card.is_owned() {
            continue;
        }
        if let Some(sub) = color_subset_mask {
            if !is_subset(card.color_identity, sub) {
                continue;
            }
        }
        if let Some(q) = search_lc.as_ref() {
            if !card.name_lower.contains(q) {
                continue;
            }
        }

        let identity = card.color_identity;
        let template = template_for(req.format, req.playstyle, identity, None);

        let mut sum = 0.0f32;
        let mut count = 0u32;
        for (r_idx, role) in Role::ALL.iter().enumerate() {
            let min = template.min(*role);
            if min == 0 {
                continue;
            }
            let have = avail[identity as usize][r_idx] as f32;
            let ratio = (have / min as f32).min(1.0);
            sum += ratio;
            count += 1;
        }
        let buildability = if count == 0 { 0.0 } else { sum / count as f32 };

        let popularity = match card.edhrec_rank {
            Some(rank) => (1.0 - (rank as f32) / 30000.0).max(0.0),
            None => 0.0,
        };

        let theme_match = if theme_wanted.is_empty() {
            1.0
        } else {
            let hits = card
                .tags
                .iter()
                .filter(|t| theme_wanted.contains(t.as_str()))
                .count();
            hits as f32 / theme_wanted.len() as f32
        };

        let score = 0.6 * buildability + 0.3 * popularity + 0.1 * theme_match;

        let bp = card.best_printing();
        let grp_id = bp.map(|p| p.grp_id).unwrap_or(0);
        let color_identity: String = mask_to_letters(identity).into_iter().collect();

        out.push(CommanderCandidate {
            grp_id,
            oracle_id: card.oracle_id.clone(),
            name: card.name.clone(),
            color_identity,
            owned: card.is_owned(),
            buildability,
            popularity,
            theme_match,
            score,
            tags: card.tags.clone(),
        });
    }

    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Rarity;
    use crate::services::deck::engine::test_fixtures::{gen_oracles, make_printing};
    use crate::services::deck::engine::types::{Printing, R, U, W, G};

    fn extra_commander(
        oracle_id: &str,
        name: &str,
        identity: u8,
        rank: Option<u32>,
        owned: i32,
        tags: Vec<&str>,
        grp: i32,
    ) -> OracleCard {
        OracleCard {
            oracle_id: oracle_id.into(),
            name: name.into(),
            name_lower: name.to_lowercase(),
            cmc: 4.0,
            mana_cost: "{2}{X}{X}".into(),
            color_identity: identity,
            type_line: "Legendary Creature".into(),
            keywords: vec![],
            oracle_text: "".into(),
            edhrec_rank: rank,
            legal_brawl: true,
            legal_standardbrawl: true,
            is_commander_eligible: true,
            power: Some(4),
            roles: vec![Role::Threat],
            tags: tags.into_iter().map(String::from).collect(),
            subtypes: vec![],
            is_land: false,
            is_basic_land: false,
            is_creature: true,
            printings: vec![Printing {
                grp_id: grp,
                set_code: "x".into(),
                collector_number: format!("{grp}"),
                rarity: Rarity::Mythic,
                owned_qty: owned,
            }],
        }
    }

    fn base_req() -> CandidateRequest {
        CandidateRequest {
            format: Format::Brawl100,
            owned_only: false,
            color_subset: None,
            theme_tags: vec![],
            search: None,
            playstyle: Playstyle::Midrange,
        }
    }

    fn with_extras() -> Vec<OracleCard> {
        let mut v = gen_oracles();
        // Poorly-supported R-only commander (fixture has few R support cards; template still needs many).
        v.push(extra_commander(
            "cmdr-r-poor",
            "Red Loner",
            R,
            Some(2000),
            1,
            vec![],
            9001,
        ));
        // Unowned U-only commander.
        v.push(extra_commander(
            "cmdr-u-unowned",
            "Blue Absent",
            U,
            Some(1500),
            0,
            vec![],
            9002,
        ));
        // Distinctly-named commander for search filter.
        v.push(extra_commander(
            "cmdr-zephyr",
            "Zephyr Duke",
            W | U,
            Some(3000),
            1,
            vec![],
            9003,
        ));
        v
    }

    #[test]
    fn well_supported_beats_unsupported() {
        let oracles = with_extras();
        let out = rank_commanders(&oracles, &base_req(), 50);
        let wg = out
            .iter()
            .position(|c| c.oracle_id == "test-commander")
            .expect("WG commander present");
        let r = out
            .iter()
            .position(|c| c.oracle_id == "cmdr-r-poor")
            .expect("R commander present");
        assert!(
            wg < r,
            "WG (buildability {:.2}) should rank above R (buildability {:.2})",
            out[wg].buildability,
            out[r].buildability
        );
        assert!(out[wg].buildability > out[r].buildability);
    }

    #[test]
    fn owned_only_drops_unowned() {
        let oracles = with_extras();
        let mut req = base_req();
        req.owned_only = true;
        let out = rank_commanders(&oracles, &req, 50);
        assert!(out.iter().all(|c| c.oracle_id != "cmdr-u-unowned"));
        assert!(out.iter().any(|c| c.oracle_id == "test-commander"));
    }

    #[test]
    fn search_filters_by_name() {
        let oracles = with_extras();
        let mut req = base_req();
        req.search = Some("zephyr".into());
        let out = rank_commanders(&oracles, &req, 50);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].oracle_id, "cmdr-zephyr");
    }

    #[test]
    fn color_subset_filters_identity() {
        let oracles = with_extras();
        let mut req = base_req();
        // Only allow W|G identity — R and U commanders drop, WG stays.
        req.color_subset = Some("WG".into());
        let out = rank_commanders(&oracles, &req, 50);
        assert!(out.iter().any(|c| c.oracle_id == "test-commander"));
        assert!(out.iter().all(|c| c.oracle_id != "cmdr-r-poor"));
        assert!(out.iter().all(|c| c.oracle_id != "cmdr-u-unowned"));
        assert!(out.iter().all(|c| c.oracle_id != "cmdr-zephyr"));
    }

    #[test]
    fn limit_respected() {
        let oracles = with_extras();
        let out = rank_commanders(&oracles, &base_req(), 2);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn theme_match_boosts_matching_commander() {
        // Two commanders with the same identity, one carrying the requested tag.
        let mut oracles: Vec<OracleCard> = Vec::new();
        // Minimal role support so buildability is equal for both.
        for i in 0..2 {
            oracles.push(extra_commander(
                &format!("cmdr-{i}"),
                &format!("Commander {i}"),
                G,
                Some(1000),
                1,
                if i == 0 { vec!["typal-elf"] } else { vec![] },
                7000 + i as i32,
            ));
        }
        // Add a basic Forest so pool isn't empty.
        oracles.push(OracleCard {
            oracle_id: "basic-forest".into(),
            name: "Forest".into(),
            name_lower: "forest".into(),
            cmc: 0.0,
            mana_cost: "".into(),
            color_identity: G,
            type_line: "Basic Land — Forest".into(),
            keywords: vec![],
            oracle_text: "{T}: Add {G}.".into(),
            edhrec_rank: None,
            legal_brawl: true,
            legal_standardbrawl: true,
            is_commander_eligible: false,
            power: None,
            roles: vec![Role::Land],
            tags: vec![],
            subtypes: vec![],
            is_land: true,
            is_basic_land: true,
            is_creature: false,
            printings: vec![make_printing(7100, Rarity::BasicLand, 4)],
        });
        let mut req = base_req();
        req.theme_tags = vec!["typal-elf".into()];
        let out = rank_commanders(&oracles, &req, 10);
        let a = out.iter().find(|c| c.oracle_id == "cmdr-0").unwrap();
        let b = out.iter().find(|c| c.oracle_id == "cmdr-1").unwrap();
        assert_eq!(a.theme_match, 1.0);
        assert_eq!(b.theme_match, 0.0);
        assert!(a.score > b.score);
    }
}
