//! What the first-person gun models need to be placed and held: where each model sits in *gun
//! space* (origin at the receiver, -Z forward, +Y up, +X right, metres), which parts move, where
//! the hands go, and the procedural hand and sleeve meshes. Also what a third-person holder (a
//! remote player) needs to carry the same gun: `GunRig::model_transform` and `grip`.
//!
//! The first-person guns are the Styloo Guns Asset Pack's (the SMG, Sniper and Pistol) and the
//! M4 and M16A4 built from primitives by `tools/assets/build_rifles.py` (assets/models/CREDITS.md):
//! authored along +X with +Y up, so `model_transform` turns them to face -Z.

use std::f32::consts::FRAC_PI_2;

use bevy::mesh::{ConicalFrustumMeshBuilder, MeshBuilder};
use bevy::prelude::*;

use crate::data::GunModel;
use crate::equipment::GrenadeKind;

/// A point as seen in a mirror standing on the YZ plane (left and right swap).
pub fn mirror_point(point: Vec3) -> Vec3 {
    Vec3::new(-point.x, point.y, point.z)
}

/// The turn that does to mirrored points what `rotation` does to the originals (M R M with M
/// the mirror): a turn about X stays, a turn about Y or Z reverses.
pub fn mirror_rotation(rotation: Quat) -> Quat {
    Quat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w)
}

/// Where something sits and how it is turned, seen in the mirror; the scale is left alone, so
/// the thing itself is not flipped (a glove is swapped for the other hand's instead).
pub fn mirror_placement(transform: Transform) -> Transform {
    Transform {
        translation: mirror_point(transform.translation),
        rotation: mirror_rotation(transform.rotation),
        scale: transform.scale,
    }
}

/// A solid thing seen in the mirror: its placement mirrored and the thing itself flipped (a
/// negative X scale). A mesh under it has its triangle winding reversed on screen.
pub fn mirror_solid(transform: Transform) -> Transform {
    Transform {
        scale: Vec3::new(-transform.scale.x, transform.scale.y, transform.scale.z),
        ..mirror_placement(transform)
    }
}

/// Where a hand sits in gun space and how it is turned. A hand's own frame: -Z the way the
/// fingers point, +Y the back of the hand, the thumb on -X for a right hand (a `left` glove is
/// the mirror image, its thumb on +X). `curl` is how far the fingers close: 0 flat, 1 a fist
/// round a grip.
#[derive(Clone, Copy, Debug)]
pub struct HandPose {
    pub position: Vec3,
    pub rotation: Quat,
    pub left: bool,
    pub curl: f32,
}

impl HandPose {
    pub fn transform(&self) -> Transform {
        Transform {
            translation: self.position,
            rotation: self.rotation,
            scale: Vec3::ONE,
        }
    }

    /// The same hold seen in a mirror: the other hand's glove at the mirrored place.
    pub fn mirrored(self) -> Self {
        Self {
            position: mirror_point(self.position),
            rotation: mirror_rotation(self.rotation),
            left: !self.left,
            curl: self.curl,
        }
    }
}

/// Right hand round a vertical pistol grip: fingers forward, curling to the left across the
/// grip's front, the back of the hand to the right.
fn right_on_grip(position: Vec3) -> HandPose {
    HandPose {
        position,
        rotation: Quat::from_rotation_z(-FRAC_PI_2),
        left: false,
        curl: 0.9,
    }
}

/// Left hand cradling the underside of a handguard: fingers to the right, curling up over it.
fn left_under(position: Vec3) -> HandPose {
    HandPose {
        position,
        // Tilted so the back of the hand turns down and to the left, into the player's view.
        rotation: Quat::from_rotation_z(-0.6)
            * Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_Y, Vec3::NEG_X)),
        left: true,
        curl: 0.6,
    }
}

/// Left hand round something vertical (a magazine, a foregrip), its palm to the right.
fn left_on_grip(position: Vec3) -> HandPose {
    right_on_grip(mirror_point(position)).mirrored()
}

