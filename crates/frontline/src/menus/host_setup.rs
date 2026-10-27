//! The Host setup screen (HOST GAME): server name, max players, the Rotation (Map + Game Mode +
//! Match Rules per entry), and per-mode score and time limits. START remembers the choices in
//! Settings and starts the server; BACK discards them.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderRange, SliderStep, SliderValue};

use crate::data::{GameData, GameModeId};
use crate::flow::{FlowCommand, MenuScreen, Settings};
use crate::maps::{MapId, MapRegistry};
use crate::rotation::{HostSetup, LimitOverride, MAX_PLAYERS, RotationEntry};
use crate::ui::{
    ACCENT, TEXT, TEXT_DIM, UiFonts, backdrop, button, compact_button, label, panel, slider,
    text_field, title,
};

const MAX_NAME_CHARS: usize = 32;
const MAX_ENTRIES: usize = 8;
const MIN_PLAYERS: f32 = 2.0;
const LABEL_WIDTH: f32 = 170.0;
const PANEL_WIDTH: f32 = 820.0;
const FONT: f32 = 20.0;

/// The setup being edited; copied back to Settings by START.
#[derive(Resource)]
pub(super) struct Draft {
    setup: HostSetup,
    /// What ADD would add.
    map: MapId,
    mode: GameModeId,
    rules: String,
    /// Mode whose limits the sliders edit.
    limit_mode: GameModeId,
}

#[derive(Component)]
struct NameField;

