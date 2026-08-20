#![cfg(test)]

use super::types::{OracleCard, Printing, Role, B, G, R, U, W};
use crate::models::Rarity;

pub(crate) fn make_printing(grp: i32, rarity: Rarity, owned: i32) -> Printing {
    Printing {
        grp_id: grp,
        set_code: "x".into(),
        collector_number: format!("{grp}"),
        rarity,
        owned_qty: owned,
    }
}

/// 400-card oracle set; commander at grp 1.
pub(crate) fn gen_oracles() -> Vec<OracleCard> {
    let mut oracles: Vec<OracleCard> = Vec::new();
    let mut next_grp: i32 = 100;

    // Commander first — grp 1, W|G Elf typal.
    oracles.push(OracleCard {
        oracle_id: "test-commander".into(),
        name: "Test Commander".into(),
        name_lower: "test commander".into(),
        cmc: 4.0,
        mana_cost: "{2}{G}{W}".into(),
        color_identity: W | G,
        type_line: "Legendary Creature — Elf".into(),
        keywords: vec![],
        oracle_text: "".into(),
        edhrec_rank: Some(500),
        legal_brawl: true,
        legal_standardbrawl: true,
        is_commander_eligible: true,
        power: Some(3),
        roles: vec![Role::Threat],
        tags: vec!["typal-elf".into()],
        subtypes: vec!["Elf".into()],
        is_land: false,
        is_basic_land: false,
        is_creature: true,
        printings: vec![make_printing(1, Rarity::Mythic, 1)],
    });

    // 6 basics.
    for (i, name) in ["Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"]
        .iter()
        .enumerate()
    {
        let (color, letter) = match i {
            0 => (W, "W"),
            1 => (U, "U"),
            2 => (B, "B"),
            3 => (R, "R"),
            4 => (G, "G"),
            _ => (0u8, "C"),
        };
        oracles.push(OracleCard {
            oracle_id: format!("basic-{}", name.to_lowercase()),
            name: name.to_string(),
            name_lower: name.to_lowercase(),
            cmc: 0.0,
            mana_cost: "".into(),
            color_identity: color,
            type_line: format!("Basic Land — {name}"),
            keywords: vec![],
            oracle_text: format!("{{T}}: Add {{{letter}}}."),
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
            printings: vec![make_printing(next_grp, Rarity::BasicLand, 4)],
        });
        next_grp += 1;
    }

    // 30 nonbasic lands: 10 G, 10 W, 10 W|G. First 10 have tag utility-land.
    for i in 0..30 {
        let (color, letter) = if i < 10 {
            (G, "g")
        } else if i < 20 {
            (W, "w")
        } else {
            (W | G, "w")
        };
        let tags = if i < 10 {
            vec!["utility-land".into()]
        } else {
            vec![]
        };
        oracles.push(OracleCard {
            oracle_id: format!("land-{i}"),
            name: format!("Nonbasic {i:02}"),
            name_lower: format!("nonbasic {i:02}"),
            cmc: 0.0,
            mana_cost: "".into(),
            color_identity: color,
            type_line: "Land".into(),
            keywords: vec![],
            oracle_text: format!("{{T}}: Add {{{letter}}}."),
            edhrec_rank: None,
            legal_brawl: true,
            legal_standardbrawl: true,
            is_commander_eligible: false,
            power: None,
            roles: vec![Role::Land],
            tags,
            subtypes: vec![],
            is_land: true,
            is_basic_land: false,
            is_creature: false,
            printings: vec![make_printing(next_grp, Rarity::Rare, 1)],
        });
        next_grp += 1;
    }

    // Per role: 25 cards across identities W, G, WG, U, R with cmc 1..=6,
    // half owned, half sb-legal.
    let identities = [W, G, W | G, U, R];
    let roles_to_fill = [
        Role::Ramp,
        Role::Draw,
        Role::Removal,
        Role::Wipe,
        Role::Counter,
        Role::Recursion,
        Role::Tutor,
        Role::Protection,
        Role::Threat,
        Role::Finisher,
        Role::Payoff,
        Role::Utility,
    ];
    for role in roles_to_fill {
        for k in 0..25u32 {
            let identity = identities[(k as usize) % identities.len()];
            let cmc = 1.0 + (k % 6) as f32;
            let owned = if k % 2 == 0 { 1 } else { 0 };
            let rarity = if k % 5 == 0 {
                Rarity::Mythic
            } else if k % 3 == 0 {
                Rarity::Rare
            } else {
                Rarity::Uncommon
            };
            let legal_sb = k % 2 == 0;
            let is_typal =
                matches!(role, Role::Threat | Role::Finisher | Role::Payoff);
            let mana_cost = format!("{{{}}}", cmc as u32);
            oracles.push(OracleCard {
                oracle_id: format!("card-{}-{k}", role.as_str()),
                name: format!("{} {k:02}", role.as_str()),
                name_lower: format!("{} {k:02}", role.as_str()),
                cmc,
                mana_cost,
                color_identity: identity,
                type_line: if is_typal {
                    "Creature — Elf".into()
                } else {
                    "Instant".into()
                },
                keywords: vec![],
                oracle_text: "".into(),
                edhrec_rank: Some(1000 + k * 100),
                legal_brawl: true,
                legal_standardbrawl: legal_sb,
                is_commander_eligible: false,
                power: if matches!(role, Role::Threat | Role::Finisher) {
                    Some(cmc as i32)
                } else {
                    None
                },
                roles: vec![role],
                tags: if is_typal {
                    vec!["typal-elf".into()]
                } else {
                    vec![]
                },
                subtypes: if is_typal { vec!["Elf".into()] } else { vec![] },
                is_land: false,
                is_basic_land: false,
                is_creature: is_typal,
                printings: vec![make_printing(next_grp, rarity, owned)],
            });
            next_grp += 1;
        }
    }

    oracles
}
