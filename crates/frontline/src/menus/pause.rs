//! The pause menu (Esc in game). The game keeps running underneath.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::flow::{FlowCommand, Overlay};
use crate::ui::{UiFonts, backdrop, button, panel, title};

pub(super) fn spawn_pause(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            Name::new("Pause menu"),
            DespawnOnExit(Overlay::Pause),
            backdrop(),
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn(panel()).with_children(|panel| {
                panel.spawn((
                    title(&fonts, "Paused", 44.0),
                    Node {
                        margin: UiRect::bottom(px(8.0)),
                        ..default()
                    },
                ));
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
                panel.spawn(button(&fonts, "Leave Game")).observe(
                    |_: On<Activate>, mut flow: MessageWriter<FlowCommand>| {
                        flow.write(FlowCommand::Leave);
                    },
                );
            });
        });
}
