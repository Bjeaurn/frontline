//! Create a Class (`Overlay::ClassEditor`), opened from the Loadout screen: pick one of the
//! Profile's custom slots, then build a Pick 10 class (`class`): a Primary with up to two
//! Attachments (three with Gunfighter) and a camo, a Secondary, a Perk per Perk Slot (and a
//! second one where that slot's Perk Greed Wildcard is equipped), equipment, up to three
//! Wildcards and a name. A points meter is always on top; each item shows its cost. A pick that
//! would break a rule (the 11th point, a third Attachment without Gunfighter, ...) is refused
//! with a short message. Everything is listed, but what the account Level, a Weapon Level or a
//! camo Challenge hasn't unlocked is greyed with what it takes (`Unlocks at Level 8`, `Unlocks at
//! M4 level 3`), and SAVE refuses a class holding any of it. SAVE writes the class into the
//! Profile; the Default Loadouts are never editable.

use bevy::ecs::observer::IntoEntityObserver;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;

use super::screen::{Preview, describe};
use super::stats::{self, StatFill};
use crate::class::{EquipmentId, EquipmentSlot, WildcardId};
use crate::data::{GameData, WeaponClass, WeaponId};
use crate::flow::Overlay;
use crate::gear::{
    AttachmentId, AttachmentSlot, Loadout, MAX_LOADOUT_NAME_CHARS, NO_CAMO, PerkEffects, PerkId,
};
use crate::profile::{CUSTOM_SLOTS, LoadoutChoice, Profile};
use crate::progression::UnlockItem;
use crate::ui::{self, UiFonts};

/// Colour of a locked value and its note.
const LOCKED: Color = Color::srgb(0.85, 0.42, 0.34);
/// An unspent point of the meter.
const PIP_EMPTY: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// Widths of the two panels and the gap between them, in pixels.
const FORM_WIDTH: f32 = 520.0;
const PREVIEW_WIDTH: f32 = 440.0;
const COLUMN_GAP: f32 = 16.0;

pub(super) struct ClassEditorPlugin;

impl Plugin for ClassEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Overlay::ClassEditor), spawn_editor)
            .add_systems(
                Update,
                (back_on_esc, refresh)
                    .chain()
                    .run_if(in_state(Overlay::ClassEditor)),
            );
    }
}

/// The class being edited: a copy of custom slot `slot`, written back by SAVE. Created by the
/// Loadout screen when it opens the editor.
#[derive(Resource, Debug)]
pub(super) struct Draft {
    slot: usize,
    loadout: Loadout,
    /// Why the last pick was refused (or the class couldn't be saved), until the next pick.
    message: Option<String>,
}

impl Draft {
    /// The editor on the custom slot `preview` names (the first slot for a Default Loadout).
    pub(super) fn opening(preview: &Preview, profile: &Profile) -> Draft {
        let slot = match preview.0 {
            LoadoutChoice::Custom(slot) => slot,
            LoadoutChoice::Default(_) => 0,
        };
        Draft::of_slot(slot, profile)
    }

    fn of_slot(slot: usize, profile: &Profile) -> Draft {
        Draft {
            slot,
            loadout: profile.custom[slot].clone(),
            message: None,
        }
    }

    /// Applies a pick's outcome: the new class, or the refusal to show (and log).
    fn apply(&mut self, result: Result<Loadout, String>, data: &GameData) {
        match result {
            Ok(loadout) => {
                self.message = None;
                if loadout != self.loadout {
                    info!(
                        "Class {}: {} / {} points",
                        self.slot + 1,
                        data.class_cost(&loadout),
                        data.class_points.budget
                    );
                    self.loadout = loadout;
                }
            }
            Err(message) => {
                info!("Class {}: pick refused: {message}", self.slot + 1);
                self.message = Some(message);
            }
        }
    }
}

/// One thing a class picks, cycled with a pair of arrow buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Primary,
    Attachment(AttachmentSlot),
    /// The Primary's camo.
    Camo,
    Secondary,
    /// Perk Slot, counting from 0.
    Perk(usize),
    /// The second Perk of a Perk Slot (its Perk Greed Wildcard), counting from 0.
    Greed(usize),
    Equipment(EquipmentSlot),
}

impl Field {
    /// The rows of the form, top to bottom: the Primary and what goes on it (the camo after the
    /// Attachments), the Secondary, the Perks and the equipment.
    const ALL: [Field; 14] = [
        Field::Primary,
        Field::Attachment(AttachmentSlot::Optic),
        Field::Attachment(AttachmentSlot::Muzzle),
        Field::Attachment(AttachmentSlot::Underbarrel),
        Field::Camo,
        Field::Secondary,
        Field::Perk(0),
        Field::Greed(0),
        Field::Perk(1),
        Field::Greed(1),
        Field::Perk(2),
        Field::Greed(2),
        Field::Equipment(EquipmentSlot::Lethal),
        Field::Equipment(EquipmentSlot::Tactical),
    ];

    fn label(self) -> String {
        match self {
            Field::Primary => "PRIMARY".into(),
            Field::Attachment(AttachmentSlot::Optic) => "OPTIC".into(),
            Field::Attachment(AttachmentSlot::Muzzle) => "MUZZLE".into(),
            Field::Attachment(AttachmentSlot::Underbarrel) => "UNDERBARREL".into(),
            Field::Camo => "CAMO".into(),
            Field::Secondary => "SECONDARY".into(),
            Field::Perk(slot) => format!("PERK {}", slot + 1),
            Field::Greed(slot) => format!("PERK {} GREED", slot + 1),
            Field::Equipment(EquipmentSlot::Lethal) => "LETHAL".into(),
            Field::Equipment(EquipmentSlot::Tactical) => "TACTICAL".into(),
        }
    }

