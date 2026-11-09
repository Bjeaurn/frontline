//! Create a Class: the screens (class list, slot overview, the Primary's own screen, the
//! full-screen pickers), built under the editor's content root whenever the screen changes, the
//! dropdown list that floats over the overview for the small slots, and the preview panels that
//! follow the highlight.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::model::{self, Choice, PAGE_SIZE, Pick, Slot};
use super::{
    Act, Action, DropdownUi, Editor, KEY_BACK, KEY_EDIT, KEY_LIST, KEY_MORE_DOWN, KEY_MORE_UP,
    KEY_NAME, KEY_NEXT, KEY_PREV, KEY_SAVE, Look, NameField, Nav, PreviewPanel, Screen, Status,
    class_choice, key_of,
};
use crate::class::EquipmentSlot;
use crate::data::GameData;
use crate::gear::{AttachmentSlot, Loadout, MAX_LOADOUT_NAME_CHARS, NO_CAMO, PerkEffects};
use crate::loadout::stats;
use crate::profile::Profile;
use crate::ui::{self, UiFonts};

/// A locked or refused thing, and a cost that doesn't fit.
pub(super) const RED: Color = Color::srgb(0.85, 0.42, 0.34);
const PIP_EMPTY: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const TILE_W: f32 = 285.0;
const TILE_H: f32 = 206.0;
const GAP: f32 = 12.0;
const GUN_TILE_W: f32 = 262.0;
const GUN_TILE_H: f32 = 168.0;
const LIST_W: f32 = 440.0;
const PICKER_W: f32 = 580.0;
pub(super) const PREVIEW_W: f32 = 560.0;

/// Everything a screen is drawn from.
pub(super) struct Ctx<'a> {
    pub fonts: &'a UiFonts,
    pub data: &'a GameData,
    pub profile: &'a Profile,
    pub editor: &'a Editor,
}

impl Look {
    fn tile() -> Look {
        Look {
            idle: Color::srgba(0.04, 0.045, 0.035, 0.92),
            idle_border: Color::srgba(0.40, 0.38, 0.27, 0.6),
            lit: Color::srgba(0.20, 0.17, 0.07, 0.96),
            lit_border: ui::AMBER,
        }
    }

    fn row() -> Look {
        Look {
            idle: Color::srgba(0.03, 0.035, 0.028, 0.8),
            idle_border: Color::NONE,
            lit: Color::srgba(0.24, 0.20, 0.08, 0.96),
            lit_border: ui::AMBER,
        }
    }

    fn button() -> Look {
        Look {
            idle: Color::srgba(0.03, 0.035, 0.028, 0.80),
            idle_border: Color::srgba(0.36, 0.35, 0.24, 0.9),
            lit: Color::srgba(0.24, 0.21, 0.09, 0.92),
            lit_border: ui::AMBER,
        }
    }

    fn field() -> Look {
        Look {
            idle: Color::srgba(0.02, 0.025, 0.02, 0.95),
            idle_border: Color::srgba(0.50, 0.48, 0.34, 0.5),
            lit: Color::srgba(0.02, 0.025, 0.02, 0.95),
            lit_border: ui::AMBER,
        }
    }
}

/// Draws the editor's current screen under `content`.
pub(super) fn build(ctx: &Ctx, commands: &mut Commands, content: Entity) {
    match ctx.editor.screen {
        Screen::List => list(ctx, commands, content),
        Screen::Overview => overview(ctx, commands, content),
        Screen::Gun => gun(ctx, commands, content),
        Screen::Picker(slot) => picker(ctx, commands, content, slot),
    }
}

// ---------------------------------------------------------------- small pieces

fn text(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    size: f32,
    color: Color,
) -> Entity {
    commands
        .spawn((ui::label(ctx.fonts, text, size, color), ChildOf(parent)))
        .id()
}

fn bold(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    size: f32,
    color: Color,
) -> Entity {
    commands
        .spawn((bold_bundle(ctx.fonts, text, size, color), ChildOf(parent)))
        .id()
}

fn bold_bundle(fonts: &UiFonts, text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: fonts.bold.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn row(commands: &mut Commands, parent: Entity, gap: f32) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(gap),
                ..default()
            },
            ChildOf(parent),
        ))
        .id()
}

fn column(commands: &mut Commands, parent: Entity, gap: f32) -> Entity {
    commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(gap),
                ..default()
            },
            ChildOf(parent),
        ))
        .id()
}

/// `1 PT`, `2 PTS`, `FREE`.
fn pts(cost: u32) -> String {
    match cost {
        0 => "FREE".into(),
        1 => "1 PT".into(),
        n => format!("{n} PTS"),
    }
}

