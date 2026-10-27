//! Create a Class (`Overlay::ClassEditor`), opened from the Loadout screen: pick one of the
//! Profile's custom slots, then its Primary with an Optic and a Muzzle Attachment, its Secondary,
//! one Perk per Perk Slot and a name. The stat bars follow the Attachments. SAVE writes the class
//! into the Profile; the Default Loadouts are never editable.

use bevy::ecs::observer::IntoEntityObserver;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;

use super::screen::{Preview, describe};
use super::stats::{self, StatFill};
use crate::data::{GameData, WeaponClass};
use crate::flow::Overlay;
use crate::gear::{
    AttachmentId, AttachmentSlot, Loadout, MAX_LOADOUT_NAME_CHARS, PERK_SLOTS, PerkEffects,
};
use crate::profile::{CUSTOM_SLOTS, LoadoutChoice, Profile};
use crate::ui::{self, UiFonts};

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
        }
    }
}

/// One thing a class picks, cycled with a pair of arrow buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Primary,
    Optic,
    Muzzle,
    Secondary,
    /// Perk Slot, counting from 0.
    Perk(usize),
}

impl Field {
    const ALL: [Field; 4 + PERK_SLOTS] = [
        Field::Primary,
        Field::Optic,
        Field::Muzzle,
        Field::Secondary,
        Field::Perk(0),
        Field::Perk(1),
        Field::Perk(2),
    ];

    fn label(self) -> String {
        match self {
            Field::Primary => "PRIMARY".into(),
            Field::Optic => "OPTIC".into(),
            Field::Muzzle => "MUZZLE".into(),
            Field::Secondary => "SECONDARY".into(),
            Field::Perk(slot) => format!("PERK {}", slot + 1),
        }
    }
}

/// The text shown for what `field` currently holds.
#[derive(Component)]
struct PickerValue(Field);

#[derive(Component)]
struct SlotValue;

#[derive(Component)]
struct NameField;

#[derive(Component)]
struct FireModeText;

#[derive(Component)]
struct Details;

/// The option after (`step` 1) or before (`step` -1) `current` in `options`, wrapping around.
fn pick<T: Copy + PartialEq>(options: &[T], current: T, step: isize) -> T {
    let at = options
        .iter()
        .position(|option| *option == current)
        .unwrap_or(0) as isize;
    options[(at + step).rem_euclid(options.len() as isize) as usize]
}

fn attachment_options(data: &GameData, slot: AttachmentSlot) -> Vec<Option<AttachmentId>> {
    std::iter::once(None)
        .chain(
            AttachmentId::ALL
                .into_iter()
                .filter(|id| data.attachment(*id).slot == slot)
                .map(Some),
        )
        .collect()
}

/// Moves `field` of `loadout` to its next (`step` 1) or previous (`step` -1) choice, then
/// repairs whatever the new choice leaves invalid (a Primary without room for the old Optic).
fn cycle(loadout: &mut Loadout, field: Field, step: isize, data: &GameData) {
    match field {
        Field::Primary => {
            let primaries: Vec<_> = crate::data::WeaponId::ALL
                .into_iter()
                .filter(|id| data.weapon(*id).class == WeaponClass::Primary)
                .collect();
            loadout.primary = pick(&primaries, loadout.primary, step);
        }
        Field::Secondary => {
            let secondaries: Vec<_> = crate::data::WeaponId::ALL
                .into_iter()
                .filter(|id| data.weapon(*id).class == WeaponClass::Secondary)
                .collect();
            loadout.secondary = pick(&secondaries, loadout.secondary, step);
        }
        Field::Optic if data.weapon(loadout.primary).accepts(AttachmentSlot::Optic) => {
            let options = attachment_options(data, AttachmentSlot::Optic);
            loadout.attachments.optic = pick(&options, loadout.attachments.optic, step);
        }
        Field::Muzzle if data.weapon(loadout.primary).accepts(AttachmentSlot::Muzzle) => {
            let options = attachment_options(data, AttachmentSlot::Muzzle);
            loadout.attachments.muzzle = pick(&options, loadout.attachments.muzzle, step);
        }
        Field::Optic | Field::Muzzle => {}
        Field::Perk(slot) => {
            let perks: Vec<_> = data.perks_in_slot(slot + 1).collect();
            loadout.perks[slot] = pick(&perks, loadout.perks[slot], step);
        }
    }
    *loadout = loadout.clone().validated(data);
}

