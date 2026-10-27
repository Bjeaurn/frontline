//! Server-side combat (Dedicated and Host): Teams, hitboxes, hitscan shots, damage, health
//! regen, death and respawn, and the Team Deathmatch Match flow. The server is the only writer
//! of `Health`, `PlayerStats`, `PlayerTeam` and `MatchState` (docs/adr/0002).
//!
//! Added to every build, but it only acts while this process runs a server
//! (`net::server_running`); the Match starts and ends with the server.

mod damage;
mod health;
mod hitbox;
mod match_flow;
mod shots;
mod spawn;
mod teams;

use bevy::prelude::*;

pub use health::PoseGate;
pub use match_flow::{ServerMap, ServerRules};

use crate::maps::map_ready;
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
                    health::respawn_players.run_if(map_ready),
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
                    teams::choose_teams,
                    shots::resolve_shots,
                    health::apply_hits,
                )
                    .chain()
                    .run_if(server_running),
            );
    }
}
