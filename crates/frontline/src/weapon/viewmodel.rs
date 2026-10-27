//! First-person gun: a blocky model per `GunModel` on its own camera and render layer, so it
//! is drawn over the world and never clips into walls. Rebuilt when the held Weapon changes.
//! Moves to the sights when aiming, bobs while walking, sways with the mouse, kicks on each
//! shot, dips during a reload, rolls while a bolt is worked, angles down and across while
//! sprinting (raising back as the sprint-out ends), drops during a mantle, and hides while
//! looking through a scope.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::camera::CameraOutputMode;
use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;

use super::effects::MuzzleFlash;
use super::{HeldWeapon, ShotFired, WeaponSystems, ease};
use crate::atmosphere;
use crate::data::{GameData, GunModel, WeaponId, WeaponStats};
use crate::flow::AppState;
use crate::player::{ControllerSystems, EyeCamera, LocalLife, Motor, MouseCaptured};
use crate::shared::LocalPlayer;

/// Render layer of the viewmodel; the world stays on layer 0.
pub(super) const VIEWMODEL_LAYER: usize = 1;

/// Gun origin relative to the camera at the hip; fully aimed it comes from `ads_offset`.
const HIP_OFFSET: Vec3 = Vec3::new(0.14, -0.15, -0.50);

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
/// Bolt cycle at its peak: the gun rolls onto its side, tips up and shifts right while the
/// handle lifts and slides back by `BOLT_TRAVEL`.
const BOLT_ROLL: f32 = -0.45;
const BOLT_TILT: f32 = 0.12;
const BOLT_SHIFT: Vec3 = Vec3::new(0.02, -0.02, 0.0);
const BOLT_TRAVEL: f32 = 0.08;
const BOLT_LIFT: f32 = 0.012;
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
/// Muzzle flash ellipsoid size, stretched along the barrel.
const FLASH_SIZE: Vec3 = Vec3::new(0.07, 0.07, 0.16);

pub(super) struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(attach_viewmodel)
            .add_systems(Startup, create_gun_materials)
            .add_systems(OnEnter(AppState::InGame), spawn_viewmodel_light)
            .add_systems(
                Update,
                (match_eye_fov, build_gun, animate_viewmodel)
                    .chain()
                    .after(WeaponSystems)
                    .after(ControllerSystems::View),
            );
    }
}

/// Draws the gun on top of the world camera. It renders LDR with MSAA into its own texture
/// (the world camera is HDR without MSAA, see `atmosphere`), cleared transparent and
/// alpha-composited over the world, with the world's tone curve and grade.
#[derive(Component)]
pub struct ViewmodelCamera;

/// The gun root under the viewmodel camera, with its procedural animation state.
#[derive(Component, Debug, Default)]
pub(super) struct ViewmodelGun {
    /// The Weapon whose model is currently built under it.
    weapon: Option<WeaponId>,
    bob_phase: f32,
    bob_amount: f32,
    sway: Vec2,
    kick: f32,
    /// Sprint pose blend, 0 ready to 1 fully lowered.
    sprint: f32,
    /// Mantle pose blend, 0 ready to 1 fully lowered.
    mantle: f32,
}

/// A mesh of the current gun model; replaced when the model changes.
#[derive(Component)]
struct GunPart;

/// The bolt handle of a bolt-action model, at rest at `.0` in gun space.
#[derive(Component)]
struct BoltHandle(Vec3);

fn viewmodel_layer() -> RenderLayers {
    RenderLayers::layer(VIEWMODEL_LAYER)
}

