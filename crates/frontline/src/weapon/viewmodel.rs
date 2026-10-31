//! First-person gun: a glTF model per `GunModel` (see `rig`) on its own camera and render
//! layer, so it is drawn over the world and never clips into walls, with gloved, sleeved arms
//! holding it. Rebuilt when the held Weapon changes. Moves to the sights when aiming, bobs while
//! walking, sways with the mouse, kicks on each shot, dips during a reload while the left hand
//! pulls the magazine and seats a new one, rolls while a bolt is worked, angles down and across
//! while sprinting (raising back as the sprint-out ends), drops during a mantle, lowers for a
//! Grenade throw (the right hand winds up with the Grenade) and hides while looking through a
//! scope.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::AssetPath;
use bevy::camera::CameraOutputMode;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::light::NotShadowCaster;
use bevy::mesh::{UvChannel, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, Face};
use bevy::world_serialization::WorldInstanceReady;

use super::effects::MuzzleFlash;
use super::rig::{self, GunRig, HandPose};
use super::{HeldWeapon, Kit, ShotFired, WeaponSystems, ease};
use crate::assets::EMBEDDED_ROOT;
use crate::atmosphere;
use crate::data::{GameData, SightKind, WeaponId, WeaponStats};
use crate::equipment::GrenadeKind;
use crate::flow::{AppState, Handedness, Settings};
use crate::gear::{AttachmentId, Attachments, NO_CAMO};
use crate::net::protocol::ThrowGrenade;
use crate::player::{ControllerSystems, EyeCamera, LocalLife, Motor, MouseCaptured};
use crate::shared::{LocalPlayer, ToServer};

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
/// Reload at its deepest: the gun comes up, tips muzzle-up and rolls so the magazine well
/// faces the player.
const RELOAD_RAISE: f32 = 0.03;
const RELOAD_TILT: f32 = 0.3;
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
/// Switching Weapons (or throwing) at its lowest point: drop and tilt down.
const SWITCH_DROP: f32 = 0.28;
const SWITCH_TILT: f32 = 0.9;

/// A Grenade throw as the viewmodel plays it, in seconds: raise the hand with the Grenade,
/// wind back, throw (the Grenade leaves the hand at `THROW_RELEASE`), lower the hand and raise
/// the gun again.
const THROW_SECS: f32 = 0.62;
const THROW_RELEASE: f32 = 0.27;
/// Right hand holding the Grenade at its three poses in camera space: ready, wound back and
/// thrown, as (position, tilt of the palm-up hand about X).
const THROW_READY: (Vec3, f32) = (Vec3::new(0.09, -0.11, -0.34), 0.15);
const THROW_BACK: (Vec3, f32) = (Vec3::new(0.15, -0.09, -0.23), -0.5);
const THROW_FORWARD: (Vec3, f32) = (Vec3::new(0.06, -0.07, -0.56), 0.6);

/// How much gun one tile of a camo texture covers, in metres.
const CAMO_TILE_METRES: f32 = 0.12;

/// How far the fingers close round a Grenade.
const THROW_CURL: f32 = 0.7;

/// Light for the viewmodel layer; world lights only reach the world layer.
const VIEWMODEL_LIGHT_LUX: f32 = 9_000.0;

pub(super) struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(attach_viewmodel)
            .add_observer(scene_ready)
            .add_systems(Startup, create_gun_materials)
            .add_systems(OnEnter(AppState::InGame), spawn_viewmodel_light)
            .add_systems(
                Update,
                (
                    match_eye_fov,
                    build_gun,
                    // The gloves are made with the gun: the menu backdrop has neither.
                    animate_viewmodel.run_if(resource_exists::<GloveMeshes>),
                )
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

/// The entities of one arm, in the layout of a right-handed player (mirrored when the player
/// is left-handed): the glove and the sleeve's two tubes and elbow joint.
#[derive(Clone, Copy, Debug)]
struct LimbEntities {
    hand: Entity,
    upper: Entity,
    fore: Entity,
    elbow: Entity,
}

/// The entities of the arms, built once with the camera.
#[derive(Clone, Copy, Debug)]
struct ArmEntities {
    /// Parent of the hands, sleeves and the held Grenade; hidden with the gun.
    root: Entity,
    /// The arm that holds the grip and throws, and the one that steadies the gun.
    strong: LimbEntities,
    support: LimbEntities,
    /// The Grenade in the strong hand, and one scene per kind under it.
    holder: Entity,
    grenades: [(GrenadeKind, Entity); 3],
}

/// The gloves, one mesh per `rig::GLOVE_CURLS` step and hand.
#[derive(Resource)]
struct GloveMeshes {
    right: [Handle<Mesh>; rig::GLOVE_CURLS.len()],
    left: [Handle<Mesh>; rig::GLOVE_CURLS.len()],
}

/// The gun root under the viewmodel camera, with its procedural animation state.
#[derive(Component, Debug)]
pub(super) struct ViewmodelGun {
    /// The Weapon, Attachments, camo (an id from `data/progression.ron`, `none` for bare
    /// metal) and handedness whose model is currently built under it.
    weapon: Option<(WeaponId, Attachments, String, Handedness)>,
    /// Rig of the model built, and where the left hand rests on it.
    rig: Option<GunRig>,
    left_rest: Option<HandPose>,
    arms: ArmEntities,
    bob_phase: f32,
    bob_amount: f32,
    sway: Vec2,
    kick: f32,
    /// Sprint pose blend, 0 ready to 1 fully lowered.
    sprint: f32,
    /// Mantle pose blend, 0 ready to 1 fully lowered.
    mantle: f32,
    /// A Grenade throw under way: its kind and seconds in.
    throw: Option<(GrenadeKind, f32)>,
}

/// A mesh of the current gun model; replaced when the model changes.
#[derive(Component)]
struct GunPart;

/// A glTF scene on the viewmodel layer: every mesh of it is moved there when it is ready; a
/// gun's gets `camo`, its magazine node is found, the nodes in `ads_hidden` go when aimed and
/// those in `removed` (what a fitted Optic replaces) are not drawn. `mirrored`: the scene is
/// under a mirrored (negative scale) root, which turns every triangle inside out on screen, so
/// its materials cull the other face.
#[derive(Component)]
struct ViewScene {
    camo: String,
    mirrored: bool,
    bare: &'static [&'static str],
    magazine: Option<&'static str>,
    ads_hidden: &'static [&'static str],
    removed: &'static [&'static str],
}

