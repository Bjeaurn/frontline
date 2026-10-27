use std::net::SocketAddr;

use bevy::prelude::Resource;
use clap::Parser;

use crate::data::DEFAULT_MATCH_RULES;
use crate::flow::FlowCommand;

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
}

/// How this process participates in a game. Inserted as a resource at startup.
#[derive(Resource, Clone, Debug)]
pub enum Mode {
    /// A window with menus. Hosting uses `port`; `start` skips the main menu.
    Windowed {
        port: u16,
        start: Option<FlowCommand>,
        name: Option<String>,
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
        }
    }

    /// The Match Rules preset this process's server runs.
    pub fn rules(&self) -> &str {
        &self.rules
    }
}
