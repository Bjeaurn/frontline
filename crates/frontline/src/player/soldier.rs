//! A procedural low-poly soldier drawn for remote players and their bodies: boots, fatigues,
//! vest and pack, a head with its Faction's headgear and a rifle held at the shoulder.
//!
//! The soldier is seven rigid parts (torso, head, aim group, two thighs and two shins), each one
//! mesh with vertex colours, so a Faction's look is just a different set of meshes under one
//! shared material. Team colour is two more small meshes (chest and back panels, armbands) under
//! a separate material. `pose` places the parts for a stance, a walk cycle and the aim pitch;
//! nothing here is skeletal and nothing touches hit zones: it is all visuals.
//!
//! Local space: feet at the origin, facing -Z, +X to the soldier's right.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;

use crate::data::{GunModel, Headgear, SoldierLook};
use crate::shared::Stance;
use crate::weapon::rig::gun_rig;

/// Rigid parts of a soldier.
#[derive(Clone, Copy, Debug)]
pub(super) enum Part {
    Torso,
    Head,
    /// Shoulders, arms and rifle: pitches with the aim.
    Aim,
    ThighLeft,
    ShinLeft,
    ThighRight,
    ShinRight,
}

pub(super) const PART_COUNT: usize = 7;
const PARTS: [Part; PART_COUNT] = [
    Part::Torso,
    Part::Head,
    Part::Aim,
    Part::ThighLeft,
    Part::ShinLeft,
    Part::ThighRight,
    Part::ShinRight,
];

/// Hip joint height standing, in metres.
const STAND_HIP: f32 = 0.93;
const CROUCH_HIP: Vec3 = Vec3::new(0.0, 0.42, 0.15);
const PRONE_HIP: Vec3 = Vec3::new(0.0, 0.14, 0.62);
const HIP_HALF_WIDTH: f32 = 0.1;
const THIGH_LENGTH: f32 = 0.46;
/// Neck and shoulder joints above the hip, along the torso.
const NECK: Vec3 = Vec3::new(0.0, 0.58, 0.0);
const SHOULDER: Vec3 = Vec3::new(0.0, 0.5, 0.0);
/// Head centre above the neck joint.
const HEAD_CENTRE: f32 = 0.13;
/// Metres of ground covered by one full walk cycle (both legs), so steps match footsteps.
const STRIDE_LENGTH: f32 = 2.2;
/// Crouched: thighs near horizontal, shins slanting back to the heel.
const CROUCH_THIGH: f32 = 1.48;
const CROUCH_KNEE: f32 = -2.11;
const CROUCH_LEAN: f32 = -0.45;

const BOOT: Color = Color::srgb(0.09, 0.08, 0.07);
const GLOVE: Color = Color::srgb(0.08, 0.08, 0.08);

/// How far into its walk cycle a soldier is and how hard it moves.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Gait {
    /// Radians into the cycle.
    pub phase: f32,
    /// 0 standing still to 1 at walking speed.
    pub walk: f32,
    /// 0 at walking speed to 1 at sprinting speed.
    pub run: f32,
}

