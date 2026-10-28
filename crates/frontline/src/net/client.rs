//! Client side (Client and the Host's local client): connects, reports the local pose every
//! fixed tick, marks replicated players as `RemotePlayer` or `LocalPlayerReplica`, and bridges
//! gameplay messages: server messages become `FromServer<M>`; `ToServer<FireShot>` and
//! `ToServer<ChooseTeam>`, `ToServer<FallDamage>`, `ToServer<RequestRespawn>` and
//! `ToServer<ForceRespawn>` are sent.

use std::hash::{BuildHasher, RandomState};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::SystemTime;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::*;

use super::protocol::{
    AmmoRefill, BombExploded, CONNECTION_TIMEOUT_SECS, ChooseTeam, ControlChannel, Damaged,
    EquipPerks, FallDamage, FireShot, ForceRespawn, Hello, HitConfirm, KillFeed, ObjectiveHold,
    PRIVATE_KEY, PROTOCOL_ID, PlayerOwner, PoseChannel, PoseUpdate, RequestRespawn, Respawn,
    ShotChannel, ShotTracer, XpAward,
};
use crate::flow::Settings;
use crate::gear::Perks;
use crate::shared::{
    FromServer, LocalPlayer, LocalPlayerReplica, PlayerPose, RemotePlayer, ToServer,
};
use crate::weapon::Kit;

pub(super) struct ClientNetPlugin;

impl Plugin for ClientNetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FromServer<HitConfirm>>()
            .add_message::<FromServer<Damaged>>()
            .add_message::<FromServer<KillFeed>>()
            .add_message::<FromServer<Respawn>>()
            .add_message::<FromServer<ShotTracer>>()
            .add_message::<FromServer<BombExploded>>()
            .add_message::<FromServer<AmmoRefill>>()
            .add_message::<FromServer<XpAward>>()
            .add_message::<ToServer<FireShot>>()
            .add_message::<ToServer<ChooseTeam>>()
            .add_message::<ToServer<ObjectiveHold>>()
            .add_message::<ToServer<FallDamage>>()
            .add_message::<ToServer<RequestRespawn>>()
            .add_message::<ToServer<ForceRespawn>>()
            .add_observer(on_connected)
            .add_observer(on_disconnected)
            .add_systems(FixedUpdate, send_pose)
            .add_systems(
                PreUpdate,
                (
                    forward_from_server::<HitConfirm>,
                    forward_from_server::<Damaged>,
                    forward_from_server::<KillFeed>,
                    forward_from_server::<Respawn>,
                    forward_from_server::<ShotTracer>,
                    forward_from_server::<BombExploded>,
                    forward_from_server::<AmmoRefill>,
                    forward_from_server::<XpAward>,
                )
                    .after(MessageSystems::Receive),
            )
            .add_systems(
                PostUpdate,
                (
                    send_shots,
                    send_perks,
                    send_team_choices,
                    send_objective_holds,
                    send_control::<FallDamage>,
                    send_control::<RequestRespawn>,
                    send_control::<ForceRespawn>,
                )
                    .before(MessageSystems::Send),
            )
            .add_systems(Update, (mark_remote_players, mark_local_replica));
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
    settings: Res<Settings>,
) {
    let Ok((local_id, mut sender)) = clients.get_mut(trigger.entity) else {
        return;
    };
    info!("Connected to server as {}", local_id.0);
    sender.send::<ControlChannel>(Hello {
        name: settings.name.clone(),
    });
}

fn on_disconnected(
    trigger: On<Add<Disconnected>>,
    clients: Query<
        (
            &Disconnected,
            Option<&PeerAddr>,
            Has<Connected>,
            Has<Connecting>,
        ),
        With<Client>,
    >,
) {
    let Ok((disconnected, peer, was_connected, was_connecting)) = clients.get(trigger.entity)
    else {
        return;
    };
    let server = peer.map_or_else(|| "host server".to_owned(), |peer| peer.0.to_string());
    if matches!(disconnected.reason, DisconnectedReason::UserRequested(_)) {
        info!("Left {server}");
    } else if was_connected {
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

/// Our own Player entity becomes the `LocalPlayerReplica`, the source of our `Health`.
fn mark_local_replica(
    clients: Query<&LocalId, (With<Client>, With<Connected>)>,
    players: Query<(Entity, &PlayerOwner), Without<LocalPlayerReplica>>,
    mut commands: Commands,
) {
    let Ok(local_id) = clients.single() else {
        return;
    };
    for (entity, owner) in &players {
        if owner.0 == local_id.0 {
            commands.entity(entity).insert(LocalPlayerReplica);
        }
    }
}

/// Drains server message `M` from the client link (the HostClient link too) into `FromServer<M>`.
fn forward_from_server<M: lightyear::prelude::Message>(
    mut receivers: Query<&mut MessageReceiver<M>, (With<Client>, With<Connected>)>,
    mut messages: MessageWriter<FromServer<M>>,
) {
    for mut receiver in &mut receivers {
        messages.write_batch(receiver.receive().map(FromServer));
    }
}

fn send_shots(
    mut shots: MessageReader<ToServer<FireShot>>,
    mut senders: Query<&mut MessageSender<FireShot>, (With<Client>, With<Connected>)>,
) {
    for mut sender in &mut senders {
        for ToServer(shot) in shots.read() {
            sender.send::<ShotChannel>(*shot);
        }
    }
}

/// Tells the server which Perks the local Loadout has in effect: on connecting and whenever
/// they change.
fn send_perks(
    local_player: Query<&Kit, With<LocalPlayer>>,
    mut senders: Query<(Entity, &mut MessageSender<EquipPerks>), (With<Client>, With<Connected>)>,
    mut sent: Local<Option<(Entity, Perks)>>,
) {
    let Ok(kit) = local_player.single() else {
        return;
    };
    let perks = kit.perks();
    for (link, mut sender) in &mut senders {
        if *sent != Some((link, perks)) {
            sender.send::<ControlChannel>(EquipPerks(perks));
            *sent = Some((link, perks));
        }
    }
}

fn send_team_choices(
    mut choices: MessageReader<ToServer<ChooseTeam>>,
    mut senders: Query<&mut MessageSender<ChooseTeam>, (With<Client>, With<Connected>)>,
) {
    for mut sender in &mut senders {
        for ToServer(choice) in choices.read() {
            sender.send::<ControlChannel>(*choice);
        }
    }
}

fn send_objective_holds(
    mut holds: MessageReader<ToServer<ObjectiveHold>>,
    mut senders: Query<&mut MessageSender<ObjectiveHold>, (With<Client>, With<Connected>)>,
) {
    for mut sender in &mut senders {
        for ToServer(hold) in holds.read() {
            sender.send::<ControlChannel>(*hold);
        }
    }
}

/// Sends each `ToServer<M>` on the `ControlChannel`.
fn send_control<M: lightyear::prelude::Message + Clone>(
    mut messages: MessageReader<ToServer<M>>,
    mut senders: Query<&mut MessageSender<M>, (With<Client>, With<Connected>)>,
) {
    for mut sender in &mut senders {
        for ToServer(message) in messages.read() {
            sender.send::<ControlChannel>(message.clone());
        }
    }
}
