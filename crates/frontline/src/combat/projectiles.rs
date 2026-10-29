//! Projectile Weapons (docs/adr/0005): a shot with a `projectile` model becomes a `Flying`
//! bullet that the server steps every fixed tick. Each step's segment is cast against the
//! World and the hitboxes as posed that tick; a player hit is the same `PlayerHit` a hitscan
//! hit makes, with the path length flown as its `distance`.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::hitbox::{Hitbox, HitboxOf, Hitboxes};
use super::match_flow::is_live;
use super::shots::PlayerHit;
use crate::ballistics::Bullet;
use crate::data::{GameData, ProjectileModel, WeaponId};
use crate::gear::Attachments;
use crate::net::protocol::{EventChannel, FireShot, ProjectileFired, ProjectileLanded};
use crate::shared::{GameLayer, MatchState};

/// One bullet in the air.
#[derive(Debug)]
struct Flying {
    id: u32,
    shooter: Entity,
    weapon: WeaponId,
    attachments: Attachments,
    origin: Vec3,
    bullet: Bullet,
    /// Metres of path flown so far, for the range limit and damage falloff.
    travelled: f32,
    /// Seconds in the air.
    age: f32,
}

/// Every bullet in the air, and the id of the next one.
#[derive(Resource, Default, Debug)]
pub(super) struct Bullets {
    next_id: u32,
    live: Vec<Flying>,
}

impl Bullets {
    /// Launches the bullet of `shot` along `direction` and returns the message that tells the
    /// clients.
    pub(super) fn launch(
        &mut self,
        shooter: Entity,
        peer: PeerId,
        shot: &FireShot,
        direction: Dir3,
        model: ProjectileModel,
    ) -> ProjectileFired {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let bullet = Bullet::fired(shot.origin, *direction, model);
        self.live.push(Flying {
            id,
            shooter,
            weapon: shot.weapon,
            attachments: shot.attachments,
            origin: shot.origin,
            bullet,
            travelled: 0.0,
            age: 0.0,
        });
        debug!(
            "Projectile #{id} fired by {peer} ({:?}) at {:.0} m/s",
            shot.weapon, model.muzzle_speed
        );
        ProjectileFired {
            id,
            shooter: peer,
            weapon: shot.weapon,
            attachments: shot.attachments,
            origin: shot.origin,
            velocity: bullet.velocity,
        }
    }
}

/// Steps every bullet one fixed tick; ends each at its first hit or at its Weapon's range.
/// While the Match isn't live the bullets in the air are dropped.
pub(super) fn fly_bullets(
    time: Res<Time>,
    mut bullets: ResMut<Bullets>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    spatial: SpatialQuery,
    shooters: Query<&Hitboxes>,
    hitboxes: Query<(&Hitbox, &HitboxOf)>,
    mut landings: Query<&mut MessageSender<ProjectileLanded>, (With<ClientOf>, With<Connected>)>,
    mut hits: MessageWriter<PlayerHit>,
) {
    if !is_live(&state) {
        bullets.live.clear();
        return;
    }
    let dt = time.delta_secs();
    bullets.live.retain_mut(|flying| {
        let stats = data.weapon(flying.weapon);
        let Some(model) = stats.projectile else {
            return false;
        };
        flying.age += dt;
        let start = flying.bullet.step(model, dt);
        let segment = flying.bullet.position - start;
        let length = segment.length().min(stats.range - flying.travelled);
        let Ok(direction) = Dir3::new(segment) else {
            return false;
        };
        let own = shooters.get(flying.shooter).ok();
        let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox])
            .with_excluded_entities(own.into_iter().flat_map(|boxes| boxes.entities()));
        let hit = spatial.cast_ray(start, direction, length, true, &filter);

        let (end, struck) = match hit {
            Some(hit) => {
                if let Ok((hitbox, HitboxOf(victim))) = hitboxes.get(hit.entity) {
                    hits.write(PlayerHit {
                        shooter: flying.shooter,
                        victim: *victim,
                        weapon: flying.weapon,
                        attachments: flying.attachments,
                        zone: hitbox.zone,
                        distance: flying.travelled + hit.distance,
                        origin: flying.origin,
                    });
                }
                (start + *direction * hit.distance, true)
            }
            None => (start + *direction * length, false),
        };
        flying.travelled += hit.as_ref().map_or(length, |hit| hit.distance);
        if !struck && flying.travelled < stats.range {
            return true;
        }

        debug!(
            "Projectile #{} landed after {:.3} s and {:.1} m ({})",
            flying.id,
            flying.age,
            flying.travelled,
            if struck { "struck" } else { "out of range" }
        );
        let landed = ProjectileLanded {
            id: flying.id,
            position: end,
            struck,
        };
        for mut sender in &mut landings {
            sender.send::<EventChannel>(landed);
        }
        false
    });
}
