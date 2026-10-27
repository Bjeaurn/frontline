//! Server-side hitboxes: one collider per body part (head, upper torso, lower torso, two arms,
//! two legs), shaped per stance and posed from `PlayerPose` every fixed tick. Each part belongs
//! to a Hit Zone; arms and legs are both limbs.
//!
//! They are deliberately not children of the Player entity and never read its `Transform`: on
//! a Host that Transform belongs to the remote-player visuals, which smooth it and lag behind.

use avian3d::prelude::{
    Collider, ColliderDisabled, CollisionLayers, LayerMask, Position, Rotation,
};
use bevy::prelude::*;

use super::damage::HitZone;
use crate::shared::{GameLayer, PlayerPose, Stance};

const HEAD_RADIUS: f32 = 0.15;
const ARM_RADIUS: f32 = 0.06;
const LEG_RADIUS: f32 = 0.09;
/// Width (x), height (y) and depth (z) of the torso boxes when upright.
const UPPER_TORSO: Vec3 = Vec3::new(0.44, 0.40, 0.26);
const LOWER_TORSO: Vec3 = Vec3::new(0.38, 0.25, 0.24);

/// A hitbox entity of the player `HitboxOf.0`.
#[derive(Component, Debug)]
#[relationship(relationship_target = Hitboxes)]
pub struct HitboxOf(pub Entity);

/// The hitbox entities of a player; despawned together with it.
#[derive(Component, Debug)]
#[relationship_target(relationship = HitboxOf, linked_spawn)]
pub struct Hitboxes(Vec<Entity>);

/// One body part with its own collider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyPart {
    Head,
    UpperTorso,
    LowerTorso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

const BODY_PARTS: [BodyPart; 7] = [
    BodyPart::Head,
    BodyPart::UpperTorso,
    BodyPart::LowerTorso,
    BodyPart::LeftArm,
    BodyPart::RightArm,
    BodyPart::LeftLeg,
    BodyPart::RightLeg,
];

impl BodyPart {
    fn zone(self) -> HitZone {
        match self {
            BodyPart::Head => HitZone::Head,
            BodyPart::UpperTorso => HitZone::UpperTorso,
            BodyPart::LowerTorso => HitZone::LowerTorso,
            BodyPart::LeftArm | BodyPart::RightArm | BodyPart::LeftLeg | BodyPart::RightLeg => {
                HitZone::Limbs
            }
        }
    }
}

/// The Hit Zone of a hitbox, its body part and the stance its collider is currently shaped for.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub zone: HitZone,
    part: BodyPart,
    stance: Stance,
}

/// A body part's shape relative to the feet, facing -Z (yaw 0), +X to the right.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PartShape {
    Sphere {
        centre: Vec3,
        radius: f32,
    },
    Box {
        centre: Vec3,
        size: Vec3,
    },
    /// A capsule whose segment runs from `from` to `to`.
    Limb {
        from: Vec3,
        to: Vec3,
        radius: f32,
    },
}

impl PartShape {
    fn collider(self) -> Collider {
        match self {
            PartShape::Sphere { radius, .. } => Collider::sphere(radius),
            PartShape::Box { size, .. } => Collider::cuboid(size.x, size.y, size.z),
            PartShape::Limb { from, to, radius } => Collider::capsule(radius, from.distance(to)),
        }
    }

    /// Collider centre and rotation relative to the feet.
    fn placement(self) -> (Vec3, Quat) {
        match self {
            PartShape::Sphere { centre, .. } | PartShape::Box { centre, .. } => {
                (centre, Quat::IDENTITY)
            }
            PartShape::Limb { from, to, .. } => (
                (from + to) / 2.0,
                Quat::from_rotation_arc(Vec3::Y, (to - from).normalize()),
            ),
        }
    }
}

/// `v` mirrored to the player's left side.
fn left(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.y, v.z)
}

