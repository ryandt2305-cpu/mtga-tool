use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{Format, Playstyle, Role, TemplateOverrides, B, G, R, U, W};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub role: f32,
    pub synergy: f32,
    pub power: f32,
    pub ownership: f32,
    pub curve: f32,
    pub pip: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub format: Format,
    pub playstyle: Playstyle,
    pub deck_size: u32,
    pub lands: u32,
    pub role_min: HashMap<Role, u32>,
    pub role_max: HashMap<Role, u32>,
    pub curve: [f32; 8],
    pub weights: Weights,
    pub nonbasic_land_cap: u32,
}

impl Template {
    pub fn nonland(&self) -> u32 {
        self.deck_size - self.lands
    }

    pub fn min(&self, r: Role) -> u32 {
        self.role_min.get(&r).copied().unwrap_or(0)
    }

    pub fn max(&self, r: Role) -> u32 {
        self.role_max.get(&r).copied().unwrap_or(0)
    }
}

fn base_table(is_brawl100: bool, has_u: bool) -> [(Role, (u32, u32)); 12] {
    if is_brawl100 {
        [
            (Role::Ramp, (8, 12)),
            (Role::Draw, (8, 12)),
            (Role::Removal, (7, 11)),
            (Role::Wipe, (1, 3)),
            (Role::Counter, if has_u { (0, 4) } else { (0, 0) }),
            (Role::Protection, (2, 5)),
            (Role::Recursion, (0, 4)),
            (Role::Tutor, (0, 4)),
            (Role::Threat, (15, 40)),
            (Role::Finisher, (3, 8)),
            (Role::Payoff, (0, 20)),
            (Role::Utility, (0, 6)),
        ]
    } else {
        [
            (Role::Ramp, (4, 7)),
            (Role::Draw, (4, 7)),
            (Role::Removal, (4, 7)),
            (Role::Wipe, (0, 2)),
            (Role::Counter, if has_u { (0, 3) } else { (0, 0) }),
            (Role::Protection, (1, 3)),
            (Role::Recursion, (0, 2)),
            (Role::Tutor, (0, 2)),
            (Role::Threat, (9, 25)),
            (Role::Finisher, (2, 5)),
            (Role::Payoff, (0, 12)),
            (Role::Utility, (0, 4)),
        ]
    }
}

