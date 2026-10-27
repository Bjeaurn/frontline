//! Wire protocol shared by client and server: replicated components, messages and channels.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::WeaponId;
use crate::shared::{
    Health, MatchState, PlayerName, PlayerPose, PlayerStats, PlayerTeam, TELEPORT_DISTANCE, Team,
};

/// Netcode protocol id; client and server must agree on it.
pub const PROTOCOL_ID: u64 = 0x4652_4f4e_544c_494e;

/// Netcode private key. A shared zero key: LAN play trusts every client (docs/adr/0002).
pub const PRIVATE_KEY: [u8; 32] = [0; 32];

/// Seconds without packets before either side drops the connection. Generous because a Host
/// renders in the same loop as its server: a multi-second hitch (shader compile, window drag,
/// alt-tab) must not kick everyone.
pub const CONNECTION_TIMEOUT_SECS: i32 = 10;

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

/// Client -> server (`ControlChannel`): put me on this team. Accepted any time; a living
/// player who switches is killed without anyone getting credit, like a suicide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChooseTeam(pub Team);

/// Unreliable channel where only the newest pose is kept.
pub struct PoseChannel;

/// Reliable, ordered channel for session control messages.
pub struct ControlChannel;

/// Reliable, unordered channel for shots (client -> server).
pub struct ShotChannel;

/// Reliable, ordered channel for gameplay events (server -> client).
pub struct EventChannel;

/// Unreliable channel for cosmetic effects such as tracers (server -> client).
pub struct EffectChannel;

/// Client -> server: the local player fired one round. `direction` already includes spread
/// and recoil (clients are trusted, docs/adr/0002); the server decides what it hit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FireShot {
    pub weapon: WeaponId,
    pub origin: Vec3,
    pub direction: Vec3,
}

/// Server -> all clients except the shooter: draw a tracer for someone else's shot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShotTracer {
    pub shooter: PeerId,
    pub origin: Vec3,
    pub end: Vec3,
}

/// Server -> shooter: one of their shots hit a player (hitmarker).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitConfirm {
    pub headshot: bool,
    pub kill: bool,
}

/// Server -> victim: they took damage from a shooter standing at `from`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Damaged {
    pub amount: f32,
    pub from: Vec3,
}

/// Server -> all clients: a kill-feed line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KillFeed {
    pub killer: String,
    pub victim: String,
    pub weapon: WeaponId,
    pub headshot: bool,
}

/// Server -> the respawning player: move the local player here and resume control.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Respawn {
    pub position: Vec3,
    pub yaw: f32,
}

pub(super) fn register(app: &mut App) {
    app.component::<PlayerPose>()
        .replicate()
        .add_interpolation_with(lerp_pose);
    app.component::<PlayerName>().replicate();
    app.component::<PlayerOwner>().replicate();
    app.component::<Health>().replicate();
    app.component::<PlayerStats>().replicate();
    app.component::<PlayerTeam>().replicate();
    app.component::<MatchState>().replicate();

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
    app.register_message::<ChooseTeam>()
        .add_direction(NetworkDirection::ClientToServer);

    app.add_channel::<ShotChannel>(ChannelSettings {
        mode: ChannelMode::UnorderedReliable(ReliableSettings::default()),
        ..default()
    })
    .add_direction(NetworkDirection::ClientToServer);
    app.add_channel::<EventChannel>(ChannelSettings {
        mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
        ..default()
    })
    .add_direction(NetworkDirection::ServerToClient);
    app.add_channel::<EffectChannel>(ChannelSettings {
        mode: ChannelMode::UnorderedUnreliable,
        ..default()
    })
    .add_direction(NetworkDirection::ServerToClient);

    app.register_message::<FireShot>()
        .add_direction(NetworkDirection::ClientToServer);
    app.register_message::<ShotTracer>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<HitConfirm>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<Damaged>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<KillFeed>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<Respawn>()
        .add_direction(NetworkDirection::ServerToClient);
}

/// Interpolates between two confirmed poses: linear position and pitch, shortest-arc yaw,
/// and the stance switches halfway. A jump over `TELEPORT_DISTANCE` is a teleport and lands
/// on the end pose at once.
fn lerp_pose(start: PlayerPose, end: PlayerPose, t: f32) -> PlayerPose {
    if start.position.distance(end.position) > TELEPORT_DISTANCE {
        return end;
    }
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
        PlayerPose {
            yaw,
            stance,
            ..default()
        }
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

    #[test]
    fn teleports_land_at_once_and_short_moves_glide() {
        let at = |x: f32| PlayerPose {
            position: Vec3::new(x, 0.0, 0.0),
            ..default()
        };
        let respawn = lerp_pose(at(0.0), at(TELEPORT_DISTANCE + 20.0), 0.1);
        assert_eq!(respawn, at(TELEPORT_DISTANCE + 20.0));
        let step = lerp_pose(at(0.0), at(1.0), 0.25);
        assert!((step.position.x - 0.25).abs() < 1e-6);
    }
}