/// Part of a gun model that is hidden while aimed (a wire stock round the eye).
#[derive(Component)]
struct AdsHidden;

/// The magazine node of the gun model, at rest at `.0` in its parent's space.
#[derive(Component)]
struct Magazine(Vec3);

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

/// Red dot window (width, height), frame bar thickness and housing depth.
const RED_DOT_WINDOW: Vec2 = Vec2::new(0.036, 0.028);
const RED_DOT_FRAME: f32 = 0.0015;
const RED_DOT_DEPTH: f32 = 0.012;
const RED_DOT_RADIUS: f32 = 0.0009;
/// ACOG window, frame bar thickness, housing length and reticle dot radius.
const ACOG_WINDOW: Vec2 = Vec2::new(0.026, 0.024);
const ACOG_FRAME: f32 = 0.007;
const ACOG_DEPTH: f32 = 0.09;
const ACOG_RADIUS: f32 = 0.0007;
/// Holographic window, frame bar thickness, housing length, reticle dot radius and the ring's
/// inner and outer radii.
const HOLO_WINDOW: Vec2 = Vec2::new(0.034, 0.026);
const HOLO_FRAME: f32 = 0.002;
const HOLO_DEPTH: f32 = 0.04;
const HOLO_DOT: f32 = 0.0007;
const HOLO_RING: (f32, f32) = (0.0062, 0.0072);
/// Prism tube radii (inner, outer), length, objective bell growth and length, and the chevron's
/// arm length and thickness.
const PRISM_TUBE: (f32, f32) = (0.0125, 0.0165);
const PRISM_DEPTH: f32 = 0.07;
const PRISM_BELL: (f32, f32) = (0.004, 0.012);
const PRISM_CHEVRON: (f32, f32) = (0.0075, 0.0009);
/// Silencer girth relative to the barrel.
const SUPPRESSOR_RADIUS_SCALE: f32 = 1.9;
/// Foregrip: radius and length. Grenade Launcher: tube radius and length.
const FOREGRIP: (f32, f32) = (0.014, 0.075);
const LAUNCHER: (f32, f32) = (0.022, 0.2);

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
    Glove,
    Sleeve,
}

impl Furniture {
    const ALL: [Furniture; 7] = [
        Furniture::Metal,
        Furniture::Polymer,
        Furniture::Sight,
        Furniture::Glass,
        Furniture::Reticle,
        Furniture::Glove,
        Furniture::Sleeve,
    ];

    /// `mirrored`: for a part under a mirrored root, whose triangles run the other way round
    /// on screen, so the face that is culled is the other one.
    fn material(self, mirrored: bool) -> StandardMaterial {
        let (color, roughness) = match self {
            Furniture::Metal => (Color::srgb(0.16, 0.16, 0.17), 0.45),
            Furniture::Polymer => (Color::srgb(0.22, 0.20, 0.17), 0.8),
            Furniture::Sight => (Color::srgb(0.04, 0.04, 0.04), 0.6),
            Furniture::Glove => (Color::srgb(0.045, 0.045, 0.05), 0.8),
            Furniture::Sleeve => (Color::srgb(0.2, 0.22, 0.14), 0.95),
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
            cull_mode: Some(if mirrored { Face::Front } else { Face::Back }),
            ..default()
        }
    }
}

/// Materials shared by every procedural part, indexed by `Furniture`: for the gun as it is,
/// and for the gun under a mirrored root.
#[derive(Resource)]
struct GunMaterials {
    plain: [Handle<StandardMaterial>; 7],
    mirrored: [Handle<StandardMaterial>; 7],
}

impl GunMaterials {
    fn get(&self, furniture: Furniture, mirrored: bool) -> Handle<StandardMaterial> {
        let set = if mirrored {
            &self.mirrored
        } else {
            &self.plain
        };
        set[furniture as usize].clone()
    }
}

fn create_gun_materials(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    data: Res<GameData>,
) {
    commands.insert_resource(GunMaterials {
        plain: Furniture::ALL.map(|furniture| materials.add(furniture.material(false))),
        mirrored: Furniture::ALL.map(|furniture| materials.add(furniture.material(true))),
    });
    let camos = data
        .progression
        .camos
        .iter()
        .map(|camo| {
            let texture = assets
                .load_builder()
                .with_settings(|settings: &mut ImageLoaderSettings| {
                    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        label: Some("camo".into()),
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    });
                })
                .load(format!("{EMBEDDED_ROOT}camos/{}", camo.texture));
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                metallic: camo.metallic,
                perceptual_roughness: camo.roughness,
                ..default()
            });
            (camo.id.clone(), material)
        })
        .collect();
    commands.insert_resource(CamoMaterials(camos));
}