pub fn template_for(
    format: Format,
    playstyle: Playstyle,
    identity: u8,
    overrides: Option<&TemplateOverrides>,
) -> Template {
    let has_u = identity & U != 0;
    let is_brawl100 = matches!(format, Format::Brawl100);
    let mut lands: u32 = if is_brawl100 { 37 } else { 24 };

    let mut role_min: HashMap<Role, u32> = HashMap::new();
    let mut role_max: HashMap<Role, u32> = HashMap::new();
    for (r, (mn, mx)) in base_table(is_brawl100, has_u).iter() {
        role_min.insert(*r, *mn);
        role_max.insert(*r, *mx);
    }

    let mut curve = [0.02, 0.10, 0.24, 0.24, 0.18, 0.12, 0.06, 0.04];
    let mut weights = Weights {
        role: 0.28,
        synergy: 0.24,
        power: 0.24,
        ownership: 0.08,
        curve: 0.08,
        pip: 0.08,
    };

    match playstyle {
        Playstyle::Aggro => {
            lands = lands.saturating_sub(2);
            let threat_delta = if is_brawl100 { 7 } else { 5 };
            if let Some(v) = role_min.get_mut(&Role::Threat) {
                *v += threat_delta;
            }
            role_max.insert(Role::Wipe, 1);
            curve = [0.03, 0.18, 0.30, 0.24, 0.14, 0.07, 0.03, 0.01];
            weights = Weights {
                role: 0.28,
                synergy: 0.19,
                power: 0.29,
                ownership: 0.08,
                curve: 0.08,
                pip: 0.08,
            };
        }
        Playstyle::Control => {
            lands += 1;
            if let Some(v) = role_min.get_mut(&Role::Wipe) {
                *v += 1;
            }
            if has_u {
                if let Some(v) = role_min.get_mut(&Role::Counter) {
                    *v += 2;
                }
            }
            if let Some(v) = role_min.get_mut(&Role::Draw) {
                *v += 2;
            }
            let threat_delta = if is_brawl100 { 5 } else { 3 };
            if let Some(v) = role_min.get_mut(&Role::Threat) {
                *v = v.saturating_sub(threat_delta);
            }
            curve = [0.02, 0.06, 0.18, 0.24, 0.22, 0.15, 0.08, 0.05];
            weights = Weights {
                role: 0.28,
                synergy: 0.19,
                power: 0.29,
                ownership: 0.08,
                curve: 0.08,
                pip: 0.08,
            };
        }
        Playstyle::Midrange => {}
    }

    if let Some(o) = overrides {
        if let Some(l) = o.lands {
            lands = l;
        }
        for (r, v) in &o.role_min {
            role_min.insert(*r, *v);
        }
        for (r, v) in &o.role_max {
            role_max.insert(*r, *v);
        }
    }

    for r in Role::ALL.iter() {
        if *r == Role::Land {
            continue;
        }
        let mn = role_min.get(r).copied().unwrap_or(0);
        let mx = role_max.get(r).copied().unwrap_or(0);
        if mn > mx {
            role_min.insert(*r, mx);
        }
    }

    role_min.insert(Role::Land, lands);
    role_max.insert(Role::Land, lands);

    Template {
        format,
        playstyle,
        deck_size: format.deck_size(),
        lands,
        role_min,
        role_max,
        curve,
        weights,
        nonbasic_land_cap: lands / 3,
    }
}

pub fn all_default_templates() -> Vec<Template> {
    let identity = W | U | B | R | G;
    let mut v = Vec::new();
    for f in [Format::Brawl100, Format::StandardBrawl60] {
        for p in [Playstyle::Aggro, Playstyle::Midrange, Playstyle::Control] {
            v.push(template_for(f, p, identity, None));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brawl100_midrange_defaults() {
        let t = template_for(Format::Brawl100, Playstyle::Midrange, W | U | G, None);
        assert_eq!(t.deck_size, 99);
        assert_eq!(t.lands, 37);
        assert_eq!(t.nonland(), 62);
        assert_eq!(t.min(Role::Ramp), 8);
        assert_eq!(t.max(Role::Counter), 4);
        assert!((t.curve.iter().sum::<f32>() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn no_blue_disables_counters() {
        let t = template_for(Format::StandardBrawl60, Playstyle::Control, R | G, None);
        assert_eq!(t.max(Role::Counter), 0);
        assert_eq!(t.min(Role::Counter), 0);
        assert_eq!(t.lands, 25);
    }

    #[test]
    fn aggro_and_overrides() {
        let mut o = TemplateOverrides::default();
        o.lands = Some(30);
        o.role_min.insert(Role::Ramp, 20);
        o.role_max.insert(Role::Ramp, 10);
        let t = template_for(Format::Brawl100, Playstyle::Aggro, G, Some(&o));
        assert_eq!(t.lands, 30);
        assert_eq!(t.min(Role::Land), 30);
        assert_eq!(t.min(Role::Ramp), 10); // clamped to max
        assert_eq!(t.min(Role::Threat), 22);
    }

    #[test]
    fn all_default_templates_has_six() {
        let all = all_default_templates();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn weights_sum_to_one_and_include_pip() {
        for t in all_default_templates() {
            let w = t.weights;
            let sum = w.role + w.synergy + w.power + w.ownership + w.curve + w.pip;
            assert!((sum - 1.0).abs() < 1e-3, "{:?} {:?}", t.playstyle, w);
            assert!(w.pip > 0.0);
        }
    }
}