fn spawn_viewmodel_light(mut commands: Commands) {
    commands.spawn((
        Name::new("Viewmodel light"),
        DespawnOnExit(AppState::InGame),
        DirectionalLight {
            illuminance: VIEWMODEL_LIGHT_LUX,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
        viewmodel_layer(),
    ));
}

/// One gun body in gun space (-Z is forward, origin at the receiver). Muzzle and sight point
/// come from the Weapon's data; the sight is built around its sight point.
struct GunShape {
    /// Box parts: size, centre and finish.
    boxes: &'static [(Vec3, Vec3, Furniture)],
    barrel_radius: f32,
    /// Where the barrel leaves the gun body; it runs forward to the muzzle.
    barrel_start: f32,
    /// Top of the receiver, where sight mounts stand.
    body_top: f32,
    sight: Sight,
    /// Bolt handle box (size, centre), if the model has one to work.
    bolt: Option<(Vec3, Vec3)>,
}

/// Kind of sight, built around `WeaponStats::sight_point`.
enum Sight {
    /// A thin open frame around a glass window with a red dot at its centre.
    RedDot,
    /// A rear aperture ring at the sight point and a front post at `front_z`, its tip on
    /// the ring's axis.
    Irons { front_z: f32 },
    /// A scope tube whose eyepiece ends at the sight point.
    Scope,
}

/// Red dot window (width, height), frame bar thickness and housing depth.
const RED_DOT_WINDOW: Vec2 = Vec2::new(0.036, 0.028);
const RED_DOT_FRAME: f32 = 0.0015;
const RED_DOT_DEPTH: f32 = 0.012;
const RED_DOT_RADIUS: f32 = 0.0009;
/// Rear aperture ring radii (hole, outside), front post width and height.
const APERTURE: (f32, f32) = (0.005, 0.0075);
const FRONT_POST: Vec2 = Vec2::new(0.0025, 0.042);
/// Scope cylinders as (radius, from, to), z measured from the eyepiece end.
const SCOPE_TUBES: [(f32, f32, f32); 3] = [
    // Eyepiece, main tube, objective bell.
    (0.026, -0.06, 0.0),
    (0.02, -0.32, -0.06),
    (0.03, -0.43, -0.32),
];
/// Scope rings, z measured from the eyepiece end.
const SCOPE_RINGS: [f32; 2] = [-0.27, -0.11];

const RIFLE: GunShape = GunShape {
    boxes: &[
        // Receiver.
        (
            Vec3::new(0.055, 0.075, 0.30),
            Vec3::new(0.0, 0.0, 0.0),
            Furniture::Metal,
        ),
        // Top rail.
        (
            Vec3::new(0.022, 0.008, 0.26),
            Vec3::new(0.0, 0.0415, -0.02),
            Furniture::Metal,
        ),
        // Handguard.
        (
            Vec3::new(0.06, 0.06, 0.20),
            Vec3::new(0.0, 0.0, -0.24),
            Furniture::Polymer,
        ),
        // Magazine.
        (
            Vec3::new(0.032, 0.14, 0.065),
            Vec3::new(0.0, -0.10, -0.07),
            Furniture::Metal,
        ),
        // Pistol grip.
        (
            Vec3::new(0.035, 0.10, 0.045),
            Vec3::new(0.0, -0.08, 0.09),
            Furniture::Polymer,
        ),
        // Stock.
        (
            Vec3::new(0.045, 0.075, 0.20),
            Vec3::new(0.0, -0.015, 0.25),
            Furniture::Polymer,
        ),
    ],
    barrel_radius: 0.012,
    barrel_start: -0.30,
    body_top: 0.0455,
    sight: Sight::RedDot,
    bolt: None,
};

/// Short and stubby: compact receiver, curved-looking magazine, thin collapsed stock.
const SMG: GunShape = GunShape {
    boxes: &[
        // Receiver.
        (
            Vec3::new(0.05, 0.07, 0.24),
            Vec3::new(0.0, 0.0, 0.0),
            Furniture::Metal,
        ),
        // Handguard.
        (
            Vec3::new(0.058, 0.058, 0.11),
            Vec3::new(0.0, -0.005, -0.175),
            Furniture::Polymer,
        ),
        // Magazine, raked forward.
        (
            Vec3::new(0.028, 0.15, 0.04),
            Vec3::new(0.0, -0.10, -0.085),
            Furniture::Metal,
        ),
        // Pistol grip.
        (
            Vec3::new(0.035, 0.10, 0.045),
            Vec3::new(0.0, -0.08, 0.075),
            Furniture::Polymer,
        ),
        // Collapsed stock struts and butt plate.
        (
            Vec3::new(0.03, 0.045, 0.12),
            Vec3::new(0.0, -0.005, 0.18),
            Furniture::Metal,
        ),
        (
            Vec3::new(0.045, 0.085, 0.02),
            Vec3::new(0.0, -0.02, 0.245),
            Furniture::Polymer,
        ),
    ],
    barrel_radius: 0.011,
    barrel_start: -0.23,
    body_top: 0.035,
    sight: Sight::Irons { front_z: -0.20 },
    bolt: None,
};

/// Long stock and barrel under a scope tube, with a bolt handle on the right.
const SNIPER: GunShape = GunShape {
    boxes: &[
        // Receiver.
        (
            Vec3::new(0.05, 0.065, 0.30),
            Vec3::new(0.0, 0.0, 0.0),
            Furniture::Metal,
        ),
        // Forend.
        (
            Vec3::new(0.06, 0.065, 0.42),
            Vec3::new(0.0, -0.015, -0.33),
            Furniture::Polymer,
        ),
        // Magazine floorplate.
        (
            Vec3::new(0.035, 0.03, 0.08),
            Vec3::new(0.0, -0.045, -0.03),
            Furniture::Metal,
        ),
        // Wrist.
        (
            Vec3::new(0.04, 0.09, 0.06),
            Vec3::new(0.0, -0.06, 0.12),
            Furniture::Polymer,
        ),
        // Buttstock.
        (
            Vec3::new(0.05, 0.10, 0.28),
            Vec3::new(0.0, -0.035, 0.29),
            Furniture::Polymer,
        ),
    ],
    barrel_radius: 0.011,
    barrel_start: -0.50,
    body_top: 0.0325,
    sight: Sight::Scope,
    bolt: Some((Vec3::new(0.07, 0.014, 0.014), Vec3::new(0.045, 0.012, 0.10))),
};

fn shape(model: GunModel) -> &'static GunShape {
    match model {
        GunModel::Rifle => &RIFLE,
        GunModel::Smg => &SMG,
        GunModel::Sniper => &SNIPER,
    }
}