#[derive(Component)]
pub(super) struct RotationList;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cycle {
    Map,
    Mode,
    Rules,
    LimitMode,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum SetupSlider {
    Players,
    Score,
    Time,
}

/// A text that shows a slider's value.
#[derive(Component, Clone, Copy)]
pub(super) struct Readout(SetupSlider);

fn sorted_rules(data: &GameData) -> Vec<String> {
    let mut names: Vec<String> = data.match_rules.keys().cloned().collect();
    names.sort_unstable();
    names
}

fn modes_of(registry: &MapRegistry, map: &MapId) -> Vec<GameModeId> {
    registry.get(map).map_or_else(Vec::new, |info| {
        GameModeId::ALL
            .into_iter()
            .filter(|&mode| info.supports(mode))
            .collect()
    })
}

fn next_after<T: PartialEq + Clone>(items: &[T], current: &T) -> Option<T> {
    let at = items.iter().position(|item| item == current);
    items
        .get(at.map_or(0, |at| (at + 1) % items.len()))
        .cloned()
}

/// The score and time limits `mode` plays with: the draft's override, else the data.
fn limits_of(draft: &Draft, data: &GameData, mode: GameModeId) -> LimitOverride {
    draft.setup.limits.get(&mode).copied().unwrap_or_else(|| {
        let settings = data.game_mode(mode);
        LimitOverride {
            score_limit: settings.score_limit,
            time_limit_secs: settings.time_limit_secs,
        }
    })
}

/// Slider ranges `(min, max, step)` of a mode's score limit and time limit (in minutes).
fn limit_ranges(mode: GameModeId) -> ((f32, f32, f32), (f32, f32, f32)) {
    match mode {
        GameModeId::TeamDeathmatch => ((10.0, 150.0, 5.0), (1.0, 30.0, 1.0)),
        GameModeId::Domination | GameModeId::Headquarters => {
            ((50.0, 500.0, 10.0), (1.0, 30.0, 1.0))
        }
        GameModeId::SearchAndDestroy => ((1.0, 10.0, 1.0), (0.5, 5.0, 0.5)),
    }
}

fn score_caption(mode: GameModeId, value: f32) -> String {
    match mode {
        GameModeId::SearchAndDestroy => format!("{value:.0} rounds to win"),
        _ => format!("{value:.0} points"),
    }
}

fn time_caption(mode: GameModeId, minutes: f32) -> String {
    match mode {
        GameModeId::SearchAndDestroy => format!("{minutes:.1} min per round"),
        _ => format!("{minutes:.0} min"),
    }
}

pub(super) fn spawn_host_setup(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<Settings>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
) {
    let mut setup = settings.host.clone();
    // A remembered Map or Mode this install lacks can't be hosted.
    setup.rotation.retain(|entry| {
        registry
            .get(&entry.map)
            .is_some_and(|info| info.supports(entry.mode))
            && data.match_rules.contains_key(&entry.rules)
    });
    if setup.rotation.is_empty() {
        setup.rotation = HostSetup::default().rotation;
    }
    setup.max_players = setup.max_players.clamp(MIN_PLAYERS as usize, MAX_PLAYERS);
    if setup.server_name.trim().is_empty() {
        setup.server_name = format!("{}'s Server", settings.name);
    }
    let first = setup.rotation[0].clone();
    let draft = Draft {
        map: first.map,
        mode: first.mode,
        rules: first.rules,
        limit_mode: GameModeId::TeamDeathmatch,
        setup,
    };
    let server_name = draft.setup.server_name.clone();
    let max_players = draft.setup.max_players as f32;

    let root = commands
        .spawn((
            Name::new("Host setup"),
            DespawnOnExit(MenuScreen::HostSetup),
            backdrop(),
            GlobalZIndex(10),
        ))
        .id();
    let panel = commands
        .spawn((ChildOf(root), panel()))
        .insert(Node {
            width: px(PANEL_WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(20.0)),
            row_gap: px(8.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();
    commands.spawn((ChildOf(panel), title(&fonts, "Host Game", 40.0)));

    let row = row(&mut commands, &fonts, panel, "Server name");
    commands.spawn((
        ChildOf(row),
        NameField,
        text_field(&fonts, &server_name, MAX_NAME_CHARS),
    ));

    let row_players = row_of(&mut commands, &fonts, panel, "Max players");
    let players = commands
        .spawn((
            ChildOf(row_players),
            SetupSlider::Players,
            slider(max_players, MIN_PLAYERS, MAX_PLAYERS as f32),
            SliderStep(1.0),
        ))
        .id();
    let _ = players;
    commands.spawn((
        ChildOf(row_players),
        Readout(SetupSlider::Players),
        label(&fonts, "", FONT, ACCENT),
    ));

    commands.spawn((
        ChildOf(panel),
        label(
            &fonts,
            "ROTATION  (played in order, then repeats)",
            FONT,
            TEXT_DIM,
        ),
        Node {
            margin: UiRect::top(px(6.0)),
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(panel),
        RotationList,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            min_height: px(60.0),
            ..default()
        },
    ));

    let add_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((ChildOf(add_row), label(&fonts, "ADD", FONT, TEXT_DIM)));
    for (cycle, width) in [
        (Cycle::Map, 150.0),
        (Cycle::Mode, 190.0),
        (Cycle::Rules, 120.0),
    ] {
        commands
            .spawn((
                ChildOf(add_row),
                cycle,
                compact_button(&fonts, "", width, FONT),
            ))
            .observe(
                move |_: On<Activate>,
                      mut draft: ResMut<Draft>,
                      registry: Res<MapRegistry>,
                      data: Res<GameData>| {
                    cycle_add(&mut draft, cycle, &registry, &data);
                },
            );
    }
    commands
        .spawn((ChildOf(add_row), compact_button(&fonts, "Add", 70.0, FONT)))
        .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
            if draft.setup.rotation.len() < MAX_ENTRIES {
                let entry = RotationEntry {
                    map: draft.map.clone(),
                    mode: draft.mode,
                    rules: draft.rules.clone(),
                };
                draft.setup.rotation.push(entry);
            }
        });

    commands.spawn((
        ChildOf(panel),
        label(&fonts, "LIMITS  (per Game Mode)", FONT, TEXT_DIM),
        Node {
            margin: UiRect::top(px(6.0)),
            ..default()
        },
    ));
    let limits = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            ChildOf(limits),
            Cycle::LimitMode,
            compact_button(&fonts, "", 190.0, FONT),
        ))
        .observe(
            |_: On<Activate>,
             mut draft: ResMut<Draft>,
             registry: Res<MapRegistry>,
             data: Res<GameData>| {
                cycle_add(&mut draft, Cycle::LimitMode, &registry, &data);
            },
        );
    for (kind, width) in [(SetupSlider::Score, 150.0), (SetupSlider::Time, 150.0)] {
        commands
            .spawn((
                ChildOf(limits),
                kind,
                slider(0.0, 0.0, 1.0),
                SliderStep(1.0),
            ))
            .insert(Node {
                width: px(width),
                height: px(22.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                ..default()
            });
        commands.spawn((
            ChildOf(limits),
            Readout(kind),
            label(&fonts, "", FONT, ACCENT),
            Node {
                width: px(130.0),
                ..default()
            },
        ));
    }

    let buttons = commands
        .spawn((
            ChildOf(panel),
            Node {
                column_gap: px(12.0),
                margin: UiRect::top(px(10.0)),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((ChildOf(buttons), button(&fonts, "Start")))
        .observe(
            |_: On<Activate>,
             name: Single<&EditableText, With<NameField>>,
             draft: Res<Draft>,
             mut settings: ResMut<Settings>,
             mut flow: MessageWriter<FlowCommand>,
             mut commands: Commands| {
                let mut setup = draft.setup.clone();
                setup.server_name = name.value().to_string().trim().to_owned();
                if setup.rotation.is_empty() {
                    setup.rotation.push(RotationEntry {
                        map: draft.map.clone(),
                        mode: draft.mode,
                        rules: draft.rules.clone(),
                    });
                }
                commands.insert_resource(setup.to_config());
                settings.set_if_neq(Settings {
                    host: setup,
                    ..settings.clone()
                });
                flow.write(FlowCommand::Host);
            },
        );
    commands
        .spawn((ChildOf(buttons), button(&fonts, "Back")))
        .observe(|_: On<Activate>, mut next: ResMut<NextState<MenuScreen>>| {
            next.set(MenuScreen::Title);
        });
    commands.insert_resource(draft);
}

/// A caption on the left and a row for the control(s).
fn row_of(commands: &mut Commands, fonts: &UiFonts, panel: Entity, caption: &str) -> Entity {
    let row = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(14.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(row),
        label(fonts, &caption.to_uppercase(), FONT, TEXT_DIM),
        Node {
            width: px(LABEL_WIDTH),
            ..default()
        },
    ));
    row
}

fn row(commands: &mut Commands, fonts: &UiFonts, panel: Entity, caption: &str) -> Entity {
    row_of(commands, fonts, panel, caption)
}

/// Advances what a picker offers to its next value, keeping Map and Mode consistent.
fn cycle_add(draft: &mut Draft, cycle: Cycle, registry: &MapRegistry, data: &GameData) {
    match cycle {
        Cycle::Map => {
            let maps: Vec<MapId> = registry.iter().map(|(id, _)| id.clone()).collect();
            if let Some(map) = next_after(&maps, &draft.map) {
                draft.map = map;
            }
            let modes = modes_of(registry, &draft.map);
            if !modes.contains(&draft.mode)
                && let Some(&mode) = modes.first()
            {
                draft.mode = mode;
            }
        }
        Cycle::Mode => {
            if let Some(mode) = next_after(&modes_of(registry, &draft.map), &draft.mode) {
                draft.mode = mode;
            }
        }
        Cycle::Rules => {
            if let Some(rules) = next_after(&sorted_rules(data), &draft.rules) {
                draft.rules = rules;
            }
        }
        Cycle::LimitMode => {
            if let Some(mode) = next_after(&GameModeId::ALL, &draft.limit_mode) {
                draft.limit_mode = mode;
            }
        }
    }
}

/// Rebuilds the Rotation list and refreshes pickers and sliders when the draft changes.
pub(super) fn refresh(
    draft: Res<Draft>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    list: Single<Entity, With<RotationList>>,
    cycles: Query<(&Cycle, &Children)>,
    mut texts: Query<&mut Text>,
    sliders: Query<(
        Entity,
        &SetupSlider,
        &SliderValue,
        &SliderRange,
        &SliderStep,
    )>,
    mut commands: Commands,
) {
    if !draft.is_changed() {
        return;
    }
    let map_name = |id: &MapId| {
        registry
            .get(id)
            .map_or_else(|| id.to_string(), |info| info.name.clone())
    };
    for (cycle, children) in &cycles {
        let caption = match cycle {
            Cycle::Map => map_name(&draft.map),
            Cycle::Mode => draft.mode.name().to_owned(),
            Cycle::Rules => draft.rules.clone(),
            Cycle::LimitMode => draft.limit_mode.name().to_owned(),
        };
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child) {
                let caption = caption.to_uppercase();
                if text.0 != caption {
                    text.0 = caption;
                }
            }
        }
    }

    let mode = draft.limit_mode;
    let limit = limits_of(&draft, &data, mode);
    let (score, time) = limit_ranges(mode);
    for (entity, kind, value, range, step) in &sliders {
        let (target, new_range, new_step) = match kind {
            SetupSlider::Players => (draft.setup.max_players as f32, None, None),
            SetupSlider::Score => (
                limit.score_limit as f32,
                Some((score.0, score.1)),
                Some(score.2),
            ),
            SetupSlider::Time => (
                limit.time_limit_secs as f32 / 60.0,
                Some((time.0, time.1)),
                Some(time.2),
            ),
        };
        if let Some((min, max)) = new_range {
            let wanted = SliderRange::new(min, max);
            if (range.start(), range.end()) != (wanted.start(), wanted.end()) {
                commands.entity(entity).insert(wanted);
            }
        }
        if let Some(wanted) = new_step
            && step.0 != wanted
        {
            commands.entity(entity).insert(SliderStep(wanted));
        }
        if (value.0 - target).abs() > 1e-3 {
            commands.entity(entity).insert(SliderValue(target));
        }
    }

    commands
        .entity(*list)
        .despawn_related::<Children>()
        .with_children(|list| {
            let last = draft.setup.rotation.len().saturating_sub(1);
            for (index, entry) in draft.setup.rotation.iter().enumerate() {
                list.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        label(
                            &fonts,
                            &format!(
                                "{}.  {}  -  {}  -  {}",
                                index + 1,
                                map_name(&entry.map),
                                entry.mode.name(),
                                entry.rules
                            ),
                            FONT,
                            TEXT,
                        ),
                    ));
                    for (caption, enabled, action) in [
                        ("^", index > 0, RowAction::Up),
                        ("v", index < last, RowAction::Down),
                        ("X", true, RowAction::Remove),
                    ] {
                        if !enabled {
                            continue;
                        }
                        row.spawn(compact_button(&fonts, caption, 34.0, FONT))
                            .observe(move |_: On<Activate>, mut draft: ResMut<Draft>| {
                                let rotation = &mut draft.setup.rotation;
                                match action {
                                    RowAction::Up => rotation.swap(index, index - 1),
                                    RowAction::Down => rotation.swap(index, index + 1),
                                    RowAction::Remove => {
                                        rotation.remove(index);
                                    }
                                }
                            });
                    }
                });
            }
        });
}

