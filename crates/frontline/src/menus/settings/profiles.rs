//! Settings > Profile (main menu only): the list of Profiles with the active one marked, and
//! SWITCH, NEW, RENAME and DELETE (which asks first). The player's name is the active Profile's
//! name (CONTEXT.md: Profile), so renaming is how it is changed. These act at once, whatever
//! APPLY and BACK do with the other tabs, and the tab is not offered in the pause menu: a
//! Match must not change Profiles under it.

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use crate::data::GameData;
use crate::flow::Settings;
use crate::profile::book::{self, Entry, MAX_NAME_CHARS};
use crate::profile::{Profile, ProfileList, Profiles};
use crate::ui::{ACCENT, AMBER, TEXT, TEXT_DIM, UiFonts, compact_button, label, rule, text_field};

/// What the tab is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    New,
    Rename,
    /// DELETE was pressed: asking before the selected Profile goes.
    ConfirmDelete,
}

/// The open tab's state; present while the Settings screen is open from the main menu.
#[derive(Resource)]
pub(super) struct ProfileTab {
    /// The row clicked; the active Profile while it is `None` or gone.
    selected: Option<String>,
    mode: Mode,
    /// What the last action did, and whether it worked.
    message: Option<(String, bool)>,
}

impl ProfileTab {
    pub(super) fn new() -> ProfileTab {
        ProfileTab {
            selected: None,
            mode: Mode::List,
            message: None,
        }
    }
}

/// Everything in the tab; rebuilt whenever the state or the list changes.
#[derive(Component)]
pub(super) struct Content;

#[derive(Component)]
pub(super) struct NameField;

/// The line under the name field: what the name will be, or why it can't be.
#[derive(Component)]
pub(super) struct NameNote;

/// CREATE or RENAME, while the name field is up.
#[derive(Component)]
pub(super) struct Confirm;

/// The Profile the buttons act on: the row clicked if it still exists, else the active one.
fn effective(selected: Option<&str>, entries: &[Entry], active: Option<&str>) -> Option<String> {
    selected
        .filter(|id| entries.iter().any(|entry| entry.id == *id))
        .or(active)
        .map(str::to_owned)
}

/// Spawns the tab's content holder under `parent` and starts its state.
pub(super) fn spawn_tab(commands: &mut Commands, parent: Entity) {
    commands.insert_resource(ProfileTab::new());
    commands.spawn((
        ChildOf(parent),
        Content,
        Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(10.0),
            ..default()
        },
    ));
}

/// Rebuilds the tab when its state, the list or the active Profile changed.
#[allow(clippy::too_many_arguments)]
pub(super) fn refresh(
    tab: Res<ProfileTab>,
    list: Res<ProfileList>,
    settings: Res<Settings>,
    profile: Res<Profile>,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    content: Single<Entity, With<Content>>,
    mut last: Local<Option<(Option<String>, String)>>,
    mut commands: Commands,
) {
    let now = (settings.active_profile.clone(), profile.name.clone());
    if !tab.is_changed() && !list.is_changed() && last.as_ref() == Some(&now) {
        return;
    }
    *last = Some(now);
    let entries = list.entries();
    let active = settings.active_profile.as_deref();
    let selected = effective(tab.selected.as_deref(), entries, active);
    let content = *content;
    commands.entity(content).despawn_related::<Children>();

    commands.spawn((
        ChildOf(content),
        label(
            &fonts,
            "Your name is your profile's name: other players see it. Each profile keeps its own classes and XP.",
            18.0,
            TEXT_DIM,
        ),
    ));
    commands.spawn((
        ChildOf(content),
        label(
            &fonts,
            &format!(
                "ACTIVE:  {}   -   LEVEL {}",
                profile.name.to_uppercase(),
                profile.level(&data)
            ),
            26.0,
            ACCENT,
        ),
    ));
    commands.spawn((ChildOf(content), rule(AMBER)));

    match tab.mode {
        Mode::List => {
            spawn_list(
                &mut commands,
                &fonts,
                content,
                entries,
                active,
                selected.as_deref(),
            );
            spawn_actions(
                &mut commands,
                &fonts,
                content,
                &list,
                active,
                selected.as_deref(),
            );
        }
        Mode::New => spawn_naming(&mut commands, &fonts, content, "New profile", "", "Create"),
        Mode::Rename => spawn_naming(
            &mut commands,
            &fonts,
            content,
            "Rename the active profile",
            &profile.name,
            "Rename",
        ),
        Mode::ConfirmDelete => {
            let target = selected
                .as_deref()
                .and_then(|id| entries.iter().find(|entry| entry.id == id));
            spawn_confirm_delete(
                &mut commands,
                &fonts,
                content,
                target,
                entries.len(),
                active,
            );
        }
    }
    if let Some((text, good)) = &tab.message {
        commands.spawn((
            ChildOf(content),
            label(&fonts, text, 22.0, if *good { ACCENT } else { AMBER }),
        ));
    }
}

