//! LAN discovery. A running server answers queries on UDP [`DISCOVERY_PORT`] with its
//! `ServerInfo`; the Server Browser broadcasts a query every second and lists who answers.
//!
//! Packets start with [`MAGIC`], [`VERSION`] and a kind byte so foreign traffic is ignored. A
//! reply carries the `ServerInfo` as RON after the header.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::server::{ClientOf, Started};
use lightyear::prelude::*;

use super::server::{ServerName, ServerSlots};
use crate::flow::{DiscoveredServer, DiscoveredServers, MenuScreen, ServerInfo};
use crate::shared::MatchState;

/// UDP port servers listen on for discovery queries.
pub const DISCOVERY_PORT: u16 = 5001;
const MAGIC: [u8; 4] = *b"FRLN";
const VERSION: u8 = 1;
const QUERY: u8 = b'?';
const REPLY: u8 = b'!';
const HEADER_LEN: usize = MAGIC.len() + 2;
const QUERY_INTERVAL: Duration = Duration::from_secs(1);
/// Servers that haven't answered for this long leave the list.
const STALE_SECS: f32 = 3.0;
/// Large enough for any `ServerInfo`; longer packets are truncated and fail to parse.
const MAX_PACKET: usize = 1024;

fn header(kind: u8) -> [u8; HEADER_LEN] {
    let [a, b, c, d] = MAGIC;
    [a, b, c, d, VERSION, kind]
}

/// The body of `packet` if it is ours, of this version and of `kind`.
fn body(packet: &[u8], kind: u8) -> Option<&[u8]> {
    packet.strip_prefix(&header(kind)[..])
}

fn bind_non_blocking(port: u16) -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Calls `handle` for every datagram waiting on `socket`.
fn drain(socket: &UdpSocket, mut handle: impl FnMut(&[u8], SocketAddr)) {
    let mut buf = [0; MAX_PACKET];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, from)) => handle(&buf[..len], from),
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            // Windows reports an earlier send's ICMP "port unreachable" here; skip it.
            Err(err) if err.kind() == ErrorKind::ConnectionReset => continue,
            Err(err) => {
                debug!("Discovery socket: {err}");
                break;
            }
        }
    }
}

/// Answers discovery queries while a server runs (Dedicated and Host).
pub(super) struct DiscoveryResponderPlugin;

impl Plugin for DiscoveryResponderPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(open_responder)
            .add_observer(close_responder)
            .add_systems(Update, answer_queries.run_if(resource_exists::<Responder>));
    }
}

#[derive(Resource)]
struct Responder {
    socket: UdpSocket,
    server: Entity,
    name: String,
    port: u16,
    max_players: u32,
}

fn open_responder(
    trigger: On<Add<Started>>,
    servers: Query<(&LocalAddr, &ServerName, &ServerSlots), With<Server>>,
    mut commands: Commands,
) {
    let Ok((addr, name, slots)) = servers.get(trigger.entity) else {
        return;
    };
    match bind_non_blocking(DISCOVERY_PORT) {
        Ok(socket) => {
            info!(
                "LAN discovery: answering as \"{}\" on udp port {DISCOVERY_PORT}",
                name.0
            );
            commands.insert_resource(Responder {
                socket,
                server: trigger.entity,
                name: name.0.clone(),
                port: addr.0.port(),
                max_players: slots.0 as u32,
            });
        }
        Err(err) => warn!(
            "LAN discovery unavailable (udp port {DISCOVERY_PORT}: {err}); this server won't \
             show up in Server Browsers, join it by IP"
        ),
    }
}

fn close_responder(
    trigger: On<Remove<Started>>,
    responder: Option<Res<Responder>>,
    mut commands: Commands,
) {
    if responder.is_some_and(|responder| responder.server == trigger.entity) {
        commands.remove_resource::<Responder>();
    }
}

