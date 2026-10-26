//! First-person gun: a blocky rifle on its own camera and render layer, so it is drawn over
//! the world and never clips into walls. Moves to the sights when aiming, bobs while
//! walking, sways with the mouse, kicks on each shot, dips during a reload, angles down and
//! across while sprinting (raising back as the sprint-out ends) and drops during a mantle.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::effects::MuzzleFlash;
use super::{HeldWeapon, ShotFired, WeaponSystems, ease};
use crate::data::GameData;
use crate::player::{ControllerSystems, EyeCamera, LocalLife, Motor, MouseCaptured};
use crate::shared::LocalPlayer;

/// Render layer of the viewmodel; the world stays on layer 0.
pub(super) const VIEWMODEL_LAYER: usize = 1;

/// Muzzle position in gun space (-Z is forward).
pub(super) const MUZZLE: Vec3 = Vec3::new(0.0, 0.015, -0.48);
/// Height of the sight line above the gun origin.
const SIGHT_HEIGHT: f32 = 0.075;
/// Gun origin relative to the camera at the hip and fully aimed (sights on the centre).
const HIP_OFFSET: Vec3 = Vec3::new(0.14, -0.15, -0.50);
const ADS_OFFSET: Vec3 = Vec3::new(0.0, -SIGHT_HEIGHT, -0.32);

/// Bob phase advance per metre walked (two steps per cycle).
const BOB_RADIANS_PER_METRE: f32 = 2.4;
/// Bob amplitude at walking speed, in metres.
const BOB_AMPLITUDE: Vec2 = Vec2::new(0.010, 0.008);
const BOB_REFERENCE_SPEED: f32 = 5.0;
/// Bob and sway that remain when fully aimed.
const ADS_MOTION_SCALE: f32 = 0.15;
/// Sway offset per unit of raw mouse motion and its cap, in metres.
const SWAY_PER_COUNT: f32 = 0.0004;
const SWAY_MAX: f32 = 0.03;
/// Sway tilt per metre of sway offset, in radians.
const SWAY_TILT: f32 = 2.5;
/// Exponential rate (1/s) at which bob amount and sway follow their targets.
const FOLLOW_RATE: f32 = 10.0;
/// Kick per shot: distance pushed back and pitch up at full kick; decays at `KICK_DECAY` 1/s.
const KICK_BACK: f32 = 0.035;
const KICK_PITCH: f32 = 0.05;
const KICK_MAX: f32 = 1.5;
const KICK_DECAY: f32 = 16.0;
/// Kick that remains when fully aimed.
const ADS_KICK_SCALE: f32 = 0.5;
/// Reload dip at its deepest: drop, tilt down and roll.
const RELOAD_DROP: f32 = 0.12;
const RELOAD_TILT: f32 = 0.6;
const RELOAD_ROLL: f32 = 0.5;
/// Sprint pose at full blend: offset from the hip position, and yaw (barrel swung left),
/// pitch (down) and roll. Eased in at `SPRINT_IN_RATE` 1/s; eased out over the sprint-out.
const SPRINT_OFFSET: Vec3 = Vec3::new(-0.03, -0.07, 0.05);
const SPRINT_YAW: f32 = 0.75;
const SPRINT_PITCH: f32 = -0.35;
const SPRINT_ROLL: f32 = 0.35;
const SPRINT_IN_RATE: f32 = 12.0;
/// Mantle pose at full blend: drop and tilt down; follows at `MANTLE_RATE` 1/s.
const MANTLE_DROP: f32 = 0.2;
const MANTLE_TILT: f32 = 0.5;
const MANTLE_RATE: f32 = 14.0;

/// Light for the viewmodel layer; world lights only reach the world layer.
const VIEWMODEL_LIGHT_LUX: f32 = 9_000.0;
const MUZZLE_LIGHT_LUMENS: f32 = 400_000.0;
const MUZZLE_LIGHT_RANGE: f32 = 8.0;

pub(super) struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(attach_viewmodel)
            .add_systems(Startup, spawn_viewmodel_light)
            .add_systems(
                Update,
                (match_eye_fov, animate_viewmodel)
                    .after(WeaponSystems)
                    .after(ControllerSystems::View),
            );
    }
}

#[derive(Component)]
struct ViewmodelCamera;

/// The gun root under the viewmodel camera, with its procedural animation state.
#[derive(Component, Debug, Default)]
pub(super) struct ViewmodelGun {
    bob_phase: f32,
    bob_amount: f32,
    sway: Vec2,
    kick: f32,
    /// Sprint pose blend, 0 ready to 1 fully lowered.
    sprint: f32,
    /// Mantle pose blend, 0 ready to 1 fully lowered.
    mantle: f32,
}

