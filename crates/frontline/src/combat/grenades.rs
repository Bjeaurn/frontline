//! Grenades on the server (`equipment`): each `ThrowGrenade` becomes a replicated `Grenade`
//! entity the server steps every fixed tick (gravity, bounces off the World, sticking to a
//! surface or a player, the fuse, a mine's trigger). Where one goes off the effect is applied
//! here: blast damage through the usual damage path (`PlayerHit`, with a falloff over distance
//! and only to players the blast can reach in a straight line), a flash that blinds and deafens,
//! a stun that slows and shakes, a smoke or gas cloud (clients draw it and the gas works on
//! the player who stands in it; the server only keeps count of the live ones). Every client is
//! told where it went off (`GrenadeDetonated`).

use avian3d::prelude::{Collider, ShapeCastConfig, SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::match_flow::is_live;
use super::shots::{HitSource, PlayerHit};
use crate::data::GameData;
use crate::equipment::{
    Body, Contact, Effect, Fuse, Grenade, GrenadeKind, Step, blast_damage, can_stick_to,
    flash_exposure, mine_trips, oldest_beyond_cap, room_for, stun_exposure, touches_body,
};
use crate::gear::EquippedPerks;
use crate::net::LinkPlayer;
use crate::net::protocol::{EventChannel, Flashed, GrenadeDetonated, Stunned, ThrowGrenade};
use crate::shared::{GameLayer, Health, MatchState, PlayerPose, PlayerTeam};

/// A Grenade stuck to a player: it goes where they go, `offset` from their feet.
#[derive(Clone, Copy, Debug)]
struct Riding {
    player: Entity,
    offset: Vec3,
}

/// A thrown Grenade's flight and who threw it. Server-only; `Grenade` is what clients see.
#[derive(Component, Debug)]
pub(super) struct Thrown {
    body: Body,
    owner: Entity,
    riding: Option<Riding>,
}

/// A smoke or gas cloud that is still up, `0` seconds from its end. Server-only: it only
/// counts toward `MAX_CLOUDS`; the cloud itself is drawn, and the gas felt, on the clients.
#[derive(Component, Debug)]
pub(super) struct CloudTimer(f32);

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
/// Where a blast, flash, stun or mine looks for a player: this share of their eye height, about
/// the chest.
const BODY_TARGET_SHARE: f32 = 0.6;

/// Starts every received throw of a living player while the Match is live, unless the server
/// already holds as many Grenades or clouds as it allows.
pub(super) fn receive_throws(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ThrowGrenade>)>,
    players: Query<(&Health, &PlayerPose)>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    out: Query<&Grenade>,
    clouds: Query<(), With<CloudTimer>>,
    mut commands: Commands,
) {
    let live = is_live(&state);
    // (Grenades out, clouds up or coming), counted at the first throw and kept up to date.
    let mut counts: Option<(usize, usize)> = None;
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
            let (grenades, cloudy) = counts.get_or_insert_with(|| {
                (
                    out.iter().count(),
                    clouds.iter().count()
                        + out
                            .iter()
                            .filter(|grenade| data.grenade(grenade.kind).effect.makes_cloud())
                            .count(),
                )
            });
            let makes_cloud = tuning.effect.makes_cloud();
            if !room_for(makes_cloud, *grenades, *cloudy) {
                info!("{:?} dropped: too many out already", throw.kind);
                continue;
            }
            *grenades += 1;
            *cloudy += usize::from(makes_cloud);
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
                    armed: false,
                },
                Thrown {
                    body: Body::thrown(throw.origin, *direction, speed),
                    owner: link.0,
                    riding: None,
                },
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
    }
}

/// Holds each player to the number of mines their equipment allows: a fresh mine beyond it
/// removes their oldest.
pub(super) fn cap_mines(
    fresh: Query<(&Grenade, &Thrown), Added<Thrown>>,
    placed: Query<(Entity, &Grenade, &Thrown)>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    for (grenade, new) in &fresh {
        let tuning = data.grenade(grenade.kind);
        if !tuning.fuse.is_mine() {
            continue;
        }
        let mut down: Vec<(Entity, f32)> = placed
            .iter()
            .filter(|(_, other, thrown)| other.kind == grenade.kind && thrown.owner == new.owner)
            .map(|(entity, _, thrown)| (entity, thrown.body.age))
            .collect();
        for (entity, _) in oldest_beyond_cap(&mut down, tuning.count as usize) {
            commands.entity(*entity).try_despawn();
        }
    }
}

