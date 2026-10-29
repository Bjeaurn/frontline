//! Bullet flight for projectile Weapons (docs/adr/0005): pure, deterministic stepping that the
//! server flies each fixed tick (sweeping every step against the World and the hitboxes) and
//! the clients evaluate in closed form to draw the tracer along the very same path.

use bevy::prelude::*;

use crate::data::ProjectileModel;

/// Extra seconds a client keeps a tracer after the bullet could have flown its whole range,
/// in case its `ProjectileLanded` never arrives.
const TRACER_EXPIRY_MARGIN_SECS: f32 = 0.5;

/// A bullet in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bullet {
    pub position: Vec3,
    pub velocity: Vec3,
}

impl Bullet {
    /// A bullet leaving `origin` along `direction` (a unit vector) at the model's muzzle speed.
    pub fn fired(origin: Vec3, direction: Vec3, model: ProjectileModel) -> Self {
        Self {
            position: origin,
            velocity: direction * model.muzzle_speed,
        }
    }

    /// Advances `dt` seconds under gravity, exactly (constant acceleration), so the path does
    /// not depend on how time is sliced. Returns the position the step started from: the
    /// segment `start -> self.position` is what the step swept.
    pub fn step(&mut self, model: ProjectileModel, dt: f32) -> Vec3 {
        let start = self.position;
        let gravity = gravity(model);
        self.position += self.velocity * dt + 0.5 * gravity * dt * dt;
        self.velocity += gravity * dt;
        start
    }
}

fn gravity(model: ProjectileModel) -> Vec3 {
    Vec3::NEG_Y * model.gravity
}

/// Where a bullet fired from `origin` with `velocity` is `t` seconds later: the closed form
/// of repeated `Bullet::step`s, for clients that draw the tracer every frame.
pub fn position_at(origin: Vec3, velocity: Vec3, model: ProjectileModel, t: f32) -> Vec3 {
    origin + velocity * t + 0.5 * gravity(model) * t * t
}

/// Seconds to cover `distance` metres flying level (the flat-fire approximation: gravity's
/// extra path length is negligible next to the speed).
pub fn level_flight_secs(model: ProjectileModel, distance: f32) -> f32 {
    distance / model.muzzle_speed
}

/// How far a bullet fired level has fallen below the aim line after `distance` metres.
#[cfg(test)]
pub fn level_drop(model: ProjectileModel, distance: f32) -> f32 {
    let t = level_flight_secs(model, distance);
    0.5 * model.gravity * t * t
}

/// How long a client keeps a tracer without hearing it land.
pub fn tracer_expiry_secs(model: ProjectileModel, range: f32) -> f32 {
    level_flight_secs(model, range) + TRACER_EXPIRY_MARGIN_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: ProjectileModel = ProjectileModel {
        muzzle_speed: 600.0,
        gravity: 12.0,
    };
    const DT: f32 = 1.0 / 64.0;

    /// Distance from `point` to the segment `a -> b`.
    fn segment_distance(a: Vec3, b: Vec3, point: Vec3) -> f32 {
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 {
            ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (a + ab * t).distance(point)
    }

    /// The tick on which a bullet fired at `direction` first sweeps through a sphere that moves
    /// as `target(t)`; the server tests the segment of a tick against the target as posed at
    /// the tick's end.
    fn first_hit_tick(
        origin: Vec3,
        direction: Vec3,
        target: impl Fn(f32) -> Vec3,
        radius: f32,
    ) -> Option<u32> {
        let mut bullet = Bullet::fired(origin, direction, MODEL);
        for tick in 1..=200 {
            let start = bullet.step(MODEL, DT);
            if segment_distance(start, bullet.position, target(tick as f32 * DT)) <= radius {
                return Some(tick);
            }
        }
        None
    }

    #[test]
    fn stepping_follows_the_closed_form_whatever_the_step_size() {
        let direction = Vec3::new(0.3, 0.2, -1.0).normalize();
        let origin = Vec3::new(1.0, 1.7, 2.0);
        let velocity = direction * MODEL.muzzle_speed;
        for steps_per_sec in [64.0_f32, 240.0, 1000.0] {
            let mut bullet = Bullet::fired(origin, direction, MODEL);
            let steps = (steps_per_sec * 0.5) as u32;
            for _ in 0..steps {
                bullet.step(MODEL, 1.0 / steps_per_sec);
            }
            let exact = position_at(origin, velocity, MODEL, 0.5);
            assert!(
                bullet.position.distance(exact) < 0.01,
                "{steps_per_sec} Hz drifted to {} from {exact}",
                bullet.position
            );
        }
    }

    #[test]
    fn a_level_shot_takes_distance_over_speed_to_arrive() {
        assert!((level_flight_secs(MODEL, 300.0) - 0.5).abs() < 1e-6);
        // Stepped at 64 Hz it crosses 300 m on tick 32 (0.5 s), and not a tick earlier.
        let mut bullet = Bullet::fired(Vec3::ZERO, Vec3::X, MODEL);
        let mut ticks = 0;
        while bullet.position.x < 300.0 {
            bullet.step(MODEL, DT);
            ticks += 1;
        }
        assert_eq!(ticks, 32);
    }

    #[test]
    fn drop_grows_with_the_square_of_distance_and_is_small_up_close() {
        assert!(level_drop(MODEL, 80.0) < 0.12);
        assert!((level_drop(MODEL, 300.0) - 1.5).abs() < 1e-4);
        assert!((level_drop(MODEL, 600.0) / level_drop(MODEL, 300.0) - 4.0).abs() < 1e-4);
        // The stepped bullet agrees with the formula.
        let mut bullet = Bullet::fired(Vec3::ZERO, Vec3::X, MODEL);
        for _ in 0..32 {
            bullet.step(MODEL, DT);
        }
        assert!((-bullet.position.y - level_drop(MODEL, bullet.position.x)).abs() < 1e-3);
    }

    #[test]
    fn a_hit_on_a_moving_target_lands_later_than_hitscan_and_needs_lead() {
        let origin = Vec3::new(0.0, 1.7, 0.0);
        // A target 240 m away strafing across the line of fire at 6 m/s (0.5 m hit radius).
        let start = Vec3::new(240.0, 1.7, 0.0);
        let target = |t: f32| start + Vec3::Z * 6.0 * t;

        // Hitscan resolves at fire time against where the target is now: aimed straight at
        // it, that is a hit on tick 0.
        assert!(segment_distance(origin, origin + Vec3::X * 300.0, start) <= 0.5);

        // The same aim as a projectile misses: the target has moved on and the bullet has fallen.
        assert_eq!(first_hit_tick(origin, Vec3::X, target, 0.5), None);

        // Led and held over, it hits, but only after the flight time (about 0.4 s).
        let flight = level_flight_secs(MODEL, 240.0);
        let aim_point = target(flight) + Vec3::Y * level_drop(MODEL, 240.0);
        let tick = first_hit_tick(origin, (aim_point - origin).normalize(), target, 0.5)
            .expect("a led shot hits");
        let secs = tick as f32 * DT;
        assert!(secs > 0.0 && (secs - flight).abs() < 2.0 * DT, "{secs} s");
    }
}
