//! Ammo packs: a player who dies drops one for everyone to see, and a living player with the
//! Scavenger Perk who walks over it picks it up and has their ammo refilled. Untouched packs
//! disappear after `GameData::ammo_pack.lifetime_secs`.

use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::*;

use crate::data::GameData;
use crate::gear::{AmmoPack, EquippedPerks};
use crate::net::protocol::{AmmoRefill, EventChannel};
use crate::shared::{Health, PlayerPose};

/// A pack's expiry, as elapsed time. Server-only.
#[derive(Component, Debug)]
pub(super) struct PackExpiry(Duration);

/// A pack may be picked up from this far above or below it, in metres.
const PICKUP_HEIGHT: f32 = 2.0;

/// Drops a pack at `position`.
pub(super) fn drop_ammo_pack(
    commands: &mut Commands,
    data: &GameData,
    time: &Time,
    position: Vec3,
) {
    commands.spawn((
        Name::new("Ammo pack"),
        AmmoPack { position },
        PackExpiry(time.elapsed() + Duration::from_secs_f32(data.ammo_pack.lifetime_secs)),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Expires old packs and hands the rest to the first Scavenger standing on one.
pub(super) fn tick_packs(
    packs: Query<(Entity, &AmmoPack, &PackExpiry)>,
    players: Query<(&PlayerPose, &Health, &EquippedPerks, &ControlledBy)>,
    mut refills: Query<&mut MessageSender<AmmoRefill>>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (pack, AmmoPack { position }, expiry) in &packs {
        if time.elapsed() >= expiry.0 {
            commands.entity(pack).try_despawn();
            continue;
        }
        let taker = players.iter().find(|(pose, health, perks, _)| {
            !health.is_dead()
                && data.perk_effects(perks.0.iter()).scavenger
                && pose.position.xz().distance(position.xz()) <= data.ammo_pack.pickup_radius
                && (pose.position.y - position.y).abs() <= PICKUP_HEIGHT
        });
        let Some((.., link)) = taker else {
            continue;
        };
        if let Ok(mut sender) = refills.get_mut(link.owner) {
            sender.send::<EventChannel>(AmmoRefill);
        }
        commands.entity(pack).try_despawn();
        info!("An ammo pack at {position} was picked up");
    }
}