/// One material per camo of `data/progression.ron`, by id.
#[derive(Resource)]
struct CamoMaterials(HashMap<String, Handle<StandardMaterial>>);

/// Texture coordinates for a camo pattern, running along the gun's own axes (one tile per
/// `CAMO_TILE_METRES`) so the pattern is the same size on every part and carries on across the
/// joins between them. `to_gun` takes the mesh's vertices into gun space.
fn camo_uvs(mesh: &Mesh, to_gun: &Transform) -> Option<Vec<[f32; 2]>> {
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    )
    else {
        return None;
    };
    Some(
        positions
            .iter()
            .zip(normals)
            .map(|(position, normal)| {
                let p = to_gun.transform_point(Vec3::from(*position));
                let n = (to_gun.rotation * Vec3::from(*normal)).abs();
                // Project along the axis the face looks down.
                let flat = if n.x > n.y && n.x > n.z {
                    Vec2::new(p.z, p.y)
                } else if n.y > n.z {
                    Vec2::new(p.x, p.z)
                } else {
                    Vec2::new(p.x, p.y)
                };
                (flat / CAMO_TILE_METRES).to_array()
            })
            .collect(),
    )
}

/// A rectangular optic around `sight`: four frame bars of thickness `frame` and length `depth`
/// (centred `shift` towards the eye) around a glass `window`, with a dot of `dot` radius at its
/// centre.
fn optic_parts(
    sight: Vec3,
    window: Vec2,
    frame: f32,
    depth: f32,
    shift: f32,
    dot: f32,
) -> Vec<(Mesh, Furniture, Transform)> {
    let block = |size: Vec3, centre: Vec3| {
        (
            Mesh::from(Cuboid::from_size(size)),
            Furniture::Sight,
            Transform::from_translation(centre),
        )
    };
    let (w, h, t) = (window.x, window.y, frame);
    let centre = sight + Vec3::Z * shift;
    let side = Vec3::new(t, h + 2.0 * t, depth);
    let bar = Vec3::new(w + 2.0 * t, t, depth);
    vec![
        block(side, centre + Vec3::X * (w + t) / 2.0),
        block(side, centre - Vec3::X * (w + t) / 2.0),
        block(bar, centre + Vec3::Y * (h + t) / 2.0),
        block(bar, centre - Vec3::Y * (h + t) / 2.0),
        (
            Mesh::from(Rectangle::from_size(window)),
            Furniture::Glass,
            Transform::from_translation(sight),
        ),
        // Just in front of the glass towards the eye, so it never z-fights it.
        (
            Mesh::from(Circle::new(dot)),
            Furniture::Reticle,
            Transform::from_translation(sight + Vec3::Z * 0.001),
        ),
    ]
}

/// Meshes of the fitted Optic (`stats.sight`) built around its sight point, as
/// (mesh, finish, transform). The iron sights and the Sniper's scope are part of the models.
fn sight_parts(rig: &GunRig, stats: &WeaponStats) -> Vec<(Mesh, Furniture, Transform)> {
    let sight = Vec3::from(stats.sight_point);
    let block = |size: Vec3, centre: Vec3| {
        (
            Mesh::from(Cuboid::from_size(size)),
            Furniture::Sight,
            Transform::from_translation(centre),
        )
    };
    // A mount block standing on the receiver up to `top`, centred at `z`.
    let mount = |top: f32, z: f32, width: f32, depth: f32| {
        let height = (top - rig.body_top).max(0.0);
        block(
            Vec3::new(width, height, depth),
            Vec3::new(sight.x, rig.body_top + height / 2.0, z),
        )
    };
    match stats.sight {
        SightKind::RedDot => {
            let (window, t) = (RED_DOT_WINDOW, RED_DOT_FRAME);
            let mut parts = optic_parts(sight, window, t, RED_DOT_DEPTH, 0.0, RED_DOT_RADIUS);
            parts.push(mount(sight.y - window.y / 2.0 - t, sight.z, 0.012, 0.03));
            parts
        }
        SightKind::Holographic => {
            let (window, t) = (HOLO_WINDOW, HOLO_FRAME);
            // The housing runs forward from the window.
            let shift = -HOLO_DEPTH / 2.0 + 0.004;
            let mut parts = optic_parts(sight, window, t, HOLO_DEPTH, shift, HOLO_DOT);
            let (inner, outer) = HOLO_RING;
            parts.push((
                Mesh::from(Annulus::new(inner, outer)),
                Furniture::Reticle,
                Transform::from_translation(sight + Vec3::Z * 0.001),
            ));
            parts.push(mount(
                sight.y - window.y / 2.0 - t,
                sight.z + shift,
                0.016,
                0.035,
            ));
            parts
        }
        SightKind::Prism => {
            let (inner, outer) = PRISM_TUBE;
            let (bell, bell_depth) = PRISM_BELL;
            let shift = -PRISM_DEPTH / 2.0 + 0.005;
            let (arm, thickness) = PRISM_CHEVRON;
            // An upward chevron: two arms from the apex down and out at 45 degrees.
            let half = arm / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
            let apex = sight + Vec3::Z * 0.001;
            let chevron = |side: f32| {
                (
                    Mesh::from(Rectangle::new(arm, thickness)),
                    Furniture::Reticle,
                    Transform::from_translation(apex + Vec3::new(side * half, -half, 0.0))
                        .with_rotation(Quat::from_rotation_z(-side * std::f32::consts::FRAC_PI_4)),
                )
            };
            vec![
                (
                    Mesh::from(Extrusion::new(Annulus::new(inner, outer), PRISM_DEPTH)),
                    Furniture::Sight,
                    Transform::from_translation(sight + Vec3::Z * shift),
                ),
                // The objective bell at the front end.
                (
                    Mesh::from(Extrusion::new(
                        Annulus::new(inner, outer + bell),
                        bell_depth,
                    )),
                    Furniture::Sight,
                    Transform::from_translation(
                        sight + Vec3::Z * (shift - PRISM_DEPTH / 2.0 + bell_depth / 2.0),
                    ),
                ),
                (
                    Mesh::from(Circle::new(inner)),
                    Furniture::Glass,
                    Transform::from_translation(sight),
                ),
                chevron(1.0),
                chevron(-1.0),
                mount(sight.y - outer, sight.z + shift, 0.02, 0.045),
            ]
        }
        SightKind::Acog => {
            let (window, t) = (ACOG_WINDOW, ACOG_FRAME);
            // The housing runs forward from the eyepiece; a rail block sits on its top.
            let shift = -ACOG_DEPTH / 2.0 + 0.005;
            let mut parts = optic_parts(sight, window, t, ACOG_DEPTH, shift, ACOG_RADIUS);
            parts.push(block(
                Vec3::new(window.x + 2.0 * t, 0.012, ACOG_DEPTH),
                sight + Vec3::new(0.0, (window.y + 2.0 * t + 0.012) / 2.0 + t / 2.0, shift),
            ));
            parts.push(mount(
                sight.y - window.y / 2.0 - t,
                sight.z + shift,
                0.024,
                0.06,
            ));
            parts
        }
        SightKind::Irons | SightKind::Scope => Vec::new(),
    }
}

