//! Shot effects: the viewmodel muzzle flash and tracers for local and remote shots. Sounds are in
//! `audio`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{ShotFired, WeaponRng, WeaponSystems};
use crate::ballistics;
use crate::data::{GameData, ProjectileModel};
use crate::net::protocol::{ProjectileFired, ProjectileLanded, ShotTracer};
use crate::shared::FromServer;

const FLASH_SECS: f32 = 0.05;
/// Random size range of each flash, as a multiple of the base mesh.
const FLASH_SCALE: (f32, f32) = (0.8, 1.3);
const TRACER_SECS: f32 = 0.06;
const TRACER_WIDTH: f32 = 0.012;
/// Shots ending closer than this to the muzzle draw no tracer.
const MIN_TRACER_LENGTH: f32 = 0.5;
/// A server-simulated bullet's streak shows this many seconds of its path behind its head
/// (about 12 m at 600 m/s), a touch wider than a hitscan tracer.
const BULLET_STREAK_SECS: f32 = 0.02;
const BULLET_TRACER_WIDTH: f32 = 0.02;
const MIN_STREAK_LENGTH: f32 = 0.01;

pub(super) struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_effect_assets).add_systems(
            Update,
            (
                show_muzzle_flash,
                spawn_local_tracers,
                spawn_remote_tracers,
                fly_bullet_tracers,
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
    if fired.read().all(|shot| shot.silenced) {
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
        if let Some(end) = shot.end {
            spawn_tracer(&mut commands, &assets, shot.muzzle, end);
        }
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

/// A server-simulated bullet in flight, drawn as a short streak along the path the server
/// steps it through (`ballistics::position_at`). It ends when the server says the bullet
/// landed (`ProjectileLanded`), or by itself if that never arrives.
#[derive(Component, Debug)]
struct BulletTracer {
    id: u32,
    origin: Vec3,
    velocity: Vec3,
    model: ProjectileModel,
    age: f32,
    expires: f32,
}

/// Spawns a streak per `ProjectileFired`, flies them and removes the landed ones.
fn fly_bullet_tracers(
    time: Res<Time>,
    mut fired: MessageReader<FromServer<ProjectileFired>>,
    mut landed: MessageReader<FromServer<ProjectileLanded>>,
    data: Res<GameData>,
    assets: Res<EffectAssets>,
    mut tracers: Query<(Entity, &mut BulletTracer, &mut Transform)>,
    mut commands: Commands,
) {
    let landed: Vec<u32> = landed
        .read()
        .map(|FromServer(landing)| landing.id)
        .collect();
    for FromServer(shot) in fired.read() {
        let stats = data.weapon(shot.weapon);
        let Some(model) = stats.projectile else {
            continue;
        };
        if landed.contains(&shot.id) {
            continue;
        }
        commands.spawn((
            Name::new("Bullet tracer"),
            BulletTracer {
                id: shot.id,
                origin: shot.origin,
                velocity: shot.velocity,
                model,
                age: 0.0,
                expires: ballistics::tracer_expiry_secs(model, stats.range),
            },
            Mesh3d(assets.tracer_mesh.clone()),
            MeshMaterial3d(assets.tracer_material.clone()),
            Transform::from_translation(shot.origin).with_scale(Vec3::new(
                BULLET_TRACER_WIDTH,
                BULLET_TRACER_WIDTH,
                MIN_STREAK_LENGTH,
            )),
            NotShadowCaster,
        ));
    }
    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.age += time.delta_secs();
        if landed.contains(&tracer.id) || tracer.age > tracer.expires {
            commands.entity(entity).despawn();
            continue;
        }
        let at = |t: f32| {
            ballistics::position_at(tracer.origin, tracer.velocity, tracer.model, t.max(0.0))
        };
        let (tail, head) = (at(tracer.age - BULLET_STREAK_SECS), at(tracer.age));
        let along = head - tail;
        let length = along.length().max(MIN_STREAK_LENGTH);
        if let Ok(direction) = Dir3::new(along) {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, *direction);
        }
        transform.translation = tail + along / 2.0;
        transform.scale.z = length;
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