fn answer_queries(
    responder: Res<Responder>,
    links: Query<(), (With<ClientOf>, With<Connected>)>,
    matches: Query<&MatchState>,
) {
    drain(&responder.socket, |packet, from| {
        if body(packet, QUERY).is_none() {
            return;
        }
        let Some(state) = matches.iter().next() else {
            return;
        };
        let info = ServerInfo {
            name: responder.name.clone(),
            port: responder.port,
            mode: state.mode,
            players: links.iter().count() as u32,
            max_players: responder.max_players,
        };
        let Ok(text) = ron::to_string(&info) else {
            return;
        };
        let mut reply = header(REPLY).to_vec();
        reply.extend_from_slice(text.as_bytes());
        if let Err(err) = responder.socket.send_to(&reply, from) {
            debug!("Discovery reply to {from}: {err}");
        }
    });
}

/// Queries the LAN while the Server Browser is open and fills `DiscoveredServers`.
pub(super) struct DiscoveryBrowserPlugin;

impl Plugin for DiscoveryBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(MenuScreen::ServerBrowser), open_probe)
            .add_systems(OnExit(MenuScreen::ServerBrowser), close_probe)
            .add_systems(
                Update,
                (send_queries, collect_replies)
                    .chain()
                    .run_if(resource_exists::<Probe>),
            );
    }
}

#[derive(Resource)]
struct Probe {
    socket: UdpSocket,
    next_query: Timer,
}

fn open_probe(mut servers: ResMut<DiscoveredServers>, mut commands: Commands) {
    servers.0.clear();
    match bind_non_blocking(0).and_then(|socket| socket.set_broadcast(true).map(|()| socket)) {
        Ok(socket) => {
            let mut next_query = Timer::new(QUERY_INTERVAL, TimerMode::Repeating);
            // Ask right away rather than a second after opening.
            next_query.set_elapsed(QUERY_INTERVAL);
            commands.insert_resource(Probe { socket, next_query });
        }
        Err(err) => warn!("LAN discovery unavailable: {err}"),
    }
}

fn close_probe(mut servers: ResMut<DiscoveredServers>, mut commands: Commands) {
    servers.0.clear();
    commands.remove_resource::<Probe>();
}

fn send_queries(mut probe: ResMut<Probe>, time: Res<Time<Real>>) {
    probe.next_query.tick(time.delta());
    if !probe.next_query.is_finished() {
        return;
    }
    // Broadcast for the LAN, loopback for a server on this machine.
    for target in [Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST] {
        let target = SocketAddrV4::new(target, DISCOVERY_PORT);
        if let Err(err) = probe.socket.send_to(&header(QUERY), target) {
            debug!("Discovery query to {target}: {err}");
        }
    }
}

fn collect_replies(
    probe: Res<Probe>,
    time: Res<Time<Real>>,
    mut servers: ResMut<DiscoveredServers>,
) {
    for server in &mut servers.0 {
        server.age_secs += time.delta_secs();
    }
    drain(&probe.socket, |packet, from| {
        let Some(info) = body(packet, REPLY)
            .and_then(|text| std::str::from_utf8(text).ok())
            .and_then(|text| ron::from_str::<ServerInfo>(text).ok())
        else {
            return;
        };
        remember(&mut servers.0, SocketAddr::new(from.ip(), info.port), info);
    });
    servers.0.retain(|server| server.age_secs <= STALE_SECS);
}

/// Adds or refreshes a server. A server on this machine answers both the loopback query and the
/// broadcast; those count as one entry, under its LAN address.
fn remember(servers: &mut Vec<DiscoveredServer>, addr: SocketAddr, info: ServerInfo) {
    let same = |server: &DiscoveredServer| {
        server.addr == addr
            || (server.info.name == info.name
                && server.addr.port() == addr.port()
                && (server.addr.ip().is_loopback() || addr.ip().is_loopback()))
    };
    match servers.iter_mut().find(|server| same(server)) {
        Some(server) => {
            if server.addr.ip().is_loopback() {
                server.addr = addr;
            }
            server.info = info;
            server.age_secs = 0.0;
        }
        None => servers.push(DiscoveredServer {
            addr,
            info,
            age_secs: 0.0,
        }),
    }
}
