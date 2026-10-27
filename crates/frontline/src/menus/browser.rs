//! The Server Browser: servers found on the LAN (click one to join), join by IP, and why the
//! last connection failed.

use std::net::{IpAddr, SocketAddr};

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};

use crate::cli::DEFAULT_PORT;
use crate::data::GameModeId;
use crate::flow::{ConnectionError, DiscoveredServers, FlowCommand, MenuScreen, ServerInfo};
use crate::ui::{
    ACCENT, MenuButton, PANEL_BORDER, TEXT, TEXT_DIM, UiFonts, backdrop, button, label, panel,
    text_field, title,
};

const LIST_WIDTH: f32 = 760.0;
/// Column widths: name, players, mode, address.
const COLUMNS: [f32; 4] = [260.0, 100.0, 180.0, 180.0];
const ERROR: Color = Color::srgb(0.92, 0.45, 0.35);

/// Holds one row per discovered server, and what it currently shows.
#[derive(Component, Default)]
pub(super) struct ServerList(Vec<(SocketAddr, ServerInfo)>);

#[derive(Component)]
pub(super) struct ConnectionErrorText;

#[derive(Component)]
struct AddressField;

pub(super) fn spawn_browser(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            Name::new("Server Browser"),
            DespawnOnExit(MenuScreen::ServerBrowser),
            backdrop(),
        ))
        .with_children(|root| {
            root.spawn(panel()).with_children(|panel| {
                panel.spawn(title(&fonts, "Server Browser", 44.0));
                panel.spawn(label(&fonts, "Servers on your LAN", 20.0, TEXT_DIM));
                panel.spawn(row(
                    &fonts,
                    ["NAME", "PLAYERS", "MODE", "ADDRESS"],
                    TEXT_DIM,
                ));
                panel.spawn((
                    ServerList::default(),
                    Node {
                        width: px(LIST_WIDTH),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        min_height: px(240.0),
                        padding: UiRect::vertical(px(6.0)),
                        border: UiRect::vertical(px(1.0)),
                        ..default()
                    },
                    BorderColor::all(PANEL_BORDER),
                    children![searching(&fonts)],
                ));
                panel.spawn((ConnectionErrorText, label(&fonts, "", 20.0, ERROR)));
                panel
                    .spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: px(12.0),
                        margin: UiRect::top(px(6.0)),
                        ..default()
                    })
                    .with_children(|by_ip| {
                        by_ip.spawn(label(&fonts, "JOIN BY IP", 22.0, ACCENT));
                        by_ip.spawn((AddressField, text_field(&fonts, "", 64)));
                        by_ip.spawn(button(&fonts, "Join")).observe(join_by_ip);
                    });
                panel.spawn(button(&fonts, "Back")).observe(
                    |_: On<Activate>, mut next: ResMut<NextState<MenuScreen>>| {
                        next.set(MenuScreen::Title);
                    },
                );
            });
        });
}

/// One line of four columns.
fn row(fonts: &UiFonts, cells: [&str; 4], color: Color) -> impl Bundle {
    let [name, players, mode, address] = cells;
    (
        Node {
            padding: UiRect::horizontal(px(12.0)),
            ..default()
        },
        children![
            cell(fonts, name, COLUMNS[0], color),
            cell(fonts, players, COLUMNS[1], color),
            cell(fonts, mode, COLUMNS[2], color),
            cell(fonts, address, COLUMNS[3], color),
        ],
    )
}

fn cell(fonts: &UiFonts, text: &str, width: f32, color: Color) -> impl Bundle {
    (
        label(fonts, text, 22.0, color),
        Node {
            width: px(width),
            ..default()
        },
    )
}

fn searching(fonts: &UiFonts) -> impl Bundle {
    (
        label(fonts, "Searching for servers...", 22.0, TEXT_DIM),
        Node {
            padding: UiRect::axes(px(12.0), px(6.0)),
            ..default()
        },
    )
}

/// Rebuilds the rows when the set of servers or what they report changes (not on mere ageing).
pub(super) fn refresh_server_list(
    servers: Res<DiscoveredServers>,
    mut list: Single<(Entity, &mut ServerList)>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    let current: Vec<_> = servers
        .0
        .iter()
        .map(|server| (server.addr, server.info.clone()))
        .collect();
    let (entity, shown) = &mut *list;
    let entity = *entity;
    if shown.0 == current {
        return;
    }
    commands.entity(entity).despawn_children();
    if current.is_empty() {
        commands.entity(entity).with_child(searching(&fonts));
    }
    for (addr, info) in &current {
        let addr = *addr;
        let players = format!("{}/{}", info.players, info.max_players);
        commands
            .spawn((
                ChildOf(entity),
                MenuButton,
                Button,
                Hovered::default(),
                Node {
                    padding: UiRect::vertical(px(6.0)),
                    border: UiRect::left(px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
            ))
            .with_child(row(
                &fonts,
                [
                    &info.name,
                    &players,
                    GameModeId::name(info.mode),
                    &addr.to_string(),
                ],
                TEXT,
            ))
            .observe(
                move |_: On<Activate>, mut flow: MessageWriter<FlowCommand>| {
                    flow.write(FlowCommand::Join(addr));
                },
            );
    }
    shown.0 = current;
}

pub(super) fn show_connection_error(
    error: Res<ConnectionError>,
    mut text: Single<&mut Text, With<ConnectionErrorText>>,
) {
    let message = error.0.as_deref().unwrap_or_default();
    if text.0 != message {
        text.0 = message.to_owned();
    }
}

/// `host:port`, or a bare host on the default port.
fn parse_address(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    text.parse().ok().or_else(|| {
        text.parse::<IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
    })
}

fn join_by_ip(
    _: On<Activate>,
    field: Single<&EditableText, With<AddressField>>,
    mut error: ResMut<ConnectionError>,
    mut flow: MessageWriter<FlowCommand>,
) {
    let text = field.value().to_string();
    match parse_address(&text) {
        Some(addr) => {
            flow.write(FlowCommand::Join(addr));
        }
        None => {
            error.0 = Some(format!(
                "Not an address: \"{}\" (try 192.168.1.20:5000)",
                text.trim()
            ))
        }
    }
}