/// The right hand gripping the knob of a bolt handle at `knob` in gun space, its palm on the
/// outer side of it; the left hand when the handle is on the left of the gun.
pub fn bolt_grip(knob: Vec3) -> HandPose {
    let on_right = knob.x >= 0.0;
    let knob = if on_right { knob } else { mirror_point(knob) };
    let mut grip = right_on_grip(knob + Vec3::new(0.028, 0.0, 0.0));
    grip.curl = 0.8;
    if on_right { grip } else { grip.mirrored() }
}

/// One gun model's rig. See the module docs for gun space.
#[derive(Clone, Debug)]
pub struct GunRig {
    /// Embedded `.glb`, loaded as `EMBEDDED_ROOT + file + "#Scene0"`.
    pub file: &'static str,
    /// Model axes scale (length, height, width) and where the model's origin lands in gun space.
    pub scale: Vec3,
    pub offset: Vec3,
    /// Name of the magazine node and how far (gun space) a reload pulls it.
    pub magazine: Option<&'static str>,
    pub mag_travel: Vec3,
    /// Nodes camo leaves bare (glass, rings, small parts).
    pub bare: &'static [&'static str],
    /// Nodes hidden while aimed (they would surround the eye).
    pub ads_hidden: &'static [&'static str],
    /// Nodes that give way to a fitted Optic (a carry handle, flip-up iron sights): an Optic
    /// stands on the rail where they are and its window would show them.
    pub optic_hidden: &'static [&'static str],
    /// Top of the receiver: sight mounts stand on it.
    pub body_top: f32,
    /// Barrel radius; a Silencer is wider by a fixed factor.
    pub barrel_radius: f32,
    /// Where the bolt handle of a bolt-action model leaves the receiver's right side (drawn
    /// procedurally).
    pub bolt: Option<Vec3>,
    /// The right hand on the grip; the point the third-person gun is held at is `right.position`.
    pub right: HandPose,
    /// The left hand on the handguard, on the magazine while reloading, and on a Foregrip or
    /// Grenade Launcher.
    pub left: HandPose,
    pub left_mag: HandPose,
    pub left_grip: HandPose,
    /// Where an Underbarrel Attachment hangs (centre), if the gun takes one.
    pub underbarrel: Option<Vec3>,
}

impl GunRig {
    /// Model space to gun space.
    pub fn model_transform(&self) -> Transform {
        Transform {
            translation: self.offset,
            rotation: Quat::from_rotation_y(FRAC_PI_2),
            scale: self.scale,
        }
    }

    /// Gun-space point of the right hand's grip: where a holder's hand goes.
    pub fn grip(&self) -> Vec3 {
        self.right.position
    }
}

