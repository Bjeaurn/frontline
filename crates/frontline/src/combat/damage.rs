//! Damage of a single shot.

use crate::data::{MatchRules, WeaponStats};
use crate::gear::{PerkEffects, StatMods};

/// Which Hit Zone a shot struck (CONTEXT.md: Hit Zone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitZone {
    Head,
    UpperTorso,
    LowerTorso,
    /// Arms and legs.
    Limbs,
}

impl std::fmt::Display for HitZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HitZone::Head => "head",
            HitZone::UpperTorso => "upper torso",
            HitZone::LowerTorso => "lower torso",
            HitZone::Limbs => "limbs",
        })
    }
}

/// Damage dealt by one shot of `stats` that travelled `distance` metres and struck `zone`.
///
/// Base damage is `damage_near` up to `falloff_start`, `damage_far` from `falloff_end` on and
/// linear in between, with the falloff distances scaled by the shooter's Attachments (`mods`);
/// the Weapon's multiplier for `zone`, the shooter's Perks and the Match Rules' `damage_scale`
/// apply on top.
pub fn shot_damage(
    stats: &WeaponStats,
    mods: &StatMods,
    perks: &PerkEffects,
    distance: f32,
    zone: HitZone,
    rules: &MatchRules,
) -> f32 {
    let falloff_start = stats.falloff_start * mods.falloff_mul;
    let falloff_end = stats.falloff_end * mods.falloff_mul;
    let span = falloff_end - falloff_start;
    let falloff = if span > 0.0 {
        ((distance - falloff_start) / span).clamp(0.0, 1.0)
    } else if distance >= falloff_end {
        1.0
    } else {
        0.0
    };
    let base = stats.damage_near + (stats.damage_far - stats.damage_near) * falloff;
    let multipliers = &stats.zone_multipliers;
    let zone_scale = match zone {
        HitZone::Head => multipliers.head,
        HitZone::UpperTorso => multipliers.upper_torso,
        HitZone::LowerTorso => multipliers.lower_torso,
        HitZone::Limbs => multipliers.limbs,
    };
    base * zone_scale * perks.damage_mul * rules.damage_scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{FallDamageRules, HudRules};

    fn rules(damage_scale: f32) -> MatchRules {
        MatchRules {
            max_health: 100.0,
            regen_delay_secs: 5.0,
            regen_per_sec: 20.0,
            respawn_delay_secs: 3.0,
            min_respawn_secs: 1.5,
            damage_scale,
            friendly_fire: false,
            fall_damage: FallDamageRules {
                min_height: 4.5,
                max_height: 12.0,
            },
            hud: HudRules {
                crosshair: true,
                hitmarkers: true,
                kill_hitmarker: true,
                kill_feed: true,
                kill_feed_delay_secs: 0.0,
                damage_direction: true,
                name_tags: true,
            },
        }
    }

    /// Damage with no Attachments, no Perks and the given Match Rules.
    fn plain(stats: &WeaponStats, distance: f32, zone: HitZone, rules: &MatchRules) -> f32 {
        shot_damage(
            stats,
            &StatMods::default(),
            &PerkEffects::default(),
            distance,
            zone,
            rules,
        )
    }

    #[test]
    fn falloff_is_flat_then_linear_then_clamped() {
        // Near 30 up to 10 m, far 20 from 30 m on; upper torso is 1x.
        let stats = WeaponStats::fixture();
        let chest = |distance| plain(&stats, distance, HitZone::UpperTorso, &rules(1.0));
        assert_eq!(chest(0.0), 30.0);
        assert_eq!(chest(10.0), 30.0);
        assert_eq!(chest(20.0), 25.0);
        assert_eq!(chest(30.0), 20.0);
        assert_eq!(chest(500.0), 20.0);

        // No falloff span: a step at the falloff distance.
        let step = WeaponStats {
            falloff_end: 10.0,
            ..WeaponStats::fixture()
        };
        assert_eq!(plain(&step, 9.9, HitZone::UpperTorso, &rules(1.0)), 30.0);
        assert_eq!(plain(&step, 10.0, HitZone::UpperTorso, &rules(1.0)), 20.0);
    }

    #[test]
    fn each_zone_applies_its_own_multiplier() {
        // Fixture multipliers: head 2.0, upper 1.0, lower 0.9, limbs 0.5; 25 base at 20 m.
        let stats = WeaponStats::fixture();
        let at = |zone| plain(&stats, 20.0, zone, &rules(1.0));
        assert_eq!(at(HitZone::Head), 50.0);
        assert_eq!(at(HitZone::UpperTorso), 25.0);
        assert_eq!(at(HitZone::LowerTorso), 22.5);
        assert_eq!(at(HitZone::Limbs), 12.5);
    }

    #[test]
    fn match_scale_multiplies_zone_damage() {
        let stats = WeaponStats::fixture();
        assert_eq!(plain(&stats, 20.0, HitZone::Head, &rules(0.5)), 25.0);
        assert_eq!(plain(&stats, 20.0, HitZone::UpperTorso, &rules(0.5)), 12.5);
    }

    #[test]
    fn attachments_move_the_falloff_and_perks_scale_the_damage() {
        // Falloff x0.5: 5 m to 15 m, so 10 m is halfway (25) where it was still full (30).
        let stats = WeaponStats::fixture();
        let mods = StatMods {
            falloff_mul: 0.5,
            ..StatMods::default()
        };
        let none = PerkEffects::default();
        let plain_mods = StatMods::default();
        let chest = |mods: &StatMods, perks: &PerkEffects, distance| {
            shot_damage(
                &stats,
                mods,
                perks,
                distance,
                HitZone::UpperTorso,
                &rules(1.0),
            )
        };
        assert_eq!(chest(&mods, &none, 10.0), 25.0);
        assert_eq!(chest(&plain_mods, &none, 10.0), 30.0);

        let stopping = PerkEffects {
            damage_mul: 1.25,
            ..none
        };
        assert_eq!(chest(&plain_mods, &stopping, 0.0), 37.5);
    }
}
