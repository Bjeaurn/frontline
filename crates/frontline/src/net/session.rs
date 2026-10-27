//! A windowed game's network session: carries out `FlowCommand::{Host, Join, Leave}`, returns to
//! the Server Browser when a connection fails or drops, and clears everything a session spawned
//! so the next one starts clean.

use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::client::{Connecting, Disconnect, Remote};
use lightyear::prelude::server::{Stop, Stopped};
use lightyear::prelude::*;

use super::protocol::PlayerOwner;
use super::{client, server};
use crate::cli::Mode;
use crate::flow::{AppState, ConnectionError, FlowCommand, MenuScreen, Settings};
use crate::gear::AmmoPack;
use crate::rotation::ServerConfig;
use crate::shared::MatchState;

/// Longest a leaving client or server waits for its goodbye packets before it is despawned.
const TEARDOWN_GRACE: Duration = Duration::from_secs(1);

pub(super) struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_flow_commands)
            .add_observer(return_on_connection_loss)
            .add_systems(Last, finish_teardown)
            .add_systems(OnExit(AppState::InGame), despawn_session_entities);
    }
}

/// A client or server being shut down (since `Time<Real>` elapsed `.0`). It is despawned once
/// its disconnect has gone out, or after `TEARDOWN_GRACE`.
#[derive(Component)]
struct Teardown(Duration);

type LiveClients<'w, 's> = Query<'w, 's, Entity, (With<Client>, Without<Teardown>)>;
type LiveServers<'w, 's> = Query<'w, 's, Entity, (With<Server>, Without<Teardown>)>;

fn apply_flow_commands(
    mut flow: MessageReader<FlowCommand>,
    mode: Res<Mode>,
    settings: Res<Settings>,
    config: Res<ServerConfig>,
    clients: LiveClients,
    servers: LiveServers,
    joining: Query<(), (With<Client>, With<Connecting>, With<PeerAddr>)>,
    time: Res<Time<Real>>,
    mut error: ResMut<ConnectionError>,
    mut next_app: ResMut<NextState<AppState>>,
    mut next_menu: ResMut<NextState<MenuScreen>>,
    mut commands: Commands,
) {
    for command in flow.read() {
        let in_session = !clients.is_empty() || !servers.is_empty();
        match *command {
            FlowCommand::Host => {
                let Mode::Windowed { port, .. } = *mode else {
                    continue;
                };
                if in_session {
                    warn!("Ignoring Host: already in a game");
                    continue;
                }
                let server_name = match settings.host.server_name.trim() {
                    "" => settings.name.clone(),
                    name => name.to_owned(),
                };
                info!(
                    "Hosting \"{server_name}\" on port {port} as \"{}\"",
                    settings.name
                );
                let server =
                    server::start_server(&mut commands, port, server_name, config.max_players);
                client::connect_host_client(&mut commands, server);
                error.0 = None;
                next_app.set_if_different(AppState::InGame);
            }
            FlowCommand::Join(addr) => {
                if in_session {
                    warn!("Ignoring Join {addr}: already in a game");
                    continue;
                }
                match client::connect_to_server(&mut commands, addr) {
                    Ok(()) => {
                        error.0 = None;
                        next_app.set_if_different(AppState::InGame);
                    }
                    Err(err) => {
                        error.0 = Some(format!("Can't connect to {addr}: {err}"));
                        next_app.set_if_different(AppState::MainMenu);
                        next_menu.set(MenuScreen::ServerBrowser);
                    }
                }
            }
            FlowCommand::Leave => {
                info!("Leaving the game");
                tear_down(&mut commands, &clients, &servers, time.elapsed());
                next_app.set_if_different(AppState::MainMenu);
                // Cancelling a Join goes back to where it was started.
                if !joining.is_empty() {
                    next_menu.set(MenuScreen::ServerBrowser);
                }
            }
            // Goodbye packets still go out this frame, before the app exits.
            FlowCommand::Quit => tear_down(&mut commands, &clients, &servers, time.elapsed()),
        }
    }
}

/// Disconnects every client and stops every server of this process.
fn tear_down(commands: &mut Commands, clients: &LiveClients, servers: &LiveServers, now: Duration) {
    for client in clients {
        commands.entity(client).insert(Teardown(now));
        commands.trigger(Disconnect { entity: client });
    }
    for server in servers {
        commands.entity(server).insert(Teardown(now));
        commands.trigger(Stop { entity: server });
    }
}

/// A Join that fails, or a connection that drops, ends the session and shows why on the Server
/// Browser. (A netcode client also starts out `Disconnected`; that is neither.)
fn return_on_connection_loss(
    trigger: On<Add<Disconnected>>,
    lost: Query<
        (
            &Disconnected,
            Option<&PeerAddr>,
            Has<Connected>,
            Has<Connecting>,
        ),
        (With<Client>, Without<Teardown>),
    >,
    app_state: Res<State<AppState>>,
    clients: LiveClients,
    servers: LiveServers,
    time: Res<Time<Real>>,
    mut error: ResMut<ConnectionError>,
    mut next_app: ResMut<NextState<AppState>>,
    mut next_menu: ResMut<NextState<MenuScreen>>,
    mut commands: Commands,
) {
    let Ok((disconnected, peer, was_connected, was_connecting)) = lost.get(trigger.entity) else {
        return;
    };
    if !(was_connected || was_connecting) || *app_state.get() != AppState::InGame {
        return;
    }
    let server = peer.map_or_else(|| "the host".to_owned(), |peer| peer.0.to_string());
    error.0 = Some(if was_connected {
        format!("Lost connection to {server} ({})", disconnected.reason)
    } else {
        format!("Couldn't connect to {server} ({})", disconnected.reason)
    });
    warn!("{}", error.0.as_deref().unwrap_or_default());
    tear_down(&mut commands, &clients, &servers, time.elapsed());
    next_app.set_if_different(AppState::MainMenu);
    next_menu.set(MenuScreen::ServerBrowser);
}

/// Despawns leaving clients once disconnected and leaving servers once stopped (lightyear
/// sends their goodbye packets in `PostUpdate`), or after the grace period.
fn finish_teardown(
    leaving: Query<(
        Entity,
        &Teardown,
        Has<Server>,
        Has<Stopped>,
        Has<Connected>,
        Has<Connecting>,
    )>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    for (entity, teardown, is_server, stopped, connected, connecting) in &leaving {
        let done = if is_server {
            stopped
        } else {
            !connected && !connecting
        };
        if done || time.elapsed() >= teardown.0 + TEARDOWN_GRACE {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Removes what the session left behind: replicated entities on a Client, and the server's
/// Players (with their hitboxes) and Match on a Host. In-game UI and the local player go by
/// `DespawnOnExit(AppState::InGame)`.
fn despawn_session_entities(
    leftovers: Query<
        Entity,
        Or<(
            With<Remote>,
            With<PlayerOwner>,
            With<MatchState>,
            With<AmmoPack>,
        )>,
    >,
    mut commands: Commands,
) {
    for entity in &leftovers {
        commands.entity(entity).try_despawn();
    }
}