fn viewmodel_layer() -> RenderLayers {
    RenderLayers::layer(VIEWMODEL_LAYER)
}

fn spawn_viewmodel_light(mut commands: Commands) {
    commands.spawn((
        Name::new("Viewmodel light"),
        DirectionalLight {
            illuminance: VIEWMODEL_LIGHT_LUX,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
        viewmodel_layer(),
    ));
}

/// A box part of the gun: size and centre in gun space.
const BOXES: [(Vec3, Vec3, Furniture); 7] = [
    // Receiver.
    (Vec3::new(0.055, 0.075, 0.30), Vec3::new(0.0, 0.0, 0.0), Furniture::Metal),
    // Handguard.
    (Vec3::new(0.06, 0.06, 0.20), Vec3::new(0.0, 0.0, -0.24), Furniture::Polymer),
    // Magazine.
    (Vec3::new(0.032, 0.14, 0.065), Vec3::new(0.0, -0.10, -0.07), Furniture::Metal),
    // Pistol grip.
    (Vec3::new(0.035, 0.10, 0.045), Vec3::new(0.0, -0.08, 0.09), Furniture::Polymer),
    // Stock.
    (Vec3::new(0.045, 0.075, 0.20), Vec3::new(0.0, -0.015, 0.25), Furniture::Polymer),
    // Rear sight, its top on the sight line.
    (Vec3::new(0.03, 0.04, 0.03), Vec3::new(0.0, SIGHT_HEIGHT - 0.02, 0.11), Furniture::Sight),
    // Front sight post, its top on the sight line.
    (Vec3::new(0.006, 0.045, 0.008), Vec3::new(0.0, SIGHT_HEIGHT - 0.0225, -0.36), Furniture::Sight),
];
const BARREL_RADIUS: f32 = 0.012;
const BARREL_START: f32 = -0.30;
/// Muzzle flash ellipsoid size, stretched along the barrel.
const FLASH_SIZE: Vec3 = Vec3::new(0.07, 0.07, 0.16);

#[derive(Clone, Copy)]
enum Furniture {
    Metal,
    Polymer,
    Sight,
}

impl Furniture {
    fn material(self) -> StandardMaterial {
        let (color, roughness) = match self {
            Furniture::Metal => (Color::srgb(0.16, 0.16, 0.17), 0.45),
            Furniture::Polymer => (Color::srgb(0.22, 0.20, 0.17), 0.8),
            Furniture::Sight => (Color::srgb(0.04, 0.04, 0.04), 0.6),
        };
        StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            ..default()
        }
    }
}

/// Builds the viewmodel camera and gun under the local player's eye camera.
fn attach_viewmodel(
    add: On<Add<EyeCamera>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let parts = [Furniture::Metal, Furniture::Polymer, Furniture::Sight]
        .map(|furniture| materials.add(furniture.material()));

    let camera = commands
        .spawn((
            Name::new("ViewmodelCamera"),
            ViewmodelCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                near: 0.01,
                far: 10.0,
                ..default()
            }),
            viewmodel_layer(),
            Transform::IDENTITY,
            ChildOf(add.entity),
        ))
        .id();
    let gun = commands
        .spawn((
            Name::new("ViewmodelGun"),
            ViewmodelGun::default(),
            Transform::from_translation(HIP_OFFSET),
            Visibility::default(),
            ChildOf(camera),
        ))
        .id();
    for (size, centre, furniture) in BOXES {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(parts[furniture as usize].clone()),
            Transform::from_translation(centre),
            viewmodel_layer(),
            NotShadowCaster,
            ChildOf(gun),
        ));
    }
    let barrel_length = BARREL_START - MUZZLE.z;
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(BARREL_RADIUS, barrel_length))),
        MeshMaterial3d(parts[Furniture::Metal as usize].clone()),
        Transform::from_xyz(0.0, MUZZLE.y, (BARREL_START + MUZZLE.z) / 2.0)
            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        viewmodel_layer(),
        NotShadowCaster,
        ChildOf(gun),
    ));
    commands.spawn((
        Name::new("MuzzleFlash"),
        MuzzleFlash::default(),
        Transform::from_translation(MUZZLE),
        Visibility::Hidden,
        ChildOf(gun),
        children![
            (
                Mesh3d(meshes.add(Sphere::new(0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.8, 0.45),
                    emissive: LinearRgba::rgb(14.0, 8.0, 2.5),
                    alpha_mode: AlphaMode::Add,
                    ..default()
                })),
                Transform::from_xyz(0.0, 0.0, -FLASH_SIZE.z / 2.0).with_scale(FLASH_SIZE),
                viewmodel_layer(),
                NotShadowCaster,
            ),
            (
                PointLight {
                    color: Color::srgb(1.0, 0.75, 0.4),
                    intensity: MUZZLE_LIGHT_LUMENS,
                    range: MUZZLE_LIGHT_RANGE,
                    shadow_maps_enabled: false,
                    ..default()
                },
                // Lights the world around the player as well as the gun.
                RenderLayers::from_layers(&[0, VIEWMODEL_LAYER]),
            ),
        ],
    ));
}

