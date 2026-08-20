use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use super::types::Role;
use crate::services::deck::store::IngestTag;

pub const META_ROOTS: &[&str] = &[
    "card-names",
    "flavors-of-vanilla",
    "activated-ability",
    "triggered-ability",
    "unique-type-line",
    "cheaper-than-mv",
    "more-expensive-than-mv",
    "drawback",
    "multiple-targets",
    "single-target-instant-sorcery",
    "staple-with-set-s-mechanic",
    "cycle",
    "token-errata",
    "token-versions-of-cards",
    "evasion",
];

pub struct TagIndex {
    by_id: HashMap<String, (String, Option<String>)>,
}

impl TagIndex {
    pub fn new(tags: &[IngestTag]) -> Self {
        let by_id = tags
            .iter()
            .map(|t| (t.tag_id.clone(), (t.slug.clone(), t.parent_id.clone())))
            .collect();
        Self { by_id }
    }

    /// Slug chain from this tag up to root (cycle-safe, max 32 hops); first element is the tag itself.
    pub fn ancestry(&self, tag_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = tag_id.to_string();
        for _ in 0..32 {
            if !seen.insert(cur.clone()) {
                break;
            }
            match self.by_id.get(&cur) {
                Some((slug, parent)) => {
                    out.push(slug.clone());
                    match parent {
                        Some(p) => cur = p.clone(),
                        None => break,
                    }
                }
                None => break,
            }
        }
        out
    }

    pub fn slug(&self, tag_id: &str) -> Option<&str> {
        self.by_id.get(tag_id).map(|(s, _)| s.as_str())
    }
}

fn push_unique(roles: &mut Vec<Role>, r: Role) {
    if !roles.contains(&r) {
        roles.push(r);
    }
}

fn sort_by_all(roles: Vec<Role>) -> Vec<Role> {
    let mut out = Vec::with_capacity(roles.len());
    for r in Role::ALL.iter() {
        if roles.contains(r) {
            out.push(*r);
        }
    }
    out
}

/// Roles implied by a card's tag ids (via ancestry roots).
pub fn roles_from_tags(index: &TagIndex, tag_ids: &[String]) -> Vec<Role> {
    let mut roles: Vec<Role> = Vec::new();
    for tid in tag_ids {
        let anc = index.ancestry(tid);
        if anc.is_empty() {
            continue;
        }
        let has = |s: &str| anc.iter().any(|x| x == s);
        let sweeper = has("sweeper");
        if sweeper {
            push_unique(&mut roles, Role::Wipe);
        } else if has("removal") {
            push_unique(&mut roles, Role::Removal);
        }
        if has("ramp") {
            push_unique(&mut roles, Role::Ramp);
        }
        if has("draw") || has("card-advantage") {
            push_unique(&mut roles, Role::Draw);
        }
        if has("counterspell") {
            push_unique(&mut roles, Role::Counter);
        }
        if has("recursion") {
            push_unique(&mut roles, Role::Recursion);
        }
        if has("tutor") {
            push_unique(&mut roles, Role::Tutor);
        }
        if has("protection") || has("damage-prevention") {
            push_unique(&mut roles, Role::Protection);
        }
        if has("utility-land") {
            push_unique(&mut roles, Role::Utility);
        }
        for slug in &anc {
            if slug.ends_with("-matters")
                || slug.starts_with("synergy-")
                || slug.starts_with("typal-")
            {
                push_unique(&mut roles, Role::Payoff);
                break;
            }
        }
    }
    sort_by_all(roles)
}