/// Steps every Grenade one fixed tick; one whose fuse is up, or a mine an enemy came near,
/// goes off. While the Match isn't live the Grenades in the air and the clouds are dropped.
#[allow(clippy::too_many_arguments)]
pub(super) fn fly_grenades(
    time: Res<Time>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    spatial: SpatialQuery,
    players: Query<(Entity, &PlayerPose, &Health, &PlayerTeam)>,
    mut grenades: Query<(Entity, &mut Grenade, &mut Thrown)>,
    mut clouds: Query<(Entity, &mut CloudTimer)>,
    mut detonations: MessageWriter<Detonation>,
    mut commands: Commands,
) {
    let live = is_live(&state);
    let dt = time.delta_secs();
    for (entity, mut cloud) in &mut clouds {
        cloud.0 -= dt;
        if !live || cloud.0 <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, mut grenade, mut thrown) in &mut grenades {
        if !live {
            commands.entity(entity).try_despawn();
            continue;
        }
        let tuning = data.grenade(grenade.kind);
        // Stuck to a player: it goes where they go, until they die or leave.
        if let Some(ride) = thrown.riding {
            match players.get(ride.player) {
                Ok((_, pose, health, _)) if !health.is_dead() => {
                    thrown.body.position = pose.position + ride.offset;
                }
                _ => {
                    thrown.body.release();
                    thrown.riding = None;
                }
            }
        }
        let flying = !thrown.body.anchored;
        let before = thrown.body.position;
        let body = Collider::sphere(tuning.radius);
        let step = thrown.body.step(tuning, dt, |from, direction, length| {
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
        if step == Step::Detonate {
            detonations.write(Detonation {
                kind: grenade.kind,
                position: thrown.body.position,
                owner: thrown.owner,
            });
            commands.entity(entity).try_despawn();
            continue;
        }
        // Flying through a player (a Semtex): it sticks to them where it touched them.
        if flying && tuning.fuse.sticks_to_players() {
            let after = thrown.body.position;
            let age = thrown.body.age;
            let owner = thrown.owner;
            let touched = players.iter().find_map(|(player, pose, health, _)| {
                if health.is_dead() || !can_stick_to(player == owner, age) {
                    return None;
                }
                let height = pose.stance.body_height();
                touches_body(before, after, tuning.radius, pose.position, height)
                    .map(|at| (player, pose.position, at))
            });
            if let Some((player, feet, at)) = touched {
                thrown.body.position = at;
                thrown.body.stick();
                thrown.riding = Some(Riding {
                    player,
                    offset: at - feet,
                });
            }
        }
        // A mine that has settled in goes off when an enemy of its owner comes near.
        let armed = thrown.body.armed(tuning.fuse);
        if let Fuse::Proximity { trigger_radius, .. } = tuning.fuse
            && armed
        {
            let Ok((.., owner_team)) = players.get(thrown.owner) else {
                // Whoever placed it left: so does it.
                commands.entity(entity).try_despawn();
                continue;
            };
            let position = thrown.body.position;
            let tripped = players.iter().any(|(player, pose, health, team)| {
                if player == thrown.owner
                    || health.is_dead()
                    || team.0.is_none()
                    || team.0 == owner_team.0
                {
                    return false;
                }
                let target = pose.position + Vec3::Y * pose.stance.eye_height() * BODY_TARGET_SHARE;
                let distance = position.distance(target);
                // Only look for a wall in the way of those near enough.
                distance <= trigger_radius
                    && mine_trips(
                        armed,
                        trigger_radius,
                        distance,
                        clear(&spatial, &filter, position, target),
                    )
            });
            if tripped {
                info!("A mine of {:?} was tripped at {position}", thrown.owner);
                detonations.write(Detonation {
                    kind: grenade.kind,
                    position,
                    owner: thrown.owner,
                });
                commands.entity(entity).try_despawn();
                continue;
            }
        }
        // Writing the replicated state only when it changed keeps a Grenade at rest quiet.
        if grenade.position != thrown.body.position {
            grenade.position = thrown.body.position;
        }
        if grenade.armed != armed {
            grenade.armed = armed;
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

/// A cloud is looked for this far above where it burst and up to `CLOUD_DROP` below that.
const CLOUD_RAISE: f32 = 0.2;
const CLOUD_DROP: f32 = 8.0;

/// Where a cloud that burst at `at` settles: on the floor `floor_distance` below the point
/// `CLOUD_RAISE` above it (the ray's hit distance), else where it burst.
fn settled_cloud(at: Vec3, floor_distance: Option<f32>) -> Vec3 {
    floor_distance.map_or(at, |distance| {
        Vec3::new(at.x, at.y + CLOUD_RAISE - distance, at.z)
    })
}

/// Applies what each Grenade that went off does, and tells every client where it went off.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_detonations(
    mut detonations: MessageReader<Detonation>,
    data: Res<GameData>,
    spatial: SpatialQuery,
    players: Query<(
        Entity,
        &PlayerPose,
        &Health,
        &ControlledBy,
        Option<&EquippedPerks>,
    )>,
    mut hits: MessageWriter<PlayerHit>,
    mut flashes: Query<&mut MessageSender<Flashed>>,
    mut stuns: Query<&mut MessageSender<Stunned>>,
    mut announce: Query<&mut MessageSender<GrenadeDetonated>, (With<ClientOf>, With<Connected>)>,
    mut commands: Commands,
) {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for detonation in detonations.read() {
        let tuning = data.grenade(detonation.kind);
        // Smoke and gas settle on the floor under where they burst, whatever height a timed fuse
        // caught them at: a cloud hanging in the air looks wrong.
        let centre = if tuning.effect.makes_cloud() {
            let above = detonation.position + Vec3::Y * CLOUD_RAISE;
            let floor = spatial
                .cast_ray(above, Dir3::NEG_Y, CLOUD_DROP, true, &filter)
                .map(|hit| hit.distance);
            settled_cloud(detonation.position, floor)
        } else {
            detonation.position
        };
        let (radius, secs) = match tuning.effect {
            Effect::Blast { radius, .. }
            | Effect::Flash { radius, .. }
            | Effect::Stun { radius, .. } => (radius, 0.0),
            Effect::Smoke { radius, secs } | Effect::Gas { radius, secs, .. } => (radius, secs),
        };
        if tuning.effect.makes_cloud() {
            commands.spawn((Name::new("Cloud"), CloudTimer(secs)));
        }
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
        for (victim, pose, health, link, perks) in &players {
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
                    // Tactical Mask: the victim's Perk cuts what the Flashbang does to them.
                    let mask = perks
                        .map(|perks| data.perk_effects(perks.0.iter()))
                        .unwrap_or_default();
                    let flashed = Flashed {
                        blind_secs: mask.flash_secs(exposure.blind * blind_secs),
                        deaf_secs: mask.flash_secs(exposure.deaf * deaf_secs),
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
                Effect::Stun { radius, secs, slow } => {
                    if distance >= radius {
                        continue;
                    }
                    let strength =
                        stun_exposure(radius, distance, clear(&spatial, &filter, centre, target));
                    let stunned = Stunned {
                        secs: strength * secs,
                        strength,
                        slow: strength * slow,
                    };
                    if stunned.secs > 0.05
                        && let Ok(mut sender) = stuns.get_mut(link.owner)
                    {
                        info!(
                            "Stun reached player at {distance:.1} m: {:.1} s at {strength:.2}",
                            stunned.secs
                        );
                        sender.send::<EventChannel>(stunned);
                    }
                }
                Effect::Smoke { .. } | Effect::Gas { .. } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloud_settles_on_the_floor_under_it_or_stays_where_it_is() {
        let at = Vec3::new(3.0, 2.5, -4.0);
        // The ray starts a little above and finds the floor 2.7 m down: the floor is at y 0.
        let settled = settled_cloud(at, Some(2.7));
        assert!((settled.y - 0.0).abs() < 1e-5, "{settled}");
        assert_eq!((settled.x, settled.z), (3.0, -4.0));
        // Nothing below within reach (over a drop, out of the map): where it burst.
        assert_eq!(settled_cloud(at, None), at);
    }
}
