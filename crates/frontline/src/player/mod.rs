//! The local first-person player and the visuals of remote players.

mod controller;
mod hud;
pub mod input;
mod remote;

use bevy::prelude::*;

/// Body radius used for the local collider and remote capsules, in metres.
const BODY_RADIUS: f32 = 0.3;

/// Spawns the `LocalPlayer` with a first-person camera and drives it from keyboard and mouse.
/// Client and Host only. Movement tuning lives at the top of `player/controller.rs`.
pub struct LocalControllerPlugin;

impl Plugin for LocalControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<input::Bindings>()
            .init_resource::<input::LookSensitivity>()
            .init_resource::<controller::MouseCaptured>()
            .init_resource::<controller::StanceShapes>()
            .add_systems(
                Startup,
                (controller::spawn_local_player, hud::spawn_crosshair),
            )
            .add_systems(
                Update,
                (
                    controller::grab_cursor,
                    controller::mouse_look,
                    controller::change_stance,
                    controller::move_player,
                    controller::update_eye,
                    controller::apply_horizontal_fov,
                )
                    .chain(),
            );
    }
}

/// Draws every `RemotePlayer` as a tinted capsule following its `PlayerPose`. Client and Host only.
pub struct RemotePlayerVisualsPlugin;

impl Plugin for RemotePlayerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, remote::load_remote_assets)
            .add_observer(remote::attach_remote_visuals)
            .add_systems(Update, remote::follow_remote_poses);
    }
}