/// Gun origin relative to the camera when fully aimed: the sight point on the screen centre,
/// `sight_eye_relief` ahead, with the gun square to the view so the sight axis is the aim line.
fn ads_offset(stats: &WeaponStats) -> Vec3 {
    let sight = Vec3::from(stats.sight_point);
    Vec3::new(-sight.x, -sight.y, -stats.sight_eye_relief - sight.z)
}

#[derive(Clone, Copy)]
enum Furniture {
    Metal,
    Polymer,
    Sight,
    /// Faintly tinted, unlit optic glass.
    Glass,
    /// Unlit red reticle.
    Reticle,
}

impl Furniture {
    const ALL: [Furniture; 5] = [
        Furniture::Metal,
        Furniture::Polymer,
        Furniture::Sight,
        Furniture::Glass,
        Furniture::Reticle,
    ];

    fn material(self) -> StandardMaterial {
        let (color, roughness) = match self {
            Furniture::Metal => (Color::srgb(0.16, 0.16, 0.17), 0.45),
            Furniture::Polymer => (Color::srgb(0.22, 0.20, 0.17), 0.8),
            Furniture::Sight => (Color::srgb(0.04, 0.04, 0.04), 0.6),
            Furniture::Glass => {
                return StandardMaterial {
                    base_color: Color::srgba(0.55, 0.75, 0.85, 0.10),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    cull_mode: None,
                    ..default()
                };
            }
            Furniture::Reticle => {
                return StandardMaterial {
                    base_color: Color::srgb(1.0, 0.08, 0.05),
                    unlit: true,
                    cull_mode: None,
                    ..default()
                };
            }
        };
        StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            ..default()
        }
    }
}

/// Materials shared by every gun model, indexed by `Furniture`.
#[derive(Resource)]
struct GunMaterials([Handle<StandardMaterial>; 5]);

fn create_gun_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(GunMaterials(
        Furniture::ALL.map(|furniture| materials.add(furniture.material())),
    ));
}