    /// Whether the row is on show: a Perk Greed row only while its Wildcard is in the class.
    fn shown(self, loadout: &Loadout) -> bool {
        match self {
            Field::Greed(slot) => loadout.has(WildcardId::perk_greed(slot)),
            _ => true,
        }
    }
}

/// The texts `refresh` keeps up to date.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum EditorText {
    /// Which custom slot is edited.
    Slot,
    /// What `field` currently holds.
    Value(Field),
    /// Under a value: what it takes to unlock it, empty when it is unlocked.
    Note(Field),
    /// Points spent and the budget, `7 / 10`.
    Points,
    /// The Primary's name and trigger mode above the stat bars.
    Heading,
    /// The Primary's Weapon Level and what comes next.
    WeaponLevel,
    /// What the fitted Attachments, Perks and Wildcards do.
    Details,
    /// Why a pick was refused or the class can't be saved yet.
    Status,
}

#[derive(Component)]
struct NameField;

/// A whole picker row; hidden while its field isn't on show.
#[derive(Component)]
struct FieldRow(Field);

/// One pip of the points meter: lit while fewer than `index + 1` points are spent.
#[derive(Component)]
struct Pip(u32);

/// A button that toggles a Wildcard; its text shows the cost and what unlocks it.
#[derive(Component)]
struct WildcardButton(WildcardId);

/// The option after (`step` 1) or before (`step` -1) `current` in `options`, wrapping around.
fn pick<T: Copy + PartialEq>(options: &[T], current: T, step: isize) -> T {
    let at = options
        .iter()
        .position(|option| *option == current)
        .unwrap_or(0) as isize;
    options[(at + step).rem_euclid(options.len() as isize) as usize]
}

/// `None` followed by `items`: every optional slot can be left empty.
fn with_none<T: Copy>(items: impl IntoIterator<Item = T>) -> Vec<Option<T>> {
    std::iter::once(None)
        .chain(items.into_iter().map(Some))
        .collect()
}

/// The Perks `slot` (counting from 0) can hold for `field`: its own Perk Slot's, minus the one
/// the other row of that slot already has.
fn perk_options(loadout: &Loadout, field: Field, data: &GameData) -> Vec<Option<PerkId>> {
    let (Field::Perk(slot) | Field::Greed(slot)) = field else {
        return vec![None];
    };
    let other = match field {
        Field::Perk(_) => loadout.perks.greed[slot],
        _ => loadout.perks.picks[slot],
    };
    with_none(data.perks_in_slot(slot + 1).filter(|id| Some(*id) != other))
}

fn equipment_options(slot: EquipmentSlot, data: &GameData) -> Vec<Option<EquipmentId>> {
    with_none(
        EquipmentId::ALL
            .into_iter()
            .filter(|id| data.class_points.equipment[id].slot == slot),
    )
}

/// The class with `field` moved to its next (`step` 1) or previous (`step` -1) choice, or why it
/// can't be (nothing to choose from). Slot and Wildcard rules are checked by `try_pick`.
fn cycle(loadout: &Loadout, field: Field, step: isize, data: &GameData) -> Result<Loadout, String> {
    let mut next = loadout.clone();
    match field {
        Field::Primary => {
            let primaries: Vec<_> = WeaponId::ALL
                .into_iter()
                .filter(|id| data.weapon(*id).class == WeaponClass::Primary)
                .collect();
            next.primary = pick(&primaries, loadout.primary, step);
            // A Primary that doesn't offer the old Attachments drops them.
            next = next.validated(data);
        }
        Field::Secondary => {
            let class = |class| {
                WeaponId::ALL
                    .into_iter()
                    .filter(move |id| data.weapon(*id).class == class)
            };
            // Primaries are only on offer with Overkill (a class that has it picks them here).
            let options =
                with_none(class(WeaponClass::Secondary).chain(
                    class(WeaponClass::Primary).filter(|_| loadout.has(WildcardId::Overkill)),
                ));
            next.secondary = pick(&options, loadout.secondary, step);
        }
        Field::Camo => {
            let options: Vec<&str> = std::iter::once(NO_CAMO)
                .chain(data.progression.camos.iter().map(|camo| camo.id.as_str()))
                .collect();
            next.camo = pick(&options, loadout.camo.as_str(), step).to_owned();
        }
        Field::Attachment(slot) => {
            if !data.weapon(loadout.primary).accepts(slot) {
                return Err(format!(
                    "The {} has no {}",
                    data.weapon(loadout.primary).short_name(),
                    Field::Attachment(slot).label().to_lowercase()
                ));
            }
            let offered = data.weapon_attachments(loadout.primary, slot);
            if offered.is_empty() {
                return Err(format!(
                    "The {} offers no {}",
                    data.weapon(loadout.primary).short_name(),
                    Field::Attachment(slot).label().to_lowercase()
                ));
            }
            let options = with_none(offered);
            next.attachments
                .set(slot, pick(&options, loadout.attachments.get(slot), step));
        }
        Field::Perk(slot) => {
            let options = perk_options(loadout, field, data);
            next.perks.picks[slot] = pick(&options, loadout.perks.picks[slot], step);
        }
        Field::Greed(slot) => {
            let options = perk_options(loadout, field, data);
            next.perks.greed[slot] = pick(&options, loadout.perks.greed[slot], step);
        }
        Field::Equipment(slot) => {
            let options = equipment_options(slot, data);
            if options.len() == 1 {
                return Err("No equipment to pick yet".to_owned());
            }
            next.set_equipment(slot, pick(&options, loadout.equipment(slot), step));
        }
    }
    Ok(next)
}

