//! Frontline: one binary for the windowed game (main menu, Client or Host) and the headless
//! Dedicated Server.

mod assets;
mod atmosphere;
mod chat;
mod class;
mod cli;
mod cod4;
mod combat;
mod data;
mod flow;
mod gear;
mod loadout;
mod maps;
mod match_hud;
mod materials;
mod menus;
mod modes;
mod net;
mod player;
mod profile;
mod progression;
mod rear_cue;
mod rotation;
mod rules;
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
        // Before `MapsPlugin`: map faces use the material library (and CoD4 overrides).
        app.add_plugins((
            assets::EmbeddedAssetsPlugin,
            materials::MaterialsPlugin,
            cod4::Cod4Plugin,
        ));
    }
    app.add_plugins(maps::MapsPlugin {
        idle: cli.map(),
        write_trenchbroom_config: cli.write_trenchbroom_config(),
    });
    let config = match cli.server_config() {
        Ok(config) => config,
        Err(err) => {
            error!("{err}");
            return AppExit::error();
        }
    };
    let checked = config.validate(
        app.world().resource::<data::GameData>(),
        app.world().resource::<maps::MapRegistry>(),
    );
    if let Err(err) = checked {
        error!("{err}");
        return AppExit::error();
    }
    // The idle Map (the main menu's backdrop) must exist too.
    let registry = app.world().resource::<maps::MapRegistry>();
    if registry.get(&cli.map()).is_none() {
        error!(
            "Unknown Map \"{}\" (--map); known: {}",
            cli.map(),
            registry.known_ids()
        );
        return AppExit::error();
    }
    app.insert_resource(config);
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
            rear_cue::RearCuePlugin,
            player::RemotePlayerVisualsPlugin,
            weapon::WeaponClientPlugin,
            match_hud::MatchHudPlugin,
            chat::ChatPlugin,
            modes::ModeVisualsPlugin,
            loadout::LoadoutPlugin,
            profile::ProfilePlugin,
            progression::ProgressionPlugin,
        ));
    }
    app.add_plugins((combat::CombatServerPlugin, net::NetPlugin));
    app.run()
}
