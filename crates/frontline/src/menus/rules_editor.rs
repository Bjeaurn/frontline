//! The Custom Match Rules editor, opened from Settings and from the Host setup screen. The
//! built-in presets (`default`, `hardcore`) are read-only starting points: COPY makes a named
//! custom set from the selected one, and a custom set edits every field of `MatchRules`. DONE
//! saves the custom sets to `<config dir>/frontline/rules.ron` (and into `GameData`, so the
//! Rotation's RULES column offers them); BACK discards the edits.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderStep, SliderValue};

use crate::data::{DEFAULT_MATCH_RULES, GameData, MatchRules};
use crate::flow::{MenuScreen, Overlay};
use crate::player::input::Bindings;
use crate::rules::{CustomRules, MAX_NAME_CHARS, check_name, copy_name, save_custom_rules};
use crate::ui::{
    ACCENT, TEXT, TEXT_DIM, UiFonts, backdrop, button, compact_button, label, panel, slider,
    text_field, title,
};

const FONT: f32 = 18.0;
const CAPTION_WIDTH: f32 = 150.0;
const COLUMN_WIDTH: f32 = 395.0;
const SLIDER_WIDTH: f32 = 130.0;
const READOUT_WIDTH: f32 = 90.0;
const LIST_WIDTH: f32 = 220.0;
/// Rate a regen switched on starts at when the set had none.
const DEFAULT_REGEN_RATE: f32 = 40.0;
/// Length a Kill-cam switched on starts at when the set had none.
const DEFAULT_KILLCAM_SECS: f32 = 5.0;

/// Where the editor was opened from, and so where DONE and BACK return to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorOrigin {
    /// Settings in the main menu.
    Settings,
    /// Settings over the game (the pause menu's).
    PauseSettings,
    HostSetup,
}

impl EditorOrigin {
    fn go_back(self, menu: &mut NextState<MenuScreen>, overlay: &mut NextState<Overlay>) {
        match self {
            EditorOrigin::Settings => menu.set(MenuScreen::Settings),
            EditorOrigin::PauseSettings => overlay.set(Overlay::Settings),
            EditorOrigin::HostSetup => menu.set(MenuScreen::HostSetup),
        }
    }
}

/// Set by whoever opens the editor.
#[derive(Resource, Clone, Copy)]
pub(super) struct RulesOrigin(pub(super) EditorOrigin);

/// The editor's working copy: the custom sets as edited so far, and which set is selected (a
/// built-in preset or a custom one).
#[derive(Resource)]
pub(super) struct Editor {
    custom: CustomRules,
    selected: String,
    status: String,
    /// Bumped to rebuild the name field (emptying it).
    name_epoch: u32,
}

impl Editor {
    fn rules<'a>(&'a self, data: &'a GameData) -> &'a MatchRules {
        self.custom
            .get(&self.selected)
            .unwrap_or_else(|| data.rules(&self.selected))
    }

    fn is_custom(&self) -> bool {
        self.custom.contains_key(&self.selected)
    }

    fn taken(&self, data: &GameData, name: &str) -> bool {
        self.custom.contains_key(name)
            || (data.match_rules.contains_key(name) && !data.is_custom_rules(name))
    }
}

/// One editable value of `MatchRules`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    MaxHealth,
    Regen,
    RegenDelay,
    RegenRate,
    RespawnDelay,
    MinRespawn,
    Killcam,
    KillcamLength,
    DamageScale,
    FriendlyFire,
    FallMin,
    FallMax,
    Crosshair,
    Hitmarkers,
    KillHitmarker,
    KillFeed,
    KillFeedDelay,
    DamageDirection,
    NameTags,
    NameTagRange,
}

enum Control {
    Slider { min: f32, max: f32, step: f32 },
    Toggle,
}

impl Field {
    const SURVIVAL: [Field; 12] = [
        Field::MaxHealth,
        Field::Regen,
        Field::RegenDelay,
        Field::RegenRate,
        Field::RespawnDelay,
        Field::MinRespawn,
        Field::Killcam,
        Field::KillcamLength,
        Field::DamageScale,
        Field::FriendlyFire,
        Field::FallMin,
        Field::FallMax,
    ];
    const HUD: [Field; 8] = [
        Field::Crosshair,
        Field::Hitmarkers,
        Field::KillHitmarker,
        Field::KillFeed,
        Field::KillFeedDelay,
        Field::DamageDirection,
        Field::NameTags,
        Field::NameTagRange,
    ];

