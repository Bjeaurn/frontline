//! The Loadout screen (`Overlay::Loadout`): the Default Loadouts (read-only) and the Profile's
//! custom ones on the left, the selected one's Weapons, Perks and stat bars on the right.
//! CHOOSE confirms, CREATE A CLASS opens the class editor (`editor`), Esc goes back. B opens it in game. The
//! player's Level and XP show on top; a custom class the Level or a camo Challenge hasn't
//! unlocked is greyed, says why, and can't be chosen. Default Loadouts are always usable.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::SpawnClock;
use super::stats::{self, StatFill};
use crate::class::EquipmentSlot;
use crate::data::GameData;
use crate::flow::Overlay;
use crate::gear::{Loadout, NO_CAMO, PERK_SLOTS, PerkEffects};
use crate::player::LocalLife;
use crate::profile::{LoadoutChoice, Profile};
use crate::shared::LocalPlayer;
use crate::ui::{self, UiFonts};
use crate::weapon::Kit;

/// Opens the Loadout screen during play.
const OPEN_KEY: KeyCode = KeyCode::KeyB;

pub(super) struct LoadoutScreenPlugin;

impl Plugin for LoadoutScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Preview>()
            .init_resource::<ReturnTo>()
            .add_systems(OnEnter(Overlay::Loadout), spawn_screen)
            .add_systems(
                Update,
                (
                    remember_origin,
                    open_with_key.run_if(in_state(Overlay::None)),
                    (back_on_esc, refresh).run_if(in_state(Overlay::Loadout)),
                )
                    .chain(),
            );
    }
}

/// The Loadout highlighted on the screen, confirmed with CHOOSE.
#[derive(Resource, Debug)]
struct Preview(pub LoadoutChoice);

impl Default for Preview {
    fn default() -> Self {
        Self(LoadoutChoice::Default(0))
    }
}

/// The overlay the screen was opened from; Esc returns there.
#[derive(Resource, Default, Debug)]
struct ReturnTo(Overlay);

#[derive(Component)]
struct LoadoutButton(LoadoutChoice);

/// The Weapon name inside a `LoadoutButton`.
#[derive(Component)]
struct ButtonWeaponName;

#[derive(Component)]
struct ClassName;

/// The Weapons, Attachments and Perks of the previewed Loadout, one per line.
#[derive(Component)]
struct ClassLines;

#[derive(Component)]
struct FireModeText;

#[derive(Component)]
struct NextSpawnNote;

/// The player's Level and XP.
#[derive(Component)]
struct LevelLine;

/// Why the previewed Loadout can't be chosen yet.
#[derive(Component)]
struct LockNote;

/// Colour of what is locked.
const LOCKED: Color = Color::srgb(0.85, 0.42, 0.34);

/// What a Loadout holds, one line each, for the details panel and tooltips.
fn describe(loadout: &Loadout, data: &GameData) -> String {
    let primary = &data.weapon(loadout.primary).name;
    let fitted: Vec<&str> = loadout
        .attachments
        .iter()
        .map(|id| data.attachment(id).name.as_str())
        .collect();
    let fitted = if fitted.is_empty() {
        String::new()
    } else {
        format!("  +  {}", fitted.join(", "))
    };
    let camo = data
        .progression
        .camo(&loadout.camo)
        .filter(|_| loadout.camo != NO_CAMO)
        .map_or(String::new(), |camo| format!("\nCAMO  {}", camo.name));
    let secondary = loadout
        .secondary
        .map_or("None", |id| data.weapon(id).name.as_str());
    let perks: Vec<String> = (0..PERK_SLOTS)
        .map(|slot| {
            let names: Vec<&str> = loadout
                .perks
                .in_slot(slot)
                .map(|perk| data.perk(perk).name.as_str())
                .collect();
            format!(
                "PERK {}  {}",
                slot + 1,
                if names.is_empty() {
                    "None".to_owned()
                } else {
                    names.join(" + ")
                }
            )
        })
        .collect();
    let equipment: Vec<&str> = EquipmentSlot::ALL
        .into_iter()
        .filter_map(|slot| loadout.equipment(slot))
        .map(|id| data.class_points.equipment[&id].name.as_str())
        .collect();
    let wildcards = if loadout.wildcards.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = loadout
            .wildcards
            .iter()
            .map(|id| data.wildcard_name(*id))
            .collect();
        format!("\nWILDCARDS  {}", names.join(", "))
    };
    let mut lines = vec![
        format!("PRIMARY  {primary}{fitted}{camo}"),
        format!("SECONDARY  {secondary}"),
    ];
    lines.extend(perks);
    if !equipment.is_empty() {
        lines.push(format!("EQUIPMENT  {}", equipment.join(" + ")));
    }
    format!(
        "{}{wildcards}\nPOINTS  {} / {}",
        lines.join("\n"),
        data.class_cost(loadout),
        data.class_points.budget
    )
}

