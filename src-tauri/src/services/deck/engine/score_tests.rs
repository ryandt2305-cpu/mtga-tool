#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::services::deck::engine::score::*;
    use crate::services::deck::engine::template::{template_for, Template};
    use crate::services::deck::engine::types::{
        sample_card, ComboCompletion, CommunityData, Format, Playstyle, Printing, Role,
        WildcardBudget, G,
    };
    use crate::models::Rarity;

    fn midrange_template() -> Template {
        template_for(Format::Brawl100, Playstyle::Midrange, G, None)
    }

    fn empty_ctx_state() -> (
        HashMap<Role, u32>,
        [u32; 8],
        HashSet<String>,
        Vec<String>,
        HashMap<String, f32>,
        CommunityData,
        HashSet<String>,
        WildcardBudget,
    ) {
        (
            HashMap::new(),
            [0u32; 8],
            HashSet::new(),
            Vec::new(),
            HashMap::new(),
            CommunityData::default(),
            HashSet::new(),
            WildcardBudget::default(),
        )
    }

    #[test]
    fn owned_beats_identical_unowned_in_owned_only() {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, community, dn, _budget) = empty_ctx_state();
        let full_budget = WildcardBudget {
            common: 4,
            uncommon: 4,
            rare: 4,
            mythic: 4,
        };
        let zero_budget = WildcardBudget::default();

        let mut owned = sample_card();
        owned.name_lower = "x".into();
        owned.roles = vec![Role::Threat];
        owned.printings[0].rarity = Rarity::Rare;
        owned.printings[0].owned_qty = 1;

        let mut unowned = owned.clone();
        unowned.printings[0].owned_qty = 0;

        let ctx_owned = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: true,
            budget: &full_budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let ctx_unowned = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: true,
            budget: &full_budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let ctx_zero = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: true,
            budget: &zero_budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };

        let s_owned = score_card(&owned, &ctx_owned);
        let s_unowned = score_card(&unowned, &ctx_unowned);
        assert!(s_owned.total > s_unowned.total);

        let s_zero = score_card(&unowned, &ctx_zero);
        assert!(!s_zero.affordable);
    }

    #[test]
    fn best_deck_ignores_ownership() {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, community, dn, budget) = empty_ctx_state();

        let mut c = sample_card();
        c.roles = vec![Role::Threat];
        c.printings[0].owned_qty = 0;

        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert_eq!(s.ownership, 0.0);
        assert!(s.affordable);
    }

    #[test]
    fn role_need_picks_most_underfilled_role() {
        let tpl = midrange_template();
        let (_, cc, theme, subs, bab, community, dn, budget) = empty_ctx_state();
        let mut rc: HashMap<Role, u32> = HashMap::new();
        rc.insert(Role::Ramp, 8);
        rc.insert(Role::Draw, 0);

        let mut c = sample_card();
        c.roles = vec![Role::Ramp, Role::Draw];

        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert_eq!(s.role, Role::Draw);
        assert!((s.role_need - 1.0).abs() < 1e-6);
    }

    #[test]
    fn synergy_from_theme_and_combo() {
        let tpl = midrange_template();
        let (rc, cc, _, subs, bab, _, _, budget) = empty_ctx_state();
        let mut theme: HashSet<String> = HashSet::new();
        theme.insert("lifegain-matters".into());

        let mut community = CommunityData::default();
        community.combos = vec![vec!["sanguine bond".into(), "exquisite blood".into()]];

        let mut dn: HashSet<String> = HashSet::new();
        dn.insert("sanguine bond".into());

        let mut c = sample_card();
        c.name = "Exquisite Blood".into();
        c.name_lower = "exquisite blood".into();
        c.roles = vec![Role::Payoff];
        c.tags = vec!["lifegain-matters".into()];

        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert!(s.synergy > 0.0);
        assert!(s.synergy >= 0.5);
    }

    #[test]
    fn curve_fit_penalises_overfilled_bucket() {
        let tpl = midrange_template();
        let (rc, _, theme, subs, bab, community, dn, budget) = empty_ctx_state();
        let mut cc = [0u32; 8];
        cc[3] = 30;

        let mut c = sample_card();
        c.cmc = 3.0;
        c.roles = vec![Role::Threat];

        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert!(s.curve_fit < 1.0);
    }

    #[test]
    fn rank_curve_shape() {
        assert!((rank_curve(500) - 1.0).abs() < 1e-6);
        assert!((rank_curve(3000) - 0.5623).abs() < 0.01);
        assert!((rank_curve(10_000) - 0.2683).abs() < 0.01);
        assert_eq!(rank_curve(30_000), 0.0);
        assert_eq!(rank_curve(1), 1.0);
    }

    #[test]
    fn power_prefers_staple_over_rank_and_floor() {
        use crate::services::deck::engine::types::CommunitySignal;
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, mut community, dn, budget) = empty_ctx_state();
        let mut c = sample_card();
        c.name_lower = "sol ring".into();
        c.roles = vec![Role::Ramp];
        c.edhrec_rank = Some(20_000);
        c.printings[0].rarity = Rarity::Common;
        community.by_name.insert(
            "sol ring".into(),
            CommunitySignal {
                staple: Some(0.8),
                ..Default::default()
            },
        );
        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert!((s.power - share_norm(0.8)).abs() < 1e-6);
        assert!(s.reasons.iter().any(|r| r == "staple 80%"));
    }

    #[test]
    fn power_uses_rank_curve_then_rarity_floor() {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, community, dn, budget) = empty_ctx_state();
        let mut c = sample_card();
        c.roles = vec![Role::Threat];
        c.edhrec_rank = Some(3000);
        c.printings[0].rarity = Rarity::Mythic;
        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        let s = score_card(&c, &ctx);
        assert!((s.power - RANK_ONLY_CAP * rank_curve(3000)).abs() < 1e-6);
        c.edhrec_rank = None;
        let s2 = score_card(&c, &ctx);
        assert!((s2.power - 0.15).abs() < 1e-6);
    }

    #[test]
    fn tokenize_word_boundaries_and_plurals() {
        let t = tokenize(
            "Creature — Elf Druid",
            "Whenever another Elf enters, you gain 1 life. Elves you control get +1/+1. Yourself.",
        );
        assert!(t.contains("elf"));
        assert!(t.contains("elves"));
        assert!(t.contains("druid"));
        assert!(t.contains("yourself"));
        assert!(!t.contains("self"));
    }

    #[test]
    fn subtype_match_is_word_boundary() {
        let tpl = midrange_template();
        let (rc, cc, theme, _subs, bab, community, dn, budget) = empty_ctx_state();
        let subs: Vec<String> = vec!["Elf".into()];
        let lower: Vec<String> = vec!["elf".into()];
        let mut a = sample_card();
        a.roles = vec![Role::Threat];
        a.oracle_text = "Elves you control get +1/+1.".into();
        let mut b = sample_card();
        b.roles = vec![Role::Threat];
        b.oracle_text = "Target player loses control of themselves.".into();
        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: Some(&lower),
            card_tokens: None,
            tag_idf: None,
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        assert!(score_card(&a, &ctx).synergy > 0.0);
        assert_eq!(score_card(&b, &ctx).synergy, 0.0);
    }

    #[test]
    fn idf_theme_score_prefers_rare_tag() {
        let tpl = midrange_template();
        let (rc, cc, _theme, subs, bab, community, dn, budget) = empty_ctx_state();
        let theme: HashSet<String> = ["creature".to_string(), "lifegain-matters".to_string()]
            .into_iter()
            .collect();
        let mut idf: HashMap<String, f32> = HashMap::new();
        idf.insert("creature".into(), 0.1);
        idf.insert("lifegain-matters".into(), 3.0);
        let mut common = sample_card();
        common.roles = vec![Role::Threat];
        common.tags = vec!["creature".into()];
        let mut rare = sample_card();
        rare.roles = vec![Role::Threat];
        rare.tags = vec!["lifegain-matters".into()];
        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: Some(&idf),
        pip_counts: &[0u32; 5],
        identity: 0,
        combo_completions: None,
        };
        assert!(score_card(&rare, &ctx).synergy > score_card(&common, &ctx).synergy * 5.0);
    }

    #[test]
    fn pip_fit_penalises_heavy_pips_off_colour() {
        use crate::services::deck::engine::types::{G, U, W};
        let mut mono_w = sample_card();
        mono_w.mana_cost = "{W}{W}{W}".into();
        let mut splash = sample_card();
        splash.mana_cost = "{2}{W}".into();
        let empty = [0u32; 5];
        // Empty three-colour deck: source share = 1/3 each. WWW share 1.0 → 1 − (1 − 0.333 − 0.15) = 0.483
        let f_mono = compute_pip_fit(&mono_w, &empty, W | U | G);
        let f_splash = compute_pip_fit(&splash, &empty, W | U | G);
        // Both are 100% W pips (card_share_W = 1.0 for each), so pip_fit is equal.
        assert!(f_mono <= f_splash);
        assert!((f_mono - 0.4833).abs() < 0.01);
        assert!((f_splash - f_mono).abs() < 1e-6);
        let mut mixed = sample_card();
        mixed.mana_cost = "{W}{U}{G}".into();
        assert!((compute_pip_fit(&mixed, &empty, W | U | G) - 1.0).abs() < 1e-6);
        // Deck already all-W: no penalty
        let all_w = [30u32, 0, 0, 0, 0];
        assert!((compute_pip_fit(&mono_w, &all_w, W | U | G) - 1.0).abs() < 1e-6);
        // Lands / colourless → 1.0
        let mut land = sample_card();
        land.is_land = true;
        assert_eq!(compute_pip_fit(&land, &empty, W), 1.0);
        let mut rock = sample_card();
        rock.mana_cost = "{2}".into();
        assert_eq!(compute_pip_fit(&rock, &empty, W), 1.0);
    }
    /// Score a card against an empty Brawl100 midrange deck state with the
    /// given community data. Returns the owned `Scored` so callers do not
    /// have to juggle the borrowed `ScoreCtx` locals.
    fn score_empty_state(
        card: &crate::services::deck::engine::types::OracleCard,
        community: &CommunityData,
    ) -> Scored {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, _unused, dn, budget) = empty_ctx_state();
        let ctx = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
            pip_counts: &[0u32; 5],
            identity: 0,
            combo_completions: None,
        };
        score_card(card, &ctx)
    }

    fn named_threat(
        name: &str,
        rank: Option<u32>,
    ) -> crate::services::deck::engine::types::OracleCard {
        let mut c = sample_card();
        c.name = name.into();
        c.name_lower = name.to_lowercase();
        c.roles = vec![Role::Threat];
        c.edhrec_rank = rank;
        c.printings[0].rarity = Rarity::Common;
        c
    }

    fn community_with(
        name: &str,
        sig: crate::services::deck::engine::types::CommunitySignal,
    ) -> CommunityData {
        let mut community = CommunityData::default();
        community.by_name.insert(name.to_lowercase(), sig);
        community
    }

    #[test]
    fn share_norm_shape() {
        assert!((share_norm(0.0) - 0.40).abs() < 1e-6);
        assert!((share_norm(1.0) - 1.0).abs() < 1e-6);
        assert!((share_norm(0.05) - 0.54).abs() < 0.01);
        assert!((share_norm(0.15) - 0.67).abs() < 0.01);
        assert!((share_norm(0.30) - 0.78).abs() < 0.01);
        assert!((share_norm(0.90) - 0.98).abs() < 0.01);
        // monotonic, never above 1
        let mut prev = -1.0f32;
        for i in 0..=20 {
            let v = share_norm(i as f32 / 20.0);
            assert!(v >= prev && v <= 1.0, "share_norm not monotonic at {i}");
            prev = v;
        }
        // negative / garbage input is clamped, never NaN
        assert!((share_norm(-0.5) - 0.40).abs() < 1e-6);
    }

    #[test]
    fn synergy_norm_and_gih_norm_shape() {
        assert_eq!(synergy_norm(0.4), 1.0);
        assert_eq!(synergy_norm(0.8), 1.0);
        assert!((synergy_norm(0.2) - 0.5).abs() < 1e-6);
        assert_eq!(synergy_norm(-0.1), 0.0);
        assert_eq!(gih_norm(0.50), 0.0);
        assert!((gih_norm(0.575) - 0.5 * GIH_CAP).abs() < 1e-6);
        assert!((gih_norm(0.65) - GIH_CAP).abs() < 1e-6);
        assert!((gih_norm(0.80) - GIH_CAP).abs() < 1e-6); // capped
    }

    #[test]
    fn on_page_low_inclusion_beats_off_page_high_rank() {
        use crate::services::deck::engine::types::CommunitySignal;
        // Median on-page EDHREC card (15 % inclusion, poor generic rank) ...
        let on_page = named_threat("Triumph of the Hordes", Some(20_000));
        let community = community_with(
            "Triumph of the Hordes",
            CommunitySignal { inclusion: Some(0.15), ..Default::default() },
        );
        // ... versus a generically popular paper-EDH card that is NOT on the page.
        let off_page = named_threat("Thassa Oracle", Some(1_000));
        let s_on = score_empty_state(&on_page, &community);
        let s_off = score_empty_state(&off_page, &community);
        assert!(
            s_on.power > s_off.power,
            "on-page {} vs off-page {}",
            s_on.power,
            s_off.power
        );
        assert!(s_off.power <= RANK_ONLY_CAP + 1e-6);
        assert!((s_off.power - RANK_ONLY_CAP * rank_curve(1_000)).abs() < 1e-6);
    }

    #[test]
    fn rank_only_elite_card_still_beats_lowest_on_page_card() {
        use crate::services::deck::engine::types::CommunitySignal;
        let barely_on_page = named_threat("Zuran Orb", Some(25_000));
        let community = community_with(
            "Zuran Orb",
            CommunitySignal { inclusion: Some(0.01), ..Default::default() },
        );
        let elite = named_threat("Arcane Signet", Some(300));
        let s_low = score_empty_state(&barely_on_page, &community);
        let s_elite = score_empty_state(&elite, &community);
        assert!(
            s_elite.power > s_low.power,
            "elite {} vs 1%-on-page {}",
            s_elite.power,
            s_low.power
        );
    }

    #[test]
    fn gih_only_is_capped_below_on_page_median() {
        use crate::services::deck::engine::types::CommunitySignal;
        let draft_bomb = named_threat("Draft Bomb", None);
        let community = community_with(
            "Draft Bomb",
            CommunitySignal { gih_wr: Some(0.70), ..Default::default() },
        );
        let s = score_empty_state(&draft_bomb, &community);
        assert!((s.power - GIH_CAP).abs() < 1e-6);
        assert!(s.power < share_norm(0.15));
        assert!(s.reasons.iter().any(|r| r == "GIH 70%"));
    }

    #[test]
    fn agreement_bonus_for_multiple_community_signals() {
        use crate::services::deck::engine::types::CommunitySignal;
        let card = named_threat("Agreed Upon", None);
        let one = community_with(
            "Agreed Upon",
            CommunitySignal { inclusion: Some(0.15), ..Default::default() },
        );
        let two = community_with(
            "Agreed Upon",
            CommunitySignal {
                inclusion: Some(0.15),
                staple: Some(0.15),
                ..Default::default()
            },
        );
        let s1 = score_empty_state(&card, &one);
        let s2 = score_empty_state(&card, &two);
        assert!((s1.power - share_norm(0.15)).abs() < 1e-6);
        assert!((s2.power - (share_norm(0.15) + AGREE_BONUS)).abs() < 1e-6);
        // Bonus never pushes above 1.0.
        let maxed = community_with(
            "Agreed Upon",
            CommunitySignal {
                inclusion: Some(1.0),
                staple: Some(1.0),
                seed_freq: Some(1.0),
                gih_wr: Some(0.9),
                ..Default::default()
            },
        );
        assert!(score_empty_state(&card, &maxed).power <= 1.0);
    }

    #[test]
    fn seed_card_power_unchanged_at_one() {
        use crate::services::deck::engine::types::CommunitySignal;
        let card = named_threat("Seeded", Some(20_000));
        let community = community_with(
            "Seeded",
            CommunitySignal { seed_freq: Some(1.0), ..Default::default() },
        );
        assert!((score_empty_state(&card, &community).power - 1.0).abs() < 1e-6);
    }

    #[test]
    fn scarcity_mult_shape() {
        // Deep pool → no surcharge; drains monotonically as budget empties.
        assert_eq!(scarcity_mult(100), 1.0);
        assert_eq!(scarcity_mult(6), 1.0);
        assert!(scarcity_mult(3) > 1.0);
        assert!(scarcity_mult(1) > scarcity_mult(3));
        // Monotonic non-increasing in remaining count.
        for n in 0..10u32 {
            assert!(scarcity_mult(n) >= scarcity_mult(n + 1));
        }
    }

    #[test]
    fn ownership_cost_scales_with_scarcity() {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, community, dn, _budget) = empty_ctx_state();
        let deep = WildcardBudget { common: 4, uncommon: 4, rare: 20, mythic: 4 };
        let scarce = WildcardBudget { common: 4, uncommon: 4, rare: 1, mythic: 4 };

        let mut unowned = sample_card();
        unowned.name_lower = "x".into();
        unowned.roles = vec![Role::Threat];
        unowned.printings[0].rarity = Rarity::Rare;
        unowned.printings[0].owned_qty = 0;

        let ctx_deep = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: true,
            budget: &deep,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
            pip_counts: &[0u32; 5],
            identity: 0,
            combo_completions: None,
        };
        let ctx_scarce = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: true,
            budget: &scarce,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
            pip_counts: &[0u32; 5],
            identity: 0,
            combo_completions: None,
        };
        let s_deep = score_card(&unowned, &ctx_deep);
        let s_scarce = score_card(&unowned, &ctx_scarce);
        assert!(s_scarce.ownership < s_deep.ownership);
        assert!((s_deep.ownership - (-0.6)).abs() < 1e-5);
        assert!((s_scarce.ownership - (-0.6 * 1.8)).abs() < 1e-5);
    }

    #[test]
    fn combo_completion_bonus_raises_synergy_and_reason() {
        let tpl = midrange_template();
        let (rc, cc, theme, subs, bab, community, dn, budget) = empty_ctx_state();

        let mut completions: HashMap<String, ComboCompletion> = HashMap::new();
        completions.insert(
            "missing piece".into(),
            ComboCompletion { bonus: 0.5, have: 2, size: 3 },
        );

        let mut card = sample_card();
        card.name = "Missing Piece".into();
        card.name_lower = "missing piece".into();
        card.roles = vec![Role::Threat];

        let ctx_with = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
            pip_counts: &[0u32; 5],
            identity: 0,
            combo_completions: Some(&completions),
        };
        let ctx_without = ScoreCtx {
            template: &tpl,
            role_counts: &rc,
            curve_counts: &cc,
            theme: &theme,
            commander_subtypes: &subs,
            build_around_bonus: &bab,
            community: &community,
            deck_names: &dn,
            owned_only: false,
            budget: &budget,
            commander_subtypes_lower: None,
            card_tokens: None,
            tag_idf: None,
            pip_counts: &[0u32; 5],
            identity: 0,
            combo_completions: None,
        };
        let s_with = score_card(&card, &ctx_with);
        let s_without = score_card(&card, &ctx_without);
        assert!(s_with.synergy > s_without.synergy);
        assert!(
            (s_with.synergy - s_without.synergy - 0.5).abs() < 1e-5
                || (s_with.synergy - 1.0).abs() < 1e-5
        );
        assert!(s_with
            .reasons
            .iter()
            .any(|r| r == "completes combo (own 2/3)"));
    }

    #[test]
    fn community_synergy_is_normalised() {
        use crate::services::deck::engine::types::CommunitySignal;
        let card = named_threat("Syn", None);
        let high = community_with(
            "Syn",
            CommunitySignal { synergy: Some(0.4), ..Default::default() },
        );
        let low = community_with(
            "Syn",
            CommunitySignal { synergy: Some(0.1), ..Default::default() },
        );
        let neg = community_with(
            "Syn",
            CommunitySignal { synergy: Some(-0.2), ..Default::default() },
        );
        let s_high = score_empty_state(&card, &high).synergy;
        let s_low = score_empty_state(&card, &low).synergy;
        let s_neg = score_empty_state(&card, &neg).synergy;
        // 0.6 weight x (1.0 - 0.25) = 0.45 apart; negative synergy clamps to 0.
        assert!((s_high - 0.6).abs() < 1e-6, "high {}", s_high);
        assert!(((s_high - s_low) - 0.45).abs() < 1e-6, "high {} low {}", s_high, s_low);
        assert_eq!(s_neg, 0.0);
    }
}