static RE_WIPE: OnceLock<Regex> = OnceLock::new();
static RE_REMOVAL_TARGET: OnceLock<Regex> = OnceLock::new();
static RE_DAMAGE: OnceLock<Regex> = OnceLock::new();
static RE_DRAW: OnceLock<Regex> = OnceLock::new();
static RE_BOUNCE: OnceLock<Regex> = OnceLock::new();
static RE_FIGHT: OnceLock<Regex> = OnceLock::new();
static RE_SHRINK: OnceLock<Regex> = OnceLock::new();
static RE_EDICT: OnceLock<Regex> = OnceLock::new();
static RE_EXILE_TARGET: OnceLock<Regex> = OnceLock::new();
static RE_POWER_DAMAGE: OnceLock<Regex> = OnceLock::new();
static RE_WIPE_SHRINK: OnceLock<Regex> = OnceLock::new();
static RE_WIPE_DAMAGE: OnceLock<Regex> = OnceLock::new();
static RE_TREASURE: OnceLock<Regex> = OnceLock::new();
static RE_ADD_MANA: OnceLock<Regex> = OnceLock::new();
static RE_COST_LESS: OnceLock<Regex> = OnceLock::new();
static RE_IMPULSE: OnceLock<Regex> = OnceLock::new();
static RE_DIG: OnceLock<Regex> = OnceLock::new();
static RE_INVESTIGATE: OnceLock<Regex> = OnceLock::new();
static RE_LOOT: OnceLock<Regex> = OnceLock::new();

fn wipe_re() -> &'static Regex {
    RE_WIPE.get_or_init(|| Regex::new(r"(Destroy|Exile) all ").unwrap())
}
fn removal_target_re() -> &'static Regex {
    RE_REMOVAL_TARGET.get_or_init(|| Regex::new(r"(Destroy|Exile) target ").unwrap())
}
fn damage_re() -> &'static Regex {
    RE_DAMAGE
        .get_or_init(|| Regex::new(r"deals \d+ damage to (any target|target creature)").unwrap())
}
fn draw_re() -> &'static Regex {
    RE_DRAW.get_or_init(|| Regex::new(r"(?i)draw .* card").unwrap())
}
fn bounce_re() -> &'static Regex {
    RE_BOUNCE.get_or_init(|| {
        Regex::new(
            r"[Rr]eturn (target|up to \w+ target|another target) (creature|permanent|nonland permanent|artifact|enchantment|planeswalker)[^.]* to (its|their) owner",
        )
        .unwrap()
    })
}
fn fight_re() -> &'static Regex {
    RE_FIGHT.get_or_init(|| Regex::new(r"fights? (target|another target|up to)").unwrap())
}
fn shrink_re() -> &'static Regex {
    RE_SHRINK
        .get_or_init(|| Regex::new(r"[Tt]arget creature (an opponent controls )?gets -\d+/-\d+").unwrap())
}
fn edict_re() -> &'static Regex {
    RE_EDICT.get_or_init(|| {
        Regex::new(r"[Ee]ach (opponent|player) sacrifices (a|an|two|three) (creature|permanent|nonland permanent)")
            .unwrap()
    })
}
fn exile_target_re() -> &'static Regex {
    RE_EXILE_TARGET.get_or_init(|| {
        Regex::new(
            r"[Ee]xile target (creature|permanent|nonland permanent|artifact|enchantment|planeswalker|artifact or enchantment)",
        )
        .unwrap()
    })
}
fn power_damage_re() -> &'static Regex {
    RE_POWER_DAMAGE.get_or_init(|| {
        Regex::new(r"deals damage equal to its power to (any target|target creature|target)").unwrap()
    })
}
fn wipe_shrink_re() -> &'static Regex {
    RE_WIPE_SHRINK.get_or_init(|| {
        Regex::new(r"([Ee]ach creature|[Aa]ll creatures|[Aa]ll other creatures) gets? -\d+/-\d+").unwrap()
    })
}
fn wipe_damage_re() -> &'static Regex {
    RE_WIPE_DAMAGE
        .get_or_init(|| Regex::new(r"deals \d+ damage to each (creature|opponent and each creature)").unwrap())
}
fn treasure_re() -> &'static Regex {
    RE_TREASURE.get_or_init(|| {
        Regex::new(r"[Cc]reate (a|an|two|three|four|X|that many) [^.]*Treasure").unwrap()
    })
}
fn add_mana_re() -> &'static Regex {
    RE_ADD_MANA.get_or_init(|| {
        Regex::new(r": Add (\{[WUBRGC0-9]\}|one mana|two mana|three mana|an amount of mana)").unwrap()
    })
}
fn cost_less_re() -> &'static Regex {
    RE_COST_LESS.get_or_init(|| Regex::new(r"cost \{\d\} less to cast").unwrap())
}
fn impulse_re() -> &'static Regex {
    RE_IMPULSE.get_or_init(|| {
        Regex::new(r"[Ee]xile the top (card|two cards|three cards|\w+ cards) of your library[^.]*\. (Until|You may)").unwrap()
    })
}
fn dig_re() -> &'static Regex {
    RE_DIG.get_or_init(|| {
        Regex::new(r"[Ll]ook at the top [^.]*\. Put (one|two|three|up to \w+) of them into your hand").unwrap()
    })
}
fn investigate_re() -> &'static Regex {
    RE_INVESTIGATE.get_or_init(|| Regex::new(r"[Ii]nvestigate").unwrap())
}
fn loot_re() -> &'static Regex {
    RE_LOOT.get_or_init(|| Regex::new(r"[Dd]iscard [^.]*\. (If you do, )?[Dd]raw").unwrap())
}

