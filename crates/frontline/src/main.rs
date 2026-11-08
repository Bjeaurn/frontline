//! Frontline: one binary for the windowed game (main menu, Client or Host) and the headless
//! Dedicated Server. The modules live in the library (`lib.rs`).

use std::time::Duration;

use bevy::prelude::*;
use clap::Parser;
use frontline::cli::{Cli, Mode};
use frontline::{
    TICK_HZ, assets, atmosphere, chat, cod4, combat, data, equipment, flow, loadout, maps,
    match_hud, materials, menus, modes, net, particles, player, profile, progression, rear_cue, ui,
    video, weapon, world,
};
use lightyear::prelude::client::ClientPlugins;
use lightyear::prelude::server::ServerPlugins;

fn main() -> AppExit {
    let cli = Cli::parse();
    let mode = cli.mode();
    let tick_duration = Duration::from_secs_f64(1.0 / TICK_HZ);

    let mut app = App::new();
    app.insert_resource(mode.clone());
    // Asset sources must exist before `AssetPlugin` is added.
    maps::register_private_source(&mut app);
    if let Some(path) = cli.map_file() {
        maps::register_map_file_source(&mut app, path);
    }
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
    if let Some(path) = cli.map_file()
        && let Err(err) = maps::add_map_file(&mut app, path)
    {
        error!("{err} (--map-file)");
        return AppExit::error();
    }
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
            video::VideoPlugin,
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
        app.add_plugins(equipment::client::EquipmentClientPlugin);
        app.add_plugins(modes::AmmoCrateVisualsPlugin);
        app.add_plugins(particles::ParticlesPlugin);
    }
    app.add_plugins((combat::CombatServerPlugin, net::NetPlugin));
    app.run()
}