impl Gait {
    /// Moves the cycle on `dt` seconds at horizontal `speed`, given the walking and sprinting
    /// speeds of the movement data.
    pub fn advance(&mut self, speed: f32, walk_speed: f32, sprint_speed: f32, dt: f32) {
        self.walk = (speed / walk_speed.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.run =
            ((speed - walk_speed) / (sprint_speed - walk_speed).max(f32::EPSILON)).clamp(0.0, 1.0);
        self.phase = (self.phase + TAU * speed / STRIDE_LENGTH * dt) % TAU;
    }
}

/// The part transforms (relative to the feet) of a soldier in `stance` mid-`gait`, aiming
/// `pitch` radians up. Indexed like `Part`.
pub(super) fn pose(stance: Stance, gait: Gait, pitch: f32) -> [Transform; PART_COUNT] {
    let pitch = pitch.clamp(-1.3, 1.3);
    let w = gait.walk;
    let r = gait.run;
    let phase = gait.phase;
    let rot_x = Quat::from_rotation_x;
    // Per side: +1 left, -1 right; opposite legs are half a cycle apart.
    let sin = |side: f32| side * phase.sin();
    let cos = |side: f32| side * phase.cos();

    let (torso, head, aim, thighs, shins) = match stance {
        Stance::Standing => {
            let hip = Vec3::new(0.0, STAND_HIP + 0.018 * w * (2.0 * phase).cos(), 0.0);
            let torso =
                Transform::from_translation(hip).with_rotation(rot_x(-(0.03 * w + 0.2 * r)));
            let swing = (0.45 + 0.3 * r) * w;
            let knee = (0.8 + 0.5 * r) * w;
            let legs = [1.0, -1.0].map(|side| {
                leg(
                    hip,
                    -side,
                    swing * sin(side),
                    -knee * cos(side).max(0.0) - 0.04 * w,
                    0.0,
                )
            });
            (
                torso,
                rot_x(pitch * 0.8),
                rot_x(pitch - 0.55 * r),
                legs.map(|(thigh, _)| thigh),
                legs.map(|(_, shin)| shin),
            )
        }
        Stance::Crouching => {
            let hip = CROUCH_HIP + Vec3::Y * 0.01 * w * (2.0 * phase).cos();
            let torso =
                Transform::from_translation(hip).with_rotation(rot_x(CROUCH_LEAN - 0.05 * w));
            let legs = [1.0, -1.0].map(|side| {
                leg(
                    hip,
                    -side,
                    CROUCH_THIGH + 0.25 * w * sin(side),
                    CROUCH_KNEE - 0.15 * w * cos(side).max(0.0),
                    0.0,
                )
            });
            (
                torso,
                rot_x(pitch * 0.8),
                rot_x(pitch),
                legs.map(|(thigh, _)| thigh),
                legs.map(|(_, shin)| shin),
            )
        }
        Stance::Prone => {
            let torso = Transform::from_translation(PRONE_HIP).with_rotation(rot_x(-FRAC_PI_2));
            let legs = [1.0, -1.0].map(|side| {
                leg(
                    PRONE_HIP,
                    -side,
                    -FRAC_PI_2 + 0.1 * w * sin(side),
                    0.0,
                    0.07 * side,
                )
            });
            (
                torso,
                rot_x(pitch.clamp(-0.5, 0.9)),
                rot_x(pitch.clamp(-0.3, 0.5)),
                legs.map(|(thigh, _)| thigh),
                legs.map(|(_, shin)| shin),
            )
        }
    };

    let lift = if stance == Stance::Prone { 0.07 } else { 0.0 };
    let head = Transform::from_translation(torso.transform_point(NECK) + Vec3::Y * lift)
        .with_rotation(head);
    let aim_lift = if stance == Stance::Prone { 0.17 } else { 0.0 };
    let aim = Transform::from_translation(torso.transform_point(SHOULDER) + Vec3::Y * aim_lift)
        .with_rotation(aim);
    [torso, head, aim, thighs[0], shins[0], thighs[1], shins[1]]
}

/// A thigh and its shin: the hip joint `hip` and the joint's side (`x` sign), the thigh's
/// pitch and splay (`yaw`), and the knee's bend relative to the thigh. Negative bends take the
/// foot backwards.
fn leg(hip: Vec3, x_sign: f32, thigh_pitch: f32, knee: f32, yaw: f32) -> (Transform, Transform) {
    let thigh = Transform::from_translation(hip + Vec3::X * HIP_HALF_WIDTH * x_sign)
        .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(thigh_pitch));
    let shin = Transform::from_translation(thigh.transform_point(Vec3::NEG_Y * THIGH_LENGTH))
        .with_rotation(thigh.rotation * Quat::from_rotation_x(knee));
    (thigh, shin)
}

/// Where the corpse tips about: height above the feet of the soldier's back surface, so a body
/// laid on its back rests on the ground.
pub(super) fn back_height(stance: Stance) -> f32 {
    match stance {
        Stance::Standing => 0.25,
        Stance::Crouching => 0.4,
        Stance::Prone => 0.0,
    }
}

