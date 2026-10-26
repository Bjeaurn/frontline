//! Types shared between the player controller, rendering and netcode.

use avian3d::prelude::PhysicsLayer;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Body posture; changes speed, eye height and hitbox height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum Stance {
    #[default]
    Standing,
    Crouching,
    Prone,
}

impl Stance {
    /// Eye height above the feet, in metres.
    pub fn eye_height(self) -> f32 {
        match self {
            Stance::Standing => 1.65,
            Stance::Crouching => 1.05,
            Stance::Prone => 0.35,
        }
    }

    /// Full body height, in metres (used for collider and hitbox).
    pub fn body_height(self) -> f32 {
        match self {
            Stance::Standing => 1.8,
            Stance::Crouching => 1.2,
            Stance::Prone => 0.5,
        }
    }
}

/// Authoritative-from-client pose of a player (docs/adr/0002).
/// `position` is the feet position; yaw/pitch in radians.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Reflect)]
pub struct PlayerPose {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub stance: Stance,
}

/// Display name of a player, chosen on the client.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Reflect)]
pub struct PlayerName(pub String);

/// The player entity controlled by this process (client or Host). Exactly zero or one exists.
/// Its `PlayerPose` is kept up to date by the controller and sent to the server by netcode.
#[derive(Component, Debug, Default)]
pub struct LocalPlayer;

/// A player controlled by another process, shown from replicated `PlayerPose`.
/// Inserted by netcode on the client; rendering reacts to it.
#[derive(Component, Debug, Default)]
pub struct RemotePlayer;

/// Current health of a player; the server is the only writer. Dead at `<= 0`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Reflect)]
pub struct Health(pub f32);

impl Health {
    pub fn is_dead(self) -> bool {
        self.0 <= 0.0
    }
}

/// Kills and deaths in the current Match; the server is the only writer.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Reflect)]
pub struct PlayerStats {
    pub kills: u32,
    pub deaths: u32,
}

/// Physics layers. Arena geometry is `World`; player hitboxes are `Hitbox` and collide with
/// nothing. Movement queries use `World` only; shots query `World` and `Hitbox`.
#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum GameLayer {
    #[default]
    World,
    Hitbox,
}

/// The server's Player entity for this process's own player: its replicated copy on a Client,
/// or the server entity itself on a Host. Carries the authoritative `Health` and `PlayerStats`.
/// Inserted by netcode on Client and Host.
#[derive(Component, Debug, Default)]
pub struct LocalPlayerReplica;

/// A network message from the server, re-emitted by client netcode as a Bevy message so any
/// number of client systems can read it.
#[derive(Message, Clone, Debug)]
pub struct FromServer<M>(pub M);

/// A network message for the server; client netcode sends it on the matching channel.
#[derive(Message, Clone, Debug)]
pub struct ToServer<M>(pub M);