#[derive(Clone, Copy)]
enum RowAction {
    Up,
    Down,
    Remove,
}

/// Moves the user's slider changes into the draft.
pub(super) fn read_sliders(
    changed: Query<(&SetupSlider, &SliderValue), Changed<SliderValue>>,
    mut draft: ResMut<Draft>,
    data: Res<GameData>,
) {
    for (kind, value) in &changed {
        match kind {
            SetupSlider::Players => {
                let players = value.0.round() as usize;
                if draft.setup.max_players != players {
                    draft.setup.max_players = players;
                }
            }
            SetupSlider::Score | SetupSlider::Time => {
                let mode = draft.limit_mode;
                let mut limit = limits_of(&draft, &data, mode);
                match kind {
                    SetupSlider::Score => limit.score_limit = value.0.round() as u32,
                    _ => limit.time_limit_secs = (value.0 * 60.0).round() as u32,
                }
                if limits_of(&draft, &data, mode) != limit {
                    draft.setup.limits.insert(mode, limit);
                }
            }
        }
    }
}

/// Keeps each readout in step with its slider's value.
pub(super) fn show_readouts(
    draft: Res<Draft>,
    sliders: Query<(&SetupSlider, &SliderValue)>,
    mut readouts: Query<(&Readout, &mut Text)>,
) {
    for (readout, mut text) in &mut readouts {
        let Some((_, value)) = sliders.iter().find(|(kind, _)| **kind == readout.0) else {
            continue;
        };
        let shown = match readout.0 {
            SetupSlider::Players => format!("{:.0}", value.0),
            SetupSlider::Score => score_caption(draft.limit_mode, value.0),
            SetupSlider::Time => time_caption(draft.limit_mode, value.0),
        };
        if text.0 != shown {
            text.0 = shown;
        }
    }
}