/// Two or three letters standing for a thing (its icon): `RD` for Red Dot, `AR` for the M4.
fn monogram(name: &str) -> String {
    if let (Some(open), Some(close)) = (name.find('('), name.find(')'))
        && open < close
    {
        return name[open + 1..close].to_uppercase();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.as_slice() {
        [] => String::new(),
        [one] if one.chars().count() <= 4 => one.to_uppercase(),
        [one] => one.chars().take(3).collect::<String>().to_uppercase(),
        many => many
            .iter()
            .filter_map(|word| word.chars().next())
            .take(3)
            .collect::<String>()
            .to_uppercase(),
    }
}

fn badge(ctx: &Ctx, commands: &mut Commands, parent: Entity, name: &str, tint: Color) {
    commands
        .spawn((
            Node {
                min_width: px(44.0),
                height: px(38.0),
                padding: UiRect::horizontal(px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(tint),
            ChildOf(parent),
        ))
        .with_child(bold_bundle(ctx.fonts, &monogram(name), 16.0, tint));
}

fn swatch(commands: &mut Commands, parent: Entity, camo: &str, data: &GameData, size: f32) {
    let (r, g, b) = data
        .progression
        .camo(camo)
        .filter(|_| camo != NO_CAMO)
        .map_or((0.1, 0.1, 0.1), |camo| camo.swatch);
    commands.spawn((
        Node {
            width: px(size),
            height: px(size),
            border: UiRect::all(px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgb(r, g, b)),
        BorderColor::all(ui::PANEL_BORDER),
        ChildOf(parent),
    ));
}

/// A selectable thing: a mouse target and a stop of the keyboard cursor.
fn item(
    commands: &mut Commands,
    parent: Entity,
    nav: Nav,
    act: Act,
    look: Look,
    node: Node,
) -> Entity {
    commands
        .spawn((
            Button,
            Hovered::default(),
            nav,
            Action(act),
            look,
            BackgroundColor(look.idle),
            BorderColor::all(look.idle_border),
            node,
            ChildOf(parent),
        ))
        .id()
}

fn push_button(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    nav: Nav,
    act: Act,
) -> Entity {
    let button = item(
        commands,
        parent,
        nav,
        act,
        Look::button(),
        Node {
            min_width: px(120.0),
            padding: UiRect::axes(px(16.0), px(6.0)),
            border: UiRect::left(px(4.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    bold(ctx, commands, button, &label.to_uppercase(), 24.0, ui::TEXT);
    button
}

/// The points meter: `7 / 10` and one pip per point of the budget.
fn meter(ctx: &Ctx, commands: &mut Commands, parent: Entity, spent: u32) {
    let budget = ctx.data.class_points.budget;
    let tint = if spent > budget { RED } else { ui::ACCENT };
    let meter = row(commands, parent, 12.0);
    text(ctx, commands, meter, "POINTS", 20.0, ui::TEXT_DIM);
    bold(
        ctx,
        commands,
        meter,
        &format!("{spent} / {budget}"),
        40.0,
        tint,
    );
    let pips = row(commands, meter, 4.0);
    for index in 0..budget {
        commands.spawn((
            Node {
                width: px(20.0),
                height: px(18.0),
                ..default()
            },
            BackgroundColor(if index < spent { tint } else { PIP_EMPTY }),
            ChildOf(pips),
        ));
    }
}

/// The title row; `right` fills the right-hand end.
fn header(ctx: &Ctx, commands: &mut Commands, content: Entity, title: &str) -> Entity {
    let header = commands
        .spawn((
            Node {
                width: px(4.0 * TILE_W + 3.0 * GAP),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    commands.spawn((ui::title(ctx.fonts, title, 40.0), ChildOf(header)));
    header
}

fn status_line(ctx: &Ctx, commands: &mut Commands, parent: Entity) {
    commands.spawn((
        Status,
        ui::label(ctx.fonts, "", 22.0, RED),
        Node {
            min_height: px(26.0),
            ..default()
        },
        ChildOf(parent),
    ));
}

fn footer(ctx: &Ctx, commands: &mut Commands, content: Entity, hint: &str) {
    commands.spawn((
        ui::label(ctx.fonts, hint, 18.0, ui::TEXT_DIM),
        Node {
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        ChildOf(content),
    ));
}

/// The custom class being edited, named for the header: `CUSTOM 1`.
fn editing_label(editor: &Editor) -> String {
    format!("CUSTOM {}", editor.slot + 1)
}

// ------------------------------------------------------------------ class list

fn list(ctx: &Ctx, commands: &mut Commands, content: Entity) {
    let header = header(ctx, commands, content, "Create a Class");
    let progress = ctx.profile.level_progress(ctx.data);
    let level = if progress.maxed() {
        format!("LEVEL {}  -  MAX", progress.level)
    } else {
        format!(
            "LEVEL {}  -  {} / {} XP",
            progress.level, progress.into_level, progress.span
        )
    };
    text(ctx, commands, header, &level, 22.0, ui::ACCENT);

    let columns = commands
        .spawn((
            Node {
                align_items: AlignItems::Stretch,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    let left = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(LIST_W),
            padding: UiRect::all(px(20.0)),
            row_gap: px(6.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();
    let defaults = ctx.data.default_loadouts.len();
    text(ctx, commands, left, "DEFAULT CLASSES", 18.0, ui::TEXT_DIM);
    for index in 0..ctx.editor.classes {
        if index == defaults {
            let label = text(ctx, commands, left, "CUSTOM CLASSES", 18.0, ui::TEXT_DIM);
            commands.entity(label).insert(Node {
                margin: UiRect::top(px(8.0)),
                ..default()
            });
        }
        class_row(ctx, commands, left, index);
    }
    let buttons = row(commands, left, 12.0);
    commands.entity(buttons).insert(Node {
        margin: UiRect::top(px(12.0)),
        column_gap: px(12.0),
        ..default()
    });
    let edit = push_button(
        ctx,
        commands,
        buttons,
        "Edit",
        Nav::new(KEY_EDIT, 0.0, 7.0),
        Act::Edit,
    );
    // Dimmed by `refresh` while the highlighted class can't be edited.
    commands.entity(edit).insert(super::EditButton);
    push_button(
        ctx,
        commands,
        buttons,
        "Back",
        Nav::new(KEY_BACK, 0.0, 8.0),
        Act::Back,
    );

    commands
        .spawn((ui::panel(), PreviewPanel, ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(4.0 * TILE_W + 3.0 * GAP - LIST_W - 16.0),
            padding: UiRect::all(px(24.0)),
            row_gap: px(6.0),
            border: UiRect::all(px(1.0)),
            ..default()
        });
    status_line(ctx, commands, content);
    footer(
        ctx,
        commands,
        content,
        "UP / DOWN  Choose        ENTER  Edit        ESC  Back",
    );
}

fn class_row(ctx: &Ctx, commands: &mut Commands, parent: Entity, index: usize) {
    let choice = class_choice(index, ctx.data.default_loadouts.len());
    let loadout = ctx.profile.get(choice, ctx.data);
    let locked = !ctx.profile.locks(choice, ctx.data).is_empty();
    let line = item(
        commands,
        parent,
        Nav::new(
            index,
            0.0,
            index as f32 + f32::from(index >= ctx.data.default_loadouts.len()),
        ),
        Act::Class(index),
        Look::row(),
        Node {
            width: percent(100.0),
            height: px(46.0),
            padding: UiRect::axes(px(14.0), px(4.0)),
            border: UiRect::left(px(4.0)),
            column_gap: px(12.0),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let name = bold(
        ctx,
        commands,
        line,
        &loadout.name.to_uppercase(),
        26.0,
        if locked { ui::TEXT_DIM } else { ui::TEXT },
    );
    commands.entity(name).insert(Node {
        flex_grow: 1.0,
        ..default()
    });
    text(
        ctx,
        commands,
        line,
        &ctx.data.weapon(loadout.primary).name,
        18.0,
        ui::TEXT_DIM,
    );
    let tag = if locked {
        Some(("LOCKED", RED))
    } else if ctx.profile.chosen == choice {
        Some(("IN USE", ui::ACCENT))
    } else {
        None
    };
    if let Some((tag, color)) = tag {
        text(ctx, commands, line, tag, 15.0, color);
    }
}

/// What a class holds, one labelled row each.
fn summary_rows(loadout: &Loadout, data: &GameData) -> Vec<(String, String)> {
    let or_none = |names: Vec<String>| {
        if names.is_empty() {
            "None".to_owned()
        } else {
            names.join(", ")
        }
    };
    let equipment = |slot: EquipmentSlot| {
        loadout.equipment(slot).map_or("None".to_owned(), |id| {
            data.class_points.equipment[&id].name.clone()
        })
    };
    let mut rows = vec![
        (
            "PRIMARY".to_owned(),
            data.weapon(loadout.primary).name.clone(),
        ),
        (
            "ATTACHMENTS".to_owned(),
            or_none(
                loadout
                    .attachments
                    .iter()
                    .map(|id| data.attachment(id).name.clone())
                    .collect(),
            ),
        ),
        (
            "CAMO".to_owned(),
            data.progression
                .camo(&loadout.camo)
                .filter(|_| loadout.camo != NO_CAMO)
                .map_or("None".to_owned(), |camo| camo.name.clone()),
        ),
        (
            "SECONDARY".to_owned(),
            loadout
                .secondary
                .map_or("None".to_owned(), |id| data.weapon(id).name.clone()),
        ),
        ("LETHAL".to_owned(), equipment(EquipmentSlot::Lethal)),
        ("TACTICAL".to_owned(), equipment(EquipmentSlot::Tactical)),
    ];
    for slot in 0..crate::gear::PERK_SLOTS {
        rows.push((
            format!("PERK {}", slot + 1),
            or_none(
                loadout
                    .perks
                    .in_slot(slot)
                    .map(|id| data.perk(id).name.clone())
                    .collect(),
            ),
        ));
    }
    rows.push((
        "WILDCARDS".to_owned(),
        or_none(
            loadout
                .wildcards
                .iter()
                .map(|id| data.wildcard_name(*id).to_owned())
                .collect(),
        ),
    ));
    rows
}

/// The right panel of the class list: the highlighted class.
pub(super) fn class_summary(ctx: &Ctx, commands: &mut Commands, panel: Entity) {
    let choice = class_choice(ctx.editor.class, ctx.data.default_loadouts.len());
    let loadout = ctx.profile.get(choice, ctx.data);
    let top = row(commands, panel, 16.0);
    commands.spawn((ui::title(ctx.fonts, &loadout.name, 34.0), ChildOf(top)));
    let kind = if matches!(choice, crate::profile::LoadoutChoice::Default(_)) {
        "DEFAULT  -  READ-ONLY"
    } else {
        "CUSTOM"
    };
    text(ctx, commands, top, kind, 16.0, ui::TEXT_DIM);
    meter(ctx, commands, panel, ctx.data.class_cost(loadout));
    let lines = column(commands, panel, 2.0);
    commands.entity(lines).insert(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(2.0),
        margin: UiRect::top(px(10.0)),
        ..default()
    });
    for (label, value) in summary_rows(loadout, ctx.data) {
        let line = row(commands, lines, 12.0);
        let name = text(ctx, commands, line, &label, 18.0, ui::TEXT_DIM);
        commands.entity(name).insert(Node {
            width: px(130.0),
            ..default()
        });
        let tint = if value == "None" {
            ui::TEXT_DIM
        } else {
            ui::TEXT
        };
        text(ctx, commands, line, &value, 24.0, tint);
    }
    for lock in ctx.profile.locks(choice, ctx.data) {
        let note = text(ctx, commands, panel, &lock, 17.0, RED);
        commands.entity(note).insert(Node {
            max_width: px(620.0),
            ..default()
        });
    }
}

// -------------------------------------------------------------- slot overview

/// The tiles of the overview: slot, column and row.
const TILES: [(Slot, f32, f32); 8] = [
    (Slot::Primary, 0.0, 0.0),
    (Slot::Secondary, 1.0, 0.0),
    (Slot::Equipment(EquipmentSlot::Lethal), 2.0, 0.0),
    (Slot::Equipment(EquipmentSlot::Tactical), 3.0, 0.0),
    (Slot::Perk(0), 0.0, 1.0),
    (Slot::Perk(1), 1.0, 1.0),
    (Slot::Perk(2), 2.0, 1.0),
    (Slot::Wildcards, 3.0, 1.0),
];

/// Name of what `slot` of `loadout` holds, if anything.
fn held(ctx: &Ctx, loadout: &Loadout, slot: Slot) -> Option<String> {
    let data = ctx.data;
    match slot {
        Slot::Primary => Some(data.weapon(loadout.primary).name.clone()),
        Slot::Secondary => loadout.secondary.map(|id| data.weapon(id).name.clone()),
        Slot::Attachment(index) => loadout
            .attachments
            .iter()
            .nth(index)
            .map(|id| data.attachment(id).name.clone()),
        Slot::Camo => data
            .progression
            .camo(&loadout.camo)
            .filter(|_| loadout.camo != NO_CAMO)
            .map(|camo| camo.name.clone()),
        Slot::Perk(slot) => loadout.perks.picks[slot].map(|id| data.perk(id).name.clone()),
        Slot::Greed(slot) => loadout.perks.greed[slot].map(|id| data.perk(id).name.clone()),
        Slot::Equipment(slot) => loadout
            .equipment(slot)
            .map(|id| data.class_points.equipment[&id].name.clone()),
        Slot::Wildcards => None,
    }
}

/// A tile with its label and cost on top; the caller fills the body.
fn tile(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    nav: Nav,
    slot: Slot,
    size: (f32, f32),
    cost: Option<u32>,
) -> Entity {
    let tile = item(
        commands,
        parent,
        nav,
        Act::Open(slot),
        Look::tile(),
        Node {
            flex_direction: FlexDirection::Column,
            width: px(size.0),
            height: px(size.1),
            padding: UiRect::all(px(12.0)),
            row_gap: px(6.0),
            border: UiRect::all(px(2.0)),
            ..default()
        },
    );
    let top = commands
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ChildOf(tile),
        ))
        .id();
    text(ctx, commands, top, &slot.label(), 17.0, ui::TEXT_DIM);
    if let Some(cost) = cost {
        text(ctx, commands, top, &pts(cost), 17.0, ui::ACCENT);
    }
    tile
}

/// The held pick as a badge and a name, `NONE` when empty, and what still locks it.
fn pick_body(
    ctx: &Ctx,
    commands: &mut Commands,
    tile: Entity,
    loadout: &Loadout,
    slot: Slot,
    sub: Option<String>,
) {
    let name = held(ctx, loadout, slot);
    let line = row(commands, tile, 10.0);
    match &name {
        Some(name) => {
            if slot == Slot::Camo {
                swatch(commands, line, &loadout.camo, ctx.data, 38.0);
            } else {
                badge(ctx, commands, line, name, ui::ACCENT);
            }
            bold(ctx, commands, line, &name.to_uppercase(), 24.0, ui::TEXT);
        }
        None => {
            badge(ctx, commands, line, "-", ui::TEXT_DIM);
            bold(ctx, commands, line, "NONE", 24.0, ui::TEXT_DIM);
        }
    }
    if let Some(sub) = sub {
        let sub = text(ctx, commands, tile, &sub, 16.0, ui::TEXT_DIM);
        commands.entity(sub).insert(Node {
            max_width: px(TILE_W - 30.0),
            ..default()
        });
    }
    if let Some(lock) = model::slot_lock(loadout, slot, ctx.profile, ctx.data) {
        text(ctx, commands, tile, &lock, 16.0, RED);
    }
}

fn overview(ctx: &Ctx, commands: &mut Commands, content: Entity) {
    let draft = &ctx.editor.draft;
    let header = header(ctx, commands, content, "Create a Class");
    let spent = ctx.data.class_cost(draft);
    meter(ctx, commands, header, spent);

    let bar = commands
        .spawn((
            Node {
                width: px(4.0 * TILE_W + 3.0 * GAP),
                align_items: AlignItems::Center,
                column_gap: px(12.0),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    text(
        ctx,
        commands,
        bar,
        &editing_label(ctx.editor),
        20.0,
        ui::ACCENT,
    );
    text(ctx, commands, bar, "NAME", 20.0, ui::TEXT_DIM);
    let field = item(
        commands,
        bar,
        Nav::new(KEY_NAME, 0.0, -1.0),
        Act::FocusName,
        Look::field(),
        Node {
            width: px(260.0),
            padding: UiRect::axes(px(10.0), px(2.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        },
    );
    commands
        .entity(field)
        .remove::<Button>()
        .insert((
            NameField,
            ui::text_field(ctx.fonts, &draft.name, MAX_LOADOUT_NAME_CHARS),
        ))
        .insert(Node {
            width: px(260.0),
            padding: UiRect::axes(px(10.0), px(2.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        });
    commands.spawn((
        Node {
            flex_grow: 1.0,
            ..default()
        },
        ChildOf(bar),
    ));
    push_button(
        ctx,
        commands,
        bar,
        "Save",
        Nav::new(KEY_SAVE, 2.0, -1.0),
        Act::Save,
    );
    push_button(
        ctx,
        commands,
        bar,
        "Back",
        Nav::new(KEY_BACK, 3.0, -1.0),
        Act::Back,
    );

    status_line(ctx, commands, content);

    let grid = commands
        .spawn((
            Node {
                width: px(4.0 * TILE_W + 3.0 * GAP),
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(GAP),
                row_gap: px(GAP),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    for (slot, x, y) in TILES {
        let cost = model::slot_cost(draft, slot, ctx.data);
        let tile = tile(
            ctx,
            commands,
            grid,
            Nav::new(key_of(slot), x, y),
            slot,
            (TILE_W, TILE_H),
            Some(cost),
        );
        match slot {
            Slot::Primary => primary_body(ctx, commands, tile, draft),
            Slot::Perk(index) => perk_body(ctx, commands, tile, draft, index, (x, y)),
            Slot::Wildcards => wildcard_body(ctx, commands, tile, draft),
            _ => pick_body(ctx, commands, tile, draft, slot, None),
        }
    }
    footer(
        ctx,
        commands,
        content,
        "ARROWS  Move        ENTER  Select        ESC  Back",
    );
}

/// The Primary tile: the weapon, its Attachments and its camo.
fn primary_body(ctx: &Ctx, commands: &mut Commands, tile: Entity, loadout: &Loadout) {
    let weapon = ctx.data.weapon(loadout.primary);
    let line = row(commands, tile, 10.0);
    badge(ctx, commands, line, &weapon.name, ui::ACCENT);
    bold(
        ctx,
        commands,
        line,
        &weapon.name.to_uppercase(),
        24.0,
        ui::TEXT,
    );
    let attachments = column(commands, tile, 1.0);
    for index in 0..model::attachment_tiles(loadout, ctx.data) {
        let line = row(commands, attachments, 8.0);
        match loadout.attachments.iter().nth(index) {
            Some(id) => {
                let attachment = ctx.data.attachment(id);
                chip(commands, line, attachment.slot);
                text(ctx, commands, line, &attachment.name, 17.0, ui::TEXT);
                text(
                    ctx,
                    commands,
                    line,
                    &pts(ctx.data.attachment_cost(id)),
                    15.0,
                    ui::ACCENT,
                );
            }
            None => {
                text(ctx, commands, line, "-  No attachment", 17.0, ui::TEXT_DIM);
            }
        }
    }
    let camo = row(commands, tile, 8.0);
    swatch(commands, camo, &loadout.camo, ctx.data, 14.0);
    let name = held(ctx, loadout, Slot::Camo).unwrap_or_else(|| "No camo".to_owned());
    text(
        ctx,
        commands,
        camo,
        &format!("CAMO  {name}"),
        17.0,
        ui::TEXT,
    );
    if let Some(lock) = model::slot_lock(loadout, Slot::Primary, ctx.profile, ctx.data) {
        text(ctx, commands, tile, &lock, 16.0, RED);
    }
}

/// A small square in the colour of an Attachment Slot.
fn chip(commands: &mut Commands, parent: Entity, slot: AttachmentSlot) {
    let color = match slot {
        AttachmentSlot::Optic => Color::srgb(0.45, 0.65, 0.85),
        AttachmentSlot::Muzzle => Color::srgb(0.85, 0.55, 0.35),
        AttachmentSlot::Underbarrel => Color::srgb(0.55, 0.80, 0.50),
    };
    commands.spawn((
        Node {
            width: px(10.0),
            height: px(10.0),
            ..default()
        },
        BackgroundColor(color),
        ChildOf(parent),
    ));
}

/// A Perk tile, with the Greed Perk inside it while that Wildcard is in the class.
fn perk_body(
    ctx: &Ctx,
    commands: &mut Commands,
    tile: Entity,
    loadout: &Loadout,
    index: usize,
    spot: (f32, f32),
) {
    let info = loadout.perks.picks[index].map(|id| ctx.data.perk(id).description.clone());
    pick_body(ctx, commands, tile, loadout, Slot::Perk(index), info);
    if !loadout.has(crate::class::WildcardId::perk_greed(index)) {
        return;
    }
    let greed = item(
        commands,
        tile,
        Nav::new(key_of(Slot::Greed(index)), spot.0, spot.1 + 0.5).layered(),
        Act::Open(Slot::Greed(index)),
        Look::button(),
        Node {
            width: percent(100.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            border: UiRect::left(px(3.0)),
            column_gap: px(10.0),
            align_items: AlignItems::Center,
            margin: UiRect::top(px(2.0)),
            ..default()
        },
    );
    text(ctx, commands, greed, "GREED", 15.0, ui::TEXT_DIM);
    let name = held(ctx, loadout, Slot::Greed(index));
    let label = bold(
        ctx,
        commands,
        greed,
        &name.as_deref().unwrap_or("None").to_uppercase(),
        20.0,
        if name.is_some() {
            ui::TEXT
        } else {
            ui::TEXT_DIM
        },
    );
    commands.entity(label).insert(Node {
        flex_grow: 1.0,
        ..default()
    });
    text(
        ctx,
        commands,
        greed,
        &pts(model::slot_cost(loadout, Slot::Greed(index), ctx.data)),
        15.0,
        ui::ACCENT,
    );
}

fn wildcard_body(ctx: &Ctx, commands: &mut Commands, tile: Entity, loadout: &Loadout) {
    let max = ctx.data.class_points.max_wildcards;
    text(
        ctx,
        commands,
        tile,
        &format!("{} / {max}", loadout.wildcards.len()),
        16.0,
        ui::TEXT_DIM,
    );
    if loadout.wildcards.is_empty() {
        bold(ctx, commands, tile, "NONE", 24.0, ui::TEXT_DIM);
    }
    for id in &loadout.wildcards {
        let line = row(commands, tile, 8.0);
        bold(
            ctx,
            commands,
            line,
            &ctx.data.wildcard_name(*id).to_uppercase(),
            20.0,
            ui::TEXT,
        );
        if let Some(lock) = model::pick_lock(loadout, &Pick::Wildcard(*id), ctx.profile, ctx.data) {
            text(ctx, commands, line, &lock, 15.0, RED);
        }
    }
}

// ------------------------------------------------------- the Primary's screen

/// The Primary's own screen: an Attachment tile per Attachment it may carry, the camo, and the
/// weapon's stats.
fn gun(ctx: &Ctx, commands: &mut Commands, content: Entity) {
    let draft = &ctx.editor.draft;
    let weapon = ctx.data.weapon(draft.primary);
    let header = header(ctx, commands, content, &weapon.name);
    meter(ctx, commands, header, ctx.data.class_cost(draft));

    let bar = commands
        .spawn((
            Node {
                width: px(4.0 * TILE_W + 3.0 * GAP),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    text(
        ctx,
        commands,
        bar,
        &format!("{}  -  PRIMARY", editing_label(ctx.editor)),
        20.0,
        ui::ACCENT,
    );
    push_button(
        ctx,
        commands,
        bar,
        "Back",
        Nav::new(KEY_BACK, 3.0, -1.0),
        Act::Back,
    );
    status_line(ctx, commands, content);

    let columns = commands
        .spawn((
            Node {
                align_items: AlignItems::Stretch,
                column_gap: px(GAP),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    let grid = commands
        .spawn((
            Node {
                width: px(2.0 * GUN_TILE_W + GAP),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                column_gap: px(GAP),
                row_gap: px(GAP),
                ..default()
            },
            ChildOf(columns),
        ))
        .id();
    let tiles = model::attachment_tiles(draft, ctx.data);
    let slots = (0..tiles).map(Slot::Attachment).chain([Slot::Camo]);
    for (at, slot) in slots.enumerate() {
        let tile = tile(
            ctx,
            commands,
            grid,
            Nav::new(key_of(slot), (at % 2) as f32, (at / 2) as f32),
            slot,
            (GUN_TILE_W, GUN_TILE_H),
            (slot != Slot::Camo).then(|| model::slot_cost(draft, slot, ctx.data)),
        );
        let sub = match (
            slot,
            draft.attachments.iter().nth(match slot {
                Slot::Attachment(index) => index,
                _ => usize::MAX,
            }),
        ) {
            (Slot::Attachment(_), Some(id)) => Some(format!(
                "{}: {}",
                match ctx.data.attachment(id).slot {
                    AttachmentSlot::Optic => "Optic",
                    AttachmentSlot::Muzzle => "Muzzle",
                    AttachmentSlot::Underbarrel => "Underbarrel",
                },
                ctx.data.attachment(id).description
            )),
            _ => None,
        };
        pick_body(ctx, commands, tile, draft, slot, sub);
    }

    let panel = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(4.0 * TILE_W + 3.0 * GAP - 2.0 * GUN_TILE_W - 2.0 * GAP),
            padding: UiRect::all(px(20.0)),
            row_gap: px(8.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();
    let stats =
        ctx.data
            .effective_weapon(draft.primary, draft.attachments, &PerkEffects::default());
    bold(
        ctx,
        commands,
        panel,
        &format!(
            "{}  -  {}",
            stats.name.to_uppercase(),
            stats::fire_mode_text(&stats)
        ),
        24.0,
        ui::TEXT,
    );
    text(
        ctx,
        commands,
        panel,
        &model::weapon_level_text(draft, ctx.profile, ctx.data),
        16.0,
        ui::ACCENT,
    );
    stats::spawn_filled_bars(commands, ctx.fonts, panel, ctx.data, &stats);
    for id in draft.attachments.iter() {
        let attachment = ctx.data.attachment(id);
        let line = text(
            ctx,
            commands,
            panel,
            &format!("{}: {}", attachment.name, attachment.description),
            15.0,
            ui::TEXT_DIM,
        );
        commands.entity(line).insert(Node {
            max_width: px(380.0),
            ..default()
        });
    }
    footer(
        ctx,
        commands,
        content,
        "ARROWS  Move        ENTER  Select        ESC  Back",
    );
}

// ------------------------------------------------------------------- dropdown

/// The longest description or reason a dropdown entry has room for on its one line.
const LINE_CHARS: usize = 50;

/// The dropdown list of the open slot with a click catcher behind it (a click outside closes the
/// list) and the preview card beside it, all children of the editor's root so they float over
/// the overview. Nothing when no list is open.
pub(super) fn dropdown(ctx: &Ctx, commands: &mut Commands, root: Entity) {
    let Some(list) = &ctx.editor.dropdown else {
        return;
    };
    let options = &ctx.editor.options;
    let place = list.place;
    commands.spawn((
        DropdownUi,
        Button,
        Action(Act::CloseList),
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            top: px(0.0),
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
        ChildOf(root),
    ));
    let panel = commands
        .spawn((DropdownUi, ui::panel(), ChildOf(root)))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: px(place.x),
            top: px(place.y),
            width: px(place.w),
            height: px(place.h),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(model::LIST_INSET - 1.0)),
            border: UiRect::all(px(1.0)),
            overflow: Overflow::clip(),
            ..default()
        })
        .id();
    let scrolls = place.scrolls(options.len());
    let visible = place.visible as isize;
    if scrolls {
        more(ctx, commands, panel, KEY_MORE_UP, list.first, "^", -visible);
    }
    for (index, choice) in options
        .iter()
        .enumerate()
        .skip(list.first)
        .take(place.visible)
    {
        list_row(ctx, commands, panel, index, choice);
    }
    if scrolls {
        let below = options.len().saturating_sub(list.first + place.visible);
        more(ctx, commands, panel, KEY_MORE_DOWN, below, "v", visible);
    }
    if let Some((x, y)) = place.card {
        commands
            .spawn((DropdownUi, PreviewPanel, ui::panel(), ChildOf(root)))
            .insert(Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(model::CARD_W),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16.0)),
                row_gap: px(8.0),
                border: UiRect::all(px(1.0)),
                ..default()
            });
    }
}

/// The `^ 2 more` or `v 3 more` line of a list that scrolls: a button scrolling by a list
/// height while there is more that way, an empty line otherwise (the list keeps its height).
fn more(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    key: usize,
    count: usize,
    arrow: &str,
    by: isize,
) {
    let node = Node {
        width: percent(100.0),
        height: px(model::HINT_H),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    if count == 0 {
        commands.spawn((node, ChildOf(parent)));
        return;
    }
    let line = item(
        commands,
        parent,
        Nav::new(key, 0.0, 0.0).mouse_only(),
        Act::Scroll(by),
        Look::row(),
        node,
    );
    text(
        ctx,
        commands,
        line,
        &format!("{arrow}  {count} more"),
        15.0,
        ui::TEXT_DIM,
    );
}

/// One entry of a dropdown list: the name (with a mark when the slot holds it) and cost on
/// top, below it why it is locked or refused (red) or else its one-line description. Greyed
/// while it can't be taken, the cost red when it doesn't fit the budget.
fn list_row(ctx: &Ctx, commands: &mut Commands, parent: Entity, index: usize, choice: &Choice) {
    let usable = choice.usable();
    let line = item(
        commands,
        parent,
        Nav::new(KEY_LIST + index, 0.0, 0.0),
        Act::Choose(index),
        Look::row(),
        Node {
            width: percent(100.0),
            height: px(model::ROW_H),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(12.0), px(4.0)),
            border: UiRect::left(px(4.0)),
            column_gap: px(12.0),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
    );
    let lines = column(commands, line, 2.0);
    commands.entity(lines).insert(Node {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        min_width: px(0.0),
        row_gap: px(2.0),
        ..default()
    });
    let top = row(commands, lines, 10.0);
    bold(
        ctx,
        commands,
        top,
        &choice.name.to_uppercase(),
        22.0,
        if usable { ui::TEXT } else { ui::TEXT_DIM },
    );
    if choice.current {
        text(ctx, commands, top, "EQUIPPED", 15.0, ui::ACCENT);
    }
    match choice.lock.as_ref().or(choice.refusal.as_ref()) {
        Some(reason) => text(
            ctx,
            commands,
            lines,
            &model::brief(reason, LINE_CHARS),
            15.0,
            RED,
        ),
        None => text(
            ctx,
            commands,
            lines,
            &model::brief(&choice.info, LINE_CHARS),
            15.0,
            ui::TEXT_DIM,
        ),
    };
    if choice.pick != Pick::Empty {
        let tint = if choice.over_budget {
            RED
        } else if usable {
            ui::ACCENT
        } else {
            ui::TEXT_DIM
        };
        bold(ctx, commands, line, &pts(choice.cost), 22.0, tint);
    }
}

// -------------------------------------------------------------------- picker

fn picker(ctx: &Ctx, commands: &mut Commands, content: Entity, slot: Slot) {
    let editor = ctx.editor;
    let title = match slot {
        Slot::Attachment(_) | Slot::Camo => format!(
            "{}  -  {}",
            ctx.data.weapon(editor.draft.primary).short_name(),
            slot.label()
        ),
        _ => slot.label(),
    };
    let header = header(ctx, commands, content, &title);
    meter(ctx, commands, header, ctx.data.class_cost(&editor.draft));

    let bar = commands
        .spawn((
            Node {
                width: px(4.0 * TILE_W + 3.0 * GAP),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    text(ctx, commands, bar, &editing_label(editor), 20.0, ui::ACCENT);
    push_button(
        ctx,
        commands,
        bar,
        "Back",
        Nav::new(KEY_BACK, 3.0, -1.0).mouse_only(),
        Act::Back,
    );
    status_line(ctx, commands, content);

    let columns = commands
        .spawn((
            Node {
                align_items: AlignItems::Stretch,
                column_gap: px(GAP),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    let list = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(PICKER_W),
            padding: UiRect::all(px(16.0)),
            row_gap: px(6.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();
    let page = model::page_of(editor.cursor);
    let shown = editor
        .options
        .iter()
        .enumerate()
        .skip(page * PAGE_SIZE)
        .take(PAGE_SIZE);
    for (index, choice) in shown {
        choice_row(ctx, commands, list, index, choice);
    }
    let pages = model::pages(editor.options.len());
    if pages > 1 {
        let pager = row(commands, list, 12.0);
        commands.entity(pager).insert(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            margin: UiRect::top(px(4.0)),
            ..default()
        });
        pager_button(ctx, commands, pager, "<", KEY_PREV, Act::Page(-1));
        text(
            ctx,
            commands,
            pager,
            &format!("PAGE {} / {pages}", page + 1),
            18.0,
            ui::TEXT_DIM,
        );
        pager_button(ctx, commands, pager, ">", KEY_NEXT, Act::Page(1));
    }
    commands
        .spawn((ui::panel(), PreviewPanel, ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(PREVIEW_W),
            padding: UiRect::all(px(20.0)),
            row_gap: px(8.0),
            border: UiRect::all(px(1.0)),
            ..default()
        });
    let hint = if pages > 1 {
        "UP / DOWN  Move        LEFT / RIGHT  Page        ENTER  Confirm        ESC  Back"
    } else {
        "UP / DOWN  Move        ENTER  Confirm        ESC  Back"
    };
    footer(ctx, commands, content, hint);
}

fn pager_button(
    ctx: &Ctx,
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    key: usize,
    act: Act,
) {
    let button = item(
        commands,
        parent,
        Nav::new(key, 0.0, 0.0).mouse_only(),
        act,
        Look::button(),
        Node {
            width: px(48.0),
            height: px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::left(px(3.0)),
            ..default()
        },
    );
    bold(ctx, commands, button, label, 24.0, ui::TEXT);
}

/// One option of a picker: icon, name, what locks or refuses it, and its cost (red when it
/// doesn't fit the budget).
fn choice_row(ctx: &Ctx, commands: &mut Commands, parent: Entity, index: usize, choice: &Choice) {
    let usable = choice.usable();
    let line = item(
        commands,
        parent,
        Nav::new(index, 0.0, index as f32),
        Act::Choose(index),
        Look::row(),
        Node {
            width: percent(100.0),
            height: px(54.0),
            padding: UiRect::axes(px(12.0), px(4.0)),
            border: UiRect::left(px(4.0)),
            column_gap: px(12.0),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let tint = if usable { ui::ACCENT } else { ui::TEXT_DIM };
    match &choice.pick {
        Pick::Camo(id) => swatch(commands, line, id, ctx.data, 38.0),
        Pick::Empty => badge(ctx, commands, line, "-", ui::TEXT_DIM),
        _ => badge(ctx, commands, line, &choice.name, tint),
    }
    let names = column(commands, line, 0.0);
    commands.entity(names).insert(Node {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        ..default()
    });
    bold(
        ctx,
        commands,
        names,
        &choice.name.to_uppercase(),
        24.0,
        if usable { ui::TEXT } else { ui::TEXT_DIM },
    );
    if let Some(reason) = choice.lock.as_ref().or(choice.refusal.as_ref()) {
        text(ctx, commands, names, reason, 15.0, RED);
    } else if choice.current {
        text(ctx, commands, names, "EQUIPPED", 15.0, ui::ACCENT);
    }
    if !matches!(choice.pick, Pick::Camo(_) | Pick::Empty) {
        let cost = if choice.over_budget {
            RED
        } else if usable {
            ui::ACCENT
        } else {
            ui::TEXT_DIM
        };
        bold(ctx, commands, line, &pts(choice.cost), 22.0, cost);
    }
}

/// The preview of a highlighted option, `width` wide: the right-hand panel of a full-screen
/// picker or the card beside a dropdown list. `slot` is what is being picked for.
pub(super) fn choice_preview(
    ctx: &Ctx,
    commands: &mut Commands,
    panel: Entity,
    slot: Slot,
    width: f32,
    choice: &Choice,
) {
    bold(
        ctx,
        commands,
        panel,
        &choice.name.to_uppercase(),
        (width / 16.0).min(34.0),
        ui::TEXT,
    );
    let info = text(ctx, commands, panel, &choice.info, 20.0, ui::TEXT_DIM);
    commands.entity(info).insert(Node {
        max_width: px(width - 50.0),
        ..default()
    });
    match &choice.pick {
        Pick::Camo(id) => swatch(commands, panel, id, ctx.data, 90.0),
        Pick::Weapon(_) | Pick::Attachment(_) => {
            let after = &choice.after;
            // A Secondary shows its own stats, a Primary or Attachment the Primary's.
            let stats = match (slot, &choice.pick) {
                (Slot::Secondary, Pick::Weapon(id)) => {
                    ctx.data
                        .effective_weapon(*id, default(), &PerkEffects::default())
                }
                _ => ctx.data.effective_weapon(
                    after.primary,
                    after.attachments,
                    &PerkEffects::default(),
                ),
            };
            bold(
                ctx,
                commands,
                panel,
                &stats::fire_mode_text(&stats).to_uppercase(),
                20.0,
                ui::ACCENT,
            );
            stats::spawn_filled_bars(commands, ctx.fonts, panel, ctx.data, &stats);
        }
        _ => {}
    }
    commands.spawn((
        Node {
            flex_grow: 1.0,
            ..default()
        },
        ChildOf(panel),
    ));
    if !matches!(choice.pick, Pick::Camo(_)) {
        let cost = row(commands, panel, 12.0);
        text(ctx, commands, cost, "COST", 20.0, ui::TEXT_DIM);
        bold(ctx, commands, cost, &pts(choice.cost), 26.0, ui::ACCENT);
    }
    let budget = ctx.data.class_points.budget;
    let spent = ctx.data.class_cost(&choice.after);
    let now = ctx.data.class_cost(&ctx.editor.draft);
    let after = row(commands, panel, 12.0);
    text(ctx, commands, after, "POINTS AFTER", 20.0, ui::TEXT_DIM);
    bold(
        ctx,
        commands,
        after,
        &format!("{spent} / {budget}"),
        26.0,
        if spent > budget { RED } else { ui::ACCENT },
    );
    if spent != now {
        let delta = i64::from(spent) - i64::from(now);
        text(
            ctx,
            commands,
            after,
            &format!("({delta:+})"),
            20.0,
            ui::TEXT_DIM,
        );
    }
    if let Some(reason) = choice.lock.as_ref().or(choice.refusal.as_ref()) {
        let note = text(ctx, commands, panel, reason, 20.0, RED);
        commands.entity(note).insert(Node {
            max_width: px(width - 50.0),
            ..default()
        });
    }
}
