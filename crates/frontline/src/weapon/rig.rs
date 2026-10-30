//! What the first-person gun models need to be placed and held: where each model sits in *gun
//! space* (origin at the receiver, -Z forward, +Y up, +X right, metres), which parts move, where
//! the hands go, and the procedural hand and sleeve meshes. Also what a third-person holder (a
//! remote player) needs to carry the same gun: `GunRig::model_transform` and `grip`.
//!
//! The models are the Styloo Guns Asset Pack's (assets/models/CREDITS.md): authored along +X with
//! +Y up, so `model_transform` turns them to face -Z.

use std::f32::consts::FRAC_PI_2;

use bevy::mesh::{Capsule3dMeshBuilder, ConicalFrustumMeshBuilder, MeshBuilder};
use bevy::prelude::*;

use crate::data::GunModel;
use crate::equipment::GrenadeKind;

/// Where a hand sits in gun space and how it is turned. A hand's own frame: -Z the way the
/// fingers point, +Y the back of the hand, the thumb on -X (before `mirror`, which makes it a
/// left hand by flipping X).
#[derive(Clone, Copy, Debug)]
pub struct HandPose {
    pub position: Vec3,
    pub rotation: Quat,
    pub mirror: bool,
}

impl HandPose {
    pub fn transform(&self) -> Transform {
        Transform {
            translation: self.position,
            rotation: self.rotation,
            scale: Vec3::new(if self.mirror { -1.0 } else { 1.0 }, 1.0, 1.0),
        }
    }
}

/// Right hand round a vertical pistol grip: fingers forward, curling to the left across the
/// grip's front, the back of the hand to the right.
fn right_on_grip(position: Vec3) -> HandPose {
    HandPose {
        position,
        rotation: Quat::from_rotation_z(-FRAC_PI_2),
        mirror: false,
    }
}

/// Left hand cradling the underside of a handguard: fingers to the right, curling up over it.
fn left_under(position: Vec3) -> HandPose {
    HandPose {
        position,
        // Tilted so the back of the hand turns down and to the left, into the player's view.
        rotation: Quat::from_rotation_z(-0.6)
            * Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_Y, Vec3::NEG_X)),
        mirror: true,
    }
}