/// Fallback roles from type/keywords/text/power.
pub fn roles_from_card(
    type_line: &str,
    oracle_text: &str,
    keywords: &[String],
    power: Option<i32>,
) -> Vec<Role> {
    let mut roles = Vec::new();
    let front_type = type_line.split("//").next().unwrap_or(type_line).trim();
    let is_land = front_type.contains("Land");
    if is_land {
        push_unique(&mut roles, Role::Land);
    }

    if !is_land
        && (oracle_text.contains("{T}: Add")
            || add_mana_re().is_match(oracle_text)
            || treasure_re().is_match(oracle_text)
            || cost_less_re().is_match(oracle_text))
    {
        push_unique(&mut roles, Role::Ramp);
    }
    if !is_land
        && oracle_text.contains("Search your library for")
        && (oracle_text.contains("land card") || oracle_text.contains("basic land"))
    {
        push_unique(&mut roles, Role::Ramp);
    }
    if oracle_text.contains("Counter target") {
        push_unique(&mut roles, Role::Counter);
    }
    if wipe_re().is_match(oracle_text)
        || wipe_shrink_re().is_match(oracle_text)
        || wipe_damage_re().is_match(oracle_text)
    {
        push_unique(&mut roles, Role::Wipe);
    }
    if removal_target_re().is_match(oracle_text)
        || damage_re().is_match(oracle_text)
        || bounce_re().is_match(oracle_text)
        || fight_re().is_match(oracle_text)
        || shrink_re().is_match(oracle_text)
        || edict_re().is_match(oracle_text)
        || exile_target_re().is_match(oracle_text)
        || power_damage_re().is_match(oracle_text)
    {
        push_unique(&mut roles, Role::Removal);
    }
    if oracle_text.contains("Draw a card")
        || draw_re().is_match(oracle_text)
        || dig_re().is_match(oracle_text)
        || impulse_re().is_match(oracle_text)
        || investigate_re().is_match(oracle_text)
        || loot_re().is_match(oracle_text)
    {
        push_unique(&mut roles, Role::Draw);
    }
    let kw_lower: Vec<String> = keywords.iter().map(|k| k.to_ascii_lowercase()).collect();
    let has_protection = kw_lower
        .iter()
        .any(|k| k == "hexproof" || k == "indestructible" || k == "ward")
        || oracle_text.contains("gains hexproof")
        || oracle_text.contains("gains indestructible")
        || oracle_text.contains("gains ward");
    if has_protection {
        push_unique(&mut roles, Role::Protection);
    }

    let is_creature = front_type.contains("Creature");
    if is_creature {
        if let Some(p) = power {
            let evasion = ["flying", "trample", "menace", "haste"];
            let has_evasion = kw_lower.iter().any(|k| evasion.contains(&k.as_str()));
            if p >= 3 {
                push_unique(&mut roles, Role::Threat);
            }
            if p >= 5 || (p >= 4 && has_evasion) {
                push_unique(&mut roles, Role::Finisher);
            }
        }
    }
    if front_type.contains("Planeswalker") {
        push_unique(&mut roles, Role::Threat);
    }

    sort_by_all(roles)
}

