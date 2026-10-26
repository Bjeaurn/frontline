//! Server-side combat (Dedicated and Host): hitboxes, hitscan shots, damage, health regen,
//! death and respawn. The server is the only writer of `Health` and `PlayerStats`
//! (docs/adr/0002). No teams yet: everyone is everyone's enemy.

mod damage;
mod health;
mod hitbox;
mod shots;
mod spawn;

use bevy::prelude::*;

pub use health::PoseGate;

pub struct CombatServerPlugin;

impl Plugin for CombatServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<shots::PlayerHit>()
            .add_observer(health::init_player)
            // Respawn before syncing, so a respawned player's hitboxes are posed at the spawn.
            .add_systems(
                FixedUpdate,
                (
                    health::regenerate,
                    health::respawn_players,
                    hitbox::sync_hitboxes,
                )
                    .chain(),
            )
            // Network messages are only buffered for the frame they arrive in, so shots are
            // resolved every frame rather than in the fixed loop.
            .add_systems(Update, (shots::resolve_shots, health::apply_hits).chain());
    }
}
