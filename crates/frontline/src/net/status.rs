//! Top-left overlay: connection state and every known player with distance, so a failed
//! connection or a missing remote player is visible in-game instead of only in the log.

use std::fmt::Write;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::*;

use super::protocol::PlayerOwner;
use crate::shared::{LocalPlayer, PlayerName, PlayerPose};

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
            Option<&LocalId>,
        ),
        With<Client>,
    >,
    local: Query<&PlayerPose, With<LocalPlayer>>,
    players: Query<(&PlayerOwner, &PlayerName, Option<&PlayerPose>)>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    let mut out = String::new();
    let mut local_id = None;
    for (connected, connecting, disconnected, peer, id) in &clients {
        let server = peer.map_or_else(|| "host".to_owned(), |p| p.0.to_string());
        if connected {
            local_id = id.map(|id| id.0);
            let _ = write!(out, "Connected to {server}");
        } else if connecting {
            let _ = write!(out, "Connecting to {server}...");
        } else if let Some(disconnected) = disconnected {
            let _ = write!(out, "Not connected to {server}: {}", disconnected.reason);
        }
    }

    let here = local.single().ok().map(|pose| pose.position);
    for (owner, name, pose) in &players {
        let name = if name.0.is_empty() { "?" } else { name.0.as_str() };
        let _ = write!(out, "\n  {name}");
        if Some(owner.0) == local_id {
            out.push_str(" (you)");
        } else {
            match (pose, here) {
                (Some(pose), Some(here)) => {
                    let _ = write!(out, "  {:.0} m", pose.position.distance(here));
                }
                (None, _) => out.push_str("  (no position yet)"),
                (Some(_), None) => {}
            }
        }
    }

    if text.0 != out {
        text.0 = out;
    }
}