pub fn gun_rig(model: GunModel) -> GunRig {
    match model {
        GunModel::Carbine => GunRig {
            file: "models/weapons/m4.glb",
            scale: Vec3::ONE,
            offset: Vec3::ZERO,
            magazine: Some("m4_magazine"),
            mag_travel: Vec3::new(0.0, -0.17, 0.03),
            bare: &["m4_trigger", "m4_magazine", "m4_rearsight", "m4_frontsight"],
            ads_hidden: &["m4_stock"],
            optic_hidden: &["m4_rearsight", "m4_frontsight"],
            body_top: 0.060,
            barrel_radius: 0.0075,
            bolt: None,
            right: right_on_grip(Vec3::new(0.034, -0.05, 0.052)),
            left: left_under(Vec3::new(-0.02, -0.006, -0.225)),
            left_mag: left_on_grip(Vec3::new(-0.034, -0.10, -0.032)),
            left_grip: left_on_grip(Vec3::new(-0.03, -0.028, -0.24)),
            underbarrel: Some(Vec3::new(0.0, 0.010, -0.24)),
        },
        GunModel::Rifle => GunRig {
            file: "models/weapons/m16.glb",
            scale: Vec3::ONE,
            offset: Vec3::ZERO,
            magazine: Some("m16_magazine"),
            mag_travel: Vec3::new(0.0, -0.17, 0.03),
            bare: &[
                "m16_trigger",
                "m16_magazine",
                "m16_rearsight",
                "m16_frontsight",
            ],
            ads_hidden: &["m16_stock"],
            optic_hidden: &["m16_handle", "m16_rearsight", "m16_frontsight"],
            body_top: 0.060,
            barrel_radius: 0.0078,
            bolt: None,
            right: right_on_grip(Vec3::new(0.034, -0.05, 0.052)),
            left: left_under(Vec3::new(-0.02, -0.006, -0.27)),
            left_mag: left_on_grip(Vec3::new(-0.034, -0.10, -0.032)),
            left_grip: left_on_grip(Vec3::new(-0.03, -0.028, -0.28)),
            underbarrel: Some(Vec3::new(0.0, 0.008, -0.27)),
        },
        GunModel::Smg => GunRig {
            file: "models/weapons/mac10.glb",
            scale: Vec3::splat(1.35),
            offset: Vec3::ZERO,
            magazine: Some("mac10magazine_low"),
            mag_travel: Vec3::new(0.0, -0.16, 0.0),
            bare: &["mac10trigger_low", "mac10magazine_low", "mac10rond_low"],
            ads_hidden: &["mac10derriere_low"],
            optic_hidden: &[],
            body_top: 0.024,
            barrel_radius: 0.01,
            bolt: None,
            right: right_on_grip(Vec3::new(0.027, -0.098, 0.043)),
            left: left_under(Vec3::new(-0.02, -0.055, -0.10)),
            left_mag: left_on_grip(Vec3::new(-0.02, -0.185, 0.03)),
            left_grip: left_on_grip(Vec3::new(-0.03, -0.06, -0.10)),
            underbarrel: Some(Vec3::new(0.0, -0.045, -0.10)),
        },
        GunModel::Sniper => GunRig {
            file: "models/weapons/awp.glb",
            scale: Vec3::ONE,
            offset: Vec3::ZERO,
            magazine: Some("snipermagazine_low"),
            mag_travel: Vec3::new(0.0, -0.12, 0.0),
            ads_hidden: &[],
            optic_hidden: &[],
            bare: &[
                "sniperscope_low",
                "snipertendon_low",
                "snipertendon_low.001",
                "snipertrigger_low",
                "snipermagazine_low",
            ],
            body_top: 0.057,
            barrel_radius: 0.0075,
            bolt: Some(Vec3::new(0.02, 0.045, 0.07)),
            right: right_on_grip(Vec3::new(0.034, -0.015, 0.105)),
            left: left_under(Vec3::new(-0.02, 0.012, -0.26)),
            left_mag: left_on_grip(Vec3::new(-0.031, -0.004, -0.021)),
            left_grip: left_on_grip(Vec3::new(-0.03, 0.0, -0.26)),
            underbarrel: None,
        },
        GunModel::Pistol => GunRig {
            file: "models/weapons/pew.glb",
            scale: Vec3::splat(1.4),
            offset: Vec3::ZERO,
            magazine: Some("pewmagazine"),
            mag_travel: Vec3::new(0.0, -0.13, 0.0),
            bare: &["pewtrigger", "pewmagazine"],
            ads_hidden: &[],
            optic_hidden: &[],
            body_top: 0.062,
            barrel_radius: 0.007,
            bolt: None,
            right: right_on_grip(Vec3::new(0.025, -0.034, 0.08)),
            left: left_on_grip(Vec3::new(-0.025, -0.045, 0.062)),
            left_mag: left_on_grip(Vec3::new(-0.025, -0.06, 0.075)),
            left_grip: left_on_grip(Vec3::new(-0.025, -0.045, 0.062)),
            underbarrel: None,
        },
    }
}

