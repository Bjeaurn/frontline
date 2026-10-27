//! Hitscan: turns each `FireShot` into a raycast, a tracer for everyone else and, on a player
//! hit, a `PlayerHit` for `health::apply_hits`.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::damage::HitZone;
use super::hitbox::{Hitbox, HitboxOf, Hitboxes};
use super::match_flow::is_live;
use crate::data::{GameData, WeaponId};
use crate::net::LinkPlayer;
use crate::net::protocol::{EffectChannel, FireShot, PlayerOwner, ShotTracer};
use crate::shared::{GameLayer, Health, MatchState};

/// A shot struck a player's hitbox.
#[derive(bevy::ecs::message::Message, Clone, Copy, Debug)]
pub struct PlayerHit {
    pub shooter: Entity,
    pub victim: Entity,
    pub weapon: WeaponId,
    pub zone: HitZone,
    /// Distance the bullet travelled, for damage falloff.
    pub distance: f32,
    /// Where the shot was fired from.
    pub origin: Vec3,
}

/// Resolves every received shot while the Match is live; after it has ended shots are ignored.
pub(super) fn resolve_shots(
    mut links: Query<(Entity, &LinkPlayer, &mut MessageReceiver<FireShot>)>,
    shooters: Query<(&Health, &PlayerOwner, &Hitboxes)>,
    hitboxes: Query<(&Hitbox, &HitboxOf)>,
    mut tracer_senders: Query<(Entity, &mut MessageSender<ShotTracer>), (With<ClientOf>, With<Connected>)>,
    state: Single<&MatchState>,
    spatial: SpatialQuery,
    data: Res<GameData>,
    mut hits: MessageWriter<PlayerHit>,
) {
    let live = is_live(&state);
    for (link, player, mut receiver) in &mut links {
        for shot in receiver.receive() {
            let Ok((health, owner, own_hitboxes)) = shooters.get(player.0) else {
                continue;
            };
            if !live || health.is_dead() {
                continue;
            }
            let Ok(direction) = Dir3::new(shot.direction) else {
                continue;
            };
            let range = data.weapon(shot.weapon).range;
            let filter = SpatialQueryFilter::from_mask([GameLayer::World, GameLayer::Hitbox])
                .with_excluded_entities(own_hitboxes.entities());
            let hit = spatial.cast_ray(shot.origin, direction, range, true, &filter);
            let distance = hit.map_or(range, |hit| hit.distance);

            if let Some((hitbox, HitboxOf(victim))) =
                hit.and_then(|hit| hitboxes.get(hit.entity).ok())
            {
                hits.write(PlayerHit {
                    shooter: player.0,
                    victim: *victim,
                    weapon: shot.weapon,
                    zone: hitbox.zone,
                    distance,
                    origin: shot.origin,
                });
            }

            let tracer = ShotTracer {
                shooter: owner.0,
                weapon: shot.weapon,
                origin: shot.origin,
                end: shot.origin + direction * distance,
            };
            for (other, mut sender) in &mut tracer_senders {
                if other != link {
                    sender.send::<EffectChannel>(tracer);
                }
            }
        }
    }
}
