//! 'Connecting to ...' panel while a Join is in progress, with CANCEL (back to the Server
//! Browser). A failed Join returns there by itself (`net::session`).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use lightyear::prelude::PeerAddr;
use lightyear::prelude::client::{Client, Connecting};

use crate::flow::{AppState, FlowCommand};
use crate::ui::{TEXT_DIM, UiFonts, backdrop, button, label, panel, title};

#[derive(Component)]
pub(super) struct ConnectingPanel;

pub(super) fn show_connecting(
    joining: Query<&PeerAddr, (With<Client>, With<Connecting>)>,
    shown: Query<Entity, With<ConnectingPanel>>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    match (joining.iter().next(), shown.iter().next()) {
        (Some(server), None) => {
            commands
                .spawn((
                    Name::new("Connecting"),
                    ConnectingPanel,
                    DespawnOnExit(AppState::InGame),
                    backdrop(),
                    // Above the in-game HUD.
                    GlobalZIndex(10),
                ))
                .with_children(|root| {
                    root.spawn(panel()).with_children(|panel| {
                        panel.spawn(title(&fonts, "Connecting", 40.0));
                        panel.spawn(label(
                            &fonts,
                            &format!("Connecting to {}...", server.0),
                            22.0,
                            TEXT_DIM,
                        ));
                        panel.spawn(button(&fonts, "Cancel")).observe(
                            |_: On<Activate>, mut flow: MessageWriter<FlowCommand>| {
                                flow.write(FlowCommand::Leave);
                            },
                        );
                    });
                });
        }
        (None, Some(panel)) => commands.entity(panel).despawn(),
        _ => {}
    }
}