/// Whether going from `before` to `after` breaks a rule that `before` kept: the first such
/// reason, short enough for the status line. A class that already breaks a rule isn't blamed
/// for it again, so picks that don't make it worse still go through.
fn refusal(before: &Loadout, after: &Loadout, data: &GameData) -> Option<String> {
    let had = data.class_errors(before, None);
    data.class_errors(after, None)
        .into_iter()
        .find(|error| !had.contains(error))
        .map(|error| error.describe(data))
}

/// `field` moved one choice, if the class stays valid; otherwise why not.
fn try_pick(
    loadout: &Loadout,
    field: Field,
    step: isize,
    data: &GameData,
) -> Result<Loadout, String> {
    let next = cycle(loadout, field, step, data)?;
    match refusal(loadout, &next, data) {
        Some(reason) => Err(reason),
        None => Ok(next),
    }
}

/// `wildcard` added to the class, or taken out of it, if the class stays valid; otherwise why
/// not.
fn try_toggle(loadout: &Loadout, wildcard: WildcardId, data: &GameData) -> Result<Loadout, String> {
    let mut next = loadout.clone();
    if let Some(at) = next.wildcards.iter().position(|id| *id == wildcard) {
        next.wildcards.remove(at);
    } else {
        next.wildcards.push(wildcard);
    }
    match refusal(loadout, &next, data) {
        Some(reason) => Err(reason),
        None => Ok(next),
    }
}

/// The class point cost shown beside a choice: `[1]`.
fn cost_tag(points: u32) -> String {
    format!("  [{points}]")
}

/// What `field` holds, for its label, with the points it costs.
fn value_text(loadout: &Loadout, field: Field, data: &GameData) -> String {
    let costs = &data.class_points.costs;
    let attachment = |id: Option<AttachmentId>| match id {
        Some(id) => format!(
            "{}{}",
            data.attachment(id).name,
            cost_tag(data.attachment_cost(id))
        ),
        None => "None".to_owned(),
    };
    let perk = |id: Option<PerkId>| match id {
        Some(id) => format!("{}{}", data.perk(id).name, cost_tag(costs.perk)),
        None => "None".to_owned(),
    };
    match field {
        Field::Primary => format!(
            "{}{}",
            data.weapon(loadout.primary).name,
            cost_tag(costs.primary)
        ),
        Field::Secondary => loadout.secondary.map_or("None".to_owned(), |id| {
            format!("{}{}", data.weapon(id).name, cost_tag(costs.secondary))
        }),
        Field::Camo => data
            .progression
            .camo(&loadout.camo)
            .map_or("None", |camo| camo.name.as_str())
            .to_owned(),
        // A built-in scope leaves no room for an Optic.
        Field::Attachment(slot) if !data.weapon(loadout.primary).accepts(slot) => {
            "Built-in scope".to_owned()
        }
        Field::Attachment(slot) if data.weapon_attachments(loadout.primary, slot).is_empty() => {
            "Not available".to_owned()
        }
        Field::Attachment(slot) => attachment(loadout.attachments.get(slot)),
        Field::Perk(slot) => perk(loadout.perks.picks[slot]),
        Field::Greed(slot) => perk(loadout.perks.greed[slot]),
        Field::Equipment(slot) => loadout.equipment(slot).map_or_else(
            || {
                if EquipmentId::ALL.is_empty() {
                    "None yet".to_owned()
                } else {
                    "None".to_owned()
                }
            },
            |id| {
                format!(
                    "{}{}",
                    data.class_points.equipment[&id].name,
                    cost_tag(data.equipment_cost(id))
                )
            },
        ),
    }
    .to_uppercase()
}