/// Meshes of a sight built around `sight` (the sight point), as (mesh, finish, transform).
fn sight_parts(shape: &GunShape, sight: Vec3) -> Vec<(Mesh, Furniture, Transform)> {
    let block = |size: Vec3, centre: Vec3| {
        (
            Mesh::from(Cuboid::from_size(size)),
            Furniture::Sight,
            Transform::from_translation(centre),
        )
    };
    // A mount block standing on the receiver up to `top`, centred at `z`.
    let mount = |top: f32, z: f32, width: f32, depth: f32| {
        let height = (top - shape.body_top).max(0.0);
        block(
            Vec3::new(width, height, depth),
            Vec3::new(sight.x, shape.body_top + height / 2.0, z),
        )
    };
    let along_axis = Quat::from_rotation_x(FRAC_PI_2);
    match shape.sight {
        Sight::RedDot => {
            let (w, h, t) = (RED_DOT_WINDOW.x, RED_DOT_WINDOW.y, RED_DOT_FRAME);
            let side = Vec3::new(t, h + 2.0 * t, RED_DOT_DEPTH);
            let bar = Vec3::new(w + 2.0 * t, t, RED_DOT_DEPTH);
            vec![
                block(side, sight + Vec3::X * (w + t) / 2.0),
                block(side, sight - Vec3::X * (w + t) / 2.0),
                block(bar, sight + Vec3::Y * (h + t) / 2.0),
                block(bar, sight - Vec3::Y * (h + t) / 2.0),
                mount(sight.y - h / 2.0 - t, sight.z, 0.012, 0.03),
                (
                    Mesh::from(Rectangle::from_size(RED_DOT_WINDOW)),
                    Furniture::Glass,
                    Transform::from_translation(sight),
                ),
                // Just in front of the glass towards the eye, so it never z-fights it.
                (
                    Mesh::from(Circle::new(RED_DOT_RADIUS)),
                    Furniture::Reticle,
                    Transform::from_translation(sight + Vec3::Z * 0.001),
                ),
            ]
        }
        Sight::Irons { front_z } => {
            let (hole, outside) = APERTURE;
            let post = Vec3::new(FRONT_POST.x, FRONT_POST.y, 0.003);
            vec![
                (
                    Mesh::from(Torus::new(hole, outside)),
                    Furniture::Sight,
                    Transform::from_translation(sight).with_rotation(along_axis),
                ),
                mount(sight.y - outside + 0.001, sight.z, 0.006, 0.008),
                block(post, Vec3::new(sight.x, sight.y - post.y / 2.0, front_z)),
            ]
        }
        Sight::Scope => SCOPE_TUBES
            .iter()
            .map(|&(radius, from, to)| {
                (
                    Mesh::from(Cylinder::new(radius, to - from)),
                    Furniture::Sight,
                    Transform::from_xyz(sight.x, sight.y, sight.z + (from + to) / 2.0)
                        .with_rotation(along_axis),
                )
            })
            .chain(
                SCOPE_RINGS
                    .iter()
                    .map(|&z| mount(sight.y - 0.019, sight.z + z, 0.02, 0.02)),
            )
            .collect(),
    }
}