    fn caption(self) -> &'static str {
        match self {
            Field::MaxHealth => "Max health",
            Field::Regen => "Health regen",
            Field::RegenDelay => "Regen delay",
            Field::RegenRate => "Regen rate",
            Field::RespawnDelay => "Respawn delay",
            Field::MinRespawn => "Min respawn",
            Field::Killcam => "Kill-cam",
            Field::KillcamLength => "Kill-cam length",
            Field::DamageScale => "Damage scale",
            Field::FriendlyFire => "Friendly fire",
            Field::FallMin => "Fall dmg from",
            Field::FallMax => "Lethal fall at",
            Field::Crosshair => "Crosshair",
            Field::Hitmarkers => "Hitmarkers",
            Field::KillHitmarker => "Kill hitmarker",
            Field::KillFeed => "Kill feed",
            Field::KillFeedDelay => "Kill feed delay",
            Field::DamageDirection => "Damage direction",
            Field::NameTags => "Name tags",
            Field::NameTagRange => "Name tag range",
        }
    }

    fn control(self) -> Control {
        let slider = |min, max, step| Control::Slider { min, max, step };
        match self {
            Field::MaxHealth => slider(5.0, 500.0, 5.0),
            Field::RegenDelay | Field::RespawnDelay | Field::MinRespawn | Field::KillFeedDelay => {
                slider(0.0, 15.0, 0.5)
            }
            Field::KillcamLength => slider(1.0, 10.0, 0.5),
            Field::RegenRate => slider(5.0, 200.0, 5.0),
            Field::DamageScale => slider(0.25, 3.0, 0.05),
            Field::FallMin => slider(1.0, 20.0, 0.5),
            Field::FallMax => slider(2.0, 40.0, 0.5),
            Field::NameTagRange => slider(10.0, 100.0, 5.0),
            Field::Regen
            | Field::Killcam
            | Field::FriendlyFire
            | Field::Crosshair
            | Field::Hitmarkers
            | Field::KillHitmarker
            | Field::KillFeed
            | Field::DamageDirection
            | Field::NameTags => Control::Toggle,
        }
    }

    /// The field's value in `rules`; a toggle is 1 (on) or 0.
    fn get(self, rules: &MatchRules) -> f32 {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        match self {
            Field::MaxHealth => rules.max_health,
            Field::Regen => flag(rules.regen_per_sec > 0.0),
            Field::RegenDelay => rules.regen_delay_secs,
            Field::RegenRate => rules.regen_per_sec,
            Field::RespawnDelay => rules.respawn_delay_secs,
            Field::MinRespawn => rules.min_respawn_secs,
            Field::Killcam => flag(rules.killcam_secs > 0.0),
            Field::KillcamLength => rules.killcam_secs,
            Field::DamageScale => rules.damage_scale,
            Field::FriendlyFire => flag(rules.friendly_fire),
            Field::FallMin => rules.fall_damage.min_height,
            Field::FallMax => rules.fall_damage.max_height,
            Field::Crosshair => flag(rules.hud.crosshair),
            Field::Hitmarkers => flag(rules.hud.hitmarkers),
            Field::KillHitmarker => flag(rules.hud.kill_hitmarker),
            Field::KillFeed => flag(rules.hud.kill_feed),
            Field::KillFeedDelay => rules.hud.kill_feed_delay_secs,
            Field::DamageDirection => flag(rules.hud.damage_direction),
            Field::NameTags => flag(rules.hud.name_tags),
            Field::NameTagRange => rules.hud.name_tag_range_m,
        }
    }

    /// Sets the field, keeping the values that depend on it consistent (the earliest respawn
    /// never beyond the respawn delay, the lethal fall above where damage starts).
    fn set(self, rules: &mut MatchRules, value: f32) {
        let on = value >= 0.5;
        match self {
            Field::MaxHealth => rules.max_health = value,
            Field::Regen => {
                if !on {
                    rules.regen_per_sec = 0.0;
                } else if rules.regen_per_sec <= 0.0 {
                    rules.regen_per_sec = DEFAULT_REGEN_RATE;
                }
            }
            Field::RegenDelay => rules.regen_delay_secs = value,
            Field::RegenRate => rules.regen_per_sec = value,
            Field::RespawnDelay => {
                rules.respawn_delay_secs = value;
                rules.min_respawn_secs = rules.min_respawn_secs.min(value);
            }
            Field::MinRespawn => rules.min_respawn_secs = value.min(rules.respawn_delay_secs),
            Field::Killcam => {
                if !on {
                    rules.killcam_secs = 0.0;
                } else if rules.killcam_secs <= 0.0 {
                    rules.killcam_secs = DEFAULT_KILLCAM_SECS;
                }
            }
            Field::KillcamLength => rules.killcam_secs = value,
            Field::DamageScale => rules.damage_scale = value,
            Field::FriendlyFire => rules.friendly_fire = on,
            Field::FallMin => {
                rules.fall_damage.min_height = value;
                rules.fall_damage.max_height = rules.fall_damage.max_height.max(value + 1.0);
            }
            Field::FallMax => {
                rules.fall_damage.max_height = value.max(rules.fall_damage.min_height + 0.5);
            }
            Field::Crosshair => rules.hud.crosshair = on,
            Field::Hitmarkers => rules.hud.hitmarkers = on,
            Field::KillHitmarker => rules.hud.kill_hitmarker = on,
            Field::KillFeed => rules.hud.kill_feed = on,
            Field::KillFeedDelay => rules.hud.kill_feed_delay_secs = value,
            Field::DamageDirection => rules.hud.damage_direction = on,
            Field::NameTags => rules.hud.name_tags = on,
            Field::NameTagRange => rules.hud.name_tag_range_m = value,
        }
    }

    /// `value` as the editor shows it.
    fn show(self, value: f32) -> String {
        match self {
            Field::MaxHealth => format!("{value:.0} HP"),
            Field::RegenRate if value <= 0.0 => "off".to_owned(),
            Field::RegenRate => format!("{value:.0} HP/s"),
            Field::KillcamLength if value <= 0.0 => "off".to_owned(),
            Field::RegenDelay
            | Field::RespawnDelay
            | Field::MinRespawn
            | Field::KillcamLength
            | Field::KillFeedDelay => {
                format!("{value:.1} s")
            }
            Field::DamageScale => format!("x{value:.2}"),
            Field::FallMin | Field::FallMax => format!("{value:.1} m"),
            Field::NameTagRange => format!("{value:.0} m"),
            Field::Regen
            | Field::Killcam
            | Field::FriendlyFire
            | Field::Crosshair
            | Field::Hitmarkers
            | Field::KillHitmarker
            | Field::KillFeed
            | Field::DamageDirection
            | Field::NameTags => if value >= 0.5 { "ON" } else { "OFF" }.to_owned(),
        }
    }
}

