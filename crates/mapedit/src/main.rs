//! MapEdit: look at Frontline Maps on the game's own rendering (materials, looks, props,
//! lights) with a free-fly camera, the Map's entities and zones drawn over it, reload on file
//! change and screenshots. It shares the game's library (`frontline`), so a Map looks here as it
//! does in the game.
//!
//!     cargo run -p mapedit -- --map zeewolde_town
//!     cargo run -p mapedit -- --map zeewolde_town --top --shot /tmp/top.png

mod camera;
mod edit;
mod mapfile;
mod mapsron;
mod menu;
mod overlay;
mod palette;
mod remote;
mod session;

use std::path::PathBuf;

use bevy::asset::io::file::FileAssetReader;
use bevy::prelude::*;
use clap::Parser;
use frontline::cli::Mode;
use frontline::flow::AppState;
use frontline::maps::{DEFAULT_MAP, MapId, MapRegistry, MapsPlugin};
use frontline::video::{ActiveVideo, VideoSettings};
use frontline::{assets, atmosphere, data, maps, materials, ui, world};

use crate::camera::{CameraPlugin, StartView, View};
use crate::edit::EditPlugin;
use crate::menu::MenuPlugin;
use crate::overlay::{MODE_KEYS, Overlay, OverlayPlugin};
use crate::palette::PalettePlugin;
use crate::remote::ControlPlugin;
use crate::session::SessionPlugin;

#[derive(Parser)]
#[command(version, about = "MapEdit: look at Frontline Maps")]
struct Args {
    /// Map id (data/maps.ron, or a private Map).
    #[arg(long, default_value = DEFAULT_MAP)]
    map: String,
    /// Look preset (data/looks.ron) instead of the Map's own.
    #[arg(long)]
    look: Option<String>,
    /// Show only what this Game Mode uses (tdm, dom, hq, sd).
    #[arg(long)]
    mode: Option<String>,
    /// Start camera: x y z (metres) then yaw and pitch (degrees; yaw 0 looks north, -z).
    #[arg(long, num_args = 5, value_names = ["X", "Y", "Z", "YAW", "PITCH"], allow_negative_numbers = true)]
    cam: Option<Vec<f32>>,
    /// Start in the top view.
    #[arg(long, conflicts_with = "cam")]
    top: bool,
    /// Take a screenshot once the Map is ready, then exit.
    #[arg(long)]
    shot: Option<PathBuf>,
    /// Window size, e.g. 1600x900.
    #[arg(long, default_value = "1600x900")]
    size: String,
    /// Port of the remote control (Bevy remote protocol over HTTP, localhost only).
    #[arg(long, default_value_t = 15702)]
    remote_port: u16,
    /// Do not start the remote control.
    #[arg(long)]
    no_remote: bool,
}

/// The game's asset folder: `assets/` next to the executable in a shipped build; under
/// `cargo run`, the game crate's.
pub fn assets_dir() -> PathBuf {
    match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(manifest) => PathBuf::from(manifest).join("../frontline/assets"),
        None => FileAssetReader::get_base_path().join("assets"),
    }
}

fn parse_size(size: &str) -> Option<(u32, u32)> {
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn main() -> AppExit {
    let args = Args::parse();
    let Some((width, height)) = parse_size(&args.size) else {
        eprintln!("--size takes WIDTHxHEIGHT, e.g. 1600x900");
        return AppExit::error();
    };
    let mode_filter = match args.mode.as_deref() {
        None => None,
        Some(key) => match MODE_KEYS.iter().find(|known| **known == key) {
            Some(known) => Some(*known),
            None => {
                eprintln!(
                    "Unknown Game Mode \"{key}\" (--mode); known: {}",
                    MODE_KEYS.join(", ")
                );
                return AppExit::error();
            }
        },
    };
    let view = match (&args.cam, args.top) {
        (Some(c), _) => View {
            position: Vec3::new(c[0], c[1], c[2]),
            yaw: c[3].to_radians(),
            pitch: c[4].to_radians(),
        },
        (None, true) => View::TOP,
        (None, false) => View::AERIAL,
    };

    let mut app = App::new();
    // The Map loader and the look system key off the game's windowed mode and Video settings.
    app.insert_resource(Mode::Windowed {
        port: 0,
        start: None,
        name: None,
        cod4: None,
    });
    maps::register_private_source(&mut app);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Frontline MapEdit".into(),
                    resolution: (width, height).into(),
                    ..default()
                }),
                // Closing the window asks before it throws away unsaved edits (menu.rs).
                close_when_requested: false,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets_dir().to_string_lossy().into_owned(),
                ..default()
            }),
    );
    app.add_plugins((
        data::DataPlugin,
        world::WorldPlugin,
        assets::EmbeddedAssetsPlugin,
        materials::MaterialsPlugin,
        ui::UiPlugin,
    ));
    app.add_plugins(MapsPlugin {
        idle: MapId::new(args.map.clone()),
        write_trenchbroom_config: false,
    });
    let registry = app.world().resource::<MapRegistry>();
    if registry.get(&MapId::new(args.map.clone())).is_none() {
        eprintln!(
            "Unknown Map \"{}\" (--map); known: {}",
            args.map,
            registry.known_ids()
        );
        return AppExit::error();
    }
    // `maps` checks the game's screen state while a Match runs; there is none here.
    app.init_state::<AppState>();
    app.insert_resource(ActiveVideo(VideoSettings::default()));
    app.add_plugins(atmosphere::AtmospherePlugin {
        look_override: args.look.clone(),
    });
    if let Some(look) = &args.look {
        let looks = app.world().resource::<atmosphere::Looks>();
        if !looks.contains(look) {
            eprintln!(
                "Unknown look \"{look}\" (--look); known: {}",
                looks.known_names()
            );
            return AppExit::error();
        }
    }
    let mut overlay = Overlay::new(mode_filter);
    // Fog hides the far side of a Map from above.
    overlay.fog = !args.top;
    app.insert_resource(StartView(view))
        .insert_resource(overlay)
        .add_plugins((
            CameraPlugin,
            OverlayPlugin,
            MenuPlugin,
            PalettePlugin,
            EditPlugin {
                assets: assets_dir(),
            },
            SessionPlugin { shot: args.shot },
        ));
    if !args.no_remote {
        app.add_plugins(ControlPlugin {
            port: args.remote_port,
        });
    }
    app.run()
}
