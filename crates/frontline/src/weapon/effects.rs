//! Shot effects: the viewmodel muzzle flash and tracers for local and remote shots. Sounds are in
//! `audio`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{ShotFired, WeaponRng, WeaponSystems};
use crate::net::protocol::ShotTracer;
use crate::shared::FromServer;

const FLASH_SECS: f32 = 0.05;
/// Random size range of each flash, as a multiple of the base mesh.
const FLASH_SCALE: (f32, f32) = (0.8, 1.3);
const TRACER_SECS: f32 = 0.06;
const TRACER_WIDTH: f32 = 0.012;
/// Shots ending closer than this to the muzzle draw no tracer.
const MIN_TRACER_LENGTH: f32 = 0.5;

pub(super) struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_effect_assets)
            .add_systems(
                Update,
                (
                    show_muzzle_flash,
                    spawn_local_tracers,
                    spawn_remote_tracers,
                    fade_muzzle_flash,
                    fade_tracers,
                )
                    .after(WeaponSystems),
            );
    }
}

/// The flash at the viewmodel muzzle; shown for `FLASH_SECS` after each shot.
#[derive(Component, Debug, Default)]
pub(super) struct MuzzleFlash {
    left: f32,
}

#[derive(Component, Debug)]
struct Tracer {
    age: f32,
}

#[derive(Resource)]
struct EffectAssets {
    tracer_mesh: Handle<Mesh>,
    tracer_material: Handle<StandardMaterial>,
}

fn load_effect_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(EffectAssets {
        tracer_mesh: meshes.add(Cuboid::from_size(Vec3::ONE)),
        tracer_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.5),
            emissive: LinearRgba::rgb(10.0, 7.0, 3.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    });
}

fn show_muzzle_flash(
    mut fired: MessageReader<ShotFired>,
    mut rng: ResMut<WeaponRng>,
    mut flash: Query<(&mut MuzzleFlash, &mut Transform, &mut Visibility)>,
) {
    if fired.read().count() == 0 {
        return;
    }
    for (mut flash, mut transform, mut visibility) in &mut flash {
        flash.left = FLASH_SECS;
        transform.rotation = Quat::from_rotation_z(rng.next() * std::f32::consts::TAU);
        transform.scale = Vec3::splat(FLASH_SCALE.0.lerp(FLASH_SCALE.1, rng.next()));
        *visibility = Visibility::Inherited;
    }
}

fn fade_muzzle_flash(time: Res<Time>, mut flash: Query<(&mut MuzzleFlash, &mut Visibility)>) {
    for (mut flash, mut visibility) in &mut flash {
        if flash.left > 0.0 {
            flash.left -= time.delta_secs();
            if flash.left <= 0.0 {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

fn spawn_tracer(commands: &mut Commands, assets: &EffectAssets, start: Vec3, end: Vec3) {
    let along = end - start;
    let length = along.length();
    if length < MIN_TRACER_LENGTH {
        return;
    }
    commands.spawn((
        Name::new("Tracer"),
        Tracer { age: 0.0 },
        Mesh3d(assets.tracer_mesh.clone()),
        MeshMaterial3d(assets.tracer_material.clone()),
        Transform {
            translation: start + along / 2.0,
            rotation: Quat::from_rotation_arc(Vec3::Z, along / length),
            scale: Vec3::new(TRACER_WIDTH, TRACER_WIDTH, length),
        },
        NotShadowCaster,
    ));
}

fn spawn_local_tracers(
    mut fired: MessageReader<ShotFired>,
    assets: Res<EffectAssets>,
    mut commands: Commands,
) {
    for shot in fired.read() {
        spawn_tracer(&mut commands, &assets, shot.muzzle, shot.end);
    }
}

fn spawn_remote_tracers(
    mut tracers: MessageReader<FromServer<ShotTracer>>,
    assets: Res<EffectAssets>,
    mut commands: Commands,
) {
    for FromServer(tracer) in tracers.read() {
        spawn_tracer(&mut commands, &assets, tracer.origin, tracer.end);
    }
}

/// Tracers thin out to nothing over `TRACER_SECS`.
fn fade_tracers(
    time: Res<Time>,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform)>,
    mut commands: Commands,
) {
    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.age += time.delta_secs();
        if tracer.age >= TRACER_SECS {
            commands.entity(entity).despawn();
        } else {
            let width = TRACER_WIDTH * (1.0 - tracer.age / TRACER_SECS);
            transform.scale.x = width;
            transform.scale.y = width;
        }
    }
}
