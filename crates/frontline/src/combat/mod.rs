//! Server-side combat (Dedicated and Host): Teams, hitboxes, hitscan shots, damage, health
//! regen, death and respawn, the Match flow (Rotation, scores, end) and the objective Game Modes.
//! The server is the only writer of `Health`, `PlayerStats`, `PlayerTeam`, `MatchState` and
//! `ModeView` (docs/adr/0002).
//!
//! Added to every build, but it only acts while this process runs a server
//! (`net::server_running`); the Match starts and ends with the server.

mod damage;
mod health;
mod hitbox;
mod match_flow;
mod mode_systems;
mod packs;
mod shots;
mod spawn;
mod teams;
mod xp;

use bevy::prelude::*;

pub use health::PoseGate;

use crate::maps::match_map_ready;
use crate::net::server_running;

pub struct CombatServerPlugin;

impl Plugin for CombatServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<shots::PlayerHit>()
            .add_observer(health::init_player)
            .add_observer(match_flow::start_match)
            .add_observer(match_flow::end_match)
            // Respawn before syncing, so a respawned player's hitboxes are posed at the spawn.
            // Respawns wait for the Map: its spawn points and colliders.
            .add_systems(
                FixedUpdate,
                (
                    health::regenerate,
                    match_flow::tick_match,
                    mode_systems::init_mode.run_if(mode_systems::is_pending),
                    (
                        mode_systems::tick_dom,
                        mode_systems::tick_hq,
                        mode_systems::tick_sd,
                    ),
                    mode_systems::publish_view,
                    health::respawn_players.run_if(match_map_ready),
                    packs::tick_packs,
                    hitbox::sync_hitboxes,
                )
                    .chain()
                    .run_if(server_running),
            )
            // Network messages are only buffered for the frame they arrive in, so team choices
            // and shots are handled every frame rather than in the fixed loop.
            .add_systems(
                Update,
                (
                    match_flow::change_map,
                    mode_systems::receive_use,
                    teams::choose_teams,
                    shots::resolve_shots,
                    health::apply_hits,
                )
                    .chain()
                    .run_if(server_running),
            );
    }
}