/// The meshes of one Faction's soldier.
pub(super) struct SoldierMeshes {
    parts: [Handle<Mesh>; PART_COUNT],
    /// Chest and back panels, under the torso.
    mark_torso: Handle<Mesh>,
    /// Armbands, under the aim group.
    mark_aim: Handle<Mesh>,
    /// The rifle every soldier carries (the first-person rifle model).
    gun: Handle<WorldAsset>,
}

/// The entities of a spawned soldier.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rig {
    pub parts: [Entity; PART_COUNT],
    pub marks: [Entity; 2],
}

impl SoldierMeshes {
    pub fn new(look: &SoldierLook, meshes: &mut Assets<Mesh>, gun: Handle<WorldAsset>) -> Self {
        Self {
            parts: PARTS.map(|part| meshes.add(build_part(part, look))),
            mark_torso: meshes.add(build_mark_torso()),
            mark_aim: meshes.add(build_mark_aim()),
            gun,
        }
    }

    pub fn part(&self, part: usize) -> &Handle<Mesh> {
        &self.parts[part]
    }

    pub fn mark(&self, mark: usize) -> &Handle<Mesh> {
        if mark == 0 {
            &self.mark_torso
        } else {
            &self.mark_aim
        }
    }

    /// Spawns the soldier under `parent`, posed as `poses`.
    pub fn spawn(
        &self,
        commands: &mut Commands,
        parent: Entity,
        body: &Handle<StandardMaterial>,
        team: &Handle<StandardMaterial>,
        poses: &[Transform; PART_COUNT],
    ) -> Rig {
        let parts = std::array::from_fn(|i| {
            commands
                .spawn((
                    Name::new(format!("Soldier{:?}", PARTS[i])),
                    Mesh3d(self.parts[i].clone()),
                    MeshMaterial3d(body.clone()),
                    poses[i],
                    ChildOf(parent),
                ))
                .id()
        });
        let mark = |commands: &mut Commands, mesh: &Handle<Mesh>, to: Entity| {
            commands
                .spawn((
                    Name::new("SoldierTeamMark"),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(team.clone()),
                    Transform::default(),
                    ChildOf(to),
                ))
                .id()
        };
        commands.spawn((
            Name::new("SoldierGun"),
            WorldAssetRoot(self.gun.clone()),
            gun_mount(),
            ChildOf(parts[Part::Aim as usize]),
        ));
        Rig {
            marks: [
                mark(commands, &self.mark_torso, parts[Part::Torso as usize]),
                mark(commands, &self.mark_aim, parts[Part::Aim as usize]),
            ],
            parts,
        }
    }
}

/// Merges coloured primitives into one mesh with vertex colours.
#[derive(Default)]
struct Builder(Option<Mesh>);

impl Builder {
    fn add(&mut self, mesh: Mesh, at: Transform, color: Color) -> &mut Self {
        let mut mesh = mesh.transformed_by(at);
        let linear = color.to_linear();
        let rgba = [linear.red, linear.green, linear.blue, linear.alpha];
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![rgba; mesh.count_vertices()]);
        match &mut self.0 {
            None => self.0 = Some(mesh),
            Some(all) => all.merge(&mesh).expect("primitives share their attributes"),
        }
        self
    }

    /// A box of `size` centred at `at`.
    fn cuboid(&mut self, size: Vec3, at: Vec3, color: Color) -> &mut Self {
        self.add(
            Cuboid::new(size.x, size.y, size.z).into(),
            Transform::from_translation(at),
            color,
        )
    }

    /// A box turned by `rotation`.
    fn cuboid_rotated(&mut self, size: Vec3, at: Vec3, rotation: Quat, color: Color) -> &mut Self {
        self.add(
            Cuboid::new(size.x, size.y, size.z).into(),
            Transform::from_translation(at).with_rotation(rotation),
            color,
        )
    }

    /// A tapering tube from `a` (radius `ra`) to `b` (radius `rb`).
    fn limb(&mut self, a: Vec3, b: Vec3, ra: f32, rb: f32, color: Color) -> &mut Self {
        let axis = b - a;
        let mesh = ConicalFrustum {
            radius_top: rb,
            radius_bottom: ra,
            height: axis.length(),
        }
        .mesh()
        .resolution(8)
        .build();
        self.add(
            mesh,
            Transform::from_translation((a + b) / 2.0)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, axis.normalize())),
            color,
        )
    }

    /// A tube along Y centred at `at`, turned by `rotation`.
    fn tube(
        &mut self,
        radius: f32,
        height: f32,
        at: Vec3,
        rotation: Quat,
        color: Color,
    ) -> &mut Self {
        self.add(
            Cylinder::new(radius, height).mesh().resolution(10).build(),
            Transform::from_translation(at).with_rotation(rotation),
            color,
        )
    }

    /// An ellipsoid with half-axes `radii`.
    fn ball(&mut self, radii: Vec3, at: Vec3, color: Color) -> &mut Self {
        self.add(
            Sphere::new(1.0).mesh().uv(12, 8),
            Transform::from_translation(at).with_scale(radii),
            color,
        )
    }

    /// The upper half of an ellipsoid with half-axes `radii`, flat at `at`'s height, tilted by
    /// `rotation`.
    fn dome(&mut self, radii: Vec3, at: Vec3, rotation: Quat, color: Color) -> &mut Self {
        let mut mesh = Sphere::new(1.0).mesh().uv(12, 8);
        clip_y(&mut mesh, 0.0, f32::INFINITY);
        self.add(
            mesh,
            Transform::from_translation(at)
                .with_rotation(rotation)
                .with_scale(radii),
            color,
        )
    }

    fn build(self) -> Mesh {
        self.0.expect("a part has geometry")
    }
}

