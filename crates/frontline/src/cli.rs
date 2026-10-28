use std::net::SocketAddr;
use std::path::PathBuf;

use bevy::prelude::Resource;
use clap::Parser;

use crate::data::{DEFAULT_MATCH_RULES, GameModeId};
use crate::flow::FlowCommand;
use crate::maps::{DEFAULT_MAP, MapId};
use crate::rotation::{RotationEntry, ServerConfig, load_rotation_file};

/// Environment variable that turns on `--write-trenchbroom-config`.
const WRITE_TRENCHBROOM_CONFIG_ENV: &str = "FRONTLINE_WRITE_TRENCHBROOM_CONFIG";

/// Default UDP port a server listens on.
pub const DEFAULT_PORT: u16 = 5000;

/// Name a Dedicated Server announces on the LAN when `--name` isn't given.
const DEFAULT_SERVER_NAME: &str = "Frontline Server";

#[derive(Parser, Debug)]
#[command(version, about = "Frontline: LAN-first multiplayer FPS")]
pub struct Cli {
    /// Skip the main menu and Host: a server plus a local player in this window.
    #[arg(long, conflicts_with_all = ["dedicated", "connect"])]
    host: bool,

    /// Run a headless Dedicated Server (no window, no local player).
    #[arg(long, conflicts_with = "connect")]
    dedicated: bool,

    /// Skip the main menu and join a server at this address, e.g. `192.168.1.20:5000`.
    #[arg(long)]
    connect: Option<SocketAddr>,

    /// UDP port to listen on when hosting or running a Dedicated Server.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Player name (overrides the saved Settings), or the Dedicated Server's name on the LAN.
    #[arg(long)]
    name: Option<String>,

    /// Match Rules preset a server runs (when hosting or dedicated), from `data/match_rules.ron`,
    /// e.g. `hardcore`.
    #[arg(long, default_value = DEFAULT_MATCH_RULES, conflicts_with = "connect")]
    rules: String,

    /// Game Mode a server plays (when hosting or dedicated): `tdm`, `dom`, `hq` or `sd`. The Map
    /// must support it.
    #[arg(long, default_value = "tdm", conflicts_with_all = ["connect", "rotation"])]
    mode: String,

    /// A Rotation file (RON list of `(map: "arena", mode: tdm, rules: "default")` entries) the
    /// server cycles through; replaces `--map`, `--mode` and `--rules`.
    #[arg(long, conflicts_with_all = ["connect", "map", "mode", "rules"])]
    rotation: Option<PathBuf>,

    /// Map a server plays (when hosting or dedicated), by id from `data/maps.ron` or the private
    /// content folder; also the main menu's backdrop.
    #[arg(long, default_value = DEFAULT_MAP, conflicts_with = "connect")]
    map: String,

    /// Dev/test: look preset from `data/looks.ron` (e.g. `night_ops`) that replaces the Map's own
    /// look for this session (when hosting or dedicated; only a window shows it).
    #[arg(long, conflicts_with = "connect")]
    look: Option<String>,

    /// Where your own CoD4 images are, for Homage Map textures (overrides the saved Setting): an
    /// install folder, a folder of `iw_*.iwd`, or a zip containing them.
    #[arg(long, conflicts_with = "dedicated")]
    cod4: Option<PathBuf>,

    /// Write TrenchBroom's game config for Frontline (FGD, texture folder) into TrenchBroom's
    /// user folder on startup. Also on when the `FRONTLINE_WRITE_TRENCHBROOM_CONFIG` environment
    /// variable is set.
    #[arg(long)]
    write_trenchbroom_config: bool,
}

/// How this process participates in a game. Inserted as a resource at startup.
#[derive(Resource, Clone, Debug)]
pub enum Mode {
    /// A window with menus. Hosting uses `port`; `start` skips the main menu.
    Windowed {
        port: u16,
        start: Option<FlowCommand>,
        name: Option<String>,
        cod4: Option<PathBuf>,
    },
    /// A headless server announcing itself as `name`.
    Dedicated { port: u16, name: String },
}

impl Cli {
    pub fn mode(&self) -> Mode {
        if self.dedicated {
            return Mode::Dedicated {
                port: self.port,
                name: self
                    .name
                    .clone()
                    .unwrap_or_else(|| DEFAULT_SERVER_NAME.to_owned()),
            };
        }
        let start = if self.host {
            Some(FlowCommand::Host)
        } else {
            self.connect.map(FlowCommand::Join)
        };
        Mode::Windowed {
            port: self.port,
            start,
            name: self.name.clone(),
            cod4: self.cod4.clone(),
        }
    }

    /// The Rotation this process's server plays: the `--rotation` file, else the single entry
    /// from `--map`, `--mode` and `--rules` (a one-entry Rotation repeats).
    pub fn server_config(&self) -> Result<ServerConfig, String> {
        if let Some(path) = &self.rotation {
            return Ok(ServerConfig::new(
                load_rotation_file(path)?,
                Default::default(),
                crate::rotation::MAX_PLAYERS,
            ));
        }
        let mode = GameModeId::from_key(&self.mode).ok_or_else(|| {
            format!(
                "Unknown Game Mode \"{}\" (--mode); known: {}",
                self.mode,
                GameModeId::known_keys()
            )
        })?;
        Ok(ServerConfig::single(RotationEntry {
            map: self.map(),
            mode,
            rules: self.rules.clone(),
        }))
    }

    /// The Map this process's server starts on.
    pub fn map(&self) -> MapId {
        MapId::new(self.map.clone())
    }

    /// The `--look` preset replacing every Map's look, if given.
    pub fn look(&self) -> Option<&str> {
        self.look.as_deref()
    }

    /// Whether to write TrenchBroom's game config on startup.
    pub fn write_trenchbroom_config(&self) -> bool {
        self.write_trenchbroom_config || std::env::var_os(WRITE_TRENCHBROOM_CONFIG_ENV).is_some()
    }
}