/// What `field` holds, for its label.
fn value_text(loadout: &Loadout, field: Field, data: &GameData) -> String {
    let attachment = |id: Option<AttachmentId>| {
        id.map_or("None", |id| data.attachment(id).name.as_str())
            .to_owned()
    };
    match field {
        Field::Primary => data.weapon(loadout.primary).name.clone(),
        Field::Secondary => data.weapon(loadout.secondary).name.clone(),
        Field::Optic if data.weapon(loadout.primary).accepts(AttachmentSlot::Optic) => {
            attachment(loadout.attachments.optic)
        }
        Field::Muzzle if data.weapon(loadout.primary).accepts(AttachmentSlot::Muzzle) => {
            attachment(loadout.attachments.muzzle)
        }
        // A built-in scope leaves no room for an Optic.
        Field::Optic | Field::Muzzle => "Built-in scope".to_owned(),
        Field::Perk(slot) => data.perk(loadout.perks[slot]).name.clone(),
    }
    .to_uppercase()
}

/// What the fitted Attachments and the Perks do, one per line.
fn effects_text(loadout: &Loadout, data: &GameData) -> String {
    let attachments = loadout.attachments.iter().map(|id| {
        let attachment = data.attachment(id);
        format!("{}: {}", attachment.name, attachment.description)
    });
    let perks = loadout.perks.iter().map(|id| {
        let perk = data.perk(*id);
        format!("{}: {}", perk.name, perk.description)
    });
    attachments.chain(perks).collect::<Vec<_>>().join("\n")
}

fn back_on_esc(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<NextState<Overlay>>) {
    if keys.just_pressed(KeyCode::Escape) {
        overlay.set(Overlay::Loadout);
    }
}

/// A row of the form: `label`, previous arrow, a value text carrying `value`, next arrow. The
/// arrows run the observers with `-1` and `+1`.
fn picker_row<M1, M2>(
    commands: &mut Commands,
    fonts: &UiFonts,
    parent: Entity,
    label: &str,
    value: impl Bundle,
    previous: impl IntoEntityObserver<M1>,
    next: impl IntoEntityObserver<M2>,
) {
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
        ui::label(fonts, label, 20.0, ui::TEXT_DIM),
        Node {
            width: px(120.0),
            ..default()
        },
        ChildOf(row),
    ));
    commands
        .spawn((ChildOf(row), ui::compact_button(fonts, "<", 36.0, 22.0)))
        .observe(previous);
    commands.spawn((
        value,
        ui::label(fonts, "", 20.0, ui::ACCENT),
        Node {
            width: px(250.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        ChildOf(row),
    ));
    commands
        .spawn((ChildOf(row), ui::compact_button(fonts, ">", 36.0, 22.0)))
        .observe(next);
}

fn spawn_editor(mut commands: Commands, fonts: Res<UiFonts>, draft: Res<Draft>) {
    let root = commands
        .spawn((
            Name::new("ClassEditor"),
            DespawnOnExit(Overlay::ClassEditor),
            ui::backdrop(),
        ))
        .id();
    commands.spawn((ui::title(&fonts, "Create a Class", 48.0), ChildOf(root)));

    let columns = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();

    let form = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(10.0),
            border: UiRect::all(px(1.0)),
            ..default()
        })
        .id();

    // Which custom slot. Switching reopens the editor on that slot's saved class.
    picker_row(
        &mut commands,
        &fonts,
        form,
        "CLASS",
        SlotValue,
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

    let name_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
            ChildOf(form),
        ))
        .id();
    commands.spawn((
        ui::label(&fonts, "NAME", 20.0, ui::TEXT_DIM),
        Node {
            width: px(120.0),
            ..default()
        },
        ChildOf(name_row),
    ));
    commands.spawn((
        NameField,
        ui::text_field(&fonts, &draft.loadout.name, MAX_LOADOUT_NAME_CHARS),
        ChildOf(name_row),
    ));

    for field in Field::ALL {
        picker_row(
            &mut commands,
            &fonts,
            form,
            &field.label(),
            PickerValue(field),
            move |_: On<Activate>, mut draft: ResMut<Draft>, data: Res<GameData>| {
                cycle(&mut draft.loadout, field, -1, &data);
            },
            move |_: On<Activate>, mut draft: ResMut<Draft>, data: Res<GameData>| {
                cycle(&mut draft.loadout, field, 1, &data);
            },
        );
    }

    let preview = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(12.0),
            border: UiRect::all(px(1.0)),
            min_width: px(460.0),
            max_width: px(460.0),
            ..default()
        })
        .id();
    commands.spawn((FireModeText, ui::title(&fonts, "", 30.0), ChildOf(preview)));
    stats::spawn_bars(&mut commands, &fonts, preview);
    commands.spawn((
        Details,
        ui::label(&fonts, "", 18.0, ui::TEXT_DIM),
        Node {
            margin: UiRect::top(px(8.0)),
            ..default()
        },
        ChildOf(preview),
    ));

    let actions = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(16.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands
        .spawn((ui::button(&fonts, "Back"), ChildOf(actions)))
        .observe(|_: On<Activate>, mut overlay: ResMut<NextState<Overlay>>| {
            overlay.set(Overlay::Loadout);
        });
    commands
        .spawn((ui::button(&fonts, "Save"), ChildOf(actions)))
        .observe(save);
}

