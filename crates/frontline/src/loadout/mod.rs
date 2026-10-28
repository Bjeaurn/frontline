//! Loadouts: the Loadout screen (Default Loadouts and the Profile's custom ones), the Create a
//! Class editor (from the main menu too), and the instant-swap window. The choice and the custom Loadouts live in the
//! `Profile`. CoD rule: a change applies on the next spawn, or at once if made within
//! `GameData::loadout_swap_secs` of spawning.

mod editor;
mod screen;
mod stats;

use bevy::prelude::*;

use crate::data::GameData;
use crate::flow::AppState;
use crate::net::protocol::Respawn;
use crate::player::LocalLife;
use crate::shared::FromServer;

pub struct LoadoutPlugin;

impl Plugin for LoadoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnClock>()
            .add_plugins((screen::LoadoutScreenPlugin, editor::ClassEditorPlugin))
            .add_systems(Update, track_spawns)
            .add_systems(OnExit(AppState::InGame), reset_spawn_clock);
    }
}

/// When the local player last spawned, for the instant-swap window.
#[derive(Resource, Default, Debug)]
pub struct SpawnClock {
    /// `Time::elapsed_secs_f64` of the latest spawn this session.
    spawned_at: Option<f64>,
}

impl SpawnClock {
    /// Whether a Loadout chosen now swaps the held Weapon at once: alive and still within
    /// the swap window of the latest spawn.
    pub fn swap_open(&self, now: f64, life: &LocalLife, data: &GameData) -> bool {
        !life.is_dead()
            && self
                .spawned_at
                .is_some_and(|at| now - at <= f64::from(data.loadout_swap_secs))
    }
}

fn track_spawns(
    time: Res<Time>,
    mut respawns: MessageReader<FromServer<Respawn>>,
    mut clock: ResMut<SpawnClock>,
) {
    if respawns.read().count() > 0 {
        clock.spawned_at = Some(time.elapsed_secs_f64());
    }
}

fn reset_spawn_clock(mut clock: ResMut<SpawnClock>) {
    clock.spawned_at = None;
}
