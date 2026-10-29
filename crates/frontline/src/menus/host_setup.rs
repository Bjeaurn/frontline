//! The Host setup screen (HOST GAME): server name, max players, the Rotation (Map + Game Mode +
//! Match Rules per entry), and per-mode score and time limits. START remembers the choices in
//! Settings and starts the server; BACK discards them.
//!
//! Each Rotation entry is a row under the column headers MAP / MODE / RULES; every cell is a
//! picker showing its current value between ◀ ▶ arrows. ADD ENTRY copies the previous entry.
//! MATCH RULES opens the rules editor (`rules_editor`) and comes back to the same draft.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderRange, SliderStep, SliderValue};

use super::rules_editor::{EditorOrigin, RulesOrigin};
use crate::data::{DEFAULT_MATCH_RULES, GameData, GameModeId};
use crate::flow::{FlowCommand, MenuScreen, Settings};
use crate::maps::{MapId, MapRegistry};
use crate::rotation::{HostSetup, LimitOverride, MAX_PLAYERS, RotationEntry};
use crate::ui::{
    ACCENT, ArrowIcon, TEXT, TEXT_DIM, UiFonts, arrow_button, backdrop, button, compact_button,
    label, panel, slider, text_field, title,
};

const MAX_NAME_CHARS: usize = 32;
const MAX_ENTRIES: usize = 8;
const MIN_PLAYERS: f32 = 2.0;
const LABEL_WIDTH: f32 = 170.0;
const PANEL_WIDTH: f32 = 900.0;
const FONT: f32 = 20.0;
const HEADER_FONT: f32 = 16.0;
const VALUE_BACKGROUND: Color = Color::srgba(0.03, 0.04, 0.03, 0.95);
/// Widths of the Rotation's columns: the number, then each picker (arrows included).
const INDEX_WIDTH: f32 = 28.0;
const MAP_WIDTH: f32 = 220.0;
const MODE_WIDTH: f32 = 240.0;
const RULES_WIDTH: f32 = 220.0;
const LIMIT_PICKER_WIDTH: f32 = 250.0;
/// Longest Match Rules name shown in a picker before it is cut.
const RULES_CHARS: usize = 14;

/// The setup being edited; copied back to Settings by START.
#[derive(Resource)]
pub(super) struct Draft {
    setup: HostSetup,
    /// Mode whose limits the sliders edit.
    limit_mode: GameModeId,
}

/// Set while the rules editor is open from this screen: coming back keeps the draft.
#[derive(Resource)]
pub(super) struct ResumeHostSetup;

#[derive(Component)]
struct NameField;

#[derive(Component)]
pub(super) struct RotationList;

/// The text of the picker that chooses which Game Mode the limit sliders edit.
#[derive(Component)]
pub(super) struct LimitModeValue;

/// A column of the Rotation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Map,
    Mode,
    Rules,
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

fn modes_of(registry: &MapRegistry, map: &MapId) -> Vec<GameModeId> {
    registry.get(map).map_or_else(Vec::new, |info| {
        GameModeId::ALL
            .into_iter()
            .filter(|&mode| info.supports(mode))
            .collect()
    })
}

/// The item `delta` places after `current` (wrapping); the first one when `current` isn't listed.
fn step_in<T: PartialEq + Clone>(items: &[T], current: &T, delta: i32) -> Option<T> {
    let len = i32::try_from(items.len()).ok().filter(|&len| len > 0)?;
    let at = items.iter().position(|item| item == current);
    let next = at.map_or(0, |at| (at as i32 + delta).rem_euclid(len));
    items.get(next as usize).cloned()
}

/// A name cut to `max` characters, with `..` where it was cut.
fn shorten(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_owned();
    }
    let kept: String = name.chars().take(max.saturating_sub(2)).collect();
    format!("{kept}..")
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

