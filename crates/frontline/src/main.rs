//! Frontline: one binary for the windowed game (main menu, Client or Host) and the headless
//! Dedicated Server.

mod assets;
mod atmosphere;
mod cli;
mod combat;
mod data;
mod flow;
mod loadout;
mod maps;
mod match_hud;
mod materials;
mod menus;
mod net;
mod player;
mod shared;
mod ui;
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
    let tick_duration = Duration::from_secs_f64(1.0 / TICK_HZ);

    let mut app = App::new();
    app.insert_resource(mode.clone());
    // Asset sources must exist before `AssetPlugin` is added.
    maps::register_private_source(&mut app);
    match mode {
        Mode::Dedicated { .. } => {
            app.add_plugins((
                MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(tick_duration)),
                bevy::log::LogPlugin::default(),
                TransformPlugin,
                bevy::state::app::StatesPlugin,
                ServerPlugins { tick_duration },
            ));
        }
        // Both sides up front: the main menu can Host or Join at any time.
        Mode::Windowed { .. } => {
            app.add_plugins((
                DefaultPlugins.set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Frontline".into(),
                        ..default()
                    }),
                    ..default()
                }),
                ServerPlugins { tick_duration },
                ClientPlugins { tick_duration },
            ));
        }
    }
    app.add_plugins((data::DataPlugin, world::WorldPlugin));
    let windowed = matches!(mode, Mode::Windowed { .. });
    if windowed {
        // Before `MapsPlugin`: map faces use the material library.
        app.add_plugins((assets::EmbeddedAssetsPlugin, materials::MaterialsPlugin));
    }
    app.add_plugins(maps::MapsPlugin {
        idle: cli.map(),
        write_trenchbroom_config: cli.write_trenchbroom_config(),
    });
    let presets = &app.world().resource::<data::GameData>().match_rules;
    if !presets.contains_key(cli.rules()) {
        let mut known: Vec<&str> = presets.keys().map(String::as_str).collect();
        known.sort_unstable();
        error!(
            "Unknown Match Rules preset \"{}\" (--rules); known: {}",
            cli.rules(),
            known.join(", ")
        );
        return AppExit::error();
    }
    let maps = app.world().resource::<maps::MapRegistry>();
    if maps.get(&cli.map()).is_none() {
        error!(
            "Unknown Map \"{}\" (--map); known: {}",
            cli.map(),
            maps.known_ids()
        );
        return AppExit::error();
    }
    app.insert_resource(combat::ServerRules(cli.rules().to_owned()))
        .insert_resource(combat::ServerMap(cli.map()));
    if windowed {
        app.add_plugins(atmosphere::AtmospherePlugin {
            look_override: cli.look().map(str::to_owned),
        });
        let looks = app.world().resource::<atmosphere::Looks>();
        if let Some(look) = cli.look().filter(|look| !looks.contains(look)) {
            error!(
                "Unknown look \"{look}\" (--look); known: {}",
                looks.known_names()
            );
            return AppExit::error();
        }
        app.add_plugins((
            flow::FlowPlugin,
            ui::UiPlugin,
            menus::MenusPlugin,
            player::LocalControllerPlugin,
            player::RemotePlayerVisualsPlugin,
            weapon::WeaponClientPlugin,
            match_hud::MatchHudPlugin,
            loadout::LoadoutPlugin,
        ));
    }
    app.add_plugins((combat::CombatServerPlugin, net::NetPlugin));
    app.run()
}
