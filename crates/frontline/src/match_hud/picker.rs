//! The team picker: opens on join and on the team key, with the smaller team pre-selected.
//! Keys 1/2 or the mouse select, Enter or a click confirms, Esc closes it once on a Team.
//! The cursor is released while it is open.

use bevy::prelude::*;

use crate::net::protocol::{ChooseTeam, PlayerOwner};
use crate::player::MenuOpen;
use crate::player::input::Bindings;
use crate::shared::{LocalPlayerReplica, PlayerTeam, Team, ToServer};

const TITLE_SIZE: f32 = 26.0;
const OPTION_SIZE: f32 = 20.0;
const HINT_SIZE: f32 = 14.0;
const OPTION_WIDTH: f32 = 240.0;
const OPTION_HEIGHT: f32 = 90.0;
const OPTION_BG: Color = Color::srgba(0.12, 0.12, 0.14, 0.9);
const OPTION_SELECTED_BG: Color = Color::srgba(0.25, 0.25, 0.3, 0.95);
const OPTION_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
const OPTION_SELECTED_BORDER: Color = Color::WHITE;

#[derive(Resource, Debug)]
pub(super) struct TeamPicker {
    open: bool,
    selected: Team,
    /// Confirm the selection this frame (Enter or a click).
    confirm: bool,
    /// Team asked for and not yet confirmed by the server; keeps the picker from reopening
    /// while a first choice is in flight.
    requested: Option<Team>,
}

impl Default for TeamPicker {
    fn default() -> Self {
        Self {
            open: false,
            selected: Team::Alpha,
            confirm: false,
            requested: None,
        }
    }
}

#[derive(Component)]
pub(super) struct PickerRoot;

/// One of the two choices.
#[derive(Component)]
pub(super) struct PickerOption(Team);

pub(super) fn spawn_picker(mut commands: Commands) {
    let option = |team: Team| {
        (
            PickerOption(team),
            Node {
                width: px(OPTION_WIDTH),
                height: px(OPTION_HEIGHT),
                border: UiRect::all(px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(OPTION_BG),
            BorderColor::all(OPTION_BORDER),
            Text::default(),
            TextFont {
                font_size: FontSize::Px(OPTION_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        )
    };
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    commands
        .spawn((
            Name::new("Team picker"),
            PickerRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(20),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((Text::new("Choose a team"), font(TITLE_SIZE), TextColor(Color::WHITE)));
            root.spawn(Node {
                column_gap: px(24.0),
                ..default()
            })
            .with_children(|row| {
                for team in Team::ALL {
                    row.spawn(option(team))
                        .observe(
                            move |_: On<PointerOver>, mut picker: ResMut<TeamPicker>| {
                                picker.selected = team;
                            },
                        )
                        .observe(
                            move |click: On<PointerClick>, mut picker: ResMut<TeamPicker>| {
                                if click.button == PointerButton::Primary {
                                    picker.selected = team;
                                    picker.confirm = true;
                                }
                            },
                        );
                }
            });
            root.spawn((
                Text::new("1 / 2 or mouse to select    Enter or click to confirm    Esc to close"),
                font(HINT_SIZE),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
        });
}

/// Players per Team, optionally leaving out our own entity.
fn team_counts<'a>(players: impl Iterator<Item = (&'a PlayerTeam, bool)>, skip_self: bool) -> [usize; 2] {
    let mut counts = [0; 2];
    for (team, is_self) in players {
        if let Some(team) = team.0
            && !(skip_self && is_self)
        {
            counts[team.index()] += 1;
        }
    }
    counts
}

/// The Team to pre-select: the smaller one counting everyone but us; on a tie, our current
/// Team (or Alpha).
fn preselect(others: [usize; 2], current: Option<Team>) -> Team {
    match others[0].cmp(&others[1]) {
        std::cmp::Ordering::Less => Team::Alpha,
        std::cmp::Ordering::Greater => Team::Bravo,
        std::cmp::Ordering::Equal => current.unwrap_or(Team::Alpha),
    }
}

/// Opens, closes and confirms the picker, and tells the controller to release the cursor
/// while it is open.
pub(super) fn drive_picker(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mut picker: ResMut<TeamPicker>,
    mut menu: ResMut<MenuOpen>,
    replica: Query<&PlayerTeam, With<LocalPlayerReplica>>,
    players: Query<(&PlayerTeam, Has<LocalPlayerReplica>), With<PlayerOwner>>,
    mut choices: MessageWriter<ToServer<ChooseTeam>>,
) {
    let Ok(&PlayerTeam(current)) = replica.single() else {
        // Not connected (yet): nothing to choose.
        picker.open = false;
        menu.0 = false;
        return;
    };
    if picker.requested.is_some_and(|team| current == Some(team)) {
        picker.requested = None;
    }

    if !picker.open {
        let must_choose = current.is_none() && picker.requested.is_none();
        if must_choose || bindings.team_menu.just_pressed(&keys) {
            picker.open = true;
            picker.confirm = false;
            picker.selected = preselect(team_counts(players.iter(), true), current);
        }
    } else {
        if keys.any_just_pressed([KeyCode::Digit1, KeyCode::Numpad1]) {
            picker.selected = Team::Alpha;
        }
        if keys.any_just_pressed([KeyCode::Digit2, KeyCode::Numpad2]) {
            picker.selected = Team::Bravo;
        }
        if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
            picker.confirm = true;
        }
        let dismiss = keys.just_pressed(KeyCode::Escape) || bindings.team_menu.just_pressed(&keys);
        if dismiss && current.is_some() {
            picker.open = false;
        }
    }

    if picker.open && std::mem::take(&mut picker.confirm) {
        let team = picker.selected;
        if current != Some(team) {
            info!("Choosing team {team:?}");
            choices.write(ToServer(ChooseTeam(team)));
            picker.requested = Some(team);
        }
        picker.open = false;
    }
    if menu.0 != picker.open {
        menu.0 = picker.open;
    }
}

/// Shows the picker with live team sizes and the selection highlighted.
pub(super) fn update_picker(
    picker: Res<TeamPicker>,
    players: Query<(&PlayerTeam, Has<LocalPlayerReplica>), With<PlayerOwner>>,
    mut root: Single<&mut Visibility, With<PickerRoot>>,
    mut options: Query<(&PickerOption, &mut Text, &mut BackgroundColor, &mut BorderColor)>,
) {
    root.set_if_neq(if picker.open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !picker.open {
        return;
    }
    let counts = team_counts(players.iter(), false);
    for (PickerOption(team), mut text, mut background, mut border) in &mut options {
        let count = counts[team.index()];
        let label = format!(
            "{team:?} ({count} player{})",
            if count == 1 { "" } else { "s" }
        );
        if text.0 != label {
            text.0 = label;
        }
        let selected = picker.selected == *team;
        background.set_if_neq(BackgroundColor(if selected {
            OPTION_SELECTED_BG
        } else {
            OPTION_BG
        }));
        border.set_if_neq(BorderColor::all(if selected {
            OPTION_SELECTED_BORDER
        } else {
            OPTION_BORDER
        }));
    }
}
