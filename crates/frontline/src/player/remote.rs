//! Capsule visuals for `RemotePlayer`s, smoothed toward their replicated `PlayerPose`, tinted
//! friendly or enemy relative to the local player's Team, and hidden while dead or teamless.
//!
//! Respawns never glide: a pose jump over `TELEPORT_DISTANCE` snaps the visual, and a revived
//! player stays hidden until its (interpolated, so later-arriving) pose has reached the spawn.

use bevy::prelude::*;
use lightyear::prelude::ConfirmedHistory;

use super::BODY_RADIUS;
use super::life::LocalLife;
use crate::match_hud::is_friendly;
use crate::shared::{Health, PlayerPose, PlayerTeam, RemotePlayer, Stance, TELEPORT_DISTANCE};

/// Exponential decay rate (1/s) of the transform toward the pose; higher follows tighter.
const FOLLOW_RATE: f32 = 20.0;
/// A revived player shows once its pose is this close to where the server respawned it.
const SPAWN_REACHED: f32 = 1.0;
/// Length of the lying capsule shown for a prone player.
const PRONE_LENGTH: f32 = 1.7;
/// How far the prone body extends behind the player's position (the head is at the front).
const PRONE_BODY_OFFSET: f32 = 0.55;
const VISOR_SIZE: Vec3 = Vec3::new(0.36, 0.1, 0.15);
/// Distance of the visor's centre in front of the eye.
const VISOR_FORWARD: f32 = 0.3;
const FRIENDLY_BODY: Color = Color::srgb(0.2, 0.42, 0.85);
const ENEMY_BODY: Color = Color::srgb(0.8, 0.2, 0.16);

#[derive(Resource)]
pub(super) struct RemoteAssets {
    /// Indexed like `STANCES`.
    bodies: [Handle<Mesh>; 3],
    visor: Handle<Mesh>,
    visor_material: Handle<StandardMaterial>,
    friendly_material: Handle<StandardMaterial>,
    enemy_material: Handle<StandardMaterial>,
}

impl RemoteAssets {
    fn body_material(&self, friendly: bool) -> Handle<StandardMaterial> {
        if friendly {
            self.friendly_material.clone()
        } else {
            self.enemy_material.clone()
        }
    }

    /// The capsule mesh drawn for `stance`.
    pub(super) fn body(&self, stance: Stance) -> Handle<Mesh> {
        self.bodies[stance_index(stance)].clone()
    }
}

const STANCES: [Stance; 3] = [Stance::Standing, Stance::Crouching, Stance::Prone];

fn stance_index(stance: Stance) -> usize {
    match stance {
        Stance::Standing => 0,
        Stance::Crouching => 1,
        Stance::Prone => 2,
    }
}

/// Visual state attached to a remote player entity.
#[derive(Component, Debug)]
pub(super) struct RemoteVisuals {
    body: Entity,
    visor: Entity,
    stance: Stance,
    eye_height: f32,
    /// Whether the body currently wears the friendly tint.
    friendly: bool,
    /// Alive and on a Team, per the latest replicated `Health` and `PlayerTeam`.
    in_play: bool,
    /// Where the server put the player on revival, until the pose has caught up with it.
    awaiting_spawn: Option<Vec3>,
    /// Whether the visuals were drawn last frame.
    shown: bool,
}

/// A remote player just died where its visual stood; `corpse` leaves a body there.
#[derive(Message, Clone, Copy, Debug)]
pub(super) struct RemoteDeath {
    /// The player's visual transform (feet, yaw) at death.
    pub transform: Transform,
    pub stance: Stance,
    pub friendly: bool,
}

fn body_mesh(stance: Stance) -> Capsule3d {
    match stance {
        Stance::Prone => {
            let radius = Stance::Prone.body_height() / 2.0;
            Capsule3d::new(radius, PRONE_LENGTH - 2.0 * radius)
        }
        Stance::Standing | Stance::Crouching => {
            let height = stance.body_height();
            Capsule3d::new(BODY_RADIUS, height - 2.0 * BODY_RADIUS)
        }
    }
}

/// Body transform relative to the feet; prone lies the capsule along the facing direction.
pub(super) fn body_transform(stance: Stance) -> Transform {
    let half = stance.body_height() / 2.0;
    match stance {
        Stance::Prone => Transform::from_xyz(0.0, half, PRONE_BODY_OFFSET)
            .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        Stance::Standing | Stance::Crouching => Transform::from_xyz(0.0, half, 0.0),
    }
}

fn visor_transform(eye_height: f32, pitch: f32) -> Transform {
    let rotation = Quat::from_rotation_x(pitch);
    Transform::from_translation(Vec3::Y * eye_height + rotation * Vec3::NEG_Z * VISOR_FORWARD)
        .with_rotation(rotation)
}

fn pose_transform(pose: &PlayerPose) -> Transform {
    Transform::from_translation(pose.position).with_rotation(Quat::from_rotation_y(pose.yaw))
}

/// Body tint of a friendly or enemy player.
pub(super) fn body_color(friendly: bool) -> Color {
    if friendly { FRIENDLY_BODY } else { ENEMY_BODY }
}

pub(super) fn body_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.8,
        ..default()
    }
}

pub(super) fn load_remote_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RemoteAssets {
        bodies: STANCES.map(|stance| meshes.add(body_mesh(stance))),
        visor: meshes.add(Cuboid::from_size(VISOR_SIZE)),
        visor_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.08, 0.1),
            perceptual_roughness: 0.3,
            ..default()
        }),
        friendly_material: materials.add(body_material(FRIENDLY_BODY)),
        enemy_material: materials.add(body_material(ENEMY_BODY)),
    });
}

