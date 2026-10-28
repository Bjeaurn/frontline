//! The pure rules of aiming steadiness: idle sway while aimed, hold breath and flinch. The
//! systems in `mod.rs` apply them to the view.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::shared::Stance;

/// Seconds of breath a full tank holds, and how long the player then pants.
pub const HOLD_SECS: f32 = 4.0;
pub const WINDED_SECS: f32 = 2.0;
/// Sway multiplier while holding breath and while out of breath.
pub const HOLD_SWAY: f32 = 0.15;
pub const WINDED_SWAY: f32 = 1.6;
/// Breath regained per second (of tank seconds) while not trying to hold.
const RECOVER_PER_SEC: f32 = 1.0;
/// Sway multiplier while crouched and prone.
const CROUCH_SWAY: f32 = 0.7;
const PRONE_SWAY: f32 = 0.4;
/// Seconds one full lap of the figure-eight takes.
pub const SWAY_PERIOD_SECS: f32 = 6.0;
/// Pitch swing of the figure-eight relative to its yaw swing.
const SWAY_PITCH_RATIO: f32 = 0.5;
/// Flinch multiplier when fully aimed.
pub const FLINCH_ADS_SCALE: f32 = 0.6;
/// The damage a Weapon's `flinch_deg` is quoted at, and the range the damage scales it within.
const FLINCH_REFERENCE_DAMAGE: f32 = 50.0;
const FLINCH_DAMAGE_RANGE: (f32, f32) = (0.2, 2.0);

/// Sway multiplier of a stance.
pub fn stance_sway(stance: Stance) -> f32 {
    match stance {
        Stance::Standing => 1.0,
        Stance::Crouching => CROUCH_SWAY,
        Stance::Prone => PRONE_SWAY,
    }
}

/// Peak sway angle in degrees: the Weapon's base amplitude through the sight's sway scale,
/// the stance and the breath, blended in with the aim (`ads`, 0 to 1).
pub fn sway_amplitude_deg(
    base_deg: f32,
    sway_scale: f32,
    stance: Stance,
    breath: f32,
    ads: f32,
) -> f32 {
    base_deg * sway_scale * stance_sway(stance) * breath * ads
}

/// The slow figure-eight at time `t` seconds as a (yaw, pitch) offset: yaw swings once per lap
/// within -1..1, pitch twice within `SWAY_PITCH_RATIO`, so the sight crosses the centre twice
/// per lap. Zero at `t = 0`.
pub fn sway_curve(t: f32) -> Vec2 {
    let phase = t / SWAY_PERIOD_SECS * TAU;
    Vec2::new(phase.sin(), (2.0 * phase).sin() * SWAY_PITCH_RATIO)
}

/// The view kick of a hit in degrees: the Weapon's `flinch_deg` scaled by how hard the hit was
/// and reduced while aimed (`ads`, 0 to 1).
pub fn flinch_deg(flinch_deg: f32, damage: f32, ads: f32) -> f32 {
    let (least, most) = FLINCH_DAMAGE_RANGE;
    let strength = (damage / FLINCH_REFERENCE_DAMAGE).clamp(least, most);
    flinch_deg * strength * 1.0_f32.lerp(FLINCH_ADS_SCALE, ads)
}

/// The breath tank of hold breath: up to `HOLD_SECS` of steadiness, then `WINDED_SECS` of
/// worse sway. The tank only refills while the player isn't trying to hold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Breath {
    /// Seconds of breath spent, 0 (full tank) to `HOLD_SECS` (empty).
    used: f32,
    /// Seconds of being out of breath left.
    winded: f32,
    holding: bool,
}

impl Breath {
    /// Advances `dt` seconds with the hold key down (`wants`, already checked against a sight
    /// that allows it and being aimed).
    pub fn tick(&mut self, dt: f32, wants: bool) {
        if self.winded > 0.0 {
            self.winded = (self.winded - dt).max(0.0);
            self.holding = false;
        } else if wants && self.used < HOLD_SECS {
            self.used += dt;
            self.holding = true;
            if self.used >= HOLD_SECS {
                self.used = HOLD_SECS;
                self.winded = WINDED_SECS;
                self.holding = false;
            }
        } else {
            self.holding = false;
            if !wants {
                self.used = (self.used - RECOVER_PER_SEC * dt).max(0.0);
            }
        }
    }

    /// Holding breath right now.
    pub fn holding(&self) -> bool {
        self.holding
    }

    /// Out of breath: sway is worse and no breath can be held.
    pub fn winded(&self) -> bool {
        self.winded > 0.0
    }

    /// Breath left in the tank, 0 (empty) to 1 (full).
    pub fn tank(&self) -> f32 {
        1.0 - self.used / HOLD_SECS
    }

