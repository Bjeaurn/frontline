//! Capsule visuals for `RemotePlayer`s, smoothed toward their replicated `PlayerPose`, tinted
//! friendly or enemy relative to the local player's Team, and hidden while dead or teamless.

use bevy::prelude::*;

use super::BODY_RADIUS;
use super::life::LocalLife;
use crate::match_hud::is_friendly;
use crate::shared::{Health, PlayerPose, PlayerTeam, RemotePlayer, Stance};

/// Exponential decay rate (1/s) of the transform toward the pose; higher follows tighter.
const FOLLOW_RATE: f32 = 20.0;
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
fn body_transform(stance: Stance) -> Transform {
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

fn body_material(color: Color) -> StandardMaterial {
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

pub(super) fn attach_remote_visuals(
    add: On<Add<RemotePlayer>>,
    mut commands: Commands,
    assets: Res<RemoteAssets>,
    life: Res<LocalLife>,
    players: Query<(&PlayerPose, Option<&PlayerTeam>)>,
) {
    let entity = add.entity;
    let Ok((pose, team)) = players.get(entity) else {
        warn!("RemotePlayer {entity} has no PlayerPose; not drawing it");
        return;
    };
    let friendly = is_friendly(life.team(), team.and_then(|team| team.0));
    let body = commands
        .spawn((
            Name::new("RemoteBody"),
            Mesh3d(assets.bodies[stance_index(pose.stance)].clone()),
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
        },
        pose_transform(pose),
        Visibility::default(),
    ));
}

/// Smooths each remote player's transform toward its pose, swaps the body on stance change
/// and re-tints it when either side's Team changes.
pub(super) fn follow_remote_poses(
    time: Res<Time>,
    assets: Res<RemoteAssets>,
    life: Res<LocalLife>,
    mut players: Query<
        (
            &PlayerPose,
            Option<&PlayerTeam>,
            &mut Transform,
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
) {
    let dt = time.delta_secs();
    for (pose, team, mut transform, mut visuals) in &mut players {
        let target = pose_transform(pose);
        transform
            .translation
            .smooth_nudge(&target.translation, FOLLOW_RATE, dt);
        transform
            .rotation
            .smooth_nudge(&target.rotation, FOLLOW_RATE, dt);

        if visuals.stance != pose.stance {
            visuals.stance = pose.stance;
            if let Ok((mut body_transform_mut, Some(mut mesh), _)) = parts.get_mut(visuals.body) {
                *body_transform_mut = body_transform(pose.stance);
                mesh.0 = assets.bodies[stance_index(pose.stance)].clone();
            }
        }

        let friendly = is_friendly(life.team(), team.and_then(|team| team.0));
        if visuals.friendly != friendly {
            visuals.friendly = friendly;
            if let Ok((_, _, Some(mut material))) = parts.get_mut(visuals.body) {
                material.0 = assets.body_material(friendly);
            }
        }

        visuals
            .eye_height
            .smooth_nudge(&pose.stance.eye_height(), FOLLOW_RATE, dt);
        if let Ok((mut visor, ..)) = parts.get_mut(visuals.visor) {
            *visor = visor_transform(visuals.eye_height, pose.pitch);
        }
    }
}

/// Hides remote players while dead or not on a Team yet.
pub(super) fn hide_dead_players(
    mut players: Query<
        (&Health, Option<&PlayerTeam>, &mut Visibility),
        (With<RemotePlayer>, With<RemoteVisuals>),
    >,
) {
    for (health, team, mut visibility) in &mut players {
        let in_play = !health.is_dead() && team.is_some_and(|team| team.0.is_some());
        visibility.set_if_neq(if in_play {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
