//! Shot effects: the viewmodel muzzle flash (a star burst, a flame cone and a light on the world),
//! a flash and smoke wisp where other players shoot, tracers that fly from the muzzle to where a
//! shot ends, and what a bullet does there: a dust puff, a few sparks and a bullet hole. Sounds
//! are in `audio`; the particles themselves in `crate::particles`, which also scales how many
//! there are with the Video quality.

use avian3d::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;

use super::viewmodel::VIEWMODEL_LAYER;
use super::{ShotFired, WeaponRng, WeaponSystems};
use crate::atmosphere::ActiveLook;
use crate::ballistics;
use crate::data::{GameData, ProjectileModel};
use crate::net::protocol::{ProjectileFired, ProjectileLanded, ShotTracer};
use crate::particles::{
    BulletHoles, Detail, Fade, ParticleAssets, PuffSpec, Shape, random_unit, spawn_bullet_hole,
    spawn_puff, spawn_spark,
};
use crate::shared::{FromServer, GameLayer};
use crate::video::ActiveVideo;

const FLASH_SECS: f32 = 0.05;
/// Random size range of each flash, as a multiple of the base mesh.
const FLASH_SCALE: (f32, f32) = (0.8, 1.3);
/// The muzzle light that also lights the world around the player.
const MUZZLE_LIGHT_LUMENS: f32 = 400_000.0;
const MUZZLE_LIGHT_RANGE: f32 = 8.0;
/// A hitscan tracer's streak: how fast its head flies and how long it is, metres.
const TRACER_SPEED: f32 = 320.0;
const TRACER_STREAK: f32 = 5.0;
const TRACER_WIDTH: f32 = 0.01;
/// Shots ending closer than this to the muzzle draw no tracer.
const MIN_TRACER_LENGTH: f32 = 0.5;
/// A server-simulated bullet's streak shows this many seconds of its path behind its head
/// (about 12 m at 600 m/s), a touch wider than a hitscan tracer.
const BULLET_STREAK_SECS: f32 = 0.02;
const BULLET_TRACER_WIDTH: f32 = 0.02;
const MIN_STREAK_LENGTH: f32 = 0.01;
/// Where another player's muzzle is from the eye their shot starts at: forward and down, metres.
const REMOTE_MUZZLE: (f32, f32) = (0.7, -0.12);
/// A shot ending within this of the world's surface (along its path) counts as striking it.
const IMPACT_SLACK: f32 = 0.4;

pub(super) struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_effect_assets)
            .add_observer(dress_muzzle_flash)
            .add_systems(
                Update,
                (
                    show_muzzle_flash,
                    spawn_local_tracers,
                    spawn_remote_tracers,
                    spawn_impacts.before(fly_bullet_tracers),
                    fly_bullet_tracers,
                    fade_muzzle_flash,
                    fly_tracers,
                )
                    .after(WeaponSystems),
            );
    }
}

/// The flash at the viewmodel muzzle; shown for `FLASH_SECS` after each shot. Its look is made by
/// `dress_muzzle_flash`.
#[derive(Component, Debug, Default)]
pub(super) struct MuzzleFlash {
    left: f32,
}

/// A hitscan tracer: a bright streak whose head flies from `start` towards `end`.
#[derive(Component, Debug)]
struct Tracer {
    start: Vec3,
    direction: Vec3,
    length: f32,
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
            emissive: LinearRgba::rgb(9.0, 5.5, 1.8),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    });
}