fn spawn_list(
    commands: &mut Commands,
    fonts: &UiFonts,
    content: Entity,
    entries: &[Entry],
    active: Option<&str>,
    selected: Option<&str>,
) {
    for entry in entries {
        let marker = if selected == Some(entry.id.as_str()) {
            "> "
        } else {
            "  "
        };
        let tag = if active == Some(entry.id.as_str()) {
            "   [ACTIVE]"
        } else {
            ""
        };
        let id = entry.id.clone();
        commands
            .spawn((
                ChildOf(content),
                compact_button(fonts, &format!("{marker}{}{tag}", entry.name), 420.0, 24.0),
            ))
            .observe(move |_: On<Activate>, mut tab: ResMut<ProfileTab>| {
                tab.selected = Some(id.clone());
                tab.message = None;
            });
    }
}

/// A row of buttons under `parent`.
fn button_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            ChildOf(parent),
            Node {
                column_gap: px(12.0),
                margin: UiRect::top(px(8.0)),
                ..default()
            },
        ))
        .id()
}

/// A button of `row`, greyed out and inert unless `enabled`.
fn action(
    commands: &mut Commands,
    fonts: &UiFonts,
    row: Entity,
    caption: &str,
    enabled: bool,
) -> Entity {
    let button = commands
        .spawn((ChildOf(row), compact_button(fonts, caption, 130.0, 24.0)))
        .id();
    if !enabled {
        commands.entity(button).insert(InteractionDisabled);
    }
    button
}

fn spawn_actions(
    commands: &mut Commands,
    fonts: &UiFonts,
    content: Entity,
    list: &ProfileList,
    active: Option<&str>,
    selected: Option<&str>,
) {
    let row = button_row(commands, content);
    let switch = action(commands, fonts, row, "Switch", selected != active);
    let new = action(commands, fonts, row, "New", book::has_room(list.entries()));
    let rename = action(commands, fonts, row, "Rename", active.is_some());
    let delete = action(commands, fonts, row, "Delete", selected.is_some());

    commands.entity(switch).observe(
        |_: On<Activate>, mut tab: ResMut<ProfileTab>, mut profiles: Profiles| {
            let Some(id) = effective(
                tab.selected.as_deref(),
                profiles.entries(),
                profiles.active_id(),
            ) else {
                return;
            };
            tab.message = Some(match profiles.switch(&id) {
                Ok(()) => (format!("Playing as {}", profiles.active_name()), true),
                Err(reason) => (reason, false),
            });
            tab.selected = Some(id);
        },
    );
    commands
        .entity(new)
        .observe(|_: On<Activate>, mut tab: ResMut<ProfileTab>| {
            tab.mode = Mode::New;
            tab.message = None;
        });
    commands
        .entity(rename)
        .observe(|_: On<Activate>, mut tab: ResMut<ProfileTab>| {
            tab.mode = Mode::Rename;
            tab.message = None;
        });
    commands.entity(delete).observe(
        |_: On<Activate>, mut tab: ResMut<ProfileTab>, profiles: Profiles| {
            tab.selected = effective(
                tab.selected.as_deref(),
                profiles.entries(),
                profiles.active_id(),
            );
            tab.mode = Mode::ConfirmDelete;
            tab.message = None;
        },
    );
}

/// The name field of NEW and RENAME, with its note and the CREATE / RENAME and CANCEL buttons.
fn spawn_naming(
    commands: &mut Commands,
    fonts: &UiFonts,
    content: Entity,
    heading: &str,
    initial: &str,
    confirm: &str,
) {
    commands.spawn((
        ChildOf(content),
        label(fonts, &heading.to_uppercase(), 22.0, TEXT_DIM),
    ));
    commands.spawn((
        ChildOf(content),
        NameField,
        AutoFocus,
        text_field(fonts, initial, MAX_NAME_CHARS),
    ));
    commands.spawn((ChildOf(content), NameNote, label(fonts, "", 20.0, ACCENT)));
    let row = button_row(commands, content);
    commands
        .spawn((
            ChildOf(row),
            Confirm,
            compact_button(fonts, confirm, 130.0, 24.0),
        ))
        .observe(
            |_: On<Activate>,
             field: Single<&EditableText, With<NameField>>,
             mut tab: ResMut<ProfileTab>,
             mut profiles: Profiles| {
                confirm_name(&field.value().to_string(), &mut tab, &mut profiles);
            },
        );
    commands
        .spawn((ChildOf(row), compact_button(fonts, "Cancel", 130.0, 24.0)))
        .observe(|_: On<Activate>, mut tab: ResMut<ProfileTab>| {
            tab.mode = Mode::List;
            tab.message = None;
        });
}