/// Starts hidden; `follow_remote_poses` shows it from the next frame if it is in play.
pub(super) fn attach_remote_visuals(
    add: On<Add<RemotePlayer>>,
    mut commands: Commands,
    assets: Res<RemoteAssets>,
    life: Res<LocalLife>,
    players: Query<(&PlayerPose, Option<&Health>, Option<&PlayerTeam>)>,
) {
    let entity = add.entity;
    let Ok((pose, health, team)) = players.get(entity) else {
        warn!("RemotePlayer {entity} has no PlayerPose; not drawing it");
        return;
    };
    let team = team.and_then(|team| team.0);
    let friendly = is_friendly(life.team(), team);
    let body = commands
        .spawn((
            Name::new("RemoteBody"),
            Mesh3d(assets.body(pose.stance)),
            MeshMaterial3d(assets.body_material(friendly)),
            body_transform(pose.stance),
            ChildOf(entity),
        ))
        .id();
    let eye_height = pose.stance.eye_height();
    let visor = commands
        .spawn((
            Name::new("RemoteVisor"),
            Mesh3d(assets.visor.clone()),
            MeshMaterial3d(assets.visor_material.clone()),
            visor_transform(eye_height, pose.pitch),
            ChildOf(entity),
        ))
        .id();
    commands.entity(entity).insert((
        RemoteVisuals {
            body,
            visor,
            stance: pose.stance,
            eye_height,
            friendly,
            in_play: health.is_some_and(|health| !health.is_dead()) && team.is_some(),
            awaiting_spawn: None,
            shown: false,
        },
        pose_transform(pose),
        Visibility::Hidden,
    ));
}

/// Moves each remote player's visuals to its pose and decides whether they are drawn.
///
/// Shown players are smoothed toward the pose, except across a teleport, which snaps.
/// Hidden ones (dead, teamless, or revived but not yet at their spawn) snap every frame, so
/// they reappear exactly where they are. A revived player's spawn is the newest replicated pose:
/// the server moves it in the same tick that restores `Health`, but `Health` applies on
/// arrival while the pose only shows once interpolation reaches it. On a Host there is no
/// interpolation and the pose is already there. Dying sends a `RemoteDeath`. Also swaps the
/// body on stance change and re-tints it when either side's Team changes.
pub(super) fn follow_remote_poses(
    time: Res<Time>,
    assets: Res<RemoteAssets>,
    life: Res<LocalLife>,
    mut players: Query<
        (
            &PlayerPose,
            &Health,
            Option<&PlayerTeam>,
            Option<&ConfirmedHistory<PlayerPose>>,
            &mut Transform,
            &mut Visibility,
            &mut RemoteVisuals,
        ),
        With<RemotePlayer>,
    >,
    mut parts: Query<
        (
            &mut Transform,
            Option<&mut Mesh3d>,
            Option<&mut MeshMaterial3d<StandardMaterial>>,
        ),
        Without<RemotePlayer>,
    >,
    mut deaths: MessageWriter<RemoteDeath>,
) {
    let dt = time.delta_secs();
    for (pose, health, team, history, mut transform, mut visibility, mut visuals) in &mut players {
        let team = team.and_then(|team| team.0);
        let in_play = !health.is_dead() && team.is_some();
        if in_play != visuals.in_play {
            visuals.in_play = in_play;
            if in_play {
                let spawn = history
                    .and_then(ConfirmedHistory::newest_present)
                    .map_or(pose.position, |(_, newest)| newest.position);
                visuals.awaiting_spawn = Some(spawn);
            } else {
                visuals.awaiting_spawn = None;
                if health.is_dead() && visuals.shown {
                    deaths.write(RemoteDeath {
                        transform: *transform,
                        stance: visuals.stance,
                        friendly: visuals.friendly,
                    });
                }
            }
        }
        if visuals
            .awaiting_spawn
            .is_some_and(|spawn| pose.position.distance(spawn) <= SPAWN_REACHED)
        {
            visuals.awaiting_spawn = None;
        }
        let shown = in_play && visuals.awaiting_spawn.is_none();

        let target = pose_transform(pose);
        let snap = !shown
            || !visuals.shown
            || transform.translation.distance(target.translation) > TELEPORT_DISTANCE;
        if snap {
            *transform = target;
            visuals.eye_height = pose.stance.eye_height();
        } else {
            transform
                .translation
                .smooth_nudge(&target.translation, FOLLOW_RATE, dt);
            transform
                .rotation
                .smooth_nudge(&target.rotation, FOLLOW_RATE, dt);
            visuals
                .eye_height
                .smooth_nudge(&pose.stance.eye_height(), FOLLOW_RATE, dt);
        }
        visuals.shown = shown;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });

        if visuals.stance != pose.stance {
            visuals.stance = pose.stance;
            if let Ok((mut body_transform_mut, Some(mut mesh), _)) = parts.get_mut(visuals.body) {
                *body_transform_mut = body_transform(pose.stance);
                mesh.0 = assets.body(pose.stance);
            }
        }

        let friendly = is_friendly(life.team(), team);
        if visuals.friendly != friendly {
            visuals.friendly = friendly;
            if let Ok((_, _, Some(mut material))) = parts.get_mut(visuals.body) {
                material.0 = assets.body_material(friendly);
            }
        }

        if let Ok((mut visor, ..)) = parts.get_mut(visuals.visor) {
            *visor = visor_transform(visuals.eye_height, pose.pitch);
        }
    }
}