/// Gives the viewmodel's `MuzzleFlash` its look: a star facing the eye, two crossed flame cones
/// along the barrel and a warm light that reaches the world. Everything but the light is drawn
/// by the viewmodel camera only.
fn dress_muzzle_flash(
    add: On<Add<MuzzleFlash>>,
    assets: Res<ParticleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let mut glow = |texture: Handle<Image>, color: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: Color::from(color),
            base_color_texture: Some(texture),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        })
    };
    let star = glow(assets.star(), LinearRgba::rgb(5.0, 3.2, 1.1));
    let flame = glow(assets.flame(), LinearRgba::rgb(4.0, 2.2, 0.7));
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let flame_length = 0.32;
    let flame_width = 0.07;
    let flame_at = Vec3::new(0.0, 0.0, -flame_length / 2.0);
    // A quad's length runs along its local y, so a quarter turn about x lays it along -z.
    let along_barrel = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let across_barrel = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * along_barrel;
    let mut flash = commands.entity(add.entity);
    flash.with_children(|flash| {
        flash.spawn((
            Mesh3d(assets.quad()),
            MeshMaterial3d(star),
            Transform::from_xyz(0.0, 0.0, -0.03).with_scale(Vec3::splat(0.34)),
            layer.clone(),
            NotShadowCaster,
        ));
        for rotation in [along_barrel, across_barrel] {
            flash.spawn((
                Mesh3d(assets.quad()),
                MeshMaterial3d(flame.clone()),
                Transform::from_translation(flame_at)
                    .with_rotation(rotation)
                    .with_scale(Vec3::new(flame_width, flame_length, 1.0)),
                layer.clone(),
                NotShadowCaster,
            ));
        }
        flash.spawn((
            PointLight {
                color: Color::srgb(1.0, 0.75, 0.4),
                intensity: MUZZLE_LIGHT_LUMENS,
                range: MUZZLE_LIGHT_RANGE,
                shadow_maps_enabled: false,
                ..default()
            },
            // Lights the world around the player as well as the gun.
            RenderLayers::from_layers(&[0, VIEWMODEL_LAYER]),
        ));
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
        Tracer {
            start,
            direction: along / length,
            length,
            age: 0.0,
        },
        Mesh3d(assets.tracer_mesh.clone()),
        MeshMaterial3d(assets.tracer_material.clone()),
        Transform::from_translation(start).with_scale(Vec3::new(
            TRACER_WIDTH,
            TRACER_WIDTH,
            MIN_STREAK_LENGTH,
        )),
        NotShadowCaster,
    ));
}

/// Flies each tracer's streak along its path and removes it once its tail has arrived.
fn fly_tracers(
    time: Res<Time>,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform)>,
    mut commands: Commands,
) {
    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.age += time.delta_secs();
        let flown = tracer.age * TRACER_SPEED;
        let (head, tail) = (
            flown.min(tracer.length),
            (flown - TRACER_STREAK).clamp(0.0, tracer.length),
        );
        if tail >= tracer.length - 0.01 {
            commands.entity(entity).despawn();
            continue;
        }
        let length = (head - tail).max(MIN_STREAK_LENGTH);
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, tracer.direction);
        transform.translation = tracer.start + tracer.direction * (tail + length / 2.0);
        transform.scale.z = length;
    }
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

