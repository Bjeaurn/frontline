//! Client side (Client and the Host's local client): connects, reports the local pose every
//! fixed tick and marks other players' replicated entities as `RemotePlayer`.

use std::hash::{BuildHasher, RandomState};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::SystemTime;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::*;

use super::protocol::{
    CONNECTION_TIMEOUT_SECS, ControlChannel, Hello, PRIVATE_KEY, PROTOCOL_ID, PlayerOwner,
    PoseChannel, PoseUpdate,
};
use crate::shared::{LocalPlayer, PlayerPose, RemotePlayer};

/// Name this process introduces itself with after connecting.
#[derive(Resource)]
struct LocalPlayerName(String);

pub(super) struct ClientNetPlugin {
    pub name: String,
}

impl Plugin for ClientNetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LocalPlayerName(self.name.clone()))
            .add_observer(on_connected)
            .add_observer(on_disconnected)
            .add_systems(FixedUpdate, send_pose)
            .add_systems(Startup, super::status::spawn_status_text)
            .add_systems(
                Update,
                (mark_remote_players, super::status::update_status_text),
            );
    }
}

/// Spawns a netcode/UDP client for `server` and starts connecting.
pub(super) fn connect_to_server(commands: &mut Commands, server: SocketAddr) -> Result {
    let auth = Authentication::Manual {
        server_addr: server,
        client_id: random_client_id(),
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: CONNECTION_TIMEOUT_SECS,
            token_expire_secs: -1,
            ..default()
        },
    )?;
    let client = commands
        .spawn((
            Name::new("Client"),
            Client,
            Link::default(),
            ReplicationReceiver,
            LocalAddr(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)),
            PeerAddr(server),
            netcode,
            UdpIo::default(),
        ))
        .id();
    info!("Connecting to {server}");
    commands.trigger(Connect { entity: client });
    Ok(())
}

/// Spawns the Host's local client, linked in-process to `server`, and connects it.
pub(super) fn connect_host_client(commands: &mut Commands, server: Entity) {
    let client = commands
        .spawn((Client, Name::new("HostClient"), LinkOf { endpoint: server }))
        .id();
    commands.trigger(Connect { entity: client });
}

fn random_client_id() -> u64 {
    RandomState::new().hash_one(SystemTime::now())
}

fn on_connected(
    trigger: On<Add<Connected>>,
    mut clients: Query<(&LocalId, &mut MessageSender<Hello>), With<Client>>,
    name: Res<LocalPlayerName>,
) {
    let Ok((local_id, mut sender)) = clients.get_mut(trigger.entity) else {
        return;
    };
    info!("Connected to server as {}", local_id.0);
    sender.send::<ControlChannel>(Hello {
        name: name.0.clone(),
    });
}

fn on_disconnected(
    trigger: On<Add<Disconnected>>,
    clients: Query<(&Disconnected, Option<&PeerAddr>, Has<Connected>, Has<Connecting>), With<Client>>,
) {
    let Ok((disconnected, peer, was_connected, was_connecting)) = clients.get(trigger.entity)
    else {
        return;
    };
    let server = peer.map_or_else(|| "host server".to_owned(), |peer| peer.0.to_string());
    if was_connected {
        warn!("Disconnected from {server}: {}", disconnected.reason);
    } else if was_connecting {
        error!("Failed to connect to {server}: {}", disconnected.reason);
    }
}

fn send_pose(
    local_player: Query<&PlayerPose, With<LocalPlayer>>,
    mut senders: Query<&mut MessageSender<PoseUpdate>, (With<Client>, With<Connected>)>,
) {
    let Ok(pose) = local_player.single() else {
        return;
    };
    for mut sender in &mut senders {
        sender.send::<PoseChannel>(PoseUpdate(*pose));
    }
}

/// Every replicated player except our own copy becomes a `RemotePlayer`.
/// Waits for `PlayerPose`: interpolated copies only get it once enough history has arrived.
fn mark_remote_players(
    clients: Query<&LocalId, (With<Client>, With<Connected>)>,
    players: Query<(Entity, &PlayerOwner), (With<PlayerPose>, Without<RemotePlayer>)>,
    mut commands: Commands,
) {
    let Ok(local_id) = clients.single() else {
        return;
    };
    for (entity, owner) in &players {
        if owner.0 != local_id.0 {
            debug!("Showing remote player {}", owner.0);
            commands.entity(entity).insert(RemotePlayer);
        }
    }
}
