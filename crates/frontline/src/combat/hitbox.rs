//! Server-side hitboxes: a head sphere and a body capsule per player, as top-level entities
//! posed from `PlayerPose` every fixed tick.
//!
//! They are deliberately not children of the Player entity and never read its `Transform`: on
//! a Host that Transform belongs to the remote-player visuals, which smooth it and lag behind.

use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::{Collider, ColliderDisabled, CollisionLayers, LayerMask, Position, Rotation};
use bevy::prelude::*;

use super::damage::HitZone;
use crate::shared::{GameLayer, PlayerPose, Stance};

const HEAD_RADIUS: f32 = 0.15;
/// Body radius when upright; matches the movement collider.
const BODY_RADIUS: f32 = 0.3;
/// Length of a prone body (head included) and how far its centre sits behind the feet
/// position; both match the prone capsule drawn in `player/remote.rs`.
const PRONE_LENGTH: f32 = 1.7;
const PRONE_CENTRE_BEHIND: f32 = 0.55;

/// A hitbox entity of the player `HitboxOf.0`.
#[derive(Component, Debug)]
#[relationship(relationship_target = Hitboxes)]
pub struct HitboxOf(pub Entity);

/// The hitbox entities of a player; despawned together with it.
#[derive(Component, Debug)]
#[relationship_target(relationship = HitboxOf, linked_spawn)]
pub struct Hitboxes(Vec<Entity>);

/// The zone of a hitbox and the stance its collider is currently shaped for.
#[derive(Component, Debug)]
pub struct Hitbox {
    pub zone: HitZone,
    stance: Stance,
}

fn shape(zone: HitZone, stance: Stance) -> Collider {
    match (zone, stance) {
        (HitZone::Head, _) => Collider::sphere(HEAD_RADIUS),
        (HitZone::Body, Stance::Prone) => {
            let radius = Stance::Prone.body_height() / 2.0;
            Collider::capsule(radius, PRONE_LENGTH - 2.0 * HEAD_RADIUS - 2.0 * radius)
        }
        (HitZone::Body, Stance::Standing | Stance::Crouching) => {
            let height = stance.body_height() - 2.0 * HEAD_RADIUS;
            Collider::capsule(BODY_RADIUS, height - 2.0 * BODY_RADIUS)
        }
    }
}

/// Hitbox centre and rotation relative to the feet, facing -Z (yaw 0). Upright stances stack
/// the head on the body; prone lies along the facing direction with the head in front.
fn local_placement(zone: HitZone, stance: Stance) -> (Vec3, Quat) {
    match stance {
        Stance::Prone => {
            let y = stance.body_height() / 2.0;
            let back = PRONE_CENTRE_BEHIND + PRONE_LENGTH / 2.0;
            let front = back - PRONE_LENGTH;
            let neck = front + 2.0 * HEAD_RADIUS;
            match zone {
                HitZone::Head => (Vec3::new(0.0, y, front + HEAD_RADIUS), Quat::IDENTITY),
                HitZone::Body => (
                    Vec3::new(0.0, y, (neck + back) / 2.0),
                    Quat::from_rotation_x(FRAC_PI_2),
                ),
            }
        }
        Stance::Standing | Stance::Crouching => {
            let height = stance.body_height();
            match zone {
                HitZone::Head => (Vec3::Y * (height - HEAD_RADIUS), Quat::IDENTITY),
                HitZone::Body => (Vec3::Y * (height / 2.0 - HEAD_RADIUS), Quat::IDENTITY),
            }
        }
    }
}

/// Spawns the (disabled) hitboxes of a new server Player; respawning enables them.
pub(super) fn spawn_hitboxes(commands: &mut Commands, player: Entity) {
    for zone in [HitZone::Head, HitZone::Body] {
        let stance = Stance::default();
        commands.spawn((
            Name::new(format!("{zone:?} hitbox")),
            Hitbox { zone, stance },
            HitboxOf(player),
            shape(zone, stance),
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
    mut hitboxes: Query<(&HitboxOf, &mut Hitbox, &mut Collider, &mut Position, &mut Rotation)>,
) {
    for (owner, mut hitbox, mut collider, mut position, mut rotation) in &mut hitboxes {
        let Ok(pose) = players.get(owner.0) else {
            continue;
        };
        if hitbox.stance != pose.stance {
            hitbox.stance = pose.stance;
            *collider = shape(hitbox.zone, pose.stance);
        }
        let yaw = Quat::from_rotation_y(pose.yaw);
        let (offset, local_rotation) = local_placement(hitbox.zone, pose.stance);
        *position = Position(pose.position + yaw * offset);
        *rotation = Rotation(yaw * local_rotation);
    }
}