/// Builds the viewmodel camera, the (empty) gun root, its muzzle flash and the arms under the
/// local player's eye camera; `build_gun` adds the model.
fn attach_viewmodel(
    add: On<Add<EyeCamera>>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
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
            // The Map's grade comes from `atmosphere`, which follows look changes.
            atmosphere::TONEMAPPING,
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

    let arms = spawn_arms(&mut commands, camera, &assets, &mut standard, &mut meshes);
    let gun = commands
        .spawn((
            Name::new("ViewmodelGun"),
            ViewmodelGun {
                weapon: None,
                rig: None,
                left_rest: None,
                arms,
                bob_phase: 0.0,
                bob_amount: 0.0,
                sway: Vec2::ZERO,
                kick: 0.0,
                sprint: 0.0,
                mantle: 0.0,
                throw: None,
            },
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
        // Its look (star, flame, light) is dressed by `effects::dress_muzzle_flash`.
        ChildOf(gun),
    ));
}

/// Gloves, sleeves and the Grenade scenes, under one root that follows the gun's visibility.
/// Their transforms are set every frame by `animate_viewmodel`.
fn spawn_arms(
    commands: &mut Commands,
    camera: Entity,
    assets: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
) -> ArmEntities {
    let root = commands
        .spawn((
            Name::new("ViewmodelArms"),
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(camera),
        ))
        .id();
    let glove = materials.add(Furniture::Glove.material(false));
    let sleeve = materials.add(Furniture::Sleeve.material(false));
    let gloves = GloveMeshes {
        right: rig::GLOVE_CURLS.map(|curl| meshes.add(rig::glove_mesh(curl, false))),
        left: rig::GLOVE_CURLS.map(|curl| meshes.add(rig::glove_mesh(curl, true))),
    };
    let upper_mesh = meshes.add(rig::upper_arm_mesh());
    let fore_mesh = meshes.add(rig::forearm_mesh());
    let elbow_mesh = meshes.add(rig::elbow_mesh());
    let mut limb = |side: &'static str| {
        let mut part =
            |name: &'static str, mesh: Handle<Mesh>, material: &Handle<StandardMaterial>| {
                commands
                    .spawn((
                        Name::new(format!("{side}{name}")),
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        viewmodel_layer(),
                        NotShadowCaster,
                        ChildOf(root),
                    ))
                    .id()
            };
        LimbEntities {
            hand: part("Hand", gloves.right[0].clone(), &glove),
            upper: part("UpperArm", upper_mesh.clone(), &sleeve),
            fore: part("Forearm", fore_mesh.clone(), &sleeve),
            elbow: part("Elbow", elbow_mesh.clone(), &sleeve),
        }
    };
    let strong = limb("Strong");
    let support = limb("Support");
    commands.insert_resource(gloves);
    let holder = commands
        .spawn((
            Name::new("HeldGrenade"),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
    let grenades = [
        GrenadeKind::Frag,
        GrenadeKind::Flashbang,
        GrenadeKind::Smoke,
    ]
    .map(|kind| {
        let (file, scale, centre) = rig::grenade_model(kind);
        let scene =
            assets.load(AssetPath::from(format!("{EMBEDDED_ROOT}{file}")).with_label("Scene0"));
        let entity = commands
            .spawn((
                Name::new(format!("Held {kind:?}")),
                ViewScene {
                    camo: NO_CAMO.to_owned(),
                    mirrored: false,
                    bare: &[],
                    magazine: None,
                    ads_hidden: &[],
                    removed: &[],
                },
                WorldAssetRoot(scene),
                // The holder is the palm-up hand's frame, upside down; the model's centre goes
                // to its origin.
                Transform::from_xyz(0.0, centre * scale, 0.0)
                    .with_rotation(Quat::from_rotation_z(PI))
                    .with_scale(Vec3::splat(scale)),
                Visibility::Hidden,
                ChildOf(holder),
            ))
            .id();
        (kind, entity)
    });
    ArmEntities {
        root,
        strong,
        support,
        holder,
        grenades,
    }
}

/// The face to cull under a mirrored root: its triangles run the other way round on screen.
fn flipped_cull(cull: Option<Face>) -> Option<Face> {
    cull.map(|face| match face {
        Face::Front => Face::Back,
        Face::Back => Face::Front,
    })
}

/// A glTF scene of the viewmodel is ready: puts all its meshes on the viewmodel layer, finds a
/// gun's magazine and skins the gun with its camo.
#[allow(clippy::too_many_arguments)]
fn scene_ready(
    ready: On<WorldInstanceReady>,
    scenes: Query<&ViewScene>,
    camos: Res<CamoMaterials>,
    descendants: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform>,
    models: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let root = ready.entity;
    let Ok(scene) = scenes.get(root) else {
        return;
    };
    let root_transform = transforms.get(root).copied().unwrap_or_default();
    let camo = camos
        .0
        .get(&scene.camo)
        .and_then(|handle| standard.get(handle))
        .map(|camo| {
            (
                camo.base_color_texture.clone(),
                camo.metallic,
                camo.perceptual_roughness,
            )
        });
    let mut skinned: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>> = HashMap::new();
    let mut flipped: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>> = HashMap::new();
    for entity in descendants.iter_descendants(root) {
        if let Ok(name) = names.get(entity)
            && scene.ads_hidden.contains(&name.as_str())
        {
            commands.entity(entity).insert(AdsHidden);
        }
        if let Ok(name) = names.get(entity)
            && scene.removed.contains(&name.as_str())
        {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        if let (Some(wanted), Ok(name)) = (scene.magazine, names.get(entity))
            && name.as_str() == wanted
            && let Ok(transform) = transforms.get(entity)
        {
            commands
                .entity(entity)
                .insert(Magazine(transform.translation));
        }
        let Ok((mesh, material)) = models.get(entity) else {
            continue;
        };
        commands
            .entity(entity)
            .insert((viewmodel_layer(), NotShadowCaster));
        let node = parents.get(entity).map(ChildOf::parent).ok();
        let bare = node
            .and_then(|node| names.get(node).ok())
            .is_some_and(|name| scene.bare.contains(&name.as_str()));
        let Some((texture, metallic, roughness)) = camo.as_ref().filter(|_| !bare) else {
            if scene.mirrored {
                let material = flipped
                    .entry(material.0.id())
                    .or_insert_with(|| {
                        let mut flipped = standard.get(&material.0).cloned().unwrap_or_default();
                        flipped.cull_mode = flipped_cull(flipped.cull_mode);
                        standard.add(flipped)
                    })
                    .clone();
                commands.entity(entity).insert(MeshMaterial3d(material));
            }
            continue;
        };
        // The mesh's own transform up to the scene root, then the root's, takes it into gun
        // space.
        let mut to_gun = transforms.get(entity).copied().unwrap_or_default();
        let mut up = entity;
        while let Ok(parent) = parents.get(up).map(ChildOf::parent) {
            if parent == root {
                break;
            }
            to_gun = transforms
                .get(parent)
                .copied()
                .unwrap_or_default()
                .mul_transform(to_gun);
            up = parent;
        }
        to_gun = root_transform.mul_transform(to_gun);
        let Some(source) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(uvs) = camo_uvs(source, &to_gun) else {
            continue;
        };
        let mut skinned_mesh = source.clone();
        skinned_mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uvs);
        let skin = skinned
            .entry(material.0.id())
            .or_insert_with(|| {
                let mut skin = standard.get(&material.0).cloned().unwrap_or_default();
                skin.base_color = Color::WHITE;
                skin.base_color_texture = texture.clone();
                // The model's own normal map keeps its UVs; only the colour is the camo's.
                skin.base_color_channel = UvChannel::Uv1;
                skin.metallic = *metallic;
                skin.perceptual_roughness = *roughness;
                if scene.mirrored {
                    skin.cull_mode = flipped_cull(skin.cull_mode);
                }
                standard.add(skin)
            })
            .clone();
        commands
            .entity(entity)
            .insert((Mesh3d(meshes.add(skinned_mesh)), MeshMaterial3d(skin)));
    }
}

/// Where the rod and the knob of a bolt handle rest in gun space, if the model has one. A
/// left-handed player's gun is mirrored, so the handle is put on the other side of the gun to
/// stay on the right of the screen, where the right hand works it.
fn bolt_handle_points(rig: &GunRig, mirrored: bool) -> Option<(Vec3, Vec3)> {
    let side = if mirrored { -1.0 } else { 1.0 };
    rig.bolt.map(|root| {
        let root = Vec3::new(side * root.x, root.y, root.z);
        (
            root + Vec3::new(side * 0.027, 0.0, 0.0),
            root + Vec3::new(side * 0.058, -0.006, 0.0),
        )
    })
}

/// Swaps the gun model when the held Weapon (or what is fitted to it) changes, moving the flash
/// to its muzzle.
#[allow(clippy::too_many_arguments)]
fn build_gun(
    mut commands: Commands,
    materials: Res<GunMaterials>,
    camos: Res<CamoMaterials>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    player: Query<(&HeldWeapon, &Kit), With<LocalPlayer>>,
    mut gun: Query<(Entity, &mut ViewmodelGun, &Children)>,
    parts: Query<(), With<GunPart>>,
    mut flash: Query<&mut Transform, With<MuzzleFlash>>,
    settings: Res<Settings>,
) {
    let (Ok((weapon, kit)), Ok((gun, mut anim, children))) = (player.single(), gun.single_mut())
    else {
        return;
    };
    // The Loadout's camo goes on its Primary only.
    let wanted = &kit.loadout().camo;
    let camo = if weapon.slot == super::Slot::Primary && camos.0.contains_key(wanted.as_str()) {
        wanted.clone()
    } else {
        NO_CAMO.to_owned()
    };
    let handedness = settings.handedness;
    let mirrored = handedness == Handedness::Left;
    let built = (weapon.id, weapon.attachments, camo.clone(), handedness);
    if anim.weapon.as_ref() == Some(&built) {
        return;
    }
    anim.weapon = Some(built);
    for child in children.iter().filter(|child| parts.contains(*child)) {
        commands.entity(child).despawn();
    }

    let stats = &*weapon.stats;
    let rig = rig::gun_rig(stats.model);
    let muzzle = Vec3::from(stats.muzzle_offset);
    // Where the barrel itself ends: a Silencer carries on from there to the muzzle.
    let barrel_end = muzzle.z + stats.muzzle_length;
    let mut part = |mesh: Mesh, furniture: Furniture, transform: Transform| {
        commands
            .spawn((
                GunPart,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.get(furniture, mirrored)),
                transform,
                viewmodel_layer(),
                NotShadowCaster,
                ChildOf(gun),
            ))
            .id()
    };
    if stats.muzzle_length > 0.0 {
        part(
            Cylinder::new(
                rig.barrel_radius * SUPPRESSOR_RADIUS_SCALE,
                stats.muzzle_length,
            )
            .into(),
            Furniture::Sight,
            Transform::from_xyz(muzzle.x, muzzle.y, (barrel_end + muzzle.z) / 2.0)
                .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        );
    }
    for (mesh, furniture, transform) in sight_parts(&rig, stats) {
        part(mesh, furniture, transform);
    }
    // The bolt handle: a rod out of the receiver's side with a knob on its end.
    let bolt: Vec<(Entity, Vec3)> = bolt_handle_points(&rig, mirrored)
        .map(|(rod, knob)| {
            vec![
                (
                    part(
                        Cylinder::new(0.0045, 0.054).into(),
                        Furniture::Metal,
                        Transform::from_translation(rod)
                            .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                    ),
                    rod,
                ),
                (
                    part(
                        Sphere::new(0.0105).into(),
                        Furniture::Metal,
                        Transform::from_translation(knob),
                    ),
                    knob,
                ),
            ]
        })
        .unwrap_or_default();
    // An Underbarrel Attachment hangs under the handguard and the left hand holds it.
    let mut left_rest = rig.left;
    if let Some(at) = rig.underbarrel {
        match weapon.attachments.underbarrel {
            Some(AttachmentId::Foregrip) => {
                part(
                    Cylinder::new(FOREGRIP.0, FOREGRIP.1).into(),
                    Furniture::Polymer,
                    Transform::from_translation(at - Vec3::Y * FOREGRIP.1 / 2.0),
                );
                left_rest = rig.left_grip;
            }
            Some(AttachmentId::GrenadeLauncher) => {
                part(
                    Cylinder::new(LAUNCHER.0, LAUNCHER.1).into(),
                    Furniture::Metal,
                    Transform::from_translation(at - Vec3::Y * 0.035)
                        .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                );
                part(
                    Cuboid::new(0.03, 0.035, 0.05).into(),
                    Furniture::Polymer,
                    Transform::from_translation(at + Vec3::new(0.0, 0.0, 0.0)),
                );
                // The tube is wider than a Foregrip: the hand sits that much further out.
                left_rest = HandPose {
                    position: rig.left_grip.position + Vec3::X * (FOREGRIP.0 - LAUNCHER.0),
                    ..rig.left_grip
                };
            }
            _ => {}
        }
    }
    for (handle, rest) in bolt {
        commands.entity(handle).insert(BoltHandle(rest));
    }
    let scene =
        assets.load(AssetPath::from(format!("{EMBEDDED_ROOT}{}", rig.file)).with_label("Scene0"));
    commands.spawn((
        GunPart,
        ViewScene {
            camo,
            mirrored,
            bare: rig.bare,
            magazine: rig.magazine,
            ads_hidden: rig.ads_hidden,
            // A fitted Optic stands where the carry handle and flip-up sights are.
            removed: if matches!(stats.sight, SightKind::Irons) {
                &[]
            } else {
                rig.optic_hidden
            },
        },
        WorldAssetRoot(scene),
        rig.model_transform(),
        Visibility::default(),
        ChildOf(gun),
    ));
    anim.rig = Some(rig);
    anim.left_rest = Some(left_rest);
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

fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A hand pose between `a` and `b`; the glove is the one of the nearer pose.
fn blend_pose(a: HandPose, b: HandPose, weight: f32) -> HandPose {
    HandPose {
        position: a.position.lerp(b.position, weight),
        rotation: a.rotation.slerp(b.rotation, weight),
        left: if weight < 0.5 { a.left } else { b.left },
        curl: a.curl.lerp(b.curl, weight),
    }
}

/// A transform between `a` and `b`.
fn blend(a: Transform, b: Transform, weight: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, weight),
        rotation: a.rotation.slerp(b.rotation, weight),
        scale: a.scale.lerp(b.scale, weight),
    }
}

