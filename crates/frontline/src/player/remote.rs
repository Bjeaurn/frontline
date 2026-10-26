//! Capsule visuals for `RemotePlayer`s, smoothed toward their replicated `PlayerPose` and
//! hidden while their `Health` is dead.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;

use super::BODY_RADIUS;
use crate::shared::{Health, PlayerPose, RemotePlayer, Stance};

/// Exponential decay rate (1/s) of the transform toward the pose; higher follows tighter.
const FOLLOW_RATE: f32 = 20.0;
/// Length of the lying capsule shown for a prone player.
const PRONE_LENGTH: f32 = 1.7;
/// How far the prone body extends behind the player's position (the head is at the front).
const PRONE_BODY_OFFSET: f32 = 0.55;
const VISOR_SIZE: Vec3 = Vec3::new(0.36, 0.1, 0.15);
/// Distance of the visor's centre in front of the eye.
const VISOR_FORWARD: f32 = 0.3;

#[derive(Resource)]
pub(super) struct RemoteAssets {
    /// Indexed like `STANCES`.
    bodies: [Handle<Mesh>; 3],
    visor: Handle<Mesh>,
    visor_material: Handle<StandardMaterial>,
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

/// A stable colour per entity, so each remote player is told apart at a glance.
fn tint(entity: Entity) -> Color {
    let mut hasher = DefaultHasher::new();
    entity.hash(&mut hasher);
    let hue = (hasher.finish() % 360) as f32;
    Color::hsl(hue, 0.65, 0.5)
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
    });
}

pub(super) fn attach_remote_visuals(
    add: On<Add<RemotePlayer>>,
    mut commands: Commands,
    assets: Res<RemoteAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    poses: Query<&PlayerPose>,
) {
    let entity = add.entity;
    let Ok(pose) = poses.get(entity) else {
        warn!("RemotePlayer {entity} has no PlayerPose; not drawing it");
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: tint(entity),
        perceptual_roughness: 0.8,
        ..default()
    });
    let body = commands
        .spawn((
            Name::new("RemoteBody"),
            Mesh3d(assets.bodies[stance_index(pose.stance)].clone()),
            MeshMaterial3d(material),
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
        },
        pose_transform(pose),
        Visibility::default(),
    ));
}

/// Smooths each remote player's transform toward its pose and swaps the body on stance change.
pub(super) fn follow_remote_poses(
    time: Res<Time>,
    assets: Res<RemoteAssets>,
    mut players: Query<(&PlayerPose, &mut Transform, &mut RemoteVisuals), With<RemotePlayer>>,
    mut parts: Query<(&mut Transform, Option<&mut Mesh3d>), Without<RemotePlayer>>,
) {
    let dt = time.delta_secs();
    for (pose, mut transform, mut visuals) in &mut players {
        let target = pose_transform(pose);
        transform
            .translation
            .smooth_nudge(&target.translation, FOLLOW_RATE, dt);
        transform
            .rotation
            .smooth_nudge(&target.rotation, FOLLOW_RATE, dt);

        if visuals.stance != pose.stance {
            visuals.stance = pose.stance;
            if let Ok((mut body_transform_mut, Some(mut mesh))) = parts.get_mut(visuals.body) {
                *body_transform_mut = body_transform(pose.stance);
                mesh.0 = assets.bodies[stance_index(pose.stance)].clone();
            }
        }

        visuals
            .eye_height
            .smooth_nudge(&pose.stance.eye_height(), FOLLOW_RATE, dt);
        if let Ok((mut visor, _)) = parts.get_mut(visuals.visor) {
            *visor = visor_transform(visuals.eye_height, pose.pitch);
        }
    }
}

pub(super) fn hide_dead_players(
    mut players: Query<(&Health, &mut Visibility), (With<RemotePlayer>, With<RemoteVisuals>)>,
) {
    for (health, mut visibility) in &mut players {
        visibility.set_if_neq(if health.is_dead() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}
