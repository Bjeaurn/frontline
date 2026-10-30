//! The pause menu (Esc in game). The game keeps running underneath. RESPAWN (not in Search &
//! Destroy) is for players stuck somewhere: it ends their life with no credit and respawns them.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::data::GameModeId;
use crate::flow::{FlowCommand, Overlay};
use crate::net::protocol::ForceRespawn;
use crate::shared::{MatchState, ToServer};
use crate::ui::{AMBER, UiFonts, backdrop, button, panel, rule, title};

pub(super) fn spawn_pause(mut commands: Commands, fonts: Res<UiFonts>, state: Query<&MatchState>) {
    let can_respawn = state
        .iter()
        .all(|state| state.mode != GameModeId::SearchAndDestroy);
    commands
        .spawn((
            Name::new("Pause menu"),
            DespawnOnExit(Overlay::Pause),
            backdrop(),
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn(panel()).with_children(|panel| {
                panel.spawn(title(&fonts, "Paused", 44.0));
                panel.spawn(rule(AMBER));
                for (caption, target) in [
                    ("Resume", Overlay::None),
                    ("Change Team", Overlay::TeamSelect),
                    ("Change Loadout", Overlay::Loadout),
                    ("Settings", Overlay::Settings),
                ] {
                    panel.spawn(button(&fonts, caption)).observe(
                        move |_: On<Activate>, mut next: ResMut<NextState<Overlay>>| {
                            next.set(target);
                        },
                    );
                }
                if can_respawn {
                    panel.spawn(button(&fonts, "Respawn")).observe(
                        |_: On<Activate>,
                         mut requests: MessageWriter<ToServer<ForceRespawn>>,
                         mut next: ResMut<NextState<Overlay>>| {
                            requests.write(ToServer(ForceRespawn));
                            next.set(Overlay::None);
                        },
                    );
                }
                panel.spawn(button(&fonts, "Leave Game")).observe(
                    |_: On<Activate>, mut flow: MessageWriter<FlowCommand>| {
                        flow.write(FlowCommand::Leave);
                    },
                );
            });
        });
}
