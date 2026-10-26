//! Loadouts: which Default Loadout the local player spawns with, and the Loadout screen.
//! The choice is saved with `Settings`. CoD rule: a change applies on the next spawn, or at
//! once if made within `GameData::loadout_swap_secs` of spawning.

mod screen;

use bevy::prelude::*;

use crate::data::{DefaultLoadout, GameData};
use crate::flow::{AppState, Settings};
use crate::net::protocol::Respawn;
use crate::player::LocalLife;
use crate::shared::FromServer;

pub struct LoadoutPlugin;

impl Plugin for LoadoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChosenLoadout>()
            .init_resource::<SpawnClock>()
            .add_plugins(screen::LoadoutScreenPlugin)
            .add_systems(
                Update,
                (
                    load_from_settings.run_if(resource_added::<Settings>),
                    track_spawns,
                ),
            )
            .add_systems(OnExit(AppState::InGame), reset_spawn_clock);
    }
}

/// The Default Loadout the local player spawns with: an index into
/// `GameData::default_loadouts`.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChosenLoadout(pub usize);

impl ChosenLoadout {
    /// The chosen Loadout; an out-of-range index (e.g. a stale settings file) means the first.
    pub fn get(self, data: &GameData) -> &DefaultLoadout {
        data.default_loadouts
            .get(self.0)
            .unwrap_or(&data.default_loadouts[0])
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

fn load_from_settings(settings: Res<Settings>, mut chosen: ResMut<ChosenLoadout>) {
    chosen.set_if_neq(ChosenLoadout(settings.loadout));
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
