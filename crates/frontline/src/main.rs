//! Frontline: one binary for client, Host (listen server + local player) and Dedicated Server.

mod cli;
mod combat;
mod data;
mod match_hud;
mod net;
mod player;
mod shared;
mod weapon;
mod world;

use std::time::Duration;

use bevy::prelude::*;
use clap::Parser;
use lightyear::prelude::client::ClientPlugins;
use lightyear::prelude::server::ServerPlugins;

use crate::cli::{Cli, Mode};

/// Simulation and network tick rate, shared by client and server.
pub const TICK_HZ: f64 = 64.0;

fn main() -> AppExit {
    let cli = Cli::parse();
    let mode = cli.mode();

    let mut app = App::new();
    match mode {
        Mode::Dedicated { .. } => {
            app.add_plugins((
                MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                    Duration::from_secs_f64(1.0 / TICK_HZ),
                )),
                bevy::log::LogPlugin::default(),
                TransformPlugin,
                bevy::state::app::StatesPlugin,
            ));
        }
        Mode::Client { .. } | Mode::Host { .. } => {
            app.add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("Frontline ({})", mode.label()),
                    ..default()
                }),
                ..default()
            }));
        }
    }
    let tick_duration = Duration::from_secs_f64(1.0 / TICK_HZ);
    match mode {
        Mode::Client { .. } => app.add_plugins(ClientPlugins { tick_duration }),
        Mode::Host { .. } => app.add_plugins((
            ServerPlugins { tick_duration },
            ClientPlugins { tick_duration },
        )),
        Mode::Dedicated { .. } => app.add_plugins(ServerPlugins { tick_duration }),
    };
    app.add_plugins((data::DataPlugin, world::WorldPlugin));
    if !matches!(mode, Mode::Dedicated { .. }) {
        app.add_plugins((
            player::LocalControllerPlugin,
            player::RemotePlayerVisualsPlugin,
            weapon::WeaponClientPlugin,
            match_hud::MatchHudPlugin,
        ));
    }
    if !matches!(mode, Mode::Client { .. }) {
        app.add_plugins(combat::CombatServerPlugin);
    }
    app.insert_resource(mode);
    app.add_plugins(net::NetPlugin);
    app.run()
}
