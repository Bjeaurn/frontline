//! Grenades on the server (`equipment`): each `ThrowGrenade` becomes a replicated `Grenade`
//! entity the server steps every fixed tick (gravity, bounces off the World, the fuse). Where
//! one goes off the effect is applied here: blast damage through the usual damage path
//! (`PlayerHit`, with a falloff over distance and only to players the blast can reach in a
//! straight line), a flash that blinds and deafens, a smoke cloud (clients draw it; it blocks
//! the view). Every client is told where it went off (`GrenadeDetonated`).

use avian3d::prelude::{Collider, ShapeCastConfig, SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::match_flow::is_live;
use super::shots::{HitSource, PlayerHit};
use crate::data::GameData;
use crate::equipment::{
    Body, Contact, Effect, Grenade, GrenadeKind, Step, blast_damage, flash_exposure,
};
use crate::net::LinkPlayer;
use crate::net::protocol::{EventChannel, Flashed, GrenadeDetonated, ThrowGrenade};
use crate::shared::{GameLayer, Health, MatchState, PlayerPose};

/// A thrown Grenade's flight and who threw it. Server-only; `Grenade` is what clients see.
#[derive(Component, Debug)]
pub(super) struct Thrown {
    body: Body,
    owner: Entity,
}

/// A Grenade went off here (server-internal, read by `apply_detonations`).
#[derive(bevy::ecs::message::Message, Clone, Copy, Debug)]
pub(super) struct Detonation {
    kind: GrenadeKind,
    position: Vec3,
    owner: Entity,
}

/// A throw may start this far from where the thrower stands (their eye, an arm's length out);
/// clients are trusted (docs/adr/0002) but not to teleport a Grenade across the Map.
const MAX_THROW_OFFSET: f32 = 3.0;
/// Where a blast or flash looks for a player: this share of their eye height, about the chest.
const BODY_TARGET_SHARE: f32 = 0.6;

/// Starts every received throw of a living player while the Match is live.
pub(super) fn receive_throws(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ThrowGrenade>)>,
    players: Query<(&Health, &PlayerPose)>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    let live = is_live(&state);
    for (link, mut receiver) in &mut links {
        for throw in receiver.receive() {
            let Ok((health, pose)) = players.get(link.0) else {
                continue;
            };
            let Ok(direction) = Dir3::new(throw.direction) else {
                continue;
            };
            if !throw.speed.is_finite() || throw.speed <= 0.0 {
                continue;
            }
            let eye = pose.position + Vec3::Y * pose.stance.eye_height();
            if !live || health.is_dead() || throw.origin.distance(eye) > MAX_THROW_OFFSET {
                continue;
            }
            let tuning = data.grenade(throw.kind);
            let speed = throw.speed.min(tuning.speed);
            info!(
                "{:?} thrown ({}) from {:?} at {:.1} m/s",
                throw.kind, tuning.name, throw.origin, speed
            );
            commands.spawn((
                Name::new("Grenade"),
                Grenade {
                    kind: throw.kind,
                    position: throw.origin,
                },
                Thrown {
                    body: Body::thrown(throw.origin, *direction, speed),
                    owner: link.0,
                },
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
    }
}

/// Steps every Grenade one fixed tick; one whose fuse is up goes off. While the Match isn't
/// live the Grenades in the air are dropped.
pub(super) fn fly_grenades(
    time: Res<Time>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    spatial: SpatialQuery,
    mut grenades: Query<(Entity, &mut Grenade, &mut Thrown)>,
    mut detonations: MessageWriter<Detonation>,
    mut commands: Commands,
) {
    let live = is_live(&state);
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, mut grenade, mut thrown) in &mut grenades {
        if !live {
            commands.entity(entity).try_despawn();
            continue;
        }
        let tuning = data.grenade(grenade.kind);
        let body = Collider::sphere(tuning.radius);
        let step = thrown
            .body
            .step(tuning, time.delta_secs(), |from, direction, length| {
                let direction = Dir3::new(direction).ok()?;
                spatial
                    .cast_shape(
                        &body,
                        from,
                        Quat::IDENTITY,
                        direction,
                        &ShapeCastConfig::from_max_distance(length),
                        &filter,
                    )
                    .map(|hit| Contact {
                        distance: hit.distance,
                        normal: if hit.normal1.length_squared() < 0.5 {
                            -*direction
                        } else {
                            hit.normal1
                        },
                    })
            });
        grenade.position = thrown.body.position;
        if step == Step::Detonate {
            detonations.write(Detonation {
                kind: grenade.kind,
                position: thrown.body.position,
                owner: thrown.owner,
            });
            commands.entity(entity).try_despawn();
        }
    }
}

/// Whether nothing of the World stands between `from` and `to`.
fn clear(spatial: &SpatialQuery, filter: &SpatialQueryFilter, from: Vec3, to: Vec3) -> bool {
    let along = to - from;
    Dir3::new(along).map_or(true, |direction| {
        spatial
            .cast_ray(from, direction, along.length(), true, filter)
            .is_none()
    })
}

/// Applies what each Grenade that went off does, and tells every client where it went off.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_detonations(
    mut detonations: MessageReader<Detonation>,
    data: Res<GameData>,
    spatial: SpatialQuery,
    players: Query<(Entity, &PlayerPose, &Health, &ControlledBy)>,
    mut hits: MessageWriter<PlayerHit>,
    mut flashes: Query<&mut MessageSender<Flashed>>,
    mut announce: Query<&mut MessageSender<GrenadeDetonated>, (With<ClientOf>, With<Connected>)>,
) {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for detonation in detonations.read() {
        let tuning = data.grenade(detonation.kind);
        let centre = detonation.position;
        let (radius, secs) = match tuning.effect {
            Effect::Blast { radius, .. } | Effect::Flash { radius, .. } => (radius, 0.0),
            Effect::Smoke { radius, secs } => (radius, secs),
        };
        info!("{:?} went off at {centre}", detonation.kind);
        let message = GrenadeDetonated {
            kind: detonation.kind,
            position: centre,
            radius,
            secs,
        };
        for mut sender in &mut announce {
            sender.send::<EventChannel>(message);
        }
        for (victim, pose, health, link) in &players {
            if health.is_dead() {
                continue;
            }
            let eye_height = pose.stance.eye_height();
            let target = pose.position + Vec3::Y * eye_height * BODY_TARGET_SHARE;
            let distance = centre.distance(target);
            match tuning.effect {
                Effect::Blast { damage, radius } => {
                    let damage = blast_damage(damage, radius, distance);
                    if damage > 0.0 && clear(&spatial, &filter, centre, target) {
                        hits.write(PlayerHit {
                            shooter: detonation.owner,
                            victim,
                            source: HitSource::Blast {
                                kind: detonation.kind,
                                damage,
                            },
                            origin: centre,
                        });
                    }
                }
                Effect::Flash {
                    radius,
                    blind_secs,
                    deaf_secs,
                } => {
                    let eye = pose.position + Vec3::Y * eye_height;
                    let to_flash = (centre - eye).normalize_or_zero();
                    let view = Quat::from_rotation_y(pose.yaw)
                        * Quat::from_rotation_x(pose.pitch)
                        * Vec3::NEG_Z;
                    let exposure = flash_exposure(
                        radius,
                        centre.distance(eye),
                        view.dot(to_flash),
                        clear(&spatial, &filter, centre, eye),
                    );
                    let flashed = Flashed {
                        blind_secs: exposure.blind * blind_secs,
                        deaf_secs: exposure.deaf * deaf_secs,
                    };
                    if (flashed.blind_secs > 0.05 || flashed.deaf_secs > 0.05)
                        && let Ok(mut sender) = flashes.get_mut(link.owner)
                    {
                        info!(
                            "Flash reached player at {:.1} m: blind {:.1} s, deaf {:.1} s",
                            centre.distance(eye),
                            flashed.blind_secs,
                            flashed.deaf_secs
                        );
                        sender.send::<EventChannel>(flashed);
                    }
                }
                Effect::Smoke { .. } => {}
            }
        }
    }
}
