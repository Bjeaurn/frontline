//! Wire protocol shared by client and server: replicated components, messages and channels.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

use crate::shared::{PlayerName, PlayerPose};

/// Netcode protocol id; client and server must agree on it.
pub const PROTOCOL_ID: u64 = 0x4652_4f4e_544c_494e;

/// Netcode private key. A shared zero key: LAN play trusts every client (docs/adr/0002).
pub const PRIVATE_KEY: [u8; 32] = [0; 32];

/// Peer that owns (controls) a replicated player entity.
/// Lets each client tell its own replicated copy apart from everyone else's.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerOwner(pub PeerId);

/// Client -> server: the sender's current pose, sent every fixed tick.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseUpdate(pub PlayerPose);

/// Client -> server: sent once after connecting to introduce the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub name: String,
}

/// Unreliable channel where only the newest pose is kept.
pub struct PoseChannel;

/// Reliable, ordered channel for session control messages.
pub struct ControlChannel;

pub(super) fn register(app: &mut App) {
    app.component::<PlayerPose>()
        .replicate()
        .add_interpolation_with(lerp_pose);
    app.component::<PlayerName>().replicate();
    app.component::<PlayerOwner>().replicate();

    app.add_channel::<PoseChannel>(ChannelSettings {
        mode: ChannelMode::SequencedUnreliable,
        ..default()
    })
    .add_direction(NetworkDirection::ClientToServer);
    app.add_channel::<ControlChannel>(ChannelSettings {
        mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
        ..default()
    })
    .add_direction(NetworkDirection::ClientToServer);

    app.register_message::<PoseUpdate>()
        .add_direction(NetworkDirection::ClientToServer);
    app.register_message::<Hello>()
        .add_direction(NetworkDirection::ClientToServer);
}

/// Interpolates between two confirmed poses: linear position and pitch, shortest-arc yaw,
/// and the stance switches halfway.
fn lerp_pose(start: PlayerPose, end: PlayerPose, t: f32) -> PlayerPose {
    let yaw_delta = (end.yaw - start.yaw + PI).rem_euclid(TAU) - PI;
    PlayerPose {
        position: start.position.lerp(end.position, t),
        yaw: start.yaw + yaw_delta * t,
        pitch: start.pitch + (end.pitch - start.pitch) * t,
        stance: if t < 0.5 { start.stance } else { end.stance },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::Stance;

    fn pose(yaw: f32, stance: Stance) -> PlayerPose {
        PlayerPose { yaw, stance, ..default() }
    }

    #[test]
    fn yaw_takes_the_short_way_across_plus_minus_pi() {
        let (a, b) = (pose(3.0, Stance::Standing), pose(-3.0, Stance::Standing));
        assert!((lerp_pose(a, b, 0.5).yaw - PI).abs() < 1e-4);
        assert!((lerp_pose(b, a, 0.5).yaw + PI).abs() < 1e-4);
        // Lands on the end orientation (modulo a full turn).
        let off = (lerp_pose(a, b, 1.0).yaw - b.yaw).rem_euclid(TAU);
        assert!(off < 1e-4 || TAU - off < 1e-4);
    }

    #[test]
    fn stance_switches_halfway() {
        let (a, b) = (pose(0.0, Stance::Standing), pose(0.0, Stance::Prone));
        assert_eq!(lerp_pose(a, b, 0.49).stance, Stance::Standing);
        assert_eq!(lerp_pose(a, b, 0.5).stance, Stance::Prone);
    }
}
