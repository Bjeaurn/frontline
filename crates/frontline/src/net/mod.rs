//! Netcode (docs/adr/0002): clients own their movement and report their pose; the server keeps
//! one Player entity per connection and replicates it to everyone. Running servers answer LAN
//! discovery queries; the Server Browser sends them.
//!
//! Add [`NetPlugin`] after lightyear's `ClientPlugins`/`ServerPlugins` and after inserting [`Mode`].
//! A Dedicated Server starts serving right away; a windowed game hosts, joins and leaves on
//! `FlowCommand`s (`session`).

mod client;
mod discovery;
pub mod protocol;
mod server;
mod session;

use bevy::prelude::*;
use lightyear::prelude::Server;
use lightyear::prelude::server::Started;

pub use server::LinkPlayer;

use crate::cli::Mode;

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        protocol::register(app);
        let mode = app
            .world()
            .get_resource::<Mode>()
            .expect("insert the Mode resource before adding NetPlugin")
            .clone();
        app.add_plugins((server::ServerNetPlugin, discovery::DiscoveryResponderPlugin));
        match mode {
            Mode::Dedicated { port, name } => {
                app.add_systems(Startup, move |mut commands: Commands| {
                    server::start_server(&mut commands, port, name.clone());
                });
            }
            Mode::Windowed { .. } => {
                app.add_plugins((
                    client::ClientNetPlugin,
                    session::SessionPlugin,
                    discovery::DiscoveryBrowserPlugin,
                ));
            }
        }
    }
}

/// Run condition: this process is running a server (Dedicated, or hosting).
pub fn server_running(servers: Query<(), (With<Server>, With<Started>)>) -> bool {
    !servers.is_empty()
}
