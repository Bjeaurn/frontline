//! Netcode (docs/adr/0002): clients own their movement and report their pose; the server keeps
//! one Player entity per connection and replicates it to everyone.
//!
//! Add [`NetPlugin`] after lightyear's `ClientPlugins`/`ServerPlugins` and after inserting [`Mode`].

mod client;
pub mod protocol;
mod server;
mod status;

use bevy::prelude::*;

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
        match mode {
            Mode::Dedicated { port } => {
                app.add_plugins(server::ServerNetPlugin)
                    .add_systems(Startup, move |mut commands: Commands| {
                        server::start_server(&mut commands, port);
                    });
            }
            Mode::Client { server, name } => {
                app.add_plugins(client::ClientNetPlugin { name })
                    .add_systems(Startup, move |mut commands: Commands| {
                        client::connect_to_server(&mut commands, server)
                    });
            }
            Mode::Host { port, name } => {
                app.add_plugins((server::ServerNetPlugin, client::ClientNetPlugin { name }))
                    .add_systems(Startup, move |mut commands: Commands| {
                        let server = server::start_server(&mut commands, port);
                        client::connect_host_client(&mut commands, server);
                    });
            }
        }
    }
}