/// Slider ranges `(min, max, step)` of a mode's score limit and time limit (in minutes): scores
/// step by 5 points (Search & Destroy: whole rounds), times by a minute.
fn limit_ranges(mode: GameModeId) -> ((f32, f32, f32), (f32, f32, f32)) {
    match mode {
        GameModeId::TeamDeathmatch => ((10.0, 150.0, 5.0), (1.0, 30.0, 1.0)),
        GameModeId::Domination | GameModeId::Headquarters => ((50.0, 500.0, 5.0), (1.0, 30.0, 1.0)),
        GameModeId::SearchAndDestroy => ((1.0, 10.0, 1.0), (1.0, 5.0, 1.0)),
    }
}

/// `value` rounded to a multiple of `step` counted from `min`, kept inside `min..=max`.
fn snap(value: f32, min: f32, max: f32, step: f32) -> f32 {
    (min + ((value - min) / step).round() * step).clamp(min, max)
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

/// A Rotation entry this install can play: its Map and Mode exist, and a Match Rules set it no
/// longer has (a deleted custom set) falls back to the default.
fn playable(
    entry: &RotationEntry,
    registry: &MapRegistry,
    data: &GameData,
) -> Option<RotationEntry> {
    registry
        .get(&entry.map)
        .is_some_and(|info| info.supports(entry.mode))
        .then(|| RotationEntry {
            rules: if data.match_rules.contains_key(&entry.rules) {
                entry.rules.clone()
            } else {
                DEFAULT_MATCH_RULES.to_owned()
            },
            ..entry.clone()
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_host_setup(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<Settings>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
    icon: Res<ArrowIcon>,
    previous: Option<Res<Draft>>,
    resume: Option<Res<ResumeHostSetup>>,
) {
    // Back from the rules editor: the same draft, not the saved setup.
    let kept = previous.filter(|_| resume.is_some());
    commands.remove_resource::<ResumeHostSetup>();
    let mut setup = kept
        .as_ref()
        .map_or_else(|| settings.host.clone(), |draft| draft.setup.clone());
    let limit_mode = kept
        .as_ref()
        .map_or(GameModeId::TeamDeathmatch, |draft| draft.limit_mode);
    // A remembered Map or Mode this install lacks can't be hosted.
    setup.rotation = setup
        .rotation
        .iter()
        .filter_map(|entry| playable(entry, &registry, &data))
        .collect();
    if setup.rotation.is_empty() {
        setup.rotation = HostSetup::default().rotation;
    }
    setup.max_players = setup.max_players.clamp(MIN_PLAYERS as usize, MAX_PLAYERS);
    if setup.server_name.trim().is_empty() {
        setup.server_name = format!("{}'s Server", settings.name);
    }
    let draft = Draft { limit_mode, setup };
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

    let row = row_of(&mut commands, &fonts, panel, "Server name");
    commands.spawn((
        ChildOf(row),
        NameField,
        text_field(&fonts, &server_name, MAX_NAME_CHARS),
    ));

    let row_players = row_of(&mut commands, &fonts, panel, "Max players");
    commands.spawn((
        ChildOf(row_players),
        SetupSlider::Players,
        slider(max_players, MIN_PLAYERS, MAX_PLAYERS as f32),
        SliderStep(1.0),
    ));
    commands.spawn((
        ChildOf(row_players),
        Readout(SetupSlider::Players),
        label(&fonts, "", FONT, ACCENT),
    ));

    // Rotation: a heading with the rules editor, the column headers, one row per entry.
    let heading = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                margin: UiRect::top(px(6.0)),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(heading),
        label(
            &fonts,
            "ROTATION  (played in order, then repeats)",
            FONT,
            TEXT_DIM,
        ),
    ));
    commands
        .spawn((
            ChildOf(heading),
            compact_button(&fonts, "Match Rules", 120.0, FONT),
        ))
        .observe(
            |_: On<Activate>,
             name: Single<&EditableText, With<NameField>>,
             mut draft: ResMut<Draft>,
             mut next: ResMut<NextState<MenuScreen>>,
             mut commands: Commands| {
                draft.setup.server_name = name.value().to_string();
                commands.insert_resource(ResumeHostSetup);
                commands.insert_resource(RulesOrigin(EditorOrigin::HostSetup));
                next.set(MenuScreen::Rules);
            },
        );
    let headers = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(headers),
        Node {
            width: px(INDEX_WIDTH),
            ..default()
        },
    ));
    for (caption, width) in [
        ("MAP", MAP_WIDTH),
        ("MODE", MODE_WIDTH),
        ("RULES", RULES_WIDTH),
    ] {
        commands.spawn((
            ChildOf(headers),
            Node {
                width: px(width),
                justify_content: JustifyContent::Center,
                border: UiRect::bottom(px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.55, 0.52, 0.38, 0.35)),
            label(&fonts, caption, HEADER_FONT, ACCENT),
        ));
    }
    commands.spawn((
        ChildOf(panel),
        RotationList,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4.0),
            min_height: px(60.0),
            ..default()
        },
    ));
    commands
        .spawn((
            ChildOf(panel),
            compact_button(&fonts, "Add entry (copy of the last)", 200.0, FONT),
        ))
        .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
            if draft.setup.rotation.len() < MAX_ENTRIES {
                let copy = draft
                    .setup
                    .rotation
                    .last()
                    .cloned()
                    .unwrap_or_else(|| HostSetup::default().rotation.remove(0));
                draft.setup.rotation.push(copy);
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
    // The Game Mode the sliders edit: a picker like the Rotation's.
    let (left, right, value) = picker(
        &mut commands,
        &fonts,
        &icon,
        limits,
        LIMIT_PICKER_WIDTH,
        draft.limit_mode.name(),
    );
    commands.entity(value).insert(LimitModeValue);
    for (button, delta) in [(left, -1), (right, 1)] {
        commands
            .entity(button)
            .observe(move |_: On<Activate>, mut draft: ResMut<Draft>| {
                if let Some(mode) = step_in(&GameModeId::ALL, &draft.limit_mode, delta) {
                    draft.limit_mode = mode;
                }
            });
    }
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

/// A picker of `width` px under `parent`: ◀, the current value, ▶. Returns the two arrow
/// buttons (to observe) and the value's text entity.
pub(super) fn picker(
    commands: &mut Commands,
    fonts: &UiFonts,
    icon: &ArrowIcon,
    parent: Entity,
    width: f32,
    value: &str,
) -> (Entity, Entity, Entity) {
    let picker = commands
        .spawn((
            ChildOf(parent),
            Node {
                width: px(width),
                align_items: AlignItems::Center,
                column_gap: px(2.0),
                ..default()
            },
        ))
        .id();
    let left = commands
        .spawn((ChildOf(picker), arrow_button(icon, false)))
        .id();
    let well = commands
        .spawn((
            ChildOf(picker),
            Node {
                flex_grow: 1.0,
                height: px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(VALUE_BACKGROUND),
        ))
        .id();
    let text = commands
        .spawn((
            ChildOf(well),
            label(fonts, &value.to_uppercase(), FONT, TEXT),
        ))
        .id();
    let right = commands
        .spawn((ChildOf(picker), arrow_button(icon, true)))
        .id();
    (left, right, text)
}

/// Moves one cell of a Rotation entry `delta` places along what it offers, keeping the entry's
/// Map and Game Mode consistent (a Map that lacks the Mode brings its first one).
fn step_entry(
    draft: &mut Draft,
    index: usize,
    column: Column,
    delta: i32,
    registry: &MapRegistry,
    data: &GameData,
) {
    let Some(mut entry) = draft.setup.rotation.get(index).cloned() else {
        return;
    };
    match column {
        Column::Map => {
            let maps: Vec<MapId> = registry.iter().map(|(id, _)| id.clone()).collect();
            if let Some(map) = step_in(&maps, &entry.map, delta) {
                entry.map = map;
            }
            let modes = modes_of(registry, &entry.map);
            if !modes.contains(&entry.mode)
                && let Some(&mode) = modes.first()
            {
                entry.mode = mode;
            }
        }
        Column::Mode => {
            if let Some(mode) = step_in(&modes_of(registry, &entry.map), &entry.mode, delta) {
                entry.mode = mode;
            }
        }
        Column::Rules => {
            if let Some(rules) = step_in(&data.rule_set_names(), &entry.rules, delta) {
                entry.rules = rules;
            }
        }
    }
    draft.setup.rotation[index] = entry;
}

/// Rebuilds the Rotation list and refreshes pickers and sliders when the draft changes.
#[allow(clippy::too_many_arguments)]
pub(super) fn refresh(
    draft: Res<Draft>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    icon: Res<ArrowIcon>,
    list: Single<Entity, With<RotationList>>,
    mut limit_mode: Single<&mut Text, With<LimitModeValue>>,
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
    let caption = draft.limit_mode.name().to_uppercase();
    if limit_mode.0 != caption {
        limit_mode.0 = caption;
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

    commands.entity(*list).despawn_related::<Children>();
    let count = draft.setup.rotation.len();
    for (index, entry) in draft.setup.rotation.iter().enumerate() {
        let row = commands
            .spawn((
                ChildOf(*list),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8.0),
                    ..default()
                },
            ))
            .id();
        commands.spawn((
            ChildOf(row),
            label(&fonts, &format!("{}.", index + 1), FONT, TEXT_DIM),
            Node {
                width: px(INDEX_WIDTH),
                ..default()
            },
        ));
        for (column, width, shown) in [
            (Column::Map, MAP_WIDTH, map_name(&entry.map)),
            (Column::Mode, MODE_WIDTH, entry.mode.name().to_owned()),
            (
                Column::Rules,
                RULES_WIDTH,
                shorten(&entry.rules, RULES_CHARS),
            ),
        ] {
            let (left, right, _) = picker(&mut commands, &fonts, &icon, row, width, &shown);
            for (arrow, delta) in [(left, -1), (right, 1)] {
                commands.entity(arrow).observe(
                    move |_: On<Activate>,
                          mut draft: ResMut<Draft>,
                          registry: Res<MapRegistry>,
                          data: Res<GameData>| {
                        step_entry(&mut draft, index, column, delta, &registry, &data);
                    },
                );
            }
        }
        for (caption, enabled, action) in [
            ("^", index > 0, RowAction::Up),
            ("v", index + 1 < count, RowAction::Down),
            ("X", count > 1, RowAction::Remove),
        ] {
            if !enabled {
                commands.spawn((
                    ChildOf(row),
                    Node {
                        width: px(34.0),
                        ..default()
                    },
                ));
                continue;
            }
            commands
                .spawn((ChildOf(row), compact_button(&fonts, caption, 34.0, FONT)))
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
    }
}

#[derive(Clone, Copy)]
enum RowAction {
    Up,
    Down,
    Remove,
}

/// Moves the user's slider changes into the draft. The limit sliders snap to their step (scores
/// by 5, times by a minute); a value the draft already shows is left alone, so an off-grid
/// default (Search & Destroy's 2.5 min) survives until someone moves the slider.
pub(super) fn read_sliders(
    changed: Query<(Entity, &SetupSlider, &SliderValue), Changed<SliderValue>>,
    mut draft: ResMut<Draft>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    for (entity, kind, value) in &changed {
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
                let (score, time) = limit_ranges(mode);
                let (shown, (min, max, step)) = match kind {
                    SetupSlider::Score => (limit.score_limit as f32, score),
                    _ => (limit.time_limit_secs as f32 / 60.0, time),
                };
                if (value.0 - shown).abs() < 1e-3 {
                    continue;
                }
                let snapped = snap(value.0, min, max, step);
                if (snapped - value.0).abs() > 1e-4 {
                    commands.entity(entity).insert(SliderValue(snapped));
                }
                match kind {
                    SetupSlider::Score => limit.score_limit = snapped.round() as u32,
                    _ => limit.time_limit_secs = (snapped * 60.0).round() as u32,
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