/// The grenade model, the scale that makes it hand-sized and the height of its middle.
pub fn grenade_model(kind: GrenadeKind) -> (&'static str, f32, f32) {
    match kind {
        GrenadeKind::Frag => ("models/weapons/nade_low.glb", 0.65, 0.05),
        GrenadeKind::Flashbang => ("models/weapons/flashbang_low.glb", 1.0, 0.0),
        GrenadeKind::Smoke => ("models/weapons/smoke_low.glb", 1.0, 0.02),
        // The 40mm round has no model of its own.
        GrenadeKind::Launcher => ("models/weapons/nade_low.glb", 0.4, 0.05),
    }
}

/// One arm: the shoulder it hangs from in camera space (behind and below the eye, off the
/// bottom of the view), the lengths of its two bones and the direction the elbow bows to.
#[derive(Clone, Copy, Debug)]
pub struct Arm {
    pub shoulder: Vec3,
    pub upper: f32,
    pub fore: f32,
    pub pole: Vec3,
}

/// The right arm; the left one is its mirror image. The bones are longer than a real arm's
/// because the gun is held further out than a real one, and the elbow hangs down and out.
pub const RIGHT_ARM: Arm = Arm {
    shoulder: Vec3::new(0.21, -0.27, 0.10),
    upper: 0.34,
    fore: 0.36,
    pole: Vec3::new(0.6, -1.0, 0.0),
};

pub fn left_arm() -> Arm {
    Arm {
        shoulder: mirror_point(RIGHT_ARM.shoulder),
        pole: mirror_point(RIGHT_ARM.pole),
        ..RIGHT_ARM
    }
}

/// Where the elbow goes so that a two-bone arm of lengths `upper` and `fore` from `shoulder`
/// reaches `target`, bowing towards `pole`. A target beyond the reach (or too close to bend)
/// gets a straight arm along the line, both bones stretched alike, so the hand is never left
/// without an arm.
pub fn solve_elbow(shoulder: Vec3, target: Vec3, upper: f32, fore: f32, pole: Vec3) -> Vec3 {
    let to = target - shoulder;
    let distance = to.length();
    let Some(direction) = to.try_normalize() else {
        return shoulder + pole.normalize_or(Vec3::NEG_Y) * upper;
    };
    let reach = distance.clamp((upper - fore).abs() + 1e-4, upper + fore);
    // Along the line to the target, and how far the elbow stands off it (law of cosines).
    let along = (reach * reach + upper * upper - fore * fore) / (2.0 * reach);
    let off = (upper * upper - along * along).max(0.0).sqrt();
    let bow = (pole - direction * pole.dot(direction))
        .try_normalize()
        .unwrap_or_else(|| direction.any_orthonormal_vector());
    shoulder + direction * along * (distance / reach).max(1.0) + bow * off
}

/// Sleeve radii at the shoulder, the elbow and the wrist; the glove's cuff radius (at the
/// palm end and the open end) and length.
const SLEEVE_RADII: (f32, f32, f32) = (0.046, 0.040, 0.030);
const CUFF_RADII: (f32, f32) = (0.031, 0.036);
const CUFF_LENGTH: f32 = 0.05;
/// Half the palm's length, where the cuff starts on the hand's +Z.
const PALM_HALF_LENGTH: f32 = 0.04;
/// How far inside the cuff the sleeve ends.
const SLEEVE_INSIDE_CUFF: f32 = 0.02;

/// The upper arm or the forearm: a tapered tube of unit length along +Y (the wide end on top).
pub fn upper_arm_mesh() -> Mesh {
    ConicalFrustumMeshBuilder::new(SLEEVE_RADII.0, SLEEVE_RADII.1, 1.0, 12).build()
}

pub fn forearm_mesh() -> Mesh {
    ConicalFrustumMeshBuilder::new(SLEEVE_RADII.1, SLEEVE_RADII.2, 1.0, 12).build()
}

/// The sleeve's joint at the elbow, where the two tubes meet.
pub fn elbow_mesh() -> Mesh {
    Sphere::new(SLEEVE_RADII.1).mesh().uv(12, 8)
}