/// Builds the viewmodel camera, the (empty) gun root and its muzzle flash under the local
/// player's eye camera; `build_gun` adds the model.
fn attach_viewmodel(
    add: On<Add<EyeCamera>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera = commands
        .spawn((
            Name::new("ViewmodelCamera"),
            ViewmodelCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                },
                ..default()
            },
            atmosphere::TONEMAPPING,
            atmosphere::color_grading(),
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
    commands.spawn((
        Name::new("MuzzleFlash"),
        MuzzleFlash::default(),
        Transform::default(),
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

/// Swaps the gun model when the held Weapon changes, moving the flash to its muzzle.
fn build_gun(
    mut commands: Commands,
    data: Res<GameData>,
    materials: Res<GunMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    player: Query<&HeldWeapon, With<LocalPlayer>>,
    mut gun: Query<(Entity, &mut ViewmodelGun, &Children)>,
    parts: Query<(), With<GunPart>>,
    mut flash: Query<&mut Transform, With<MuzzleFlash>>,
) {
    let (Ok(weapon), Ok((gun, mut anim, children))) = (player.single(), gun.single_mut()) else {
        return;
    };
    if anim.weapon == Some(weapon.id) {
        return;
    }
    anim.weapon = Some(weapon.id);
    for child in children.iter().filter(|child| parts.contains(*child)) {
        commands.entity(child).despawn();
    }

    let stats = data.weapon(weapon.id);
    let shape = shape(stats.model);
    let muzzle = Vec3::from(stats.muzzle_offset);
    let mut part = |mesh: Mesh, furniture: Furniture, transform: Transform| {
        commands
            .spawn((
                GunPart,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.0[furniture as usize].clone()),
                transform,
                viewmodel_layer(),
                NotShadowCaster,
                ChildOf(gun),
            ))
            .id()
    };
    for &(size, centre, furniture) in shape.boxes {
        part(
            Cuboid::from_size(size).into(),
            furniture,
            Transform::from_translation(centre),
        );
    }
    let barrel_length = shape.barrel_start - muzzle.z;
    part(
        Cylinder::new(shape.barrel_radius, barrel_length).into(),
        Furniture::Metal,
        Transform::from_xyz(muzzle.x, muzzle.y, (shape.barrel_start + muzzle.z) / 2.0)
            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
    );
    for (mesh, furniture, transform) in sight_parts(shape, Vec3::from(stats.sight_point)) {
        part(mesh, furniture, transform);
    }
    if let Some((size, centre)) = shape.bolt {
        let handle = part(
            Cuboid::from_size(size).into(),
            Furniture::Metal,
            Transform::from_translation(centre),
        );
        commands.entity(handle).insert(BoltHandle(centre));
    }
    for mut transform in &mut flash {
        transform.translation = muzzle;
    }
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

#[allow(clippy::too_many_arguments)]
fn animate_viewmodel(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    captured: Res<MouseCaptured>,
    motion: Res<AccumulatedMouseMotion>,
    mut fired: MessageReader<ShotFired>,
    player: Query<(&HeldWeapon, &Motor), With<LocalPlayer>>,
    mut gun: Query<(&mut ViewmodelGun, &mut Transform, &mut Visibility), Without<BoltHandle>>,
    mut bolt_handle: Query<(&BoltHandle, &mut Transform), Without<ViewmodelGun>>,
) {
    let shots = fired.read().count();
    let Ok((weapon, motor)) = player.single() else {
        return;
    };
    let Ok((mut anim, mut transform, mut visibility)) = gun.single_mut() else {
        return;
    };
    let stats = data.weapon(weapon.id);
    visibility.set_if_neq(if life.is_dead() || weapon.scoped(stats) {
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

    let dip = weapon
        .reload_progress(stats)
        .map_or(0.0, |progress| (progress * PI).sin());
    let bolt = weapon.bolt_phase().map_or(0.0, |phase| (phase * PI).sin());
    for (handle, mut handle_transform) in &mut bolt_handle {
        handle_transform.translation = handle.0 + Vec3::new(0.0, BOLT_LIFT, BOLT_TRAVEL) * bolt;
    }

    if weapon.sprinting {
        anim.sprint.smooth_nudge(&1.0, SPRINT_IN_RATE, dt);
    } else {
        anim.sprint = anim.sprint.min(weapon.sprint_out_fraction(stats));
    }
    let sprint = ease(anim.sprint);
    let mantle_target = if weapon.mantling { 1.0 } else { 0.0 };
    anim.mantle.smooth_nudge(&mantle_target, MANTLE_RATE, dt);
    let mantle = ease(anim.mantle);

    let ads_offset = ads_offset(stats);
    let wobble = (bob + anim.sway) * motion_scale;
    transform.translation = HIP_OFFSET.lerp(ads_offset, ads)
        + SPRINT_OFFSET * sprint
        + BOLT_SHIFT * bolt
        + Vec3::new(
            wobble.x,
            wobble.y - RELOAD_DROP * dip - MANTLE_DROP * mantle,
            KICK_BACK * kick,
        );
    transform.rotation = Quat::from_euler(
        EulerRot::YXZ,
        anim.sway.x * SWAY_TILT * motion_scale + SPRINT_YAW * sprint,
        KICK_PITCH * kick - RELOAD_TILT * dip
            + anim.sway.y * SWAY_TILT * motion_scale
            + SPRINT_PITCH * sprint
            - MANTLE_TILT * mantle
            + BOLT_TILT * bolt,
        RELOAD_ROLL * dip + SPRINT_ROLL * sprint + BOLT_ROLL * bolt,
    );
}
