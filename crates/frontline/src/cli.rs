use std::net::SocketAddr;

use bevy::prelude::Resource;
use clap::Parser;

/// Default UDP port a server listens on.
pub const DEFAULT_PORT: u16 = 5000;

#[derive(Parser, Debug)]
#[command(version, about = "Frontline: LAN-first multiplayer FPS")]
pub struct Cli {
    /// Run a Host: a server plus a local player in this window.
    #[arg(long, conflicts_with_all = ["dedicated", "connect"])]
    host: bool,

    /// Run a headless Dedicated Server (no window, no local player).
    #[arg(long, conflicts_with = "connect")]
    dedicated: bool,

    /// Join a server at this address, e.g. `192.168.1.20:5000`.
    #[arg(long)]
    connect: Option<SocketAddr>,

    /// UDP port to listen on when hosting or running a Dedicated Server.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Player name shown to others.
    #[arg(long, default_value = "Player")]
    name: String,
}

/// How this process participates in a game. Inserted as a resource at startup.
#[derive(Resource, Clone, Debug)]
pub enum Mode {
    Client { server: SocketAddr, name: String },
    Host { port: u16, name: String },
    Dedicated { port: u16 },
}

impl Cli {
    pub fn mode(&self) -> Mode {
        if self.dedicated {
            Mode::Dedicated { port: self.port }
        } else if self.host {
            Mode::Host {
                port: self.port,
                name: self.name.clone(),
            }
        } else {
            Mode::Client {
                server: self
                    .connect
                    .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT))),
                name: self.name.clone(),
            }
        }
    }
}

impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Client { .. } => "client",
            Mode::Host { .. } => "host",
            Mode::Dedicated { .. } => "dedicated",
        }
    }
}
