//! Top-left overlay shown only while there is a connection problem (connecting, or not
//! connected), so a failed connection is visible in-game, not only in the log. Players are
//! listed on the Scoreboard instead.

use std::fmt::Write;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::*;

const FONT_SIZE: f32 = 14.0;
const TEXT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);

#[derive(Component)]
pub(super) struct StatusText;

pub(super) fn spawn_status_text(mut commands: Commands) {
    commands.spawn((
        Name::new("Net status"),
        StatusText,
        Text::default(),
        TextFont {
            font_size: FontSize::Px(FONT_SIZE),
            ..default()
        },
        TextColor(TEXT_COLOR),
        Node {
            position_type: PositionType::Absolute,
            left: px(10.0),
            top: px(8.0),
            ..default()
        },
    ));
}

pub(super) fn update_status_text(
    clients: Query<
        (
            Has<Connected>,
            Has<Connecting>,
            Option<&Disconnected>,
            Option<&PeerAddr>,
        ),
        With<Client>,
    >,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    let mut out = String::new();
    for (connected, connecting, disconnected, peer) in &clients {
        let server = peer.map_or_else(|| "host".to_owned(), |p| p.0.to_string());
        if connected {
            continue;
        } else if connecting {
            let _ = write!(out, "Connecting to {server}...");
        } else if let Some(disconnected) = disconnected {
            let _ = write!(out, "Not connected to {server}: {}", disconnected.reason);
        } else {
            let _ = write!(out, "Not connected to {server}");
        }
    }
    if text.0 != out {
        text.0 = out;
    }
}
