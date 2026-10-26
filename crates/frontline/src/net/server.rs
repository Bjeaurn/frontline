//! Server side (Dedicated and Host): accepts clients, owns one Player entity per connection,
//! writes the poses clients report into it and replicates it to everyone.

use std::net::{Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::protocol::{
    CONNECTION_TIMEOUT_SECS, Hello, PRIVATE_KEY, PROTOCOL_ID, PlayerOwner, PoseUpdate,
};
use crate::shared::{PlayerName, PlayerPose};

pub(super) struct ServerNetPlugin;

impl Plugin for ServerNetPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(log_started)
            .add_observer(spawn_player)
            .add_observer(log_disconnect)
            .add_systems(Update, (receive_hello, receive_poses));
    }
}

/// The server-side Player entity of a client link.
#[derive(Component, Debug)]
struct LinkPlayer(Entity);

/// Spawns the netcode/UDP server listening on all interfaces and starts it.
pub(super) fn start_server(commands: &mut Commands, port: u16) -> Entity {
    let server = commands
        .spawn((
            Name::new("Server"),
            NetcodeServer::new(NetcodeConfig {
                protocol_id: PROTOCOL_ID,
                private_key: PRIVATE_KEY,
                client_timeout_secs: CONNECTION_TIMEOUT_SECS,
                // Clients build their own token with whatever address they dialled (a LAN IP),
                // which never equals the 0.0.0.0 bind address.
                server_addr_check: false,
                ..default()
            }),
            LocalAddr(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port)),
            ServerUdpIo,
        ))
        .id();
    commands.trigger(Start { entity: server });
    server
}

fn log_started(trigger: On<Add<Started>>, servers: Query<&LocalAddr, With<Server>>) {
    if let Ok(addr) = servers.get(trigger.entity) {
        info!("Server listening on udp://{}", addr.0);
    }
}

fn spawn_player(
    trigger: On<Add<Connected>>,
    links: Query<&RemoteId, With<ClientOf>>,
    mut commands: Commands,
) {
    let Ok(&RemoteId(peer)) = links.get(trigger.entity) else {
        return;
    };
    info!("Client {peer} connected");
    let player = commands
        .spawn((
            Name::new(format!("Player {peer}")),
            PlayerPose::default(),
            PlayerName::default(),
            PlayerOwner(peer),
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer)),
            ControlledBy {
                owner: trigger.entity,
                lifetime: Lifetime::SessionBased,
            },
        ))
        .id();
    commands
        .entity(trigger.entity)
        .insert((ReplicationSender, LinkPlayer(player)));
}

fn log_disconnect(
    trigger: On<Add<Disconnected>>,
    links: Query<(&RemoteId, &Disconnected, Option<&LinkPlayer>, Has<Connected>), With<ClientOf>>,
    names: Query<&PlayerName>,
) {
    let Ok((RemoteId(peer), disconnected, player, was_connected)) = links.get(trigger.entity)
    else {
        return;
    };
    if !was_connected {
        return;
    }
    let name = player
        .and_then(|player| names.get(player.0).ok())
        .map_or("", |name| name.0.as_str());
    info!(
        "Client {peer} \"{name}\" disconnected: {}",
        disconnected.reason
    );
}

fn receive_hello(
    mut links: Query<(&RemoteId, &LinkPlayer, &mut MessageReceiver<Hello>)>,
    mut names: Query<&mut PlayerName>,
) {
    for (RemoteId(peer), player, mut receiver) in &mut links {
        for hello in receiver.receive() {
            info!("Client {peer} joined as \"{}\"", hello.name);
            if let Ok(mut name) = names.get_mut(player.0) {
                name.0 = hello.name;
            }
        }
    }
}

fn receive_poses(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<PoseUpdate>)>,
    mut poses: Query<&mut PlayerPose>,
) {
    for (player, mut receiver) in &mut links {
        let Some(PoseUpdate(latest)) = receiver.receive().last() else {
            continue;
        };
        if let Ok(mut pose) = poses.get_mut(player.0) {
            pose.set_if_neq(latest);
        }
    }
}