/// `value` rounded to a multiple of `step` counted from `min`, kept inside `min..=max`, with
/// the float noise of the multiplication rounded away.
fn snap(value: f32, min: f32, max: f32, step: f32) -> f32 {
    let snapped = (min + ((value - min) / step).round() * step).clamp(min, max);
    (snapped * 1000.0).round() / 1000.0
}

#[derive(Component)]
pub(super) struct SetList;
#[derive(Component)]
pub(super) struct NameSlot(u32);
#[derive(Component)]
struct NameField;
#[derive(Component)]
pub(super) struct StatusLine;
/// The pane with the controls of a custom set.
#[derive(Component)]
pub(super) struct EditPane;
/// The pane showing a built-in preset's values.
#[derive(Component)]
pub(super) struct ReadPane;
/// A text showing a field's value.
#[derive(Component)]
pub(super) struct FieldValue(Field);
#[derive(Component)]
pub(super) struct FieldSlider(Field);
#[derive(Component)]
pub(super) struct FieldToggle(Field);

pub(super) fn spawn_menu_rules(
    commands: Commands,
    fonts: Res<UiFonts>,
    data: Res<GameData>,
    origin: Res<RulesOrigin>,
) {
    spawn_editor(commands, &fonts, &data, *origin, true);
}

pub(super) fn spawn_overlay_rules(
    commands: Commands,
    fonts: Res<UiFonts>,
    data: Res<GameData>,
    origin: Res<RulesOrigin>,
) {
    spawn_editor(commands, &fonts, &data, *origin, false);
}