/// NEW or RENAME with `raw`: back to the list on success, the reason shown on failure.
fn confirm_name(raw: &str, tab: &mut ProfileTab, profiles: &mut Profiles) {
    let (result, done) = match tab.mode {
        Mode::New => (profiles.create(raw), "Created and switched to"),
        Mode::Rename => (profiles.rename(raw), "Renamed to"),
        Mode::List | Mode::ConfirmDelete => return,
    };
    match result {
        Ok(()) => {
            tab.message = Some((format!("{done} {}", profiles.active_name()), true));
            tab.selected = profiles.active_id().map(str::to_owned);
            tab.mode = Mode::List;
        }
        Err(reason) => tab.message = Some((reason, false)),
    }
}

fn spawn_confirm_delete(
    commands: &mut Commands,
    fonts: &UiFonts,
    content: Entity,
    target: Option<&Entry>,
    count: usize,
    active: Option<&str>,
) {
    let Some(target) = target else {
        return;
    };
    commands.spawn((
        ChildOf(content),
        label(
            fonts,
            &format!("Delete the profile \"{}\"?", target.name),
            26.0,
            AMBER,
        ),
    ));
    commands.spawn((
        ChildOf(content),
        label(
            fonts,
            "Its classes, XP and weapon progress are gone for good.",
            20.0,
            TEXT,
        ),
    ));
    let consequence = if count == 1 {
        "It is your last profile: you will be asked for a new name."
    } else if active == Some(target.id.as_str()) {
        "It is the active profile: another one becomes active."
    } else {
        "Your active profile stays as it is."
    };
    commands.spawn((ChildOf(content), label(fonts, consequence, 20.0, TEXT_DIM)));
    let row = button_row(commands, content);
    let delete = action(commands, fonts, row, "Delete", true);
    let keep = action(commands, fonts, row, "Keep", true);
    commands.entity(delete).observe(
        |_: On<Activate>, mut tab: ResMut<ProfileTab>, mut profiles: Profiles| {
            let Some(id) = effective(
                tab.selected.as_deref(),
                profiles.entries(),
                profiles.active_id(),
            ) else {
                return;
            };
            let name = profiles
                .entries()
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.name.clone())
                .unwrap_or_default();
            tab.message = Some(match profiles.delete(&id) {
                Ok(()) => (format!("Deleted {name}"), true),
                Err(reason) => (reason, false),
            });
            tab.selected = None;
            tab.mode = Mode::List;
        },
    );
    commands
        .entity(keep)
        .observe(|_: On<Activate>, mut tab: ResMut<ProfileTab>| {
            tab.mode = Mode::List;
        });
}

/// Keeps the note and the CREATE / RENAME button in step with the typed name.
pub(super) fn show_name_state(
    tab: Res<ProfileTab>,
    field: Single<&EditableText, With<NameField>>,
    list: Res<ProfileList>,
    settings: Res<Settings>,
    note: Single<(&mut Text, &mut TextColor), With<NameNote>>,
    confirm: Single<(Entity, Has<InteractionDisabled>), With<Confirm>>,
    mut commands: Commands,
) {
    let raw = field.value().to_string();
    let checked = match (tab.mode, settings.active_profile.as_deref()) {
        (Mode::Rename, Some(id)) => list.check_rename(&raw, id),
        _ => list.check_new(&raw),
    };
    let (shown, color) = match &checked {
        Ok(name) => (format!("Name: {name}"), ACCENT),
        Err(reason) => (reason.clone(), TEXT_DIM),
    };
    let (mut text, mut text_color) = note.into_inner();
    if text.0 != shown {
        text.0 = shown;
    }
    if text_color.0 != color {
        text_color.0 = color;
    }
    let (entity, disabled) = *confirm;
    if checked.is_ok() == disabled {
        if disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

/// Enter does what CREATE / RENAME does.
pub(super) fn confirm_on_enter(
    keys: Res<ButtonInput<KeyCode>>,
    field: Single<&EditableText, With<NameField>>,
    mut tab: ResMut<ProfileTab>,
    mut profiles: Profiles,
) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        confirm_name(&field.value().to_string(), &mut tab, &mut profiles);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            name: id.to_uppercase(),
        }
    }

    #[test]
    fn the_buttons_act_on_the_row_clicked_else_the_active_profile() {
        let entries = [entry("ann"), entry("bob")];
        assert_eq!(
            effective(Some("bob"), &entries, Some("ann")),
            Some("bob".into())
        );
        assert_eq!(effective(None, &entries, Some("ann")), Some("ann".into()));
        // The clicked row went away (deleted): back to the active one.
        assert_eq!(
            effective(Some("gone"), &entries, Some("ann")),
            Some("ann".into())
        );
        assert_eq!(effective(None, &[], None), None);
    }
}