/// Union of tag-derived and card-text-derived roles, deduped, stable order by Role::ALL.
pub fn classify(
    index: &TagIndex,
    tag_ids: &[String],
    type_line: &str,
    oracle_text: &str,
    keywords: &[String],
    power: Option<i32>,
) -> Vec<Role> {
    let mut combined = roles_from_tags(index, tag_ids);
    for r in roles_from_card(type_line, oracle_text, keywords, power) {
        push_unique(&mut combined, r);
    }
    sort_by_all(combined)
}

/// Theme tags for synergy = slugs whose ancestry root is NOT in META_ROOTS.
pub fn theme_slugs(index: &TagIndex, tag_ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tid in tag_ids {
        let anc = index.ancestry(tid);
        if anc.is_empty() {
            continue;
        }
        let root = anc.last().expect("non-empty");
        if META_ROOTS.iter().any(|m| *m == root.as_str()) {
            continue;
        }
        let slug = anc.first().expect("non-empty").clone();
        if !out.contains(&slug) {
            out.push(slug);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, slug: &str, parent: Option<&str>) -> IngestTag {
        IngestTag {
            tag_id: id.into(),
            slug: slug.into(),
            label: slug.into(),
            parent_id: parent.map(String::from),
        }
    }

    fn idx() -> TagIndex {
        TagIndex::new(&[
            t("t-removal", "removal", None),
            t("t-spot", "spot-removal", Some("t-removal")),
            t("t-sweeper", "sweeper", Some("t-removal")),
            t("t-oneside", "sweeper-one-sided", Some("t-sweeper")),
            t("t-ramp", "ramp", None),
            t("t-mp", "mana-producer", Some("t-ramp")),
            t("t-dork", "mana-dork", Some("t-mp")),
            t("t-ca", "card-advantage", None),
            t("t-draw", "draw", Some("t-ca")),
            t("t-cantrip", "cantrip", Some("t-draw")),
            t("t-lg", "lifegain-matters", None),
            t("t-names", "card-names", None),
            t("t-allit", "alliteration", Some("t-names")),
            t("t-loop-a", "loop-a", Some("t-loop-b")),
            t("t-loop-b", "loop-b", Some("t-loop-a")),
        ])
    }

    #[test]
    fn ancestry_walks_to_root_and_survives_cycles() {
        let i = idx();
        assert_eq!(i.ancestry("t-dork"), vec!["mana-dork", "mana-producer", "ramp"]);
        assert!(i.ancestry("t-loop-a").len() <= 32);
    }

    #[test]
    fn tag_roles() {
        let i = idx();
        assert_eq!(roles_from_tags(&i, &["t-spot".into()]), vec![Role::Removal]);
        assert_eq!(roles_from_tags(&i, &["t-oneside".into()]), vec![Role::Wipe]);
        assert_eq!(
            roles_from_tags(&i, &["t-dork".into(), "t-cantrip".into()]),
            vec![Role::Ramp, Role::Draw]
        );
        assert_eq!(roles_from_tags(&i, &["t-lg".into()]), vec![Role::Payoff]);
        assert!(roles_from_tags(&i, &["t-allit".into()]).is_empty());
    }

    #[test]
    fn fallback_roles() {
        assert_eq!(
            roles_from_card("Basic Land — Forest", "({T}: Add {G}.)", &[], None),
            vec![Role::Land]
        );
        assert_eq!(
            roles_from_card("Creature — Elf Druid", "{T}: Add {G}.", &[], Some(1)),
            vec![Role::Ramp]
        );
        assert_eq!(
            roles_from_card("Instant", "Counter target spell.", &[], None),
            vec![Role::Counter]
        );
        assert_eq!(
            roles_from_card("Sorcery", "Destroy all creatures.", &[], None),
            vec![Role::Wipe]
        );
        assert_eq!(
            roles_from_card("Instant", "Destroy target creature.", &[], None),
            vec![Role::Removal]
        );
        assert_eq!(
            roles_from_card("Creature — Dragon", "", &["Flying".into()], Some(4)),
            vec![Role::Threat, Role::Finisher]
        );
        assert_eq!(
            roles_from_card("Creature — Bear", "", &[], Some(2)),
            Vec::<Role>::new()
        );
    }

    #[test]
    fn extended_fallback_roles() {
        let r = |t: &str, o: &str| roles_from_card(t, o, &[], None);
        // Removal
        assert_eq!(
            r("Instant", "Return target creature to its owner's hand."),
            vec![Role::Removal]
        );
        assert_eq!(
            r(
                "Sorcery",
                "Target creature you control fights target creature you don't control."
            ),
            vec![Role::Removal]
        );
        assert_eq!(
            r("Instant", "Target creature gets -3/-3 until end of turn."),
            vec![Role::Removal]
        );
        assert_eq!(
            r("Sorcery", "Each opponent sacrifices a creature."),
            vec![Role::Removal]
        );
        assert_eq!(
            r("Instant", "Exile target artifact or enchantment."),
            vec![Role::Removal]
        );
        assert_eq!(
            r(
                "Instant",
                "Target creature deals damage equal to its power to any target."
            ),
            vec![Role::Removal]
        );
        // Wipe
        assert_eq!(
            r("Sorcery", "Each creature gets -2/-2 until end of turn."),
            vec![Role::Wipe]
        );
        assert_eq!(
            r("Sorcery", "All creatures get -1/-1 until end of turn."),
            vec![Role::Wipe]
        );
        assert_eq!(
            r(
                "Sorcery",
                "Anger of the Gods deals 3 damage to each creature."
            ),
            vec![Role::Wipe]
        );
        // Ramp
        assert_eq!(
            r(
                "Creature — Goblin",
                "When this creature enters, create a Treasure token."
            ),
            vec![Role::Ramp]
        );
        assert_eq!(
            r("Artifact", "{1}, {T}: Add one mana of any color."),
            vec![Role::Ramp]
        );
        assert_eq!(
            r(
                "Enchantment",
                "Creature spells you cast cost {1} less to cast."
            ),
            vec![Role::Ramp]
        );
        // Draw
        assert_eq!(
            r(
                "Sorcery",
                "Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom."
            ),
            vec![Role::Draw]
        );
        assert_eq!(
            r(
                "Instant",
                "Exile the top card of your library. Until end of turn, you may play that card."
            ),
            vec![Role::Draw]
        );
        assert_eq!(
            r(
                "Creature — Human",
                "When this creature enters, investigate."
            ),
            vec![Role::Draw]
        );
        assert_eq!(
            r("Sorcery", "Discard a card. If you do, draw two cards."),
            vec![Role::Draw]
        );
        // Negatives
        assert_eq!(r("Instant", "Scry 2."), Vec::<Role>::new());
        assert_eq!(r("Instant", "Surveil 2."), Vec::<Role>::new());
        // Land text must not become Ramp
        assert_eq!(r("Land", "{T}: Add {G}."), vec![Role::Land]);
    }

    #[test]
    fn theme_slugs_drop_meta_roots() {
        let i = idx();
        assert_eq!(
            theme_slugs(&i, &["t-allit".into(), "t-lg".into(), "t-dork".into()]),
            vec!["lifegain-matters", "mana-dork"]
        );
    }
}