/// Left hand round something vertical (a magazine, a foregrip), its palm to the right.
fn left_on_grip(position: Vec3) -> HandPose {
    HandPose {
        position,
        rotation: Quat::from_rotation_z(FRAC_PI_2),
        mirror: true,
    }
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
        GunModel::Rifle => GunRig {
            file: "models/weapons/ak47.glb",
            scale: Vec3::new(1.0, 1.0, 1.35),
            offset: Vec3::ZERO,
            magazine: Some("akmagazine_low"),
            mag_travel: Vec3::new(0.0, -0.17, 0.04),
            bare: &["aktrigger_low", "akmagazine_low"],
            ads_hidden: &[],
            body_top: 0.058,
            barrel_radius: 0.008,
            bolt: None,
            right: right_on_grip(Vec3::new(0.034, -0.05, 0.05)),
            left: left_under(Vec3::new(-0.02, -0.012, -0.24)),
            left_mag: left_on_grip(Vec3::new(-0.04, -0.08, -0.12)),
            left_grip: left_on_grip(Vec3::new(-0.034, -0.034, -0.25)),
            underbarrel: Some(Vec3::new(0.0, -0.03, -0.25)),
        },
        GunModel::Smg => GunRig {
            file: "models/weapons/mac10.glb",
            scale: Vec3::splat(1.35),
            offset: Vec3::ZERO,
            magazine: Some("mac10magazine_low"),
            mag_travel: Vec3::new(0.0, -0.16, 0.0),
            bare: &["mac10trigger_low", "mac10magazine_low", "mac10rond_low"],
            ads_hidden: &["mac10derriere_low"],
            body_top: 0.024,
            barrel_radius: 0.01,
            bolt: None,
            right: right_on_grip(Vec3::new(0.03, -0.075, 0.05)),
            left: left_under(Vec3::new(-0.02, -0.055, -0.10)),
            left_mag: left_on_grip(Vec3::new(-0.04, -0.14, -0.03)),
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
            left_mag: left_on_grip(Vec3::new(-0.04, -0.01, -0.02)),
            left_grip: left_on_grip(Vec3::new(-0.034, 0.0, -0.26)),
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
            body_top: 0.062,
            barrel_radius: 0.007,
            bolt: None,
            right: right_on_grip(Vec3::new(0.03, -0.034, 0.08)),
            left: left_on_grip(Vec3::new(-0.03, -0.04, 0.06)),
            left_mag: left_on_grip(Vec3::new(-0.03, -0.06, 0.07)),
            left_grip: left_on_grip(Vec3::new(-0.03, -0.04, 0.06)),
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

/// Where the forearms run to, in camera space: behind and below the eye, off the bottom of the
/// view.
pub const RIGHT_ELBOW: Vec3 = Vec3::new(0.24, -0.46, 0.18);
pub const LEFT_ELBOW: Vec3 = Vec3::new(-0.22, -0.48, 0.14);

/// Sleeve radii at the elbow and the wrist, glove cuff radius and length.
const SLEEVE_RADII: (f32, f32) = (0.046, 0.03);
const CUFF: (f32, f32) = (0.032, 0.045);

/// A glove, fingers curled by `curl` (0 flat, 1 a fist round a grip), in the hand's own frame
/// (see `HandPose`), with a cuff at the wrist (+Z).
pub fn hand_mesh(curl: f32) -> Mesh {
    // The back of the hand: a flattened ellipsoid.
    let mut mesh = Sphere::new(1.0)
        .mesh()
        .uv(10, 6)
        .transformed_by(Transform::from_scale(Vec3::new(0.042, 0.02, 0.05)));
    let mut add = |part: Mesh| {
        mesh.merge(&part).expect("glove parts share attributes");
    };
    let segment = |from: Vec3, direction: Vec3, length: f32, radius: f32| -> Mesh {
        let height = (length - 2.0 * radius).max(0.001);
        Capsule3dMeshBuilder::new(radius, height, 8, 4)
            .build()
            .transformed_by(Transform {
                translation: from + direction * length / 2.0,
                rotation: Quat::from_rotation_arc(Vec3::Y, direction),
                scale: Vec3::ONE,
            })
    };
    // Cumulative bend (radians) of the three finger segments at a full curl; toward the palm
    // (-Y).
    let bends = [0.5, 1.75, 3.0];
    let lengths = [
        [0.038, 0.027, 0.022],
        [0.042, 0.03, 0.024],
        [0.04, 0.028, 0.023],
        [0.033, 0.023, 0.02],
    ];
    for (finger, lengths) in lengths.iter().enumerate() {
        let x = -0.027 + 0.018 * finger as f32;
        let radius = if finger == 3 { 0.0085 } else { 0.0095 };
        let mut from = Vec3::new(x, -0.002, -0.041);
        for (&length, bend) in lengths.iter().zip(bends) {
            let angle = bend * curl;
            let direction = Vec3::new(0.0, -angle.sin(), -angle.cos());
            add(segment(from, direction, length, radius));
            from += direction * length;
        }
    }
    // Thumb: from the index side of the palm, forward and across.
    let mut from = Vec3::new(-0.042, -0.004, -0.0);
    let first = Vec3::new(-0.35, -0.2 - 0.25 * curl, -0.9).normalize();
    let second = Vec3::new(0.1 + 0.2 * curl, -0.3 - 0.5 * curl, -0.9).normalize();
    for (direction, length) in [(first, 0.04), (second, 0.032)] {
        add(segment(from, direction, length, 0.0105));
        from += direction * length;
    }
    // Cuff.
    add(
        ConicalFrustumMeshBuilder::new(CUFF.0, CUFF.0 - 0.002, CUFF.1, 10)
            .build()
            .transformed_by(Transform {
                translation: Vec3::new(0.0, 0.0, 0.04 + CUFF.1 / 2.0),
                rotation: Quat::from_rotation_x(FRAC_PI_2),
                scale: Vec3::ONE,
            }),
    );
    mesh
}

/// A forearm of unit length along +Y (the elbow end at the top), scaled to the arm's length.
pub fn sleeve_mesh() -> Mesh {
    ConicalFrustumMeshBuilder::new(SLEEVE_RADII.0, SLEEVE_RADII.1, 1.0, 10).build()
}

/// Wrist of a hand at `hand` (its transform), where the sleeve starts.
pub fn wrist(hand: &Transform) -> Vec3 {
    hand.translation + hand.rotation * (Vec3::new(0.0, 0.0, 0.04 + CUFF.1) * hand.scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS: [GunModel; 4] = [
        GunModel::Rifle,
        GunModel::Smg,
        GunModel::Sniper,
        GunModel::Pistol,
    ];

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

    #[test]
    fn a_left_hand_is_the_mirror_of_a_right_hand() {
        for model in MODELS {
            let rig = gun_rig(model);
            assert!(!rig.right.transform().scale.x.is_sign_negative());
            for pose in [rig.left, rig.left_mag, rig.left_grip] {
                assert!(pose.transform().scale.x.is_sign_negative(), "{model:?}");
            }
        }
    }

    #[test]
    fn gloves_and_sleeves_build_for_every_curl() {
        // Capsule meshes panic on too few latitudes: building every curl proves the setup.
        for curl in [0.0, 0.5, 0.75, 0.9, 1.0] {
            assert!(hand_mesh(curl).count_vertices() > 100);
        }
        assert!(sleeve_mesh().count_vertices() > 10);
    }

    #[test]
    fn the_wrist_is_behind_the_palm_along_its_own_axis() {
        let hand =
            Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_y(FRAC_PI_2));
        let wrist = wrist(&hand);
        // Hand-local +Z turned a quarter about Y points along +X.
        assert!((wrist - hand.translation).normalize().dot(Vec3::X) > 0.999);
    }
}
