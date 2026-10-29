//! The Controls tab: invert Y and the key-rebinding list. Click an action, press the new key
//! (or mouse button, for the mouse actions); Esc cancels. A key already used by another action
//! is taken from it. The list edits the screen's `Draft`; APPLY saves it.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::video::on_off;
use super::{Draft, setting_row};
use crate::flow::Settings;
use crate::menus::host_setup::picker;
use crate::player::input::{Action, Bindings, Device, Rebinding, is_bindable};
use crate::ui::{ArrowIcon, TEXT_DIM, UiFonts, compact_button, label};

const ACTION_COLUMNS: usize = 3;
const FONT: f32 = 18.0;

/// The invert Y picker's text.
#[derive(Component)]
pub(super) struct InvertYValue;

/// The button that rebinds `.0`; its text shows the binding.
#[derive(Component)]
pub(super) struct RebindButton(Action);

pub(super) fn spawn_controls_tab(
    commands: &mut Commands,
    fonts: &UiFonts,
    icon: &ArrowIcon,
    tab: Entity,
    settings: &Settings,
) {
    let row = setting_row(commands, fonts, tab, "Invert Y");
    let (left, right, value) = picker(commands, fonts, icon, row, 140.0, on_off(settings.invert_y));
    commands.entity(value).insert(InvertYValue);
    for arrow in [left, right] {
        commands
            .entity(arrow)
            .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
                draft.invert_y = !draft.invert_y;
            });
    }

    let header = commands
        .spawn((
            ChildOf(tab),
            Node {
                margin: UiRect::top(px(8.0)),
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(header),
        label(
            fonts,
            "KEY BINDINGS: CLICK ONE, THEN PRESS A KEY (ESC CANCELS)",
            FONT,
            TEXT_DIM,
        ),
    ));
    commands
        .spawn((
            ChildOf(header),
            compact_button(fonts, "Reset all", 120.0, FONT),
        ))
        .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
            draft.bindings = Bindings::default();
        });

    let grid = commands
        .spawn((
            ChildOf(tab),
            Node {
                column_gap: px(16.0),
                ..default()
            },
        ))
        .id();
    let per_column = Action::ALL.len().div_ceil(ACTION_COLUMNS);
    for actions in Action::ALL.chunks(per_column) {
        let column = commands
            .spawn((
                ChildOf(grid),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    ..default()
                },
            ))
            .id();
        for &action in actions {
            let cell = commands
                .spawn((
                    ChildOf(column),
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(8.0),
                        ..default()
                    },
                ))
                .id();
            commands.spawn((
                ChildOf(cell),
                label(fonts, &action.label().to_uppercase(), FONT, TEXT_DIM),
                Node {
                    width: px(124.0),
                    ..default()
                },
            ));
            commands
                .spawn((
                    ChildOf(cell),
                    RebindButton(action),
                    compact_button(fonts, &settings.bindings.describe(action), 122.0, FONT),
                ))
                .observe(move |_: On<Activate>, mut rebinding: ResMut<Rebinding>| {
                    rebinding.start(action);
                });
        }
    }
}

/// Keeps the invert Y picker and every binding button in step with the draft.
pub(super) fn show_values(
    draft: Option<Res<Draft>>,
    rebinding: Res<Rebinding>,
    mut invert: Query<&mut Text, With<InvertYValue>>,
    buttons: Query<(&RebindButton, &Children)>,
    mut texts: Query<&mut Text, Without<InvertYValue>>,
) {
    let Some(draft) = draft else {
        return;
    };
    let shown = on_off(draft.invert_y).to_uppercase();
    for mut text in &mut invert {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
    for (button, children) in &buttons {
        let shown = if rebinding.action() == Some(button.0) {
            match button.0.device() {
                Device::Keyboard => "PRESS A KEY",
                Device::Mouse => "PRESS A BUTTON",
            }
            .to_owned()
        } else {
            draft.bindings.describe(button.0).to_uppercase()
        };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child)
                && text.0 != shown
            {
                text.0 = shown.clone();
            }
        }
    }
}

/// Binds the pressed key (or mouse button) to the action being rebound; Esc cancels.
pub(super) fn capture_rebind(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut rebinding: ResMut<Rebinding>,
    mut draft: ResMut<Draft>,
) {
    let Some(action) = rebinding.listening() else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        rebinding.answer();
        return;
    }
    match action.device() {
        Device::Keyboard => {
            if let Some(key) = keys.get_just_pressed().find(|key| is_bindable(**key)) {
                draft.bindings.rebind_key(action, *key);
                rebinding.answer();
            }
        }
        Device::Mouse => {
            if let Some(button) = mouse.get_just_pressed().next() {
                draft.bindings.rebind_mouse(action, *button);
                rebinding.answer();
            }
        }
    }
}

/// Ends a rebind that was answered this frame, and lets a fresh one start listening.
pub(super) fn advance_rebinding(mut rebinding: ResMut<Rebinding>) {
    if rebinding.active() {
        rebinding.advance();
    }
}