fn open_with_key(keys: Res<ButtonInput<KeyCode>>, mut overlay: ResMut<NextState<Overlay>>) {
    if keys.just_pressed(OPEN_KEY) {
        overlay.set(Overlay::Loadout);
    }
}

/// Remembers where the screen was opened from. Coming back from the editor changes nothing.
fn remember_origin(
    mut transitions: MessageReader<StateTransitionEvent<Overlay>>,
    mut return_to: ResMut<ReturnTo>,
) {
    for transition in transitions.read() {
        if transition.entered == Some(Overlay::Loadout)
            && transition.exited != transition.entered
            && transition.exited != Some(Overlay::ClassEditor)
        {
            return_to.0 = transition.exited.unwrap_or_default();
        }
    }
}

fn back_on_esc(
    keys: Res<ButtonInput<KeyCode>>,
    return_to: Res<ReturnTo>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        overlay.set(return_to.0);
    }
}

/// One selectable Loadout in the list.
fn class_button(
    commands: &mut Commands,
    fonts: &UiFonts,
    list: Entity,
    choice: LoadoutChoice,
    loadout: &Loadout,
    data: &GameData,
) {
    commands
        .spawn((
            ui::button(fonts, &loadout.name),
            LoadoutButton(choice),
            ChildOf(list),
        ))
        .with_child((
            ButtonWeaponName,
            ui::label(
                fonts,
                &data.weapon(loadout.primary).name,
                18.0,
                ui::TEXT_DIM,
            ),
            Node {
                margin: UiRect::left(px(28.0)),
                ..default()
            },
        ))
        .observe(move |_: On<Activate>, mut preview: ResMut<Preview>| {
            preview.0 = choice;
        });
}