/// What the fitted Attachments, Perks and Wildcards do, one per line.
fn effects_text(loadout: &Loadout, data: &GameData) -> String {
    let attachments = loadout.attachments.iter().map(|id| {
        let attachment = data.attachment(id);
        format!("{}: {}", attachment.name, attachment.description)
    });
    let perks = loadout.perks.iter().map(|id| {
        let perk = data.perk(id);
        format!("{}: {}", perk.name, perk.description)
    });
    let wildcards = loadout.wildcards.iter().map(|id| {
        let wildcard = &data.class_points.wildcards[id];
        format!("{}: {}", wildcard.name, wildcard.description)
    });
    attachments
        .chain(perks)
        .chain(wildcards)
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `field` of `loadout` still needs before it can be used, if it isn't unlocked: the
/// account Level for a Weapon, Perk, equipment or Wildcard, the Weapon Level for an Attachment,
/// the Challenge progress for a camo.
fn lock_note(
    loadout: &Loadout,
    field: Field,
    profile: &Profile,
    data: &GameData,
) -> Option<String> {
    let progression = &data.progression;
    let level = profile.level(data);
    let by_level = |item: UnlockItem| {
        (!progression.is_unlocked(item, level))
            .then(|| format!("Unlocks at Level {}", progression.unlock_level(item)))
    };
    match field {
        Field::Primary => by_level(UnlockItem::Weapon(loadout.primary)),
        Field::Secondary => loadout
            .secondary
            .and_then(|id| by_level(UnlockItem::Weapon(id))),
        Field::Attachment(slot) => {
            let id = loadout.attachments.get(slot)?;
            data.attachment_lock(loadout.primary, id, &profile.record(loadout.primary))
                .map(|need| format!("Unlocks at {need}"))
        }
        Field::Perk(slot) => {
            loadout.perks.picks[slot].and_then(|id| by_level(UnlockItem::Perk(id)))
        }
        Field::Greed(slot) => {
            loadout.perks.greed[slot].and_then(|id| by_level(UnlockItem::Perk(id)))
        }
        Field::Equipment(slot) => loadout
            .equipment(slot)
            .and_then(|id| by_level(UnlockItem::Equipment(id))),
        Field::Camo => {
            let camo = progression.camo(&loadout.camo)?;
            let progress = progression.camo_progress(camo, &profile.record(loadout.primary));
            (!progress.done()).then(|| format!("Locked: {}", progress.text()))
        }
    }
}

/// The text of a Wildcard button: `GUNFIGHTER  [1]`, and what unlocks it while locked.
fn wildcard_text(id: WildcardId, profile: &Profile, data: &GameData) -> String {
    let item = UnlockItem::Wildcard(id);
    let lock = if data.progression.is_unlocked(item, profile.level(data)) {
        String::new()
    } else {
        format!("  -  LEVEL {}", data.progression.unlock_level(item))
    };
    format!(
        "{}{}{lock}",
        data.wildcard_name(id),
        cost_tag(data.class_points.costs.wildcard)
    )
    .to_uppercase()
}

/// The Weapon Level line under the heading: the level, progress to the next and what it opens.
fn weapon_level_text(loadout: &Loadout, profile: &Profile, data: &GameData) -> String {
    let levels = &data.progression.weapon_levels;
    let progress = levels.progress(profile.record(loadout.primary).xp);
    let next = data
        .progression
        .attachments_unlocked_at(loadout.primary, progress.level + 1);
    let next = if progress.maxed() {
        "MAX".to_owned()
    } else {
        let opens = if next.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = next
                .iter()
                .map(|id| data.attachment(*id).name.as_str())
                .collect();
            format!("  -  NEXT: {}", names.join(", ").to_uppercase())
        };
        format!("{} / {} XP{opens}", progress.into_level, progress.span)
    };
    format!("WEAPON LEVEL {}  -  {next}", progress.level)
}

fn back_on_esc(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<NextState<Overlay>>) {
    if keys.just_pressed(KeyCode::Escape) {
        overlay.set(Overlay::Loadout);
    }
}

