//! Turns each `FireShot` into a hit: hitscan Weapons get a raycast, a tracer for everyone else
//! and, on a player hit, a `PlayerHit` for `health::apply_hits`; Weapons with a `projectile`
//! model launch a bullet instead (`projectiles`, docs/adr/0005).

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::damage::HitZone;
use super::hitbox::{Hitbox, HitboxOf, Hitboxes};
use super::match_flow::is_live;
use super::projectiles::Bullets;
use crate::data::{GameData, WeaponId};
use crate::equipment::GrenadeKind;
use crate::gear::Attachments;
use crate::net::LinkPlayer;
use crate::net::protocol::{
    EffectChannel, EventChannel, FireShot, PlayerOwner, ProjectileFired, ShotTracer,
};
use crate::shared::{GameLayer, Health, MatchState};

/// What struck a player: a bullet, or a Grenade's blast.
#[derive(Clone, Copy, Debug)]
pub enum HitSource {
    Bullet {
        weapon: WeaponId,
        /// The Attachments fitted to it.
        attachments: Attachments,
        zone: HitZone,
        /// Distance the bullet travelled, for damage falloff.
        distance: f32,
    },
    /// A Grenade of `kind` went off in reach: `damage` is already the blast's falloff at the
    /// victim's distance.
    Blast { kind: GrenadeKind, damage: f32 },
}

/// Something struck a player: a shot hit their hitbox, or a blast reached them.
#[derive(bevy::ecs::message::Message, Clone, Copy, Debug)]
pub struct PlayerHit {
    pub shooter: Entity,
    pub victim: Entity,
    pub source: HitSource,
    /// Where the shot was fired from, or the blast went off: the victim's damage indicator
    /// points there.
    pub origin: Vec3,
}

/// Resolves every received shot while the Match is live; after it has ended shots are ignored.
pub(super) fn resolve_shots(
    mut links: Query<(Entity, &LinkPlayer, &mut MessageReceiver<FireShot>)>,
    shooters: Query<(&Health, &PlayerOwner, &Hitboxes)>,
    hitboxes: Query<(&Hitbox, &HitboxOf)>,
    mut tracer_senders: Query<
        (Entity, &mut MessageSender<ShotTracer>),
        (With<ClientOf>, With<Connected>),
    >,
    mut fired_senders: Query<
        &mut MessageSender<ProjectileFired>,
        (With<ClientOf>, With<Connected>),
    >,
    mut bullets: ResMut<Bullets>,
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
            if let Some(model) = data.weapon(shot.weapon).projectile {
                let fired = bullets.launch(player.0, owner.0, &shot, direction, model);
                for mut sender in &mut fired_senders {
                    sender.send::<EventChannel>(fired);
                }
                continue;
            }
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
                    source: HitSource::Bullet {
                        weapon: shot.weapon,
                        attachments: shot.attachments,
                        zone: hitbox.zone,
                        distance,
                    },
                    origin: shot.origin,
                });
            }

            let tracer = ShotTracer {
                shooter: owner.0,
                weapon: shot.weapon,
                attachments: shot.attachments,
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