fn spawn_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    data: Res<GameData>,
    profile: Res<Profile>,
    mut preview: ResMut<Preview>,
) {
    preview.0 = profile.chosen;
    let root = commands
        .spawn((
            Name::new("LoadoutScreen"),
            DespawnOnExit(Overlay::Loadout),
            ui::backdrop(),
        ))
        .id();
    commands.spawn((ui::title(&fonts, "Choose Loadout", 48.0), ChildOf(root)));
    commands.spawn((
        LevelLine,
        ui::label(&fonts, "", 22.0, ui::ACCENT),
        ChildOf(root),
    ));

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

    let list = commands.spawn((ui::panel(), ChildOf(columns))).id();
    commands.spawn((
        ui::label(&fonts, "DEFAULT CLASSES", 18.0, ui::TEXT_DIM),
        ChildOf(list),
    ));
    for (index, loadout) in data.default_loadouts.iter().enumerate() {
        class_button(
            &mut commands,
            &fonts,
            list,
            LoadoutChoice::Default(index),
            loadout,
            &data,
        );
    }
    commands.spawn((
        ui::label(&fonts, "CUSTOM CLASSES", 18.0, ui::TEXT_DIM),
        Node {
            margin: UiRect::top(px(10.0)),
            ..default()
        },
        ChildOf(list),
    ));
    for (slot, loadout) in profile.custom.iter().enumerate() {
        class_button(
            &mut commands,
            &fonts,
            list,
            LoadoutChoice::Custom(slot),
            loadout,
            &data,
        );
    }

    let details = commands
        .spawn((ui::panel(), ChildOf(columns)))
        .insert(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24.0)),
            row_gap: px(12.0),
            border: UiRect::all(px(1.0)),
            min_width: px(500.0),
            ..default()
        })
        .id();
    commands.spawn((ClassName, ui::title(&fonts, "", 34.0), ChildOf(details)));
    commands.spawn((
        ClassLines,
        ui::label(&fonts, "", 20.0, ui::TEXT),
        ChildOf(details),
    ));
    commands.spawn((
        FireModeText,
        ui::label(&fonts, "", 20.0, ui::TEXT_DIM),
        ChildOf(details),
    ));
    stats::spawn_bars(&mut commands, &fonts, details);

    commands.spawn((LockNote, ui::label(&fonts, "", 22.0, LOCKED), ChildOf(root)));
    commands.spawn((
        NextSpawnNote,
        ui::label(&fonts, "Applies on next spawn", 22.0, ui::ACCENT),
        Visibility::Hidden,
        ChildOf(root),
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
        .observe(
            |_: On<Activate>, return_to: Res<ReturnTo>, mut overlay: ResMut<NextState<Overlay>>| {
                overlay.set(return_to.0);
            },
        );
    commands
        .spawn((ui::button(&fonts, "Create a Class"), ChildOf(actions)))
        .observe(|_: On<Activate>, mut overlay: ResMut<NextState<Overlay>>| {
            overlay.set(Overlay::ClassEditor);
        });
    commands
        .spawn((ui::button(&fonts, "Choose"), ChildOf(actions)))
        .observe(choose);
}

/// Confirms the previewed Loadout, saving the choice in the Profile, and returns to the game.
fn choose(
    _: On<Activate>,
    preview: Res<Preview>,
    data: Res<GameData>,
    mut profile: ResMut<Profile>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    let locks = profile.locks(preview.0, &data);
    if !locks.is_empty() {
        info!("Can't choose that class: {}", locks.join("; "));
        return;
    }
    if profile.chosen != preview.0 {
        profile.chosen = preview.0;
    }
    info!("Loadout chosen: {}", profile.loadout(&data).name);
    overlay.set(Overlay::None);
}

/// Highlights the previewed Loadout and shows its Weapons, Perks, stats and whether it
/// applies now.
#[allow(clippy::too_many_arguments)]
fn refresh(
    time: Res<Time>,
    data: Res<GameData>,
    preview: Res<Preview>,
    profile: Res<Profile>,
    clock: Res<SpawnClock>,
    life: Res<LocalLife>,
    kit: Query<&Kit, With<LocalPlayer>>,
    buttons: Query<(&LoadoutButton, &Children)>,
    mut texts: Query<(&mut TextColor, Has<ButtonWeaponName>)>,
    mut name: Single<&mut Text, (With<ClassName>, Without<ClassLines>, Without<FireModeText>)>,
    mut lines: Single<&mut Text, (With<ClassLines>, Without<ClassName>, Without<FireModeText>)>,
    mut fire_mode: Single<&mut Text, (With<FireModeText>, Without<ClassName>, Without<ClassLines>)>,
    mut fills: Query<(&StatFill, &mut Node)>,
    mut note: Single<&mut Visibility, With<NextSpawnNote>>,
    mut level_line: Single<
        &mut Text,
        (
            With<LevelLine>,
            Without<LockNote>,
            Without<ClassName>,
            Without<ClassLines>,
            Without<FireModeText>,
        ),
    >,
    mut lock_note: Single<
        &mut Text,
        (
            With<LockNote>,
            Without<LevelLine>,
            Without<ClassName>,
            Without<ClassLines>,
            Without<FireModeText>,
        ),
    >,
) {
    let loadout = profile.get(preview.0, &data);

    for (button, children) in &buttons {
        let selected = button.0 == preview.0;
        let locked = !profile.locks(button.0, &data).is_empty();
        for child in children {
            if let Ok((mut color, weapon_name)) = texts.get_mut(*child) {
                let wanted = match (selected, locked, weapon_name) {
                    (true, _, _) => ui::ACCENT,
                    (false, true, _) | (false, false, true) => ui::TEXT_DIM,
                    (false, false, false) => ui::TEXT,
                };
                color.set_if_neq(TextColor(wanted));
            }
        }
    }

    if preview.is_changed() || profile.is_changed() || name.is_added() {
        let stats = data.effective_weapon(
            loadout.primary,
            loadout.attachments,
            &PerkEffects::default(),
        );
        name.0 = loadout.name.to_uppercase();
        lines.0 = describe(loadout, &data);
        fire_mode.0 = stats::fire_mode_text(&stats).to_owned();
        stats::update_bars(&mut fills, &data, &stats);
    }

    let progress = profile.level_progress(&data);
    let level_text = if progress.maxed() {
        format!("LEVEL {}  -  MAX", progress.level)
    } else {
        format!(
            "LEVEL {}  -  {} / {} XP",
            progress.level, progress.into_level, progress.span
        )
    };
    let level_text = if profile.custom_unlocked(&data) {
        level_text
    } else {
        format!(
            "{level_text}  -  custom classes unlock at Level {}",
            data.progression
                .unlock_level(crate::progression::UnlockItem::CustomLoadouts)
        )
    };
    if level_line.0 != level_text {
        level_line.0 = level_text;
    }
    let locks = profile.locks(preview.0, &data).join("; ");
    if lock_note.0 != locks {
        lock_note.0 = locks;
    }

    let equipped = kit.single().is_ok_and(|kit| kit.loadout() == loadout);
    let deferred = !equipped && !clock.swap_open(time.elapsed_secs_f64(), &life, &data);
    note.set_if_neq(if deferred {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}