/// Body proportions per stance. Upright stances stack head, chest, belly and legs, with both
/// arms reaching forward to hold a rifle (the support hand further out). Prone lies along the
/// facing direction, matching the prone capsule drawn in `player/remote.rs`: head in front
/// of the feet position, legs trailing 1.4 m behind it, propped up on the elbows.
fn part_shape(part: BodyPart, stance: Stance) -> PartShape {
    let limb = |from, to, radius| PartShape::Limb { from, to, radius };
    match stance {
        Stance::Standing => match part {
            BodyPart::Head => PartShape::Sphere {
                centre: Vec3::new(0.0, 1.65, 0.0),
                radius: HEAD_RADIUS,
            },
            BodyPart::UpperTorso => PartShape::Box {
                centre: Vec3::new(0.0, 1.30, 0.0),
                size: UPPER_TORSO,
            },
            BodyPart::LowerTorso => PartShape::Box {
                centre: Vec3::new(0.0, 0.975, 0.0),
                size: LOWER_TORSO,
            },
            BodyPart::RightArm => limb(
                Vec3::new(0.27, 1.40, 0.0),
                Vec3::new(0.14, 1.22, -0.38),
                ARM_RADIUS,
            ),
            BodyPart::LeftArm => limb(
                Vec3::new(-0.27, 1.40, 0.0),
                Vec3::new(-0.06, 1.28, -0.52),
                ARM_RADIUS,
            ),
            BodyPart::RightLeg | BodyPart::LeftLeg => {
                let (hip, foot) = (Vec3::new(0.11, 0.80, 0.0), Vec3::new(0.11, 0.09, 0.0));
                if part == BodyPart::RightLeg {
                    limb(hip, foot, LEG_RADIUS)
                } else {
                    limb(left(hip), left(foot), LEG_RADIUS)
                }
            }
        },
        // Folded down to 1.2 m: shorter torso, leaning slightly forward, knees bent forward.
        Stance::Crouching => match part {
            BodyPart::Head => PartShape::Sphere {
                centre: Vec3::new(0.0, 1.05, -0.06),
                radius: HEAD_RADIUS,
            },
            BodyPart::UpperTorso => PartShape::Box {
                centre: Vec3::new(0.0, 0.72, -0.03),
                size: Vec3::new(UPPER_TORSO.x, 0.36, UPPER_TORSO.z),
            },
            BodyPart::LowerTorso => PartShape::Box {
                centre: Vec3::new(0.0, 0.45, 0.0),
                size: Vec3::new(LOWER_TORSO.x, 0.18, LOWER_TORSO.z),
            },
            BodyPart::RightArm => limb(
                Vec3::new(0.27, 0.84, -0.03),
                Vec3::new(0.14, 0.66, -0.40),
                ARM_RADIUS,
            ),
            BodyPart::LeftArm => limb(
                Vec3::new(-0.27, 0.84, -0.03),
                Vec3::new(-0.06, 0.72, -0.54),
                ARM_RADIUS,
            ),
            BodyPart::RightLeg | BodyPart::LeftLeg => {
                let (hip, foot) = (Vec3::new(0.12, 0.38, 0.02), Vec3::new(0.14, 0.10, -0.30));
                if part == BodyPart::RightLeg {
                    limb(hip, foot, LEG_RADIUS)
                } else {
                    limb(left(hip), left(foot), LEG_RADIUS)
                }
            }
        },
        Stance::Prone => match part {
            BodyPart::Head => PartShape::Sphere {
                centre: Vec3::new(0.0, 0.25, -0.15),
                radius: HEAD_RADIUS,
            },
            BodyPart::UpperTorso => PartShape::Box {
                centre: Vec3::new(0.0, 0.13, 0.20),
                size: Vec3::new(UPPER_TORSO.x, UPPER_TORSO.z, UPPER_TORSO.y),
            },
            BodyPart::LowerTorso => PartShape::Box {
                centre: Vec3::new(0.0, 0.12, 0.525),
                size: Vec3::new(LOWER_TORSO.x, LOWER_TORSO.z, LOWER_TORSO.y),
            },
            BodyPart::RightArm => limb(
                Vec3::new(0.24, 0.16, 0.05),
                Vec3::new(0.12, 0.12, -0.38),
                ARM_RADIUS,
            ),
            BodyPart::LeftArm => limb(
                Vec3::new(-0.24, 0.16, 0.05),
                Vec3::new(-0.10, 0.12, -0.50),
                ARM_RADIUS,
            ),
            BodyPart::RightLeg | BodyPart::LeftLeg => {
                let (hip, foot) = (Vec3::new(0.11, 0.09, 0.72), Vec3::new(0.16, 0.09, 1.31));
                if part == BodyPart::RightLeg {
                    limb(hip, foot, LEG_RADIUS)
                } else {
                    limb(left(hip), left(foot), LEG_RADIUS)
                }
            }
        },
    }
}