/// A row of the form: `label`, previous arrow, a value text carrying `value`, next arrow. The
/// arrows run the observers with `-1` and `+1`. Returns the row.
fn picker_row<M1, M2>(
    commands: &mut Commands,
    fonts: &UiFonts,
    parent: Entity,
    label: &str,
    widths: (f32, f32),
    value: EditorText,
    previous: impl IntoEntityObserver<M1>,
    next: impl IntoEntityObserver<M2>,
) -> Entity {
    let row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ui::label(fonts, label, 18.0, ui::TEXT_DIM),
        Node {
            width: px(widths.0),
            ..default()
        },
        ChildOf(row),
    ));
    commands
        .spawn((ChildOf(row), ui::compact_button(fonts, "<", 36.0, 20.0)))
        .observe(previous);
    let cell = commands
        .spawn((
            Node {
                width: px(widths.1),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    commands.spawn((value, ui::label(fonts, "", 18.0, ui::ACCENT), ChildOf(cell)));
    // Only a Field's value can be locked, and so only it has a note.
    if let EditorText::Value(field) = value {
        commands.spawn((
            EditorText::Note(field),
            ui::label(fonts, "", 14.0, LOCKED),
            Node {
                display: Display::None,
                ..default()
            },
            ChildOf(cell),
        ));
    }
    commands
        .spawn((ChildOf(row), ui::compact_button(fonts, ">", 36.0, 20.0)))
        .observe(next);
    row
}

fn spawn_editor(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    draft: Res<Draft>,
    data: Res<GameData>,
) {
    let root = commands
        .spawn((
            Name::new("ClassEditor"),
            DespawnOnExit(Overlay::ClassEditor),
            ui::backdrop(),
        ))
        // The backdrop's layout, with less room between the rows: the editor is a tall screen.
        .insert(Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(8.0),
            ..default()
        })
        .id();

    // Title, with BACK and SAVE at its right.
    let top = commands
        .spawn((
            Node {
                width: px(FORM_WIDTH + PREVIEW_WIDTH + COLUMN_GAP),
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        ui::title(&fonts, "Create a Class", 34.0),
        Node {
            flex_grow: 1.0,
            ..default()
        },
        ChildOf(top),
    ));
    commands
        .spawn((ui::compact_button(&fonts, "Back", 90.0, 20.0), ChildOf(top)))
        .observe(|_: On<Activate>, mut overlay: ResMut<NextState<Overlay>>| {
            overlay.set(Overlay::Loadout);
        });
    commands
        .spawn((ui::compact_button(&fonts, "Save", 90.0, 20.0), ChildOf(top)))
        .observe(save);

    // Which custom slot, its name and the points meter (always on screen).
    let header = commands
        .spawn((
            Node {
                width: px(FORM_WIDTH + PREVIEW_WIDTH + COLUMN_GAP),
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    // Switching reopens the editor on that slot's saved class.
    picker_row(
        &mut commands,
        &fonts,
        header,
        "CLASS",
        (60.0, 120.0),
        EditorText::Slot,
        |_: On<Activate>,
         mut draft: ResMut<Draft>,
         profile: Res<Profile>,
         mut overlay: ResMut<NextState<Overlay>>| {
            let slot = (draft.slot + CUSTOM_SLOTS - 1) % CUSTOM_SLOTS;
            *draft = Draft::of_slot(slot, &profile);
            overlay.set(Overlay::ClassEditor);
        },
        |_: On<Activate>,
         mut draft: ResMut<Draft>,
         profile: Res<Profile>,
         mut overlay: ResMut<NextState<Overlay>>| {
            let slot = (draft.slot + 1) % CUSTOM_SLOTS;
            *draft = Draft::of_slot(slot, &profile);
            overlay.set(Overlay::ClassEditor);
        },
    );
    commands.spawn((
        ui::label(&fonts, "NAME", 18.0, ui::TEXT_DIM),
        ChildOf(header),
    ));
    commands
        .spawn((
            NameField,
            ui::text_field(&fonts, &draft.loadout.name, MAX_LOADOUT_NAME_CHARS),
            ChildOf(header),
        ))
        .insert(Node {
            width: px(230.0),
            padding: UiRect::axes(px(10.0), px(2.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        });
    let meter = commands
        .spawn((
            Name::new("PointsMeter"),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                margin: UiRect::left(px(8.0)),
                ..default()
            },
            ChildOf(header),
        ))
        .id();
    commands.spawn((
        ui::label(&fonts, "POINTS", 18.0, ui::TEXT_DIM),
        ChildOf(meter),
    ));
    commands.spawn((
        EditorText::Points,
        ui::label(&fonts, "", 26.0, ui::ACCENT),
        ChildOf(meter),
    ));
    let pips = commands
        .spawn((
            Node {
                column_gap: px(3.0),
                ..default()
            },
            ChildOf(meter),
        ))
        .id();
    for index in 0..data.class_points.budget {
        commands.spawn((
            Pip(index),
            Node {
                width: px(16.0),
                height: px(14.0),
                ..default()
            },
            BackgroundColor(PIP_EMPTY),
            ChildOf(pips),
        ));
    }

    let columns = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: px(COLUMN_GAP),
                ..default()
            },
            ChildOf(root),
        ))
        .id();

    let form = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            width: px(FORM_WIDTH),
            padding: UiRect::all(px(16.0)),
            row_gap: px(3.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();

    for field in Field::ALL {
        let row = picker_row(
            &mut commands,
            &fonts,
            form,
            &field.label(),
            (130.0, 260.0),
            EditorText::Value(field),
            move |_: On<Activate>, mut draft: ResMut<Draft>, data: Res<GameData>| {
                let result = try_pick(&draft.loadout, field, -1, &data);
                draft.apply(result, &data);
            },
            move |_: On<Activate>, mut draft: ResMut<Draft>, data: Res<GameData>| {
                let result = try_pick(&draft.loadout, field, 1, &data);
                draft.apply(result, &data);
            },
        );
        commands.entity(row).insert(FieldRow(field));
    }

    // Wildcards: one toggle each, wrapping under the pickers.
    let wildcards = commands
        .spawn((
            Node {
                align_items: AlignItems::FlexStart,
                column_gap: px(8.0),
                margin: UiRect::top(px(3.0)),
                ..default()
            },
            ChildOf(form),
        ))
        .id();
    commands.spawn((
        ui::label(&fonts, "WILDCARDS", 18.0, ui::TEXT_DIM),
        Node {
            width: px(130.0),
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        ChildOf(wildcards),
    ));
    let buttons = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                width: px(340.0),
                column_gap: px(6.0),
                row_gap: px(5.0),
                ..default()
            },
            ChildOf(wildcards),
        ))
        .id();
    for id in WildcardId::ALL {
        commands
            .spawn((
                WildcardButton(id),
                ChildOf(buttons),
                ui::compact_button(&fonts, data.wildcard_name(id), 0.0, 15.0),
            ))
            .observe(
                move |_: On<Activate>, mut draft: ResMut<Draft>, data: Res<GameData>| {
                    let result = try_toggle(&draft.loadout, id, &data);
                    draft.apply(result, &data);
                },
            );
    }

    let preview = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16.0)),
            row_gap: px(8.0),
            border: UiRect::all(px(1.0)),
            min_width: px(PREVIEW_WIDTH),
            max_width: px(PREVIEW_WIDTH),
            ..default()
        })
        .id();
    commands.spawn((
        EditorText::Heading,
        ui::title(&fonts, "", 28.0),
        ChildOf(preview),
    ));
    commands.spawn((
        EditorText::WeaponLevel,
        ui::label(&fonts, "", 16.0, ui::ACCENT),
        ChildOf(preview),
    ));
    stats::spawn_bars(&mut commands, &fonts, preview);
    commands.spawn((
        EditorText::Details,
        ui::label(&fonts, "", 15.0, ui::TEXT_DIM),
        Node {
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        ChildOf(preview),
    ));

    commands.spawn((
        EditorText::Status,
        ui::label(&fonts, "", 20.0, LOCKED),
        ChildOf(root),
    ));
}