/// Another player's shot: a tracer, and a flash and smoke wisp at where their muzzle should be.
fn spawn_remote_tracers(
    mut tracers: MessageReader<FromServer<ShotTracer>>,
    assets: Res<EffectAssets>,
    particles: Res<ParticleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<WeaponRng>,
    mut commands: Commands,
) {
    for FromServer(tracer) in tracers.read() {
        spawn_tracer(&mut commands, &assets, tracer.origin, tracer.end);
        let Ok(direction) = Dir3::new(tracer.end - tracer.origin) else {
            continue;
        };
        let muzzle = tracer.origin + *direction * REMOTE_MUZZLE.0 + Vec3::Y * REMOTE_MUZZLE.1;
        spawn_puff(
            &mut commands,
            &mut materials,
            &particles,
            PuffSpec {
                at: muzzle + *direction * 0.1,
                start_size: 0.3,
                end_size: 0.12,
                grow_secs: FLASH_SECS,
                life: FLASH_SECS,
                shape: Shape::Star,
                color: LinearRgba::rgb(5.0, 3.2, 1.1),
                alpha: 1.0,
                additive: true,
                drag: 0.0,
                fade: Fade::Out { in_secs: 0.01 },
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
        spawn_puff(
            &mut commands,
            &mut materials,
            &particles,
            PuffSpec {
                at: muzzle,
                velocity: *direction * 0.8 + Vec3::Y * 0.3,
                start_size: 0.05,
                end_size: 0.22,
                grow_secs: 0.5,
                life: 0.5,
                color: LinearRgba::rgb(0.5, 0.5, 0.52),
                alpha: 0.25,
                gravity: -0.4,
                ..default()
            },
        );
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

/// Where a shot flying from `from` to `to` meets the World, and the surface's normal there.
/// `None` when it ended in the air or in something that isn't the World (a player).
fn surface_at_end(spatial: &SpatialQuery, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    let along = to - from;
    let length = along.length();
    let direction = Dir3::new(along).ok()?;
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let hit = spatial.cast_ray(from, direction, length + IMPACT_SLACK, true, &filter)?;
    (hit.distance > length - IMPACT_SLACK).then(|| (from + *direction * hit.distance, hit.normal))
}

/// What a bullet does where it strikes the World: a flash of light, dust, sparks and a hole.
#[allow(clippy::too_many_arguments)]
fn spawn_impact(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    particles: &ParticleAssets,
    holes: &mut BulletHoles,
    detail: Detail,
    dust: LinearRgba,
    rng: &mut WeaponRng,
    at: Vec3,
    normal: Vec3,
) {
    spawn_bullet_hole(commands, particles, holes, detail, at, normal, rng.next());
    spawn_puff(
        commands,
        materials,
        particles,
        PuffSpec {
            at: at + normal * 0.03,
            start_size: 0.22,
            end_size: 0.05,
            grow_secs: 0.07,
            life: 0.07,
            shape: Shape::Star,
            color: LinearRgba::rgb(4.0, 2.6, 1.0),
            alpha: 1.0,
            additive: true,
            drag: 0.0,
            fade: Fade::Out { in_secs: 0.01 },
            angle: rng.next() * std::f32::consts::TAU,
            ..default()
        },
    );
    for _ in 0..detail.count(3) {
        let drift = normal * 0.9 + random_unit(rng) * 0.7;
        spawn_puff(
            commands,
            materials,
            particles,
            PuffSpec {
                at: at + normal * 0.05,
                velocity: drift * (0.7 + rng.next() * 0.9),
                drag: 3.2,
                gravity: -0.35,
                start_size: 0.07 + rng.next() * 0.05,
                end_size: 0.32 + rng.next() * 0.2,
                grow_secs: 0.6,
                life: 0.5 + rng.next() * 0.35,
                color: dust,
                alpha: 0.38,
                spin: rng.signed() * 1.5,
                angle: rng.next() * std::f32::consts::TAU,
                ..default()
            },
        );
    }
    if detail > Detail::Low {
        for _ in 0..detail.count(4) {
            let velocity = normal * (2.0 + rng.next() * 3.0) + random_unit(rng) * 2.5;
            spawn_spark(commands, particles, at + normal * 0.02, velocity, 0.2 + rng.next() * 0.2);
        }
    }
}

/// Shots striking the World: yours (from the client's own ray), other players' (their tracers)
/// and server-simulated bullets (where they land).
#[allow(clippy::too_many_arguments)]
fn spawn_impacts(
    mut fired: MessageReader<ShotFired>,
    mut tracers: MessageReader<FromServer<ShotTracer>>,
    mut landed: MessageReader<FromServer<ProjectileLanded>>,
    bullets: Query<&BulletTracer>,
    spatial: SpatialQuery,
    particles: Res<ParticleAssets>,
    look: Res<ActiveLook>,
    video: Res<ActiveVideo>,
    mut holes: ResMut<BulletHoles>,
    mut rng: ResMut<WeaponRng>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let detail = Detail::of(&video.0.quality);
    let dust = look.dust();
    let mut strikes: Vec<(Vec3, Vec3)> = Vec::new();
    for shot in fired.read() {
        if let Some(end) = shot.end
            && let Some(strike) = surface_at_end(&spatial, shot.muzzle, end)
        {
            strikes.push(strike);
        }
    }
    for FromServer(tracer) in tracers.read() {
        if let Some(strike) = surface_at_end(&spatial, tracer.origin, tracer.end) {
            strikes.push(strike);
        }
    }
    for FromServer(landing) in landed.read() {
        if !landing.struck {
            continue;
        }
        let Some(bullet) = bullets.iter().find(|bullet| bullet.id == landing.id) else {
            continue;
        };
        // The last metre of its flight, along the way it came.
        let heading = (landing.position - bullet.origin).normalize_or_zero();
        if let Some(strike) =
            surface_at_end(&spatial, landing.position - heading, landing.position)
        {
            strikes.push(strike);
        }
    }
    for (at, normal) in strikes {
        spawn_impact(
            &mut commands,
            &mut materials,
            &particles,
            &mut holes,
            detail,
            dust,
            &mut rng,
            at,
            normal,
        );
    }
}