/// Places a unit tube from `wide_end` to `narrow_end`.
pub fn tube_transform(wide_end: Vec3, narrow_end: Vec3) -> Transform {
    let along = wide_end - narrow_end;
    let length = along.length().max(1e-4);
    Transform {
        translation: (wide_end + narrow_end) / 2.0,
        rotation: Quat::from_rotation_arc(Vec3::Y, along / length),
        scale: Vec3::new(1.0, length, 1.0),
    }
}

/// Wrist of a hand at `hand` (its transform), the far end of the cuff.
pub fn wrist(hand: &Transform) -> Vec3 {
    hand.translation + hand.rotation * Vec3::new(0.0, 0.0, PALM_HALF_LENGTH + CUFF_LENGTH)
}

/// Where the forearm's sleeve ends, a little inside the cuff.
pub fn sleeve_end(hand: &Transform) -> Vec3 {
    wrist(hand) - hand.rotation * Vec3::new(0.0, 0.0, SLEEVE_INSIDE_CUFF)
}

/// How many glove meshes there are: the fingers close in steps of these curls and the nearest
/// is drawn.
pub const GLOVE_CURLS: [f32; 4] = [0.35, 0.6, 0.8, 0.95];

pub fn nearest_curl(curl: f32) -> usize {
    (0..GLOVE_CURLS.len())
        .min_by(|a, b| {
            let (a, b) = (
                (GLOVE_CURLS[*a] - curl).abs(),
                (GLOVE_CURLS[*b] - curl).abs(),
            );
            a.total_cmp(&b)
        })
        .unwrap_or(0)
}

/// A block of `size` (x wide, y thick, z long) with its centre at `centre`, its length turned
/// to point along `direction`.
fn block(size: Vec3, centre: Vec3, direction: Vec3) -> Mesh {
    Mesh::from(Cuboid::from_size(size)).transformed_by(Transform {
        translation: centre,
        rotation: Quat::from_rotation_arc(Vec3::NEG_Z, direction),
        scale: Vec3::ONE,
    })
}