/// Spawns the (disabled) hitboxes of a new server Player; respawning enables them.
pub(super) fn spawn_hitboxes(commands: &mut Commands, player: Entity) {
    let stance = Stance::default();
    for part in BODY_PARTS {
        commands.spawn((
            Name::new(format!("{part:?} hitbox")),
            Hitbox {
                zone: part.zone(),
                part,
                stance,
            },
            HitboxOf(player),
            part_shape(part, stance).collider(),
            CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
            ColliderDisabled,
        ));
    }
}

/// Enables or disables every hitbox of a player.
pub(super) fn set_hitboxes_enabled(commands: &mut Commands, hitboxes: &Hitboxes, enabled: bool) {
    for &hitbox in &hitboxes.0 {
        if enabled {
            commands.entity(hitbox).remove::<ColliderDisabled>();
        } else {
            commands.entity(hitbox).insert(ColliderDisabled);
        }
    }
}

impl Hitboxes {
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// Poses every hitbox (disabled ones too) from its player's `PlayerPose`, reshaping it on
/// stance changes. Writes `Position`/`Rotation` directly and unconditionally, so the physics
/// step that follows always refreshes the query acceleration structure, including for a
/// hitbox re-enabled this tick; shots resolved in `Update` then see this tick's poses.
pub(super) fn sync_hitboxes(
    players: Query<&PlayerPose>,
    mut hitboxes: Query<(
        &HitboxOf,
        &mut Hitbox,
        &mut Collider,
        &mut Position,
        &mut Rotation,
    )>,
) {
    for (owner, mut hitbox, mut collider, mut position, mut rotation) in &mut hitboxes {
        let Ok(pose) = players.get(owner.0) else {
            continue;
        };
        let shape = part_shape(hitbox.part, pose.stance);
        if hitbox.stance != pose.stance {
            hitbox.stance = pose.stance;
            *collider = shape.collider();
        }
        let yaw = Quat::from_rotation_y(pose.yaw);
        let (offset, local_rotation) = shape.placement();
        *position = Position(pose.position + yaw * offset);
        *rotation = Rotation(yaw * local_rotation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lowest and highest point of a part, relative to the feet.
    fn vertical_extent(shape: PartShape) -> (f32, f32) {
        match shape {
            PartShape::Sphere { centre, radius } => (centre.y - radius, centre.y + radius),
            PartShape::Box { centre, size } => (centre.y - size.y / 2.0, centre.y + size.y / 2.0),
            PartShape::Limb { from, to, radius } => {
                (from.y.min(to.y) - radius, from.y.max(to.y) + radius)
            }
        }
    }

    #[test]
    fn every_stance_fits_its_body_height_and_stands_on_the_ground() {
        for stance in [Stance::Standing, Stance::Crouching, Stance::Prone] {
            let (bottom, top) = BODY_PARTS
                .iter()
                .map(|&part| vertical_extent(part_shape(part, stance)))
                .fold((f32::MAX, f32::MIN), |(lo, hi), (b, t)| {
                    (lo.min(b), hi.max(t))
                });
            assert!(bottom.abs() < 0.02, "{stance:?} floats or sinks: {bottom}");
            assert!(
                top <= stance.body_height() + 1e-4,
                "{stance:?} is taller than its body: {top}"
            );
        }
    }

    #[test]
    fn limb_capsules_span_their_endpoints() {
        let shape = part_shape(BodyPart::RightLeg, Stance::Standing);
        let PartShape::Limb { from, to, .. } = shape else {
            panic!("legs are limbs");
        };
        let (centre, rotation) = shape.placement();
        let half = from.distance(to) / 2.0;
        assert!((centre + rotation * Vec3::Y * half).distance(to) < 1e-5);
        assert!((centre - rotation * Vec3::Y * half).distance(from) < 1e-5);
    }
}
