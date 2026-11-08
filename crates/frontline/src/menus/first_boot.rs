//! The name prompt (`MenuScreen::FirstBoot`): shown before the title screen while no Profile
//! exists, on the first boot and after the last Profile was deleted. The name typed becomes a new
//! Profile (CONTEXT.md: Profile), which is what other players see; CONTINUE only works for a
//! name that is not empty (after trimming) and not taken.

use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use crate::flow::{AppState, MenuScreen};
use crate::profile::book::MAX_NAME_CHARS;
use crate::profile::{ProfileList, Profiles, has_active_profile, suggested_name};
use crate::ui::{
    ACCENT, AMBER, TEXT_DIM, UiFonts, backdrop, button, label, panel, rule, text_field, title,
};

#[derive(Component)]
struct NameField;

/// The line under the field: what the name will be, or why it can't be.
#[derive(Component)]
struct Note;

#[derive(Component)]
struct Continue;

/// Why the last CONTINUE failed (the Profile file couldn't be written); cleared by typing.
#[derive(Resource, Default)]
struct Failure(Option<String>);

pub(super) struct FirstBootPlugin;

impl Plugin for FirstBootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(MenuScreen::FirstBoot), spawn_first_boot)
            .add_systems(
                Update,
                (
                    ask_while_none
                        .run_if(in_state(AppState::MainMenu))
                        .run_if(not(in_state(MenuScreen::FirstBoot)))
                        .run_if(not(has_active_profile)),
                    (show_state, continue_on_enter)
                        .chain()
                        .run_if(in_state(MenuScreen::FirstBoot)),
                ),
            );
    }
}

/// Sends the main menu to the name prompt while no Profile is active.
fn ask_while_none(mut next: ResMut<NextState<MenuScreen>>) {
    next.set(MenuScreen::FirstBoot);
}

fn spawn_first_boot(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.insert_resource(Failure::default());
    let suggestion = suggested_name();
    commands
        .spawn((
            Name::new("First boot"),
            DespawnOnExit(MenuScreen::FirstBoot),
            backdrop(),
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn(panel()).with_children(|panel| {
                panel.spawn(title(&fonts, "Welcome", 44.0));
                panel.spawn(rule(AMBER));
                panel.spawn(label(
                    &fonts,
                    "Pick a profile name. Other players see it, and your classes and XP are kept under it.",
                    20.0,
                    TEXT_DIM,
                ));
                panel.spawn((
                    NameField,
                    AutoFocus,
                    text_field(&fonts, &suggestion, MAX_NAME_CHARS),
                ));
                panel.spawn((Note, label(&fonts, "", 20.0, ACCENT)));
                panel
                    .spawn((Continue, button(&fonts, "Continue")))
                    .observe(
                        |_: On<Activate>,
                         field: Single<&EditableText, With<NameField>>,
                         mut profiles: Profiles,
                         failure: ResMut<Failure>,
                         next: ResMut<NextState<MenuScreen>>| {
                            create(&field.value().to_string(), &mut profiles, failure, next);
                        },
                    );
            });
        });
}

/// Makes the Profile and goes on to the title screen, or keeps the prompt up with the reason.
fn create(
    raw: &str,
    profiles: &mut Profiles,
    mut failure: ResMut<Failure>,
    mut next: ResMut<NextState<MenuScreen>>,
) {
    match profiles.create(raw) {
        Ok(()) => next.set(MenuScreen::Title),
        Err(reason) => failure.0 = Some(reason),
    }
}

/// Enter does what CONTINUE does.
fn continue_on_enter(
    keys: Res<ButtonInput<KeyCode>>,
    field: Single<&EditableText, With<NameField>>,
    mut profiles: Profiles,
    failure: ResMut<Failure>,
    next: ResMut<NextState<MenuScreen>>,
) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        create(&field.value().to_string(), &mut profiles, failure, next);
    }
}

/// Keeps the note and CONTINUE in step with the typed name.
fn show_state(
    field: Single<&EditableText, With<NameField>>,
    list: Res<ProfileList>,
    mut failure: ResMut<Failure>,
    mut typed: Local<String>,
    note: Single<(&mut Text, &mut TextColor), With<Note>>,
    continue_button: Single<(Entity, Has<InteractionDisabled>), With<Continue>>,
    mut commands: Commands,
) {
    let raw = field.value().to_string();
    if *typed != raw {
        failure.0 = None;
        typed.clone_from(&raw);
    }
    let checked = list.check_new(&raw);
    let (shown, color) = match (&failure.0, &checked) {
        (Some(reason), _) => (reason.clone(), AMBER),
        (None, Ok(name)) => (format!("You will play as {name}"), ACCENT),
        (None, Err(reason)) => (reason.clone(), TEXT_DIM),
    };
    let (mut text, mut text_color) = note.into_inner();
    if text.0 != shown {
        text.0 = shown;
    }
    if text_color.0 != color {
        text_color.0 = color;
    }
    let (entity, disabled) = *continue_button;
    if checked.is_ok() == disabled {
        if disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}
