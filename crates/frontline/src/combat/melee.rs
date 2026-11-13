//! The knife on the server: each `Melee` strike sweeps a sphere of the knife's reach
//! (`KnifeTuning`) ahead of the striker's eye and, on a Hitbox with no World in between, makes
//! a `PlayerHit` for `health::apply_hits`. Clients are trusted (docs/adr/0002), but a strike
//! must start at the striker's eye and comes at most once per swing.

use std::time::Duration;

use avian3d::prelude::{Collider, ShapeCastConfig, SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::*;

use super::hitbox::{HitboxOf, Hitboxes};
use super::match_flow::is_live;
use super::shots::{HitSource, PlayerHit};
use crate::data::GameData;
use crate::net::LinkPlayer;
use crate::net::protocol::Melee;
use crate::shared::{GameLayer, Health, MatchState, PlayerPose};

/// A strike may start this far from the striker's eye (pose and eye height lag a little).
const MAX_STRIKE_OFFSET: f32 = 1.5;
/// Share of a swing that must pass before the next strike counts: some slack for the network.
const SWING_SLACK: f32 = 0.8;

/// When the player last struck (elapsed time), to hold them to one strike per swing.
#[derive(Component, Debug, Default)]
pub(super) struct LastStrike(Option<Duration>);

impl LastStrike {
    /// A strike at `now` counts once most of a swing of `swing_secs` has passed since the last.
    fn allow(&mut self, now: Duration, swing_secs: f32) -> bool {
        let wait = Duration::from_secs_f32(swing_secs * SWING_SLACK);
        if self.0.is_some_and(|last| now.saturating_sub(last) < wait) {
            return false;
        }
        self.0 = Some(now);
        true
    }
}

/// Resolves every received strike of a living player while the Match is live.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_melee(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<Melee>)>,
    mut strikers: Query<(&Health, &PlayerPose, &Hitboxes, &mut LastStrike)>,
    hitboxes: Query<&HitboxOf>,
    state: Single<&MatchState>,
    spatial: SpatialQuery,
    data: Res<GameData>,
    time: Res<Time>,
    mut hits: MessageWriter<PlayerHit>,
) {
    let live = is_live(&state);
    let knife = &data.knife;
    for (player, mut receiver) in &mut links {
        for strike in receiver.receive() {
            let Ok((health, pose, own, mut last)) = strikers.get_mut(player.0) else {
                continue;
            };
            let Ok(direction) = Dir3::new(strike.direction) else {
                continue;
            };
            let eye = pose.position + Vec3::Y * pose.stance.eye_height();
            if !live
                || health.is_dead()
                || !strike.origin.is_finite()
                || strike.origin.distance(eye) > MAX_STRIKE_OFFSET
                || !last.allow(time.elapsed(), knife.swing_secs)
            {
                continue;
            }
            let bodies = SpatialQueryFilter::from_mask(GameLayer::Hitbox)
                .with_excluded_entities(own.entities());
            let Some(hit) = spatial.cast_shape(
                &Collider::sphere(knife.radius),
                strike.origin,
                Quat::IDENTITY,
                direction,
                &ShapeCastConfig::from_max_distance(knife.range),
                &bodies,
            ) else {
                debug!("Knife strike from {:?} found no one", strike.origin);
                continue;
            };
            let Ok(HitboxOf(victim)) = hitboxes.get(hit.entity) else {
                continue;
            };
            // No knifing through a wall: the World may not stand between the eye and where the
            // sweep met the body.
            let reach = hit.distance;
            let walls = SpatialQueryFilter::from_mask(GameLayer::World);
            if reach > 0.0
                && spatial
                    .cast_ray(strike.origin, direction, reach, true, &walls)
                    .is_some()
            {
                debug!("Knife strike blocked by the World");
                continue;
            }
            hits.write(PlayerHit {
                shooter: player.0,
                victim: *victim,
                source: HitSource::Melee {
                    damage: knife.damage,
                },
                origin: strike.origin,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_strike_counts_per_swing() {
        let mut last = LastStrike::default();
        let at = Duration::from_secs_f32;
        assert!(last.allow(at(10.0), 0.6));
        assert!(!last.allow(at(10.2), 0.6), "still swinging");
        assert!(last.allow(at(10.5), 0.6), "most of a swing has passed");
        assert!(!last.allow(at(10.6), 0.6));
    }
}
