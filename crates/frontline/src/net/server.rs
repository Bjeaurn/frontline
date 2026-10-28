//! Server side (Dedicated and Host): accepts clients, owns one Player entity per connection,
//! writes the poses clients report into it and replicates it to everyone.

use std::net::{Ipv4Addr, SocketAddr};

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::chat::{self, ChatRate, SystemChat};
use super::protocol::{
    CONNECTION_TIMEOUT_SECS, EquipPerks, Hello, PRIVATE_KEY, PROTOCOL_ID, PlayerOwner, PoseUpdate,
};
use super::server_running;
use crate::combat::PoseGate;
use crate::gear::EquippedPerks;
use crate::shared::{Health, PlayerLevel, PlayerName, PlayerPose, PlayerStats, PlayerTeam};

pub(super) struct ServerNetPlugin;

impl Plugin for ServerNetPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(log_started)
            .add_observer(log_stopped)
            .add_observer(spawn_player)
            .add_observer(log_disconnect)
            .add_systems(
                Update,
                (
                    receive_hello,
                    chat::receive_levels.after(receive_hello),
                    chat::receive_chat.after(receive_hello),
                    receive_perks,
                    receive_poses,
                )
                    .run_if(server_running),
            );
    }
}

/// The server-side Player entity of a client link.
#[derive(Component, Debug)]
pub struct LinkPlayer(pub Entity);

/// Name a server announces to LAN discovery.
#[derive(Component, Debug)]
pub(super) struct ServerName(pub String);

/// Most players the server accepts, announced to LAN discovery.
#[derive(Component, Debug)]
pub(super) struct ServerSlots(pub usize);

/// Spawns the netcode/UDP server listening on all interfaces and starts it.
pub(super) fn start_server(
    commands: &mut Commands,
    port: u16,
    name: String,
    max_players: usize,
) -> Entity {
    let server = commands
        .spawn((
            Name::new("Server"),
            ServerName(name),
            ServerSlots(max_players),
            NetcodeServer::new(
                NetcodeConfig {
                    protocol_id: PROTOCOL_ID,
                    private_key: PRIVATE_KEY,
                    client_timeout_secs: CONNECTION_TIMEOUT_SECS,
                    // Clients build their own token with whatever address they dialled (a LAN
                    // IP), which never equals the 0.0.0.0 bind address.
                    server_addr_check: false,
                    ..default()
                }
                .with_max_clients(max_players),
            ),
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

fn log_stopped(trigger: On<Add<Stopped>>, servers: Query<&LocalAddr, With<Server>>) {
    if let Ok(addr) = servers.get(trigger.entity) {
        info!("Server on udp://{} stopped", addr.0);
    }
}

/// Spawns the client's Player out of play: dead and on no Team until it picks one.
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
            Health(0.0),
            PlayerStats::default(),
            PlayerLevel::default(),
            ChatRate::default(),
            PlayerTeam(None),
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
    links: Query<
        (
            &RemoteId,
            &Disconnected,
            Option<&LinkPlayer>,
            Has<Connected>,
        ),
        With<ClientOf>,
    >,
    names: Query<&PlayerName>,
    mut chat: SystemChat,
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
    if !name.is_empty() {
        chat.say(format!("{name} left"));
    }
}

fn receive_hello(
    mut links: Query<(&RemoteId, &LinkPlayer, &mut MessageReceiver<Hello>)>,
    mut names: Query<&mut PlayerName>,
    mut chat: SystemChat,
) {
    for (RemoteId(peer), player, mut receiver) in &mut links {
        for hello in receiver.receive() {
            info!("Client {peer} joined as \"{}\"", hello.name);
            if !hello.name.is_empty() {
                chat.say(format!("{} joined", hello.name));
            }
            if let Ok(mut name) = names.get_mut(player.0) {
                name.0 = hello.name;
            }
        }
    }
}

/// Keeps the Perks each client reports as its Player's `EquippedPerks`, replicated to everyone.
/// Clients are trusted (docs/adr/0002).
fn receive_perks(
    mut links: Query<(&RemoteId, &LinkPlayer, &mut MessageReceiver<EquipPerks>)>,
    mut commands: Commands,
) {
    for (RemoteId(peer), player, mut receiver) in &mut links {
        for EquipPerks(perks) in receiver.receive() {
            info!("Client {peer} equipped Perks {perks:?}");
            commands.entity(player.0).insert(EquippedPerks(perks));
        }
    }
}

/// Writes each client's newest pose into its Player, except while a `PoseGate` holds it at a
/// fresh spawn point.
fn receive_poses(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<PoseUpdate>)>,
    mut poses: Query<(&mut PlayerPose, Option<&PoseGate>)>,
    mut commands: Commands,
) {
    for (player, mut receiver) in &mut links {
        let Some(PoseUpdate(latest)) = receiver.receive().last() else {
            continue;
        };
        let Ok((mut pose, gate)) = poses.get_mut(player.0) else {
            continue;
        };
        if let Some(gate) = gate {
            if !gate.releases(latest.position) {
                continue;
            }
            debug!("{}: client pose accepted again after respawn", player.0);
            commands.entity(player.0).remove::<PoseGate>();
        }
        pose.set_if_neq(latest);
    }
}