/// Flattens vertices of `mesh` (a unit sphere) into the band `min..=max` in y.
fn clip_y(mesh: &mut Mesh, min: f32, max: f32) {
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for position in positions {
            position[1] = position[1].clamp(min, max);
        }
    }
}

fn rgb(color: (f32, f32, f32)) -> Color {
    Color::srgb(color.0, color.1, color.2)
}

/// `color` with every linear channel scaled by `factor`.
fn shade(color: Color, factor: f32) -> Color {
    let linear = color.to_linear();
    Color::linear_rgb(
        linear.red * factor,
        linear.green * factor,
        linear.blue * factor,
    )
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn build_part(part: Part, look: &SoldierLook) -> Mesh {
    match part {
        Part::Torso => build_torso(look),
        Part::Head => build_head(look),
        Part::Aim => build_aim(look),
        Part::ThighLeft | Part::ThighRight => build_thigh(look),
        Part::ShinLeft | Part::ShinRight => build_shin(look),
    }
}

/// Origin at the hip joint, the leg hanging down.
fn build_thigh(look: &SoldierLook) -> Mesh {
    let uniform = rgb(look.uniform);
    let mut b = Builder::default();
    b.limb(
        v(0.0, -THIGH_LENGTH, 0.0),
        Vec3::ZERO,
        0.077,
        0.105,
        uniform,
    )
    // Cargo pocket and knee pad.
    .cuboid(
        v(0.12, 0.12, 0.04),
        v(0.0, -0.2, -0.085),
        shade(uniform, 0.8),
    )
    .cuboid(
        v(0.1, 0.1, 0.05),
        v(0.0, -0.455, -0.07),
        shade(rgb(look.vest), 0.7),
    );
    b.build()
}

/// Origin at the knee, the leg hanging down to the sole 0.47 m below.
fn build_shin(look: &SoldierLook) -> Mesh {
    let uniform = rgb(look.uniform);
    let mut b = Builder::default();
    b.limb(v(0.0, -0.3, 0.0), Vec3::ZERO, 0.062, 0.077, uniform)
        .limb(v(0.0, -0.46, 0.0), v(0.0, -0.28, 0.0), 0.062, 0.07, BOOT)
        .cuboid(v(0.11, 0.07, 0.2), v(0.0, -0.43, -0.06), BOOT)
        .cuboid(
            v(0.125, 0.03, 0.27),
            v(0.0, -0.455, -0.045),
            shade(BOOT, 0.5),
        );
    b.build()
}

/// Origin at the hip centre, the spine along +Y up to the neck joint at `NECK`.
fn build_torso(look: &SoldierLook) -> Mesh {
    let uniform = rgb(look.uniform);
    let vest = rgb(look.vest);
    let pouch = shade(vest, 0.7);
    let pack = shade(vest, 0.85);
    let mut b = Builder::default();
    b.cuboid(v(0.34, 0.2, 0.21), v(0.0, 0.05, 0.0), uniform)
        .cuboid(v(0.35, 0.04, 0.225), v(0.0, 0.15, 0.0), shade(vest, 0.6))
        .cuboid(v(0.37, 0.42, 0.22), v(0.0, 0.37, 0.0), uniform)
        // Plate carrier with ammo pouches and shoulder straps.
        .cuboid(v(0.39, 0.34, 0.255), v(0.0, 0.4, 0.0), vest);
    for x in [-0.11, 0.0, 0.11] {
        b.cuboid(v(0.085, 0.12, 0.06), v(x, 0.28, -0.15), pouch);
    }
    b.cuboid(v(0.07, 0.03, 0.25), v(-0.1, 0.575, 0.0), vest)
        .cuboid(v(0.07, 0.03, 0.25), v(0.1, 0.575, 0.0), vest)
        .cuboid(v(0.2, 0.05, 0.16), v(0.0, 0.6, 0.0), uniform)
        .tube(0.05, 0.1, v(0.0, 0.62, 0.0), Quat::IDENTITY, rgb(look.skin))
        // Assault pack with a rolled mat on top, and hip pouches.
        .cuboid(v(0.3, 0.34, 0.13), v(0.0, 0.4, 0.19), pack)
        .tube(
            0.055,
            0.28,
            v(0.0, 0.6, 0.2),
            Quat::from_rotation_z(FRAC_PI_2),
            shade(vest, 0.6),
        )
        .cuboid(v(0.06, 0.1, 0.1), v(-0.205, 0.08, 0.04), pouch)
        .cuboid(v(0.06, 0.1, 0.1), v(0.205, 0.08, 0.04), pouch);
    b.build()
}

/// Origin at the neck joint; the head centre `HEAD_CENTRE` above it, facing -Z.
fn build_head(look: &SoldierLook) -> Mesh {
    let skin = rgb(look.skin);
    let cloth = rgb(look.headgear_color);
    let dark = Color::srgb(0.06, 0.06, 0.07);
    let centre = v(0.0, HEAD_CENTRE, 0.0);
    let mut b = Builder::default();
    b.ball(v(0.088, 0.108, 0.1), centre, skin).cuboid(
        v(0.02, 0.03, 0.03),
        centre + v(0.0, -0.01, -0.1),
        shade(skin, 0.9),
    );
    if look.mask {
        // Balaclava over the lower face, leaving a strip for the eyes.
        let mut mask = Sphere::new(1.0).mesh().uv(12, 8);
        clip_y(&mut mask, -1.0, 0.18);
        b.add(
            mask,
            Transform::from_translation(centre).with_scale(v(0.094, 0.113, 0.106)),
            shade(rgb(look.uniform), 0.35),
        );
    } else {
        for x in [-0.035, 0.035] {
            b.cuboid(v(0.022, 0.012, 0.012), centre + v(x, 0.015, -0.093), dark);
        }
    }
    if look.goggles {
        b.cuboid(v(0.185, 0.045, 0.045), centre + v(0.0, 0.018, -0.082), dark)
            .cuboid(v(0.19, 0.03, 0.2), centre + v(0.0, 0.018, 0.02), dark);
    }
    let top = HEAD_CENTRE + 0.02;
    match look.headgear {
        Headgear::Helmet => {
            b.dome(
                v(0.126, 0.115, 0.135),
                v(0.0, top, 0.01),
                Quat::IDENTITY,
                cloth,
            )
            .cuboid(
                v(0.05, 0.04, 0.03),
                v(0.0, top + 0.07, -0.125),
                shade(cloth, 0.4),
            )
            .cuboid(
                v(0.19, 0.035, 0.04),
                v(0.0, top + 0.01, 0.135),
                shade(cloth, 0.9),
            );
        }
        Headgear::Beret => {
            b.dome(
                v(0.14, 0.07, 0.15),
                v(0.015, top + 0.015, 0.0),
                Quat::from_rotation_z(-0.18),
                cloth,
            )
            .cuboid(
                v(0.015, 0.03, 0.015),
                v(0.0, top + 0.08, 0.0),
                shade(cloth, 0.7),
            );
        }
        Headgear::Boonie => {
            b.dome(
                v(0.108, 0.09, 0.118),
                v(0.0, top, 0.0),
                Quat::IDENTITY,
                cloth,
            )
            .tube(
                0.19,
                0.012,
                v(0.0, top + 0.002, 0.0),
                Quat::IDENTITY,
                shade(cloth, 0.9),
            );
        }
        Headgear::Cap => {
            b.dome(
                v(0.108, 0.09, 0.118),
                v(0.0, top, 0.0),
                Quat::IDENTITY,
                cloth,
            )
            .cuboid_rotated(
                v(0.15, 0.012, 0.1),
                v(0.0, top + 0.01, -0.135),
                Quat::from_rotation_x(0.22),
                shade(cloth, 0.85),
            );
        }
        Headgear::Beanie => {
            b.dome(
                v(0.106, 0.1, 0.116),
                v(0.0, top - 0.005, 0.0),
                Quat::IDENTITY,
                cloth,
            )
            .tube(
                0.108,
                0.04,
                v(0.0, top + 0.005, 0.0),
                Quat::IDENTITY,
                shade(cloth, 1.4),
            );
        }
        Headgear::Shemagh => {
            b.dome(
                v(0.115, 0.1, 0.125),
                v(0.0, top - 0.01, 0.005),
                Quat::IDENTITY,
                cloth,
            )
            .tube(
                0.1,
                0.07,
                v(0.0, 0.03, 0.0),
                Quat::IDENTITY,
                shade(cloth, 0.9),
            )
            .cuboid_rotated(
                v(0.2, 0.15, 0.03),
                v(0.0, 0.09, 0.115),
                Quat::from_rotation_x(-0.2),
                shade(cloth, 0.95),
            );
        }
        Headgear::Ushanka => {
            b.dome(
                v(0.12, 0.1, 0.13),
                v(0.0, top - 0.005, 0.005),
                Quat::IDENTITY,
                cloth,
            )
            .cuboid(
                v(0.2, 0.045, 0.04),
                v(0.0, top + 0.015, -0.12),
                shade(cloth, 1.3),
            )
            .cuboid(
                v(0.035, 0.11, 0.13),
                v(-0.118, top - 0.06, 0.01),
                shade(cloth, 1.25),
            )
            .cuboid(
                v(0.035, 0.11, 0.13),
                v(0.118, top - 0.06, 0.01),
                shade(cloth, 1.25),
            );
        }
    }
    b.build()
}

/// Where the held rifle's origin (its receiver) is, in aim space.
const GUN_ORIGIN: Vec3 = Vec3::new(0.1, -0.17, -0.28);
const UPPER_ARM: f32 = 0.29;
const FOREARM: f32 = 0.29;

/// The held rifle's model transform in aim space: the viewmodel's rifle, in the same place in the
/// hands as in first person.
fn gun_mount() -> Transform {
    Transform::from_translation(GUN_ORIGIN) * gun_rig(GunModel::Rifle).model_transform()
}

/// Shoulder, elbow and hand (aim space) of the arm on `side` (+1 the soldier's right, -1 the
/// left) reaching `hand`: two equal bones, the elbow bent outward and down.
fn arm(side: f32, hand: Vec3) -> [Vec3; 3] {
    let shoulder = v(0.21 * side, 0.0, 0.0);
    let reach = hand - shoulder;
    let len = reach.length();
    let elbow = if len >= UPPER_ARM + FOREARM {
        shoulder + reach * (UPPER_ARM / (UPPER_ARM + FOREARM))
    } else {
        let along = (UPPER_ARM * UPPER_ARM - FOREARM * FOREARM + len * len) / (2.0 * len);
        let height = (UPPER_ARM * UPPER_ARM - along * along).max(0.0).sqrt();
        let direction = reach / len;
        let pole = v(0.5 * side, -1.0, 0.3);
        let bend = (pole - direction * pole.dot(direction)).normalize_or(Vec3::NEG_Y);
        shoulder + direction * along + bend * height
    };
    [shoulder, elbow, hand]
}

/// Both arms, in aim space: the right on the grip, the left under the handguard.
fn arms() -> [[Vec3; 3]; 2] {
    let rig = gun_rig(GunModel::Rifle);
    [
        arm(1.0, GUN_ORIGIN + rig.grip()),
        arm(-1.0, GUN_ORIGIN + rig.left.position),
    ]
}

/// Origin at the shoulder centre: both arms holding the rifle (a separate model) along -Z.
fn build_aim(look: &SoldierLook) -> Mesh {
    let uniform = rgb(look.uniform);
    let mut b = Builder::default();
    for [shoulder, elbow, hand] in arms() {
        b.limb(shoulder, elbow, 0.062, 0.05, uniform)
            .limb(elbow, hand, 0.05, 0.04, uniform)
            .ball(Vec3::splat(0.045), hand, GLOVE);
    }
    b.build()
}

/// Chest and back panels, in torso space.
fn build_mark_torso() -> Mesh {
    let mut b = Builder::default();
    b.cuboid(v(0.14, 0.06, 0.012), v(0.0, 0.5, -0.134), Color::WHITE)
        .cuboid(v(0.2, 0.15, 0.012), v(0.0, 0.42, 0.262), Color::WHITE);
    b.build()
}

/// Armbands on both upper arms, in aim space.
fn build_mark_aim() -> Mesh {
    let mut b = Builder::default();
    for [shoulder, elbow, _] in arms() {
        let along = |t: f32| shoulder.lerp(elbow, t);
        // The upper arm tapers from 0.062 to 0.05.
        let radius = |t: f32| 0.062 + (0.05 - 0.062) * t + 0.005;
        b.limb(
            along(0.35),
            along(0.55),
            radius(0.35),
            radius(0.55),
            Color::WHITE,
        );
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The head centre of a pose, in feet space.
    fn head_centre(poses: &[Transform; PART_COUNT]) -> Vec3 {
        poses[Part::Head as usize].transform_point(Vec3::Y * HEAD_CENTRE)
    }

    /// The sole of a shin, in feet space.
    fn sole(poses: &[Transform; PART_COUNT], shin: Part) -> Vec3 {
        poses[shin as usize].transform_point(Vec3::NEG_Y * 0.47)
    }

    #[test]
    fn head_sits_at_eye_height() {
        for stance in [Stance::Standing, Stance::Crouching, Stance::Prone] {
            let head = head_centre(&pose(stance, Gait::default(), 0.0));
            assert!(
                (head.y - stance.eye_height()).abs() < 0.05,
                "{stance:?}: head at {} for eye height {}",
                head.y,
                stance.eye_height()
            );
        }
    }

    #[test]
    fn upright_stances_stand_on_the_ground() {
        for stance in [Stance::Standing, Stance::Crouching] {
            let poses = pose(stance, Gait::default(), 0.0);
            for shin in [Part::ShinLeft, Part::ShinRight] {
                let sole = sole(&poses, shin);
                assert!(sole.y.abs() < 0.03, "{stance:?}: sole at {}", sole.y);
            }
        }
    }

    #[test]
    fn walking_swings_the_legs_and_keeps_the_feet_above_the_ground() {
        let mut gait = Gait::default();
        let mut forward = [f32::MAX, f32::MIN];
        for _ in 0..200 {
            gait.advance(2.5, 2.5, 5.0, 0.01);
            let poses = pose(Stance::Standing, gait, 0.0);
            let left = sole(&poses, Part::ShinLeft);
            let right = sole(&poses, Part::ShinRight);
            assert!(left.y > -0.05 && right.y > -0.05, "{left} {right}");
            forward[0] = forward[0].min(left.z);
            forward[1] = forward[1].max(left.z);
        }
        assert!(forward[0] < -0.2 && forward[1] > 0.1, "{forward:?}");
    }

    #[test]
    fn gait_scales_with_speed() {
        let mut gait = Gait::default();
        gait.advance(0.0, 2.5, 5.0, 0.1);
        assert_eq!((gait.walk, gait.run, gait.phase), (0.0, 0.0, 0.0));
        gait.advance(5.0, 2.5, 5.0, 0.1);
        assert_eq!((gait.walk, gait.run), (1.0, 1.0));
        assert!(gait.phase > 0.0);
    }
}