fn spawn_editor(
    mut commands: Commands,
    fonts: &UiFonts,
    data: &GameData,
    origin: RulesOrigin,
    in_menu: bool,
) {
    let origin = origin.0;
    commands.insert_resource(Editor {
        custom: data.custom_rules(),
        selected: DEFAULT_MATCH_RULES.to_owned(),
        status: "Built-in presets are read-only: pick one and COPY it to make your own.".to_owned(),
        name_epoch: 0,
    });
    let root = commands
        .spawn((
            Name::new("Match Rules editor"),
            backdrop(),
            GlobalZIndex(10),
        ))
        .id();
    if in_menu {
        commands
            .entity(root)
            .insert(DespawnOnExit(MenuScreen::Rules));
    } else {
        commands.entity(root).insert(DespawnOnExit(Overlay::Rules));
    }
    let panel = commands
        .spawn((ChildOf(root), panel()))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(20.0)),
            row_gap: px(8.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();
    commands.spawn((ChildOf(panel), title(fonts, "Match Rules", 40.0)));

    let body = commands
        .spawn((
            ChildOf(panel),
            Node {
                column_gap: px(18.0),
                ..default()
            },
        ))
        .id();

    // Left: the sets, then COPY and DELETE.
    let left = commands
        .spawn((
            ChildOf(body),
            Node {
                width: px(LIST_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((ChildOf(left), label(fonts, "SETS", FONT, ACCENT)));
    commands.spawn((
        ChildOf(left),
        SetList,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(left),
        label(fonts, "NEW SET NAME", 16.0, TEXT_DIM),
        Node {
            margin: UiRect::top(px(10.0)),
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(left),
        NameSlot(u32::MAX),
        Node {
            width: px(LIST_WIDTH),
            ..default()
        },
    ));
    commands
        .spawn((
            ChildOf(left),
            compact_button(fonts, "Copy selected", LIST_WIDTH, FONT),
        ))
        .observe(copy_selected);
    commands
        .spawn((
            ChildOf(left),
            compact_button(fonts, "Delete selected", LIST_WIDTH, FONT),
        ))
        .observe(delete_selected);

    // Right: the controls of a custom set, or the values of a built-in preset.
    let right = commands
        .spawn((
            ChildOf(body),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
        ))
        .id();
    for editable in [true, false] {
        let pane = commands
            .spawn((
                ChildOf(right),
                Node {
                    column_gap: px(20.0),
                    ..default()
                },
            ))
            .id();
        if editable {
            commands.entity(pane).insert(EditPane);
        } else {
            commands.entity(pane).insert(ReadPane);
        }
        for (heading, fields) in [("SURVIVAL", &Field::SURVIVAL[..]), ("HUD", &Field::HUD[..])] {
            let column = commands
                .spawn((
                    ChildOf(pane),
                    Node {
                        width: px(COLUMN_WIDTH),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        ..default()
                    },
                ))
                .id();
            commands.spawn((ChildOf(column), label(fonts, heading, FONT, ACCENT)));
            for &field in fields {
                field_row(&mut commands, fonts, column, field, editable);
            }
        }
    }

    commands.spawn((
        ChildOf(panel),
        StatusLine,
        label(fonts, "", FONT, ACCENT),
        Node {
            min_height: px(24.0),
            ..default()
        },
    ));
    let buttons = commands
        .spawn((
            ChildOf(panel),
            Node {
                column_gap: px(12.0),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((ChildOf(buttons), button(fonts, "Done")))
        .observe(
            move |_: On<Activate>,
                  mut editor: ResMut<Editor>,
                  mut data: ResMut<GameData>,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                if let Some((name, err)) = editor
                    .custom
                    .iter()
                    .find_map(|(name, rules)| Some((name.clone(), rules.validate().err()?)))
                {
                    editor.status = format!("\"{name}\": {err}");
                    return;
                }
                save_custom_rules(&editor.custom);
                data.use_custom_rules(editor.custom.clone());
                origin.go_back(&mut menu, &mut overlay);
            },
        );
    commands
        .spawn((ChildOf(buttons), button(fonts, "Back")))
        .observe(
            move |_: On<Activate>,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                origin.go_back(&mut menu, &mut overlay);
            },
        );
}

/// A field's caption and, when `editable`, its control; its value text either way.
fn field_row(
    commands: &mut Commands,
    fonts: &UiFonts,
    column: Entity,
    field: Field,
    editable: bool,
) {
    let row = commands
        .spawn((
            ChildOf(column),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                height: px(26.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(row),
        label(fonts, &field.caption().to_uppercase(), FONT, TEXT_DIM),
        Node {
            width: px(CAPTION_WIDTH),
            ..default()
        },
    ));
    if editable {
        match field.control() {
            Control::Slider { min, max, step } => {
                commands
                    .spawn((
                        ChildOf(row),
                        FieldSlider(field),
                        slider(min, min, max),
                        SliderStep(step),
                    ))
                    .insert(Node {
                        width: px(SLIDER_WIDTH),
                        height: px(22.0),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        ..default()
                    });
            }
            Control::Toggle => {
                commands
                    .spawn((
                        ChildOf(row),
                        FieldToggle(field),
                        compact_button(fonts, "", SLIDER_WIDTH, FONT),
                    ))
                    .observe(
                        move |_: On<Activate>, mut editor: ResMut<Editor>, data: Res<GameData>| {
                            if !editor.is_custom() {
                                return;
                            }
                            let selected = editor.selected.clone();
                            let current = field.get(editor.rules(&data));
                            if let Some(rules) = editor.custom.get_mut(&selected) {
                                field.set(rules, if current >= 0.5 { 0.0 } else { 1.0 });
                            }
                        },
                    );
            }
        }
    }
    // A toggle's button already says ON or OFF.
    if editable && matches!(field.control(), Control::Toggle) {
        return;
    }
    commands.spawn((
        ChildOf(row),
        FieldValue(field),
        label(fonts, "", FONT, TEXT),
        Node {
            width: px(READOUT_WIDTH),
            ..default()
        },
    ));
}

fn copy_selected(
    _: On<Activate>,
    name: Single<&EditableText, With<NameField>>,
    mut editor: ResMut<Editor>,
    data: Res<GameData>,
) {
    let typed = name.value().to_string();
    let chosen = if typed.trim().is_empty() {
        Ok(copy_name(&editor.selected, |name| {
            editor.taken(&data, name)
        }))
    } else {
        check_name(&typed, |name| editor.taken(&data, name))
    };
    match chosen {
        Ok(new) => {
            let rules = editor.rules(&data).clone();
            editor.status = format!(
                "Copied \"{}\" as \"{new}\"; edit it on the right.",
                editor.selected
            );
            editor.custom.insert(new.clone(), rules);
            editor.selected = new;
            editor.name_epoch += 1;
        }
        Err(err) => editor.status = err,
    }
}

fn delete_selected(_: On<Activate>, mut editor: ResMut<Editor>) {
    if !editor.is_custom() {
        editor.status = "Built-in presets can't be deleted.".to_owned();
        return;
    }
    let gone = editor.selected.clone();
    editor.custom.remove(&gone);
    editor.selected = DEFAULT_MATCH_RULES.to_owned();
    editor.status = format!("Deleted \"{gone}\".");
}

/// Brings the screen in step with the editor: the list of sets, which pane shows, every value,
/// the status line and the name field.
#[allow(clippy::too_many_arguments)]
pub(super) fn refresh(
    editor: Res<Editor>,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    list: Single<Entity, With<SetList>>,
    mut slot: Single<(Entity, &mut NameSlot)>,
    mut status: Single<&mut Text, With<StatusLine>>,
    mut panes: Query<(&mut Node, Has<EditPane>), Or<(With<EditPane>, With<ReadPane>)>>,
    mut values: Query<(&FieldValue, &mut Text), Without<StatusLine>>,
    toggles: Query<(&FieldToggle, &Children)>,
    sliders: Query<(Entity, &FieldSlider, &SliderValue)>,
    mut texts: Query<&mut Text, (Without<FieldValue>, Without<StatusLine>)>,
    mut commands: Commands,
) {
    if !editor.is_changed() {
        return;
    }
    let custom = editor.is_custom();
    for (mut node, is_edit) in &mut panes {
        node.display = if is_edit == custom {
            Display::Flex
        } else {
            Display::None
        };
    }
    if status.0 != editor.status {
        status.0 = editor.status.clone();
    }
    let rules = editor.rules(&data);
    for (value, mut text) in &mut values {
        let shown = value.0.show(value.0.get(rules));
        if text.0 != shown {
            text.0 = shown;
        }
    }
    for (toggle, children) in &toggles {
        let shown = toggle.0.show(toggle.0.get(rules));
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != shown
            {
                text.0 = shown.clone();
            }
        }
    }
    for (entity, field, value) in &sliders {
        let target = field.0.get(rules);
        if (value.0 - target).abs() > 1e-3 {
            commands.entity(entity).insert(SliderValue(target));
        }
    }

    commands.entity(*list).despawn_related::<Children>();
    for name in data
        .rule_set_names()
        .into_iter()
        .filter(|name| !data.is_custom_rules(name))
        .chain(editor.custom.keys().cloned())
    {
        let builtin = !editor.custom.contains_key(&name);
        let marker = if name == editor.selected { "> " } else { "" };
        let shown = format!("{marker}{name}{}", if builtin { " (built-in)" } else { "" });
        commands
            .spawn((
                ChildOf(*list),
                compact_button(&fonts, &shown, LIST_WIDTH, FONT),
            ))
            .observe(move |_: On<Activate>, mut editor: ResMut<Editor>| {
                editor.selected = name.clone();
            });
    }

    let (slot_entity, slot_epoch) = &mut *slot;
    if slot_epoch.0 != editor.name_epoch {
        slot_epoch.0 = editor.name_epoch;
        commands.entity(*slot_entity).despawn_related::<Children>();
        commands
            .spawn((
                ChildOf(*slot_entity),
                NameField,
                text_field(&fonts, "", MAX_NAME_CHARS),
            ))
            .insert(Node {
                width: px(LIST_WIDTH),
                padding: UiRect::axes(px(8.0), px(3.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            });
    }
}

/// Moves the user's slider changes into the selected custom set. A value the set already has
/// is left alone, so refreshing the sliders never edits anything.
pub(super) fn read_sliders(
    changed: Query<(Entity, &FieldSlider, &SliderValue), Changed<SliderValue>>,
    mut editor: ResMut<Editor>,
    data: Res<GameData>,
    mut commands: Commands,
) {
    if !editor.is_custom() {
        return;
    }
    for (entity, field, value) in &changed {
        let Control::Slider { min, max, step } = field.0.control() else {
            continue;
        };
        if (value.0 - field.0.get(editor.rules(&data))).abs() < 1e-3 {
            continue;
        }
        let snapped = snap(value.0, min, max, step);
        if (snapped - value.0).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(snapped));
        }
        let selected = editor.selected.clone();
        if let Some(rules) = editor.custom.get_mut(&selected) {
            field.0.set(rules, snapped);
        }
    }
}

/// Esc leaves the editor like BACK.
pub(super) fn esc_back(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    origin: Res<RulesOrigin>,
    mut menu: ResMut<NextState<MenuScreen>>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    if bindings.pause.just_pressed(&keys) {
        origin.0.go_back(&mut menu, &mut overlay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_keep_dependent_values_consistent() {
        let mut rules = GameData::load().rules("default").clone();
        Field::RespawnDelay.set(&mut rules, 1.0);
        assert_eq!(rules.min_respawn_secs, 1.0);
        Field::MinRespawn.set(&mut rules, 9.0);
        assert_eq!(rules.min_respawn_secs, 1.0);
        Field::FallMin.set(&mut rules, 15.0);
        assert!(rules.fall_damage.max_height > 15.0);
        Field::FallMax.set(&mut rules, 3.0);
        assert!(rules.fall_damage.max_height > rules.fall_damage.min_height);
        Field::Regen.set(&mut rules, 0.0);
        assert_eq!(Field::Regen.get(&rules), 0.0);
        Field::Regen.set(&mut rules, 1.0);
        assert_eq!(rules.regen_per_sec, DEFAULT_REGEN_RATE);
        Field::Killcam.set(&mut rules, 0.0);
        assert_eq!(rules.killcam_secs, 0.0);
        assert_eq!(Field::Killcam.get(&rules), 0.0);
        Field::Killcam.set(&mut rules, 1.0);
        assert_eq!(rules.killcam_secs, DEFAULT_KILLCAM_SECS);
        Field::KillcamLength.set(&mut rules, 8.0);
        Field::Killcam.set(&mut rules, 1.0);
        assert_eq!(
            rules.killcam_secs, 8.0,
            "switching on again keeps the length"
        );
        assert_eq!(rules.validate(), Ok(()));
    }

    #[test]
    fn every_field_round_trips_through_its_control() {
        let rules = GameData::load().rules("hardcore").clone();
        for field in Field::SURVIVAL.into_iter().chain(Field::HUD) {
            let mut copy = rules.clone();
            field.set(&mut copy, field.get(&rules));
            assert_eq!(copy, rules, "{field:?}");
        }
        assert_eq!(snap(41.3, 5.0, 500.0, 5.0), 40.0);
        assert_eq!(snap(1.07, 0.25, 3.0, 0.05), 1.05);
        assert_eq!(snap(900.0, 5.0, 500.0, 5.0), 500.0);
    }
}
