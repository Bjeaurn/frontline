//! Damage of a single shot.

use crate::data::{MatchRules, WeaponStats};

/// Which hitbox a shot struck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitZone {
    Head,
    Body,
}

/// Damage dealt by one shot of `stats` that travelled `distance` metres and struck `zone`.
///
/// Body damage is `damage_near` up to `falloff_start`, `damage_far` from `falloff_end` on and
/// linear in between; a headshot multiplies it by `headshot_multiplier`, and the Match Rules'
/// `damage_scale` applies on top.
pub fn shot_damage(stats: &WeaponStats, distance: f32, zone: HitZone, rules: &MatchRules) -> f32 {
    let span = stats.falloff_end - stats.falloff_start;
    let falloff = if span > 0.0 {
        ((distance - stats.falloff_start) / span).clamp(0.0, 1.0)
    } else if distance >= stats.falloff_end {
        1.0
    } else {
        0.0
    };
    let body = stats.damage_near + (stats.damage_far - stats.damage_near) * falloff;
    let zone_scale = match zone {
        HitZone::Head => stats.headshot_multiplier,
        HitZone::Body => 1.0,
    };
    body * zone_scale * rules.damage_scale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(damage_scale: f32) -> MatchRules {
        MatchRules {
            max_health: 100.0,
            regen_delay_secs: 5.0,
            regen_per_sec: 20.0,
            respawn_delay_secs: 3.0,
            damage_scale,
            friendly_fire: false,
        }
    }

    #[test]
    fn falloff_is_flat_then_linear_then_clamped() {
        // Near 30 up to 10 m, far 20 from 30 m on.
        let stats = WeaponStats::fixture();
        let body = |distance| shot_damage(&stats, distance, HitZone::Body, &rules(1.0));
        assert_eq!(body(0.0), 30.0);
        assert_eq!(body(10.0), 30.0);
        assert_eq!(body(20.0), 25.0);
        assert_eq!(body(30.0), 20.0);
        assert_eq!(body(500.0), 20.0);

        // No falloff span: a step at the falloff distance.
        let step = WeaponStats { falloff_end: 10.0, ..WeaponStats::fixture() };
        assert_eq!(shot_damage(&step, 9.9, HitZone::Body, &rules(1.0)), 30.0);
        assert_eq!(shot_damage(&step, 10.0, HitZone::Body, &rules(1.0)), 20.0);
    }

    #[test]
    fn headshot_and_match_scale_multiply_falloff_damage() {
        let stats = WeaponStats::fixture();
        assert_eq!(shot_damage(&stats, 20.0, HitZone::Head, &rules(1.0)), 50.0);
        assert_eq!(shot_damage(&stats, 20.0, HitZone::Head, &rules(0.5)), 25.0);
        assert_eq!(shot_damage(&stats, 20.0, HitZone::Body, &rules(0.5)), 12.5);
    }
}