/// A glove: the palm, four fingers of three blocks each, a thumb of two and a cuff at the
/// wrist (+Z), the fingers curled by `curl` (0 flat, 1 a fist round a grip), in the hand's own
/// frame (see `HandPose`). `left` mirrors it into a left hand.
pub fn glove_mesh(curl: f32, left: bool) -> Mesh {
    // The palm, a little narrower at the wrist end.
    let mut mesh = block(
        Vec3::new(0.078, 0.031, 0.05),
        Vec3::new(0.0, 0.0, -0.015),
        Vec3::NEG_Z,
    );
    let mut add = |part: Mesh| {
        mesh.merge(&part).expect("glove parts share attributes");
    };
    add(block(
        Vec3::new(0.066, 0.029, 0.042),
        Vec3::new(0.0, 0.0, 0.019),
        Vec3::NEG_Z,
    ));
    // A bone: a block from `from` along `direction`, a little over-long so a bend has no gap.
    let bone = |from: Vec3, direction: Vec3, length: f32, width: f32, thick: f32| -> Mesh {
        let length = length + 0.004;
        block(
            Vec3::new(width, thick, length),
            from + direction * length / 2.0,
            direction,
        )
    };
    // Cumulative bend (radians) of the three finger bones at a full curl, towards the palm
    // (-Y).
    let bends = [0.5, 1.75, 3.0];
    let lengths = [
        [0.036, 0.026, 0.022],
        [0.04, 0.029, 0.023],
        [0.037, 0.027, 0.022],
        [0.03, 0.021, 0.019],
    ];
    for (finger, lengths) in lengths.iter().enumerate() {
        let x = -0.0278 + 0.0185 * finger as f32;
        let (width, thick) = if finger == 3 {
            (0.0145, 0.0135)
        } else {
            (0.0165, 0.016)
        };
        let mut from = Vec3::new(x, 0.0, -PALM_HALF_LENGTH + 0.002);
        for (&length, bend) in lengths.iter().zip(bends) {
            let angle = bend * curl;
            let direction = Vec3::new(0.0, -angle.sin(), -angle.cos());
            add(bone(from, direction, length, width, thick));
            from += direction * length;
        }
    }
    // Thumb: from the index side of the palm, forward and across.
    let mut from = Vec3::new(-0.036, -0.002, 0.014);
    let first = Vec3::new(-0.35, -0.2 - 0.25 * curl, -0.9).normalize();
    let second = Vec3::new(0.1 + 0.2 * curl, -0.3 - 0.5 * curl, -0.9).normalize();
    for (direction, length) in [(first, 0.04), (second, 0.032)] {
        add(bone(from, direction, length, 0.021, 0.019));
        from += direction * length;
    }
    // Cuff, flaring towards the open end.
    add(
        ConicalFrustumMeshBuilder::new(CUFF_RADII.1, CUFF_RADII.0, CUFF_LENGTH, 12)
            .build()
            .transformed_by(Transform {
                translation: Vec3::new(0.0, 0.0, PALM_HALF_LENGTH + CUFF_LENGTH / 2.0),
                rotation: Quat::from_rotation_x(FRAC_PI_2),
                scale: Vec3::ONE,
            }),
    );
    if left {
        mesh = mesh
            .transformed_by(Transform::from_scale(Vec3::new(-1.0, 1.0, 1.0)))
            .with_inverted_winding()
            .expect("a glove is a triangle list");
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS: [GunModel; 5] = [
        GunModel::Carbine,
        GunModel::Rifle,
        GunModel::Smg,
        GunModel::Sniper,
        GunModel::Pistol,
    ];

    /// The JSON chunk of a model file, without whitespace.
    fn model_json(rig: &GunRig) -> String {
        let path = format!("{}/assets/{}", env!("CARGO_MANIFEST_DIR"), rig.file);
        let glb = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(&glb[..4], b"glTF", "{path}");
        let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + length]).expect("a JSON chunk");
        json.split_whitespace().collect()
    }

    #[test]
    fn every_node_a_rig_names_is_in_its_model() {
        // A typo here is silent in the game: the magazine stays put in a reload, a camo covers
        // the trigger, a carry handle stays in the red dot's window.
        for model in MODELS {
            let rig = gun_rig(model);
            let json = model_json(&rig);
            let named = rig
                .magazine
                .into_iter()
                .chain(rig.bare.iter().copied())
                .chain(rig.ads_hidden.iter().copied())
                .chain(rig.optic_hidden.iter().copied());
            for name in named {
                assert!(
                    json.contains(&format!("\"name\":\"{name}\"")),
                    "{model:?}: no node {name} in {}",
                    rig.file
                );
            }
        }
    }

    #[test]
    fn a_weapon_s_muzzle_and_sights_match_its_model() {
        let data = crate::data::GameData::load();
        for (id, stats) in &data.weapons {
            let rig = gun_rig(stats.model);
            // An Optic's mount stands on the receiver, and the muzzle is ahead of the hand
            // that steadies the handguard.
            assert!(stats.sight_point.1 > rig.body_top, "{id:?} optic height");
            assert!(
                stats.muzzle_offset.2 < rig.left.position.z - 0.05,
                "{id:?} muzzle"
            );
            // The Weapon's own sight is above the receiver too.
            assert!(
                stats.iron_point.1 > rig.body_top,
                "{id:?} iron sight height"
            );
        }
    }

    #[test]
    fn models_point_down_negative_z_with_y_up() {
        for model in MODELS {
            let to_gun = gun_rig(model).model_transform();
            // Authored along +X (barrel) with +Y up.
            let forward = to_gun.rotation * Vec3::X;
            let up = to_gun.rotation * Vec3::Y;
            assert!(
                (forward - Vec3::NEG_Z).length() < 1e-5,
                "{model:?} {forward}"
            );
            assert!((up - Vec3::Y).length() < 1e-5, "{model:?} {up}");
        }
    }

    fn poses(model: GunModel) -> [HandPose; 4] {
        let rig = gun_rig(model);
        [rig.right, rig.left, rig.left_mag, rig.left_grip]
    }

    #[test]
    fn mirroring_a_pose_twice_gives_it_back() {
        for model in MODELS {
            for pose in poses(model) {
                let twice = pose.mirrored().mirrored();
                assert!((twice.position - pose.position).length() < 1e-6);
                assert!(twice.rotation.dot(pose.rotation).abs() > 0.999_999);
                assert_eq!((twice.left, twice.curl), (pose.left, pose.curl));
            }
        }
    }

    #[test]
    fn a_mirrored_hand_is_the_mirror_image_of_the_hand() {
        // Every point of the glove, mirrored along with the hand, lands on the mirror image of
        // where it was.
        let local_points = [
            Vec3::new(0.03, 0.01, -0.07),
            Vec3::new(-0.04, -0.02, 0.02),
            Vec3::new(0.0, 0.0, 0.09),
        ];
        for model in MODELS {
            for pose in poses(model) {
                let mirrored = pose.mirrored();
                for local in local_points {
                    let here = pose.transform().transform_point(local);
                    let there = mirrored.transform().transform_point(mirror_point(local));
                    assert!((there - mirror_point(here)).length() < 1e-5, "{model:?}");
                }
            }
        }
    }

    /// Whether every triangle runs counter-clockwise seen from the side its vertex normals
    /// point to, which is what back-face culling draws.
    fn winding_agrees_with_normals(mesh: &Mesh) -> bool {
        use bevy::mesh::{Indices, VertexAttributeValues};
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return false;
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            return false;
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            return false;
        };
        indices.chunks(3).all(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize]));
            let normal = Vec3::from(normals[triangle[0] as usize]);
            (b - a).cross(c - a).dot(normal) > 0.0
        })
    }

    #[test]
    fn a_left_glove_is_the_mirror_of_the_right_one_and_still_faces_outwards() {
        use bevy::mesh::VertexAttributeValues;
        for curl in GLOVE_CURLS {
            let right = glove_mesh(curl, false);
            let left = glove_mesh(curl, true);
            assert!(right.count_vertices() > 100);
            // A mirror reverses the winding: unless it is put right again, a left hand would be
            // drawn inside out.
            assert!(winding_agrees_with_normals(&right), "right, curl {curl}");
            assert!(winding_agrees_with_normals(&left), "left, curl {curl}");
            let (
                Some(VertexAttributeValues::Float32x3(r)),
                Some(VertexAttributeValues::Float32x3(l)),
            ) = (
                right.attribute(Mesh::ATTRIBUTE_POSITION),
                left.attribute(Mesh::ATTRIBUTE_POSITION),
            )
            else {
                panic!("positions");
            };
            assert!(
                r.iter()
                    .zip(l)
                    .all(|(r, l)| (Vec3::from(*r) - mirror_point(Vec3::from(*l))).length() < 1e-6)
            );
        }
    }

    #[test]
    fn the_glove_closest_to_a_curl_is_the_one_drawn() {
        for (index, curl) in GLOVE_CURLS.into_iter().enumerate() {
            assert_eq!(nearest_curl(curl), index);
            assert_eq!(nearest_curl(curl + 0.02), index);
        }
        assert_eq!(nearest_curl(0.0), 0);
        assert_eq!(nearest_curl(2.0), GLOVE_CURLS.len() - 1);
    }

    #[test]
    fn the_wrist_is_behind_the_palm_along_its_own_axis() {
        let hand =
            Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2));
        let wrist = wrist(&hand);
        // Hand-local +Z turned a quarter about Y points along +X.
        assert!((wrist - hand.translation).normalize().dot(Vec3::X) > 0.999);
        // The sleeve stops inside the cuff, short of the wrist, on the same line.
        let end = sleeve_end(&hand);
        assert!(end.distance(hand.translation) < wrist.distance(hand.translation));
        assert!((end - hand.translation).normalize().dot(Vec3::X) > 0.999);
    }

    const SHOULDER: Vec3 = Vec3::new(0.2, -0.3, 0.1);
    const POLE: Vec3 = Vec3::new(0.6, -1.0, 0.0);

    #[test]
    fn the_elbow_keeps_both_bones_their_length_when_the_hand_is_in_reach() {
        for target in [
            Vec3::new(0.17, -0.2, -0.45),
            Vec3::new(-0.1, -0.1, -0.3),
            Vec3::new(0.3, -0.5, -0.1),
        ] {
            let elbow = solve_elbow(SHOULDER, target, 0.34, 0.36, POLE);
            assert!((elbow.distance(SHOULDER) - 0.34).abs() < 1e-4, "{target}");
            assert!((elbow.distance(target) - 0.36).abs() < 1e-4, "{target}");
        }
    }

    #[test]
    fn the_elbow_bows_to_the_pole_side() {
        let target = Vec3::new(0.17, -0.2, -0.45);
        let elbow = solve_elbow(SHOULDER, target, 0.34, 0.36, POLE);
        let middle = (SHOULDER + target) / 2.0;
        assert!((elbow - middle).dot(POLE) > 0.0);
        // The other way round with the pole turned about.
        let flipped = solve_elbow(SHOULDER, target, 0.34, 0.36, -POLE);
        assert!((flipped - middle).dot(POLE) < 0.0);
    }

    #[test]
    fn a_hand_out_of_reach_gets_a_straight_arm_towards_it() {
        let target = Vec3::new(0.2, -0.3, -2.0);
        let elbow = solve_elbow(SHOULDER, target, 0.34, 0.36, POLE);
        assert!(elbow.is_finite());
        // On the line from shoulder to hand, the bones stretched in proportion.
        let line = (target - SHOULDER).normalize();
        assert!((elbow - SHOULDER).normalize().dot(line) > 0.9999);
        let ratio = elbow.distance(SHOULDER) / elbow.distance(target);
        assert!((ratio - 0.34 / 0.36).abs() < 1e-3, "{ratio}");
    }

    #[test]
    fn a_hand_on_the_shoulder_still_gets_an_elbow() {
        for target in [SHOULDER, SHOULDER + Vec3::splat(1e-6)] {
            assert!(solve_elbow(SHOULDER, target, 0.34, 0.36, POLE).is_finite());
        }
        // And a pole along the line to the hand does not make the bow degenerate.
        let target = SHOULDER + Vec3::new(0.0, 0.0, -0.5);
        assert!(solve_elbow(SHOULDER, target, 0.34, 0.36, Vec3::NEG_Z).is_finite());
    }

    #[test]
    fn a_tube_runs_from_the_wide_end_to_the_narrow_end() {
        let (wide, narrow) = (Vec3::new(0.2, -0.1, 0.3), Vec3::new(-0.1, -0.4, -0.2));
        let tube = tube_transform(wide, narrow);
        // The unit tube's wide end is its top.
        assert!((tube.transform_point(Vec3::Y * 0.5) - wide).length() < 1e-5);
        assert!((tube.transform_point(Vec3::NEG_Y * 0.5) - narrow).length() < 1e-5);
    }

    #[test]
    fn mirroring_a_solid_flips_it_and_mirroring_a_placement_does_not() {
        let transform = Transform::from_xyz(0.1, -0.2, -0.3).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.4,
            -0.2,
            0.3,
        ));
        let solid = mirror_solid(transform);
        assert_eq!(solid.scale, Vec3::new(-1.0, 1.0, 1.0));
        assert_eq!(mirror_placement(transform).scale, Vec3::ONE);
        // A point of the solid, mirrored, is where the mirrored solid has it.
        let local = Vec3::new(0.05, 0.1, -0.2);
        assert!(
            (solid.transform_point(local) - mirror_point(transform.transform_point(local)))
                .length()
                < 1e-5
        );
    }
}