/// Uses the eye camera's field of view, so the gun zooms with aiming and the muzzle lines up
/// with world-space tracers.
fn match_eye_fov(
    eye: Query<&Projection, (With<EyeCamera>, Without<ViewmodelCamera>)>,
    mut viewmodel: Query<&mut Projection, With<ViewmodelCamera>>,
) {
    let Ok(Projection::Perspective(eye)) = eye.single() else {
        return;
    };
    for mut projection in &mut viewmodel {
        if let Projection::Perspective(perspective) = &mut *projection
            && perspective.fov != eye.fov
        {
            perspective.fov = eye.fov;
        }
    }
}

fn animate_viewmodel(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    captured: Res<MouseCaptured>,
    motion: Res<AccumulatedMouseMotion>,
    mut fired: MessageReader<ShotFired>,
    player: Query<(&HeldWeapon, &Motor), With<LocalPlayer>>,
    mut gun: Query<(&mut ViewmodelGun, &mut Transform, &mut Visibility)>,
) {
    let shots = fired.read().count();
    let Ok((weapon, motor)) = player.single() else {
        return;
    };
    let Ok((mut anim, mut transform, mut visibility)) = gun.single_mut() else {
        return;
    };
    visibility.set_if_neq(if life.is_dead() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
    let dt = time.delta_secs();
    let ads = weapon.ads_eased();
    let motion_scale = 1.0_f32.lerp(ADS_MOTION_SCALE, ads);

    let velocity = motor.velocity();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let bob_target = if motor.grounded() {
        (speed / BOB_REFERENCE_SPEED).min(1.5)
    } else {
        0.0
    };
    anim.bob_amount.smooth_nudge(&bob_target, FOLLOW_RATE, dt);
    anim.bob_phase = (anim.bob_phase + speed * BOB_RADIANS_PER_METRE * dt) % TAU;
    let bob = Vec2::new(
        anim.bob_phase.sin() * BOB_AMPLITUDE.x,
        -anim.bob_phase.cos().abs() * BOB_AMPLITUDE.y,
    ) * anim.bob_amount;

    let sway_target = if captured.active() {
        (Vec2::new(-motion.delta.x, motion.delta.y) * SWAY_PER_COUNT).clamp_length_max(SWAY_MAX)
    } else {
        Vec2::ZERO
    };
    anim.sway.smooth_nudge(&sway_target, FOLLOW_RATE, dt);

    anim.kick = ((anim.kick + shots as f32).min(KICK_MAX)) * (-KICK_DECAY * dt).exp();
    let kick = anim.kick * 1.0_f32.lerp(ADS_KICK_SCALE, ads);

    let stats = data.weapon(weapon.id);
    let dip = weapon
        .reload_progress(stats)
        .map_or(0.0, |progress| (progress * PI).sin());

    if weapon.sprinting {
        anim.sprint.smooth_nudge(&1.0, SPRINT_IN_RATE, dt);
    } else {
        anim.sprint = anim.sprint.min(weapon.sprint_out_fraction(stats));
    }
    let sprint = ease(anim.sprint);
    let mantle_target = if weapon.mantling { 1.0 } else { 0.0 };
    anim.mantle.smooth_nudge(&mantle_target, MANTLE_RATE, dt);
    let mantle = ease(anim.mantle);

    let wobble = (bob + anim.sway) * motion_scale;
    transform.translation = HIP_OFFSET.lerp(ADS_OFFSET, ads)
        + SPRINT_OFFSET * sprint
        + Vec3::new(
            wobble.x,
            wobble.y - RELOAD_DROP * dip - MANTLE_DROP * mantle,
            KICK_BACK * kick,
        );
    transform.rotation = Quat::from_euler(
        EulerRot::YXZ,
        anim.sway.x * SWAY_TILT * motion_scale + SPRINT_YAW * sprint,
        KICK_PITCH * kick - RELOAD_TILT * dip + anim.sway.y * SWAY_TILT * motion_scale
            + SPRINT_PITCH * sprint
            - MANTLE_TILT * mantle,
        RELOAD_ROLL * dip + SPRINT_ROLL * sprint,
    );
}