    /// What the breath does to the sway amplitude.
    pub fn sway_mul(&self) -> f32 {
        if self.winded() {
            WINDED_SWAY
        } else if self.holding {
            HOLD_SWAY
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sway_is_a_slow_figure_eight_inside_its_amplitude() {
        assert_eq!(sway_curve(0.0), Vec2::ZERO);
        let samples: Vec<Vec2> = (0..600).map(|i| sway_curve(i as f32 * 0.01)).collect();
        assert!(
            samples
                .iter()
                .all(|s| s.x.abs() <= 1.0 && s.y.abs() <= 0.5 + 1e-6)
        );
        let widest = samples.iter().map(|s| s.x).fold(0.0, f32::max);
        let tallest = samples.iter().map(|s| s.y).fold(0.0, f32::max);
        assert!(widest > 0.99 && tallest > 0.45, "it uses its whole range");
        // One lap is one figure-eight: it passes through the centre twice and repeats.
        let lap = sway_curve(SWAY_PERIOD_SECS);
        assert!(lap.length() < 1e-4, "a lap ends where it began");
        let half = sway_curve(SWAY_PERIOD_SECS / 2.0);
        assert!(half.length() < 1e-4, "and crosses the centre half-way");
        // Slow: it never moves faster than a few amplitudes per second.
        let fastest = samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).length() / 0.01)
            .fold(0.0, f32::max);
        assert!(fastest < 4.0, "{fastest}");
    }

    #[test]
    fn amplitude_follows_scale_stance_breath_and_aim() {
        let standing = sway_amplitude_deg(1.0, 1.3, Stance::Standing, 1.0, 1.0);
        assert!((standing - 1.3).abs() < 1e-6);
        let crouched = sway_amplitude_deg(1.0, 1.3, Stance::Crouching, 1.0, 1.0);
        let prone = sway_amplitude_deg(1.0, 1.3, Stance::Prone, 1.0, 1.0);
        assert!((crouched / standing - 0.7).abs() < 1e-6);
        assert!((prone / standing - 0.4).abs() < 1e-6);
        let held = sway_amplitude_deg(1.0, 1.3, Stance::Standing, HOLD_SWAY, 1.0);
        assert!((held / standing - 0.15).abs() < 1e-6);
        let winded = sway_amplitude_deg(1.0, 1.3, Stance::Standing, WINDED_SWAY, 1.0);
        assert!((winded / standing - 1.6).abs() < 1e-6);
        assert_eq!(
            sway_amplitude_deg(1.0, 1.3, Stance::Standing, 1.0, 0.0),
            0.0
        );
    }

    #[test]
    fn a_hit_kicks_by_damage_and_less_when_aimed() {
        let hip = flinch_deg(2.0, 50.0, 0.0);
        assert!((hip - 2.0).abs() < 1e-6);
        assert!(
            (flinch_deg(2.0, 50.0, 1.0) - 1.2).abs() < 1e-6,
            "x0.6 aimed"
        );
        assert!(flinch_deg(2.0, 100.0, 0.0) > hip);
        assert!(flinch_deg(2.0, 10.0, 0.0) < hip);
        assert!(flinch_deg(2.0, 1000.0, 0.0) <= 2.0 * 2.0 + 1e-6, "capped");
        assert!(flinch_deg(2.0, 0.0, 0.0) > 0.0, "a graze still nudges");
    }

    #[test]
    fn holding_steadies_for_four_seconds_then_pants_for_two() {
        let mut breath = Breath::default();
        assert_eq!(breath.sway_mul(), 1.0);
        breath.tick(0.1, true);
        assert!(breath.holding() && breath.sway_mul() == HOLD_SWAY);
        // Held for 3.5 s in all: still steady.
        for _ in 0..34 {
            breath.tick(0.1, true);
        }
        assert!(breath.holding());
        // The tank runs dry at 4 s: out of breath.
        let mut held = 3.5;
        while !breath.winded() {
            breath.tick(0.1, true);
            held += 0.1;
            assert!(held < 4.3, "never ran out");
        }
        assert!(held > 3.9 && !breath.holding());
        assert_eq!(breath.sway_mul(), WINDED_SWAY);
        // Out of breath for two seconds, whatever the key does.
        for _ in 0..19 {
            breath.tick(0.1, true);
        }
        assert!(breath.winded(), "still panting after 1.9 s");
        breath.tick(0.2, true);
        assert!(!breath.winded());
        // Still holding the key with an empty tank: no steadiness, no recovery.
        breath.tick(1.0, true);
        assert!(!breath.holding() && breath.sway_mul() == 1.0);
        assert_eq!(breath.tank(), 0.0);
    }

    #[test]
    fn the_tank_refills_only_while_not_holding() {
        let mut breath = Breath::default();
        for _ in 0..20 {
            breath.tick(0.1, true);
        }
        assert!((breath.tank() - 0.5).abs() < 1e-4, "2 s of 4 spent");
        // Letting go refills it at one tank-second per second.
        for _ in 0..10 {
            breath.tick(0.1, false);
        }
        assert!((breath.tank() - 0.75).abs() < 1e-4);
        assert!(!breath.holding() && breath.sway_mul() == 1.0);
        // Holding again spends it from where it stands: 3 s left of breath.
        for _ in 0..29 {
            breath.tick(0.1, true);
        }
        assert!(breath.holding());
        for _ in 0..2 {
            breath.tick(0.1, true);
        }
        assert!(breath.winded(), "the partly refilled tank ran out");
        // Two shorter holds with a breather between never get out of breath.
        let mut breath = Breath::default();
        for _ in 0..30 {
            breath.tick(0.1, true);
        }
        for _ in 0..10 {
            breath.tick(0.1, false);
        }
        for _ in 0..15 {
            breath.tick(0.1, true);
        }
        assert!(!breath.winded() && breath.holding());
        // A full tank doesn't go past full.
        for _ in 0..100 {
            breath.tick(0.1, false);
        }
        assert_eq!(breath.tank(), 1.0);
    }
}
