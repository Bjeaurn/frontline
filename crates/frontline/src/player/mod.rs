//! The local first-person player, its HUD, and the visuals of remote players.

mod controller;
mod hud;
pub mod input;
mod life;
mod remote;

use bevy::prelude::*;

pub use controller::{
    ControllerModifiers, EyeCamera, MenuOpen, Motor, MouseCaptured, PITCH_LIMIT,
};
pub use life::LocalLife;

/// Body radius used for the local collider and remote capsules, in metres.
const BODY_RADIUS: f32 = 0.3;

/// Phases of the local controller in `Update`, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerSystems {
    /// Life state, respawn, cursor grab and mouse look. `PlayerPose` yaw/pitch come from here;
    /// weapon recoil adds to them between `Look` and `Move`.
    Look,
    /// Stance and movement; reads `ControllerModifiers`.
    Move,
    /// Camera placement and field of view.
    View,
}

/// Spawns the `LocalPlayer` with a first-person camera and drives it from keyboard and mouse.
/// Client and Host only. Movement tuning lives in `data/movement.ron`.
pub struct LocalControllerPlugin;

impl Plugin for LocalControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<input::Bindings>()
            .init_resource::<input::LookSensitivity>()
            .init_resource::<controller::MouseCaptured>()
            .init_resource::<controller::MenuOpen>()
            .init_resource::<controller::StanceShapes>()
            .init_resource::<LocalLife>()
            .configure_sets(
                Update,
                (
                    ControllerSystems::Look,
                    ControllerSystems::Move,
                    ControllerSystems::View,
                )
                    .chain(),
            )
            .add_systems(Startup, (controller::spawn_local_player, hud::spawn_hud))
            .add_systems(
                Update,
                (
                    (
                        life::track_life,
                        controller::apply_respawn,
                        controller::grab_cursor,
                        controller::mouse_look,
                    )
                        .chain()
                        .in_set(ControllerSystems::Look),
                    (controller::change_stance, controller::move_player)
                        .chain()
                        .in_set(ControllerSystems::Move),
                    (controller::update_eye, controller::apply_horizontal_fov)
                        .chain()
                        .in_set(ControllerSystems::View),
                    (
                        hud::update_damage_vignette,
                        hud::spawn_damage_indicators,
                        hud::update_damage_indicators,
                        hud::add_kill_feed_lines,
                        hud::fade_kill_feed,
                        hud::update_death_screen,
                    )
                        .after(ControllerSystems::View),
                ),
            );
    }
}

/// Draws every `RemotePlayer` as a tinted capsule following its `PlayerPose`, hidden while
/// dead. Client and Host only.
pub struct RemotePlayerVisualsPlugin;

impl Plugin for RemotePlayerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, remote::load_remote_assets)
            .add_observer(remote::attach_remote_visuals)
            .add_systems(
                Update,
                (remote::follow_remote_poses, remote::hide_dead_players),
            );
    }
}