/// Writes the draft, with the typed name, into its custom slot of the Profile and returns to the
/// Loadout screen on it.
fn save(
    _: On<Activate>,
    draft: Res<Draft>,
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
    info!(
        "Saved custom class {}: \"{}\"\n{}",
        draft.slot + 1,
        loadout.name,
        describe(&loadout, &data)
    );
    if profile.custom[draft.slot] != loadout {
        profile.custom[draft.slot] = loadout;
    }
    preview.0 = LoadoutChoice::Custom(draft.slot);
    overlay.set(Overlay::Loadout);
}

/// Shows the draft: each picker's choice, the primary's stat bars with its Attachments, and what
/// the Attachments and Perks do.
#[allow(clippy::too_many_arguments)]
fn refresh(
    data: Res<GameData>,
    draft: Res<Draft>,
    mut values: Query<(&PickerValue, &mut Text), Without<SlotValue>>,
    mut slot: Single<
        &mut Text,
        (
            With<SlotValue>,
            Without<PickerValue>,
            Without<Details>,
            Without<FireModeText>,
        ),
    >,
    mut details: Single<
        &mut Text,
        (
            With<Details>,
            Without<PickerValue>,
            Without<SlotValue>,
            Without<FireModeText>,
        ),
    >,
    mut heading: Single<
        &mut Text,
        (
            With<FireModeText>,
            Without<PickerValue>,
            Without<SlotValue>,
            Without<Details>,
        ),
    >,
    mut fills: Query<(&StatFill, &mut Node)>,
) {
    if !draft.is_changed() && !slot.is_added() {
        return;
    }
    let loadout = &draft.loadout;
    for (value, mut text) in &mut values {
        let shown = value_text(loadout, value.0, &data);
        if text.0 != shown {
            text.0 = shown;
        }
    }
    slot.0 = format!("CUSTOM {}", draft.slot + 1);
    details.0 = effects_text(loadout, &data);

    let stats = data.effective_weapon(
        loadout.primary,
        loadout.attachments,
        &PerkEffects::default(),
    );
    heading.0 = format!(
        "{}  -  {}",
        stats.name.to_uppercase(),
        stats::fire_mode_text(&stats)
    );
    stats::update_bars(&mut fills, &data, &stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::WeaponId;

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
        let data = GameData::load();
        let mut loadout = data.default_loadouts[0].clone();

        // Optic: none -> Red Dot -> ACOG -> none.
        loadout.attachments.optic = None;
        cycle(&mut loadout, Field::Optic, 1, &data);
        assert_eq!(loadout.attachments.optic, Some(AttachmentId::RedDot));
        cycle(&mut loadout, Field::Optic, 1, &data);
        assert_eq!(loadout.attachments.optic, Some(AttachmentId::Acog));
        cycle(&mut loadout, Field::Optic, 1, &data);
        assert_eq!(loadout.attachments.optic, None);

        // Muzzle: none <-> Silencer.
        cycle(&mut loadout, Field::Muzzle, 1, &data);
        assert_eq!(loadout.attachments.muzzle, Some(AttachmentId::Silencer));
        cycle(&mut loadout, Field::Muzzle, -1, &data);
        assert_eq!(loadout.attachments.muzzle, None);

        // Perk Slot 1 swaps between its two Perks, never leaving the slot.
        let first = loadout.perks[0];
        cycle(&mut loadout, Field::Perk(0), 1, &data);
        assert_ne!(loadout.perks[0], first);
        assert_eq!(data.perk(loadout.perks[0]).slot, 1);
        cycle(&mut loadout, Field::Perk(0), 1, &data);
        assert_eq!(loadout.perks[0], first);
    }

    #[test]
    fn a_primary_without_room_drops_its_optic_and_secondaries_stay_secondary() {
        let data = GameData::load();
        let mut loadout = data.default_loadouts[0].clone();
        loadout.attachments.optic = Some(AttachmentId::Acog);
        loadout.primary = WeaponId::Smg;
        // Smg -> Sniper: its built-in scope leaves no room for an Optic.
        cycle(&mut loadout, Field::Primary, 1, &data);
        assert_eq!(loadout.primary, WeaponId::SniperRifle);
        assert_eq!(loadout.attachments.optic, None);
        // The Optic picker does nothing on it.
        cycle(&mut loadout, Field::Optic, 1, &data);
        assert_eq!(loadout.attachments.optic, None);

        cycle(&mut loadout, Field::Secondary, 1, &data);
        assert_eq!(data.weapon(loadout.secondary).class, WeaponClass::Secondary);
    }
}
