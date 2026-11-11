//! MapEdit: look at Frontline Maps on the game's own rendering (materials, looks, props,
//! lights) with a free-fly camera, the Map's entities and zones drawn over it, reload on file
//! change and screenshots. It shares the game's library (`frontline`), so a Map looks here as it
//! does in the game. Its viewer mode (`viewer`) shows one model, or a soldier as other players
//! are drawn, instead of a Map.
//!
//!     cargo run -p mapedit -- --map z_town
//!     cargo run -p mapedit -- --map z_town --top --shot /tmp/top.png
//!     cargo run -p mapedit -- --model models/weapons/ak47.glb
//!     cargo run -p mapedit -- --soldier ranger --stance crouching --walk --shot /tmp/ranger.png

mod camera;
mod edit;
mod inspector;
mod mapfile;
mod mapsron;
mod menu;
mod overlay;
mod palette;
mod playtest;
mod prefab;
mod remote;
mod selection;
mod session;
mod transform;
mod viewer;

use std::path::PathBuf;

use bevy::asset::io::file::FileAssetReader;
use bevy::prelude::*;
use clap::Parser;
use frontline::cli::Mode;
use frontline::flow::AppState;
use frontline::maps::{DEFAULT_MAP, MapId, MapRegistry, MapsPlugin};
use frontline::shared::Stance;
use frontline::video::{ActiveVideo, VideoSettings};
use frontline::{assets, atmosphere, data, maps, materials, ui, world};

use crate::camera::{CameraPlugin, StartView, View};
use crate::edit::EditPlugin;
use crate::inspector::InspectorPlugin;
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
    /// Look at one glTF model instead of a Map: an asset path under the game's assets folder
    /// (models/weapons/ak47.glb) or any .glb file. Drag to orbit, wheel to zoom.
    #[arg(long, conflicts_with_all = ["soldier", "top"])]
    model: Option<String>,
    /// Look at a soldier instead of a Map, drawn as other players are in a Match: a Faction id
    /// from data/factions.ron (ranger, harmattan, ...).
    #[arg(long, conflicts_with = "top")]
    soldier: Option<String>,
    /// The soldier's stance: standing, crouching or prone.
    #[arg(long, default_value = "standing", requires = "soldier")]
    stance: String,
    /// The soldier walks along its facing (at the walking speed of its stance).
    #[arg(long, requires = "soldier", conflicts_with = "sprint")]
    walk: bool,
    /// The soldier sprints.
    #[arg(long, requires = "soldier")]
    sprint: bool,
    /// The soldier's aim pitch in degrees, up positive.
    #[arg(
        long,
        default_value_t = 0.0,
        allow_negative_numbers = true,
        requires = "soldier"
    )]
    aim: f32,
    /// The soldier wears the enemy's team colour instead of the friendly one.
    #[arg(long, requires = "soldier")]
    enemy: bool,
    /// Show the body the soldier leaves when it dies instead of the soldier.
    #[arg(long, requires = "soldier")]
    corpse: bool,
    /// Frames to wait before a `--shot` of a model or soldier (150 by default, stepped at a
    /// fixed 60 Hz).
    #[arg(long)]
    frames: Option<u32>,
    /// Start camera: x y z (metres) then yaw and pitch (degrees; yaw 0 looks north, -z). With
    /// `--model` or `--soldier` the camera orbits what it looks at from there.
    #[arg(long, num_args = 5, value_names = ["X", "Y", "Z", "YAW", "PITCH"], allow_negative_numbers = true)]
    cam: Option<Vec<f32>>,
    /// Start in the top view.
    #[arg(long, conflicts_with = "cam")]
    top: bool,
    /// Take a screenshot once the Map (model, soldier) is ready, then exit.
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

/// The viewer mode (`--model`, `--soldier`).
fn run_viewer(args: Args, size: (u32, u32)) -> AppExit {
    let subject = match (args.model, args.soldier) {
        (Some(model), _) => viewer::Subject::Model(model),
        (None, Some(faction)) => {
            let stance = match args.stance.as_str() {
                "standing" => Stance::Standing,
                "crouching" => Stance::Crouching,
                "prone" => Stance::Prone,
                other => {
                    eprintln!(
                        "Unknown stance \"{other}\" (--stance); known: standing, crouching, prone"
                    );
                    return AppExit::error();
                }
            };
            let pace = match (args.walk, args.sprint) {
                (_, true) => viewer::Pace::Sprint,
                (true, false) => viewer::Pace::Walk,
                (false, false) => viewer::Pace::Still,
            };
            viewer::Subject::Soldier(viewer::SoldierArgs {
                faction,
                stance,
                pace,
                aim_degrees: args.aim,
                enemy: args.enemy,
                corpse: args.corpse,
            })
        }
        (None, None) => unreachable!("the caller checked for a model or a soldier"),
    };
    let cam = match args.cam.as_deref() {
        None => None,
        Some(&[x, y, z, yaw, pitch]) => Some([x, y, z, yaw, pitch]),
        Some(_) => unreachable!("clap takes exactly five numbers"),
    };
    viewer::run(viewer::ViewerArgs {
        subject,
        look: args.look,
        cam,
        shot: args.shot,
        frames: args.frames,
        size,
        assets_dir: assets_dir(),
    })
}

fn main() -> AppExit {
    let args = Args::parse();
    let Some((width, height)) = parse_size(&args.size) else {
        eprintln!("--size takes WIDTHxHEIGHT, e.g. 1600x900");
        return AppExit::error();
    };
    if args.model.is_some() || args.soldier.is_some() {
        return run_viewer(args, (width, height));
    }
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
            InspectorPlugin,
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