/// The right hand's pose in camera space during a throw `t` seconds in: how far it has left the
/// gun (0 to 1), the pose, whether the Grenade is still in it and how far the gun is lowered.
fn throw_pose(t: f32) -> (f32, Transform, bool, f32) {
    let pose = |(position, tilt): (Vec3, f32)| Transform {
        translation: position,
        // Palm up, fingers forward; the tilt flicks the fingertips up (+) or down (-).
        rotation: Quat::from_rotation_x(tilt) * Quat::from_rotation_z(PI),
        scale: Vec3::ONE,
    };
    let (ready, back, forward) = (pose(THROW_READY), pose(THROW_BACK), pose(THROW_FORWARD));
    let wound = blend(ready, back, smooth(0.12, 0.2, t));
    let transform = blend(wound, forward, smooth(0.2, 0.28, t));
    let weight = smooth(0.0, 0.12, t) * (1.0 - smooth(0.3, THROW_SECS, t));
    let lowered = smooth(0.0, 0.12, t) * (1.0 - smooth(0.38, THROW_SECS, t));
    (weight, transform, t < THROW_RELEASE, lowered)
}

#[allow(clippy::too_many_arguments)]
fn animate_viewmodel(
    time: Res<Time>,
    life: Res<LocalLife>,
    captured: Res<MouseCaptured>,
    motion: Res<AccumulatedMouseMotion>,
    mut fired: MessageReader<ShotFired>,
    mut throws: MessageReader<ToServer<ThrowGrenade>>,
    player: Query<(&HeldWeapon, &Motor), With<LocalPlayer>>,
    mut gun: Query<(Entity, &mut ViewmodelGun, &mut Visibility)>,
    bolt_handle: Query<(Entity, &BoltHandle)>,
    ads_hidden: Query<Entity, With<AdsHidden>>,
    magazines: Query<(Entity, &Magazine)>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility, Without<ViewmodelGun>>,
    settings: Res<Settings>,
    (gloves, mut glove_meshes): (Res<GloveMeshes>, Query<&mut Mesh3d>),
) {
    let shots = fired.read().count();
    let thrown = throws
        .read()
        .map(|throw| throw.0.kind)
        .find(|kind| *kind != GrenadeKind::Launcher);
    let Ok((weapon, motor)) = player.single() else {
        return;
    };
    let Ok((gun_entity, mut anim, mut visibility)) = gun.single_mut() else {
        return;
    };
    let stats = &*weapon.stats;
    // A left-handed player's view is the right-handed one in a mirror: everything is worked
    // out as for a right hand and the results mirrored at the end, so only what is not part of
    // the gun's own movement (the mouse sway and the walking bob, which follow the screen) is
    // turned about first.
    let mirrored = settings.handedness == Handedness::Left;
    let side = if mirrored { -1.0 } else { 1.0 };
    let hidden = life.is_dead() || weapon.scoped(stats);
    visibility.set_if_neq(if hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
    // Hide what would surround the eye while aimed.
    for entity in &ads_hidden {
        if let Ok(mut part_visibility) = visibilities.get_mut(entity) {
            part_visibility.set_if_neq(if weapon.ads_eased() > 0.5 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    }
    let arms = anim.arms;
    if let Ok(mut arms_visibility) = visibilities.get_mut(arms.root) {
        arms_visibility.set_if_neq(if hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    let dt = time.delta_secs();
    let ads = weapon.ads_eased();
    let motion_scale = 1.0_f32.lerp(ADS_MOTION_SCALE * stats.sway_scale, ads);

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

    let reload = weapon.reload_progress();
    let dip = reload.map_or(0.0, |progress| (progress * PI).sin());
    // The left hand reaches for the magazine, pulls it and seats a new one, then goes back
    // to the handguard: how far it is on the magazine and how far the magazine is out.
    let (hand_on_mag, mag_out) = reload.map_or((0.0, 0.0), |p| {
        (
            smooth(0.0, 0.18, p) - smooth(0.78, 0.95, p),
            smooth(0.18, 0.4, p) - smooth(0.55, 0.78, p),
        )
    });
    let bolt = weapon.bolt_phase().map_or(0.0, |phase| (phase * PI).sin());
    for (entity, handle) in &bolt_handle {
        if let Ok(mut handle_transform) = transforms.get_mut(entity) {
            handle_transform.translation = handle.0 + Vec3::new(0.0, BOLT_LIFT, BOLT_TRAVEL) * bolt;
        }
    }
    if let Some(rig) = &anim.rig {
        let model = rig.model_transform();
        // The pull is in gun space; the node moves in model space.
        let local = (model.rotation.inverse() * rig.mag_travel * mag_out) / model.scale;
        for (entity, magazine) in &magazines {
            if let Ok(mut magazine_transform) = transforms.get_mut(entity) {
                magazine_transform.translation = magazine.0 + local;
            }
        }
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

    // The throw under way (or a new one).
    if let Some(kind) = thrown {
        anim.throw = Some((kind, 0.0));
    }
    anim.throw = anim
        .throw
        .map(|(kind, t)| (kind, t + dt))
        .filter(|(_, t)| *t < THROW_SECS);
    let (throw_weight, throw_transform, throw_holding, throw_lowered) = match anim.throw {
        Some((_, t)) => throw_pose(t),
        None => (0.0, Transform::IDENTITY, false, 0.0),
    };

    let switching = weapon.switch_lowering().max(throw_lowered);
    let ads_offset = ads_offset(stats);
    let wobble = (bob + anim.sway) * motion_scale;
    let gun_right_handed = Transform {
        translation: HIP_OFFSET.lerp(ads_offset, ads)
            + SPRINT_OFFSET * sprint
            + BOLT_SHIFT * bolt
            + Vec3::new(
                wobble.x * side,
                wobble.y + RELOAD_RAISE * dip - MANTLE_DROP * mantle - SWITCH_DROP * switching,
                KICK_BACK * kick,
            ),
        rotation: Quat::from_euler(
            EulerRot::YXZ,
            anim.sway.x * SWAY_TILT * motion_scale * side + SPRINT_YAW * sprint,
            KICK_PITCH * kick
                + RELOAD_TILT * dip
                + anim.sway.y * SWAY_TILT * motion_scale
                + SPRINT_PITCH * sprint
                - MANTLE_TILT * mantle
                + BOLT_TILT * bolt
                - SWITCH_TILT * switching,
            RELOAD_ROLL * dip + SPRINT_ROLL * sprint + BOLT_ROLL * bolt,
        ),
        scale: Vec3::ONE,
    };
    let gun_transform = if mirrored {
        rig::mirror_solid(gun_right_handed)
    } else {
        gun_right_handed
    };
    if let Ok(mut transform) = transforms.get_mut(gun_entity) {
        *transform = gun_transform;
    }

    // The arms, held to the gun (the strong hand, for a throw, to the Grenade) in camera space.
    let (Some(rig), Some(left_rest)) = (&anim.rig, anim.left_rest) else {
        return;
    };
    // The bolt handle's knob is worked by the right hand of the screen: the strong hand when
    // right-handed, the other one when left-handed.
    let knob = bolt_handle_points(rig, mirrored)
        .map(|(_, knob)| rig::bolt_grip(knob + Vec3::new(0.0, BOLT_LIFT, BOLT_TRAVEL) * bolt));
    let bolt_weight = smooth(0.0, 0.35, bolt);
    let mut strong_pose = rig.right;
    let mut support_pose = left_rest;
    let mut mag_pose = rig.left_mag;
    mag_pose.position += rig.mag_travel * mag_out;
    support_pose = blend_pose(support_pose, mag_pose, hand_on_mag);
    if let Some(grip) = knob {
        if mirrored {
            support_pose = blend_pose(support_pose, grip, bolt_weight);
        } else {
            strong_pose = blend_pose(strong_pose, grip, bolt_weight);
        }
    }
    // Camera space, as a right-handed player has it.
    let strong_on_gun = gun_right_handed.mul_transform(strong_pose.transform());
    let strong = blend(strong_on_gun, throw_transform, throw_weight);
    let strong_curl = strong_pose.curl.lerp(THROW_CURL, throw_weight);
    let support = gun_right_handed.mul_transform(support_pose.transform());
    let mirror_point = |point: Vec3| {
        if mirrored {
            rig::mirror_point(point)
        } else {
            point
        }
    };
    for (limb, hand, curl, left, arm) in [
        (
            arms.strong,
            strong,
            strong_curl,
            strong_pose.left,
            rig::RIGHT_ARM,
        ),
        (
            arms.support,
            support,
            support_pose.curl,
            support_pose.left,
            rig::left_arm(),
        ),
    ] {
        let end = rig::sleeve_end(&hand);
        let elbow = rig::solve_elbow(arm.shoulder, end, arm.upper, arm.fore, arm.pole);
        let hand_on_screen = if mirrored {
            rig::mirror_placement(hand)
        } else {
            hand
        };
        if let Ok(mut transform) = transforms.get_mut(limb.hand) {
            *transform = hand_on_screen;
        }
        // The other hand's glove when mirrored.
        let gloves = if left != mirrored {
            &gloves.left
        } else {
            &gloves.right
        };
        if let Ok(mut mesh) = glove_meshes.get_mut(limb.hand) {
            let wanted = &gloves[rig::nearest_curl(curl)];
            if mesh.0 != *wanted {
                mesh.0 = wanted.clone();
            }
        }
        for (entity, placed) in [
            (
                limb.upper,
                rig::tube_transform(mirror_point(arm.shoulder), mirror_point(elbow)),
            ),
            (
                limb.fore,
                rig::tube_transform(mirror_point(elbow), mirror_point(end)),
            ),
            (limb.elbow, Transform::from_translation(mirror_point(elbow))),
        ] {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                *transform = placed;
            }
        }
    }
    // The held Grenade sits in the palm of the throwing hand.
    if let Ok(mut transform) = transforms.get_mut(arms.holder) {
        let held = strong.mul_transform(Transform::from_xyz(0.0, -0.045, -0.035));
        *transform = if mirrored {
            rig::mirror_placement(held)
        } else {
            held
        };
    }
    let holding = anim.throw.map(|(kind, _)| kind).filter(|_| throw_holding);
    if let Ok(mut holder_visibility) = visibilities.get_mut(arms.holder) {
        holder_visibility.set_if_neq(if holding.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (kind, entity) in arms.grenades {
        if let Ok(mut grenade_visibility) = visibilities.get_mut(entity) {
            grenade_visibility.set_if_neq(if holding == Some(kind) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_throw_starts_and_ends_with_the_gun_up_and_the_hand_on_it() {
        for t in [0.0, THROW_SECS] {
            let (weight, _, _, lowered) = throw_pose(t);
            assert_eq!((weight, lowered), (0.0, 0.0), "at {t} s");
        }
    }

    #[test]
    fn the_grenade_leaves_the_hand_while_it_is_still_thrown_out() {
        // Held while the hand rises and winds up, gone before it comes back to the gun.
        assert!(throw_pose(0.1).2);
        assert!(!throw_pose(THROW_RELEASE + 0.01).2);
        assert_eq!(throw_pose(THROW_RELEASE + 0.01).0, 1.0);
    }

    #[test]
    fn the_gun_is_lowered_for_the_whole_wind_up() {
        for t in [0.15, 0.2, 0.25, 0.3] {
            assert_eq!(throw_pose(t).3, 1.0, "at {t} s");
        }
    }
}