/// Writes the draft, with the typed name, into its custom slot of the Profile and returns to the
/// Loadout screen on it. A class that breaks a rule or holds something locked is refused.
fn save(
    _: On<Activate>,
    mut draft: ResMut<Draft>,
    name: Single<&EditableText, With<NameField>>,
    data: Res<GameData>,
    mut profile: ResMut<Profile>,
    mut preview: ResMut<Preview>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    let mut loadout = draft.loadout.clone();
    let typed = name.value().to_string();
    loadout.name = if typed.trim().is_empty() {
        format!("Custom {}", draft.slot + 1)
    } else {
        typed
    };
    let loadout = loadout.validated(&data);
    let errors = class_problems(&loadout, &profile, &data);
    if !errors.is_empty() {
        let text = errors.join("; ");
        info!("Can't save class {}: {text}", draft.slot + 1);
        draft.message = Some(format!("Can't save yet: {text}"));
        return;
    }
    info!(
        "Saved custom class {}: \"{}\" ({} / {} points)\n{}",
        draft.slot + 1,
        loadout.name,
        data.class_cost(&loadout),
        data.class_points.budget,
        describe(&loadout, &data)
    );
    if profile.custom[draft.slot] != loadout {
        profile.custom[draft.slot] = loadout;
    }
    preview.0 = LoadoutChoice::Custom(draft.slot);
    overlay.set(Overlay::Loadout);
}

/// Everything wrong with `loadout` for this Profile (rules and unlocks), one text each.
fn class_problems(loadout: &Loadout, profile: &Profile, data: &GameData) -> Vec<String> {
    data.class_errors(loadout, Some(profile.standing(loadout, data)))
        .iter()
        .map(|error| error.describe(data))
        .collect()
}

/// Shows the draft: the points meter, each picker's choice and cost (greyed, with what it takes,
/// while locked), the Wildcard buttons, the primary's stat bars with its Attachments, what the
/// Attachments, Perks and Wildcards do, and why the last pick was refused or the class can't be
/// saved yet.
#[allow(clippy::too_many_arguments)]
fn refresh(
    data: Res<GameData>,
    draft: Res<Draft>,
    profile: Res<Profile>,
    added: Query<(), Added<EditorText>>,
    mut texts: Query<(&EditorText, &mut Text, &mut TextColor)>,
    mut notes: Query<(&EditorText, &mut Node), (Without<StatFill>, Without<FieldRow>)>,
    mut rows: Query<(&FieldRow, &mut Node), (Without<EditorText>, Without<StatFill>)>,
    mut pips: Query<(&Pip, &mut BackgroundColor)>,
    buttons: Query<(&WildcardButton, &Children)>,
    mut button_texts: Query<(&mut Text, &mut TextColor), Without<EditorText>>,
    mut fills: Query<(&StatFill, &mut Node), (Without<EditorText>, Without<FieldRow>)>,
) {
    if added.is_empty() && !draft.is_changed() && !profile.is_changed() {
        return;
    }
    let loadout = &draft.loadout;
    let problems = class_problems(loadout, &profile, &data);
    let spent = data.class_cost(loadout);
    let stats = data.effective_weapon(
        loadout.primary,
        loadout.attachments,
        &PerkEffects::default(),
    );
    for (kind, mut text, mut color) in &mut texts {
        let (shown, tint) = match *kind {
            EditorText::Slot => (format!("CUSTOM {}", draft.slot + 1), ui::ACCENT),
            EditorText::Value(field) => {
                let locked = lock_note(loadout, field, &profile, &data).is_some();
                (
                    value_text(loadout, field, &data),
                    if locked { ui::TEXT_DIM } else { ui::ACCENT },
                )
            }
            EditorText::Note(field) => (
                lock_note(loadout, field, &profile, &data).unwrap_or_default(),
                LOCKED,
            ),
            EditorText::Points => (
                format!("{spent} / {}", data.class_points.budget),
                if spent > data.class_points.budget {
                    LOCKED
                } else {
                    ui::ACCENT
                },
            ),
            EditorText::Heading => (
                format!(
                    "{}  -  {}",
                    stats.name.to_uppercase(),
                    stats::fire_mode_text(&stats)
                ),
                ui::TEXT,
            ),
            EditorText::WeaponLevel => (weapon_level_text(loadout, &profile, &data), ui::ACCENT),
            EditorText::Details => (effects_text(loadout, &data), ui::TEXT_DIM),
            // A refusal shows until the next pick; otherwise whatever still stops a save.
            EditorText::Status => (
                match (&draft.message, problems.is_empty()) {
                    (Some(message), _) => message.clone(),
                    (None, true) => String::new(),
                    (None, false) => format!("Can't save yet: {}", problems.join("; ")),
                },
                LOCKED,
            ),
        };
        if text.0 != shown {
            text.0 = shown;
        }
        color.set_if_neq(TextColor(tint));
    }
    // A note only takes room while it says something.
    for (kind, mut node) in &mut notes {
        if let EditorText::Note(field) = *kind {
            let display = if lock_note(loadout, field, &profile, &data).is_some() {
                Display::Flex
            } else {
                Display::None
            };
            node.set_if_neq(Node {
                display,
                ..node.clone()
            });
        }
    }
    for (row, mut node) in &mut rows {
        let display = if row.0.shown(loadout) {
            Display::Flex
        } else {
            Display::None
        };
        node.set_if_neq(Node {
            display,
            ..node.clone()
        });
    }
    for (pip, mut color) in &mut pips {
        let lit = if pip.0 < spent {
            if spent > data.class_points.budget {
                LOCKED
            } else {
                ui::ACCENT
            }
        } else {
            PIP_EMPTY
        };
        color.set_if_neq(BackgroundColor(lit));
    }
    for (button, children) in &buttons {
        let id = button.0;
        let unlocked = data
            .progression
            .is_unlocked(UnlockItem::Wildcard(id), profile.level(&data));
        let tint = if loadout.has(id) {
            ui::ACCENT
        } else if unlocked {
            ui::TEXT
        } else {
            ui::TEXT_DIM
        };
        for child in children {
            if let Ok((mut text, mut color)) = button_texts.get_mut(*child) {
                let shown = wildcard_text(id, &profile, &data);
                if text.0 != shown {
                    text.0 = shown;
                }
                color.set_if_neq(TextColor(tint));
            }
        }
    }
    stats::update_bars(&mut fills, &data, &stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::{Attachments, PERK_SLOTS, Perks};

    fn data() -> GameData {
        GameData::load()
    }

    /// A Primary and nothing else.
    fn bare(primary: WeaponId) -> Loadout {
        Loadout {
            name: "Test".into(),
            primary,
            attachments: Attachments::default(),
            secondary: None,
            perks: Perks::default(),
            lethal: None,
            tactical: None,
            wildcards: Vec::new(),
            camo: NO_CAMO.into(),
        }
    }

    /// Applies a pick that must go through.
    fn go(loadout: &mut Loadout, field: Field, step: isize, data: &GameData) {
        *loadout = try_pick(loadout, field, step, data).expect("the pick goes through");
    }

    #[test]
    fn pick_wraps_both_ways() {
        let options = [1, 2, 3];
        assert_eq!(pick(&options, 3, 1), 1);
        assert_eq!(pick(&options, 1, -1), 3);
        assert_eq!(pick(&options, 2, 1), 3);
        // A value that is not an option starts from the first.
        assert_eq!(pick(&options, 9, 1), 2);
    }

    #[test]
    fn cycling_walks_the_choices_of_each_field() {
        let data = data();
        let mut loadout = bare(WeaponId::AssaultRifle);
        // Without the points for it a 4x Optic is refused, so give the class room first.
        let optic = Field::Attachment(AttachmentSlot::Optic);
        for expected in [
            AttachmentId::RedDot,
            AttachmentId::Holographic,
            AttachmentId::Prism,
            AttachmentId::Acog,
        ] {
            go(&mut loadout, optic, 1, &data);
            assert_eq!(loadout.attachments.optic, Some(expected));
        }
        go(&mut loadout, optic, 1, &data);
        assert_eq!(loadout.attachments.optic, None, "wraps to empty");

        // Muzzle: none <-> Silencer. Underbarrel: none <-> Foregrip.
        let muzzle = Field::Attachment(AttachmentSlot::Muzzle);
        go(&mut loadout, muzzle, 1, &data);
        assert_eq!(loadout.attachments.muzzle, Some(AttachmentId::Silencer));
        go(&mut loadout, muzzle, -1, &data);
        assert_eq!(loadout.attachments.muzzle, None);

        // Perk Slot 1 swaps between its two Perks and empty, never leaving the slot.
        let first = Field::Perk(0);
        go(&mut loadout, first, 1, &data);
        assert_eq!(loadout.perks.picks[0], Some(PerkId::SleightOfHand));
        go(&mut loadout, first, 1, &data);
        assert_eq!(loadout.perks.picks[0], Some(PerkId::Scavenger));
        assert_eq!(data.perk(PerkId::Scavenger).slot, 1);
        go(&mut loadout, first, 1, &data);
        assert_eq!(loadout.perks.picks[0], None);
    }

    #[test]
    fn the_camo_picker_walks_none_then_every_camo_and_wraps() {
        let data = data();
        let mut loadout = bare(WeaponId::AssaultRifle);
        assert_eq!(loadout.camo, NO_CAMO);
        for camo in &data.progression.camos {
            go(&mut loadout, Field::Camo, 1, &data);
            assert_eq!(loadout.camo, camo.id);
        }
        go(&mut loadout, Field::Camo, 1, &data);
        assert_eq!(loadout.camo, NO_CAMO);
        go(&mut loadout, Field::Camo, -1, &data);
        assert_eq!(loadout.camo, data.progression.camos.last().unwrap().id);
    }

    #[test]
    fn a_primary_without_room_drops_its_optic_and_the_optic_picker_says_why() {
        let data = data();
        let mut loadout = bare(WeaponId::Smg);
        loadout.attachments.optic = Some(AttachmentId::RedDot);
        // Smg -> Sniper: its built-in scope leaves no room for an Optic.
        go(&mut loadout, Field::Primary, 1, &data);
        assert_eq!(loadout.primary, WeaponId::SniperRifle);
        assert_eq!(loadout.attachments.optic, None);
        let optic = Field::Attachment(AttachmentSlot::Optic);
        assert!(try_pick(&loadout, optic, 1, &data).is_err());
        assert_eq!(value_text(&loadout, optic, &data), "BUILT-IN SCOPE");
    }

    #[test]
    fn the_secondary_picker_offers_primaries_only_with_overkill() {
        let data = data();
        let mut loadout = bare(WeaponId::AssaultRifle);
        let secondary = Field::Secondary;
        go(&mut loadout, secondary, 1, &data);
        assert_eq!(loadout.secondary, Some(WeaponId::Pistol));
        go(&mut loadout, secondary, 1, &data);
        assert_eq!(
            loadout.secondary, None,
            "without Overkill: pistol or nothing"
        );

        loadout = try_toggle(&loadout, WildcardId::Overkill, &data).unwrap();
        for expected in [
            WeaponId::Pistol,
            WeaponId::AssaultRifle,
            WeaponId::Smg,
            WeaponId::SniperRifle,
        ] {
            go(&mut loadout, secondary, 1, &data);
            assert_eq!(loadout.secondary, Some(expected));
        }
        // Overkill can't be taken out from under a Primary in the second slot.
        assert!(
            try_toggle(&loadout, WildcardId::Overkill, &data)
                .unwrap_err()
                .contains("Overkill")
        );
    }

    /// The whole flow of building the exactly-10-point class, then the 11th point refused.
    #[test]
    fn building_a_ten_point_class_and_being_refused_the_eleventh() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        let optic = Field::Attachment(AttachmentSlot::Optic);
        let muzzle = Field::Attachment(AttachmentSlot::Muzzle);
        let underbarrel = Field::Attachment(AttachmentSlot::Underbarrel);
        go(&mut class, optic, 1, &data); // Red Dot
        go(&mut class, muzzle, 1, &data); // Silencer
        assert_eq!(data.class_cost(&class), 3);

        // A third Attachment needs Gunfighter.
        let refused = try_pick(&class, underbarrel, 1, &data).unwrap_err();
        assert!(refused.contains("Gunfighter"), "{refused}");
        class = try_toggle(&class, WildcardId::Gunfighter, &data).unwrap();
        go(&mut class, underbarrel, 1, &data); // Foregrip
        assert_eq!(
            data.class_cost(&class),
            5,
            "Primary 1 + 3 Attachments + Gunfighter 1"
        );

        for slot in 0..PERK_SLOTS {
            go(&mut class, Field::Perk(slot), 1, &data);
        }
        assert_eq!(data.class_cost(&class), 8);

        // A second Perk needs its slot's Greed.
        assert!(
            try_pick(&class, Field::Greed(0), 1, &data)
                .unwrap_err()
                .contains("Perk 1 Greed")
        );
        class = try_toggle(&class, WildcardId::PerkGreed1, &data).unwrap();
        go(&mut class, Field::Greed(0), 1, &data);
        assert_eq!(data.class_cost(&class), 10, "exactly 10 points");
        assert!(data.class_errors(&class, None).is_empty());

        // The 11th point is refused, whatever it would be spent on.
        let eleventh = try_pick(&class, Field::Secondary, 1, &data).unwrap_err();
        assert_eq!(eleventh, "Not enough points: 11 / 10");
        assert!(try_toggle(&class, WildcardId::Overkill, &data).is_err());
        // Freeing a point makes room again: drop the Silencer, then the Secondary fits.
        go(&mut class, muzzle, 1, &data); // wraps to none
        assert_eq!(class.attachments.muzzle, None);
        go(&mut class, Field::Secondary, 1, &data);
        assert_eq!(class.secondary, Some(WeaponId::Pistol));
        assert_eq!(data.class_cost(&class), 10);
    }

    #[test]
    fn a_heavy_optic_costs_two_and_is_refused_at_the_limit() {
        let data = data();
        let mut class = bare(WeaponId::AssaultRifle);
        class.secondary = Some(WeaponId::Pistol);
        class.perks.picks = [
            Some(PerkId::SleightOfHand),
            Some(PerkId::StoppingPower),
            Some(PerkId::Marathon),
        ];
        class.attachments.muzzle = Some(AttachmentId::Silencer);
        class.wildcards = vec![WildcardId::Gunfighter, WildcardId::PerkGreed1];
        class.perks.greed[0] = Some(PerkId::Scavenger);
        // Primary 1 + Secondary 1 + 3 Perks + Silencer 1 + 2 Wildcards + Greed Perk 1 = 9.
        assert_eq!(data.class_cost(&class), 9);
        let optic = Field::Attachment(AttachmentSlot::Optic);
        // Stepping back from "none" lands on the last Optic, the 4x ACOG at 2 points.
        let refused = try_pick(&class, optic, -1, &data).unwrap_err();
        assert_eq!(refused, "Not enough points: 11 / 10");
        // The 1-point Red Dot still fits.
        let red_dot = try_pick(&class, optic, 1, &data).unwrap();
        assert_eq!(data.class_cost(&red_dot), 10);
        assert_eq!(
            value_text(&bare(WeaponId::AssaultRifle), Field::Primary, &data),
            "M4 (AR)  [1]"
        );
        assert_eq!(data.attachment_cost(AttachmentId::Acog), 2);
    }

    #[test]
    fn locked_items_say_which_track_unlocks_them() {
        let data = data();
        let mut profile = Profile::fresh(&data, 0);
        let mut class = bare(WeaponId::AssaultRifle);
        class.attachments.optic = Some(AttachmentId::Holographic);
        class.wildcards = vec![WildcardId::Overkill];
        class.secondary = Some(WeaponId::SniperRifle);
        let optic = Field::Attachment(AttachmentSlot::Optic);
        assert_eq!(
            lock_note(&class, optic, &profile, &data).as_deref(),
            Some("Unlocks at M4 level 3")
        );
        assert!(wildcard_text(WildcardId::Overkill, &profile, &data).contains("LEVEL 12"));
        assert!(wildcard_text(WildcardId::Gunfighter, &profile, &data).contains("LEVEL 6"));

        // Weapon Level 3 (30 kills) and Level 12 open them.
        profile.xp = data.progression.levels.xp_to_reach(12);
        profile.weapons.insert(
            WeaponId::AssaultRifle,
            crate::progression::WeaponRecord {
                xp: 30,
                ..Default::default()
            },
        );
        assert_eq!(lock_note(&class, optic, &profile, &data), None);
        assert!(!wildcard_text(WildcardId::Overkill, &profile, &data).contains("LEVEL"));
        assert!(class_problems(&class, &profile, &data).is_empty());
    }
}
