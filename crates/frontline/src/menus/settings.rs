//! Settings, from the main menu or the pause menu: name, mouse sensitivity, field of view,
//! master volume and the CoD4 folder for Homage Map textures (docs/adr/0004; CHECK applies it
//! at once so the status line can say what was found). APPLY saves them (`flow` writes the file) and goes back; BACK discards.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderStep, SliderValue};

use super::rules_editor::{EditorOrigin, RulesOrigin};
use crate::cod4::{Cod4, parse_path_input};
use crate::flow::{MenuScreen, Overlay, Settings};
use crate::ui::{
    ACCENT, TEXT_DIM, UiFonts, backdrop, button, compact_button, label, panel, slider, text_field,
    title,
};

const MAX_NAME_CHARS: usize = 24;
const MAX_PATH_CHARS: usize = 300;
const SENSITIVITY_RANGE: (f32, f32) = (0.0005, 0.006);
const FOV_RANGE: (f32, f32) = (65.0, 110.0);

/// Where the screen was opened from, and so where APPLY/BACK return to.
#[derive(Clone, Copy)]
enum Origin {
    MainMenu,
    Pause,
}

impl Origin {
    fn go_back(self, menu: &mut NextState<MenuScreen>, overlay: &mut NextState<Overlay>) {
        match self {
            Origin::MainMenu => menu.set(MenuScreen::Title),
            Origin::Pause => overlay.set(Overlay::Pause),
        }
    }
}

#[derive(Component)]
struct NameField;

/// The CoD4 path text field.
#[derive(Component)]
struct Cod4Field;

/// The line under the CoD4 field saying what was found.
#[derive(Component)]
pub(super) struct Cod4Status;

/// Which setting a slider edits.
#[derive(Component, Clone, Copy)]
pub(super) enum SettingSlider {
    Sensitivity,
    Fov,
    Volume,
}

impl SettingSlider {
    fn readout(self, value: f32) -> String {
        match self {
            // Shown in thousandths: the default 0.002 reads "2.0".
            SettingSlider::Sensitivity => format!("{:.1}", value * 1000.0),
            SettingSlider::Fov => format!("{value:.0}°"),
            SettingSlider::Volume => format!("{:.0}%", value * 100.0),
        }
    }
}

/// Text showing the value of the slider `.0`.
#[derive(Component)]
pub(super) struct Readout(Entity);

pub(super) fn spawn_menu_settings(
    commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<Settings>,
) {
    spawn_settings(commands, &fonts, &settings, Origin::MainMenu);
}

pub(super) fn spawn_overlay_settings(
    commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<Settings>,
) {
    spawn_settings(commands, &fonts, &settings, Origin::Pause);
}

fn spawn_settings(mut commands: Commands, fonts: &UiFonts, settings: &Settings, origin: Origin) {
    let root = commands
        .spawn((Name::new("Settings"), backdrop(), GlobalZIndex(10)))
        .id();
    match origin {
        Origin::MainMenu => commands
            .entity(root)
            .insert(DespawnOnExit(MenuScreen::Settings)),
        Origin::Pause => commands
            .entity(root)
            .insert(DespawnOnExit(Overlay::Settings)),
    };
    let panel = commands.spawn((ChildOf(root), panel())).id();
    commands.spawn((ChildOf(panel), title(fonts, "Settings", 44.0)));

    let name_row = setting_row(&mut commands, fonts, panel, "Name");
    commands.spawn((
        ChildOf(name_row),
        NameField,
        text_field(fonts, &settings.name, MAX_NAME_CHARS),
    ));

    for (kind, caption, value, (min, max), step) in [
        (
            SettingSlider::Sensitivity,
            "Mouse sensitivity",
            settings.mouse_sensitivity,
            SENSITIVITY_RANGE,
            0.0001,
        ),
        (
            SettingSlider::Fov,
            "Field of view",
            settings.fov_deg,
            FOV_RANGE,
            1.0,
        ),
        (
            SettingSlider::Volume,
            "Master volume",
            settings.master_volume,
            (0.0, 1.0),
            0.05,
        ),
    ] {
        let row = setting_row(&mut commands, fonts, panel, caption);
        let slider = commands
            .spawn((
                ChildOf(row),
                kind,
                slider(value, min, max),
                SliderStep(step),
            ))
            .id();
        commands.spawn((
            ChildOf(row),
            Readout(slider),
            label(fonts, &kind.readout(value), 22.0, ACCENT),
            Node {
                width: px(70.0),
                ..default()
            },
        ));
    }

    let cod4_row = setting_row(&mut commands, fonts, panel, "CoD4 textures");
    let field = commands
        .spawn((
            ChildOf(cod4_row),
            Cod4Field,
            text_field(
                fonts,
                &settings
                    .cod4_path
                    .as_ref()
                    .map_or_else(String::new, |path| path.display().to_string()),
                MAX_PATH_CHARS,
            ),
        ))
        .id();
    commands.entity(field).insert(Node {
        width: px(420.0),
        padding: UiRect::axes(px(10.0), px(4.0)),
        border: UiRect::all(px(1.0)),
        overflow: Overflow::clip(),
        ..default()
    });
    commands
        .spawn((
            ChildOf(cod4_row),
            compact_button(fonts, "Check", 90.0, 22.0),
        ))
        .observe(
            |_: On<Activate>,
             name: Single<&EditableText, With<NameField>>,
             cod4_field: Single<&EditableText, With<Cod4Field>>,
             sliders: Query<(&SettingSlider, &SliderValue)>,
             mut settings: ResMut<Settings>| {
                commit(&name, &cod4_field, &sliders, &mut settings);
            },
        );
    let status_row = setting_row(&mut commands, fonts, panel, "");
    commands.spawn((
        ChildOf(status_row),
        Cod4Status,
        label(fonts, "", 20.0, TEXT_DIM),
    ));

    // The Custom Match Rules editor; the settings edited so far are applied on the way, so
    // coming back shows them.
    let rules_row = setting_row(&mut commands, fonts, panel, "Match rules");
    commands
        .spawn((
            ChildOf(rules_row),
            compact_button(fonts, "Custom rule sets", 240.0, 22.0),
        ))
        .observe(
            move |_: On<Activate>,
                  name: Single<&EditableText, With<NameField>>,
                  cod4_field: Single<&EditableText, With<Cod4Field>>,
                  sliders: Query<(&SettingSlider, &SliderValue)>,
                  mut settings: ResMut<Settings>,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>,
                  mut commands: Commands| {
                commit(&name, &cod4_field, &sliders, &mut settings);
                match origin {
                    Origin::MainMenu => {
                        commands.insert_resource(RulesOrigin(EditorOrigin::Settings));
                        menu.set(MenuScreen::Rules);
                    }
                    Origin::Pause => {
                        commands.insert_resource(RulesOrigin(EditorOrigin::PauseSettings));
                        overlay.set(Overlay::Rules);
                    }
                }
            },
        );

    let buttons = commands
        .spawn((
            ChildOf(panel),
            Node {
                column_gap: px(12.0),
                margin: UiRect::top(px(12.0)),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((ChildOf(buttons), button(fonts, "Apply")))
        .observe(
            move |_: On<Activate>,
                  name: Single<&EditableText, With<NameField>>,
                  cod4_field: Single<&EditableText, With<Cod4Field>>,
                  sliders: Query<(&SettingSlider, &SliderValue)>,
                  mut settings: ResMut<Settings>,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                commit(&name, &cod4_field, &sliders, &mut settings);
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

/// A row with a caption on the left; returns it for the control(s).
fn setting_row(commands: &mut Commands, fonts: &UiFonts, panel: Entity, caption: &str) -> Entity {
    let row = commands
        .spawn((
            ChildOf(panel),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(row),
        label(fonts, &caption.to_uppercase(), 22.0, TEXT_DIM),
        Node {
            width: px(220.0),
            ..default()
        },
    ));
    row
}

/// Keeps each readout in step with its slider.
pub(super) fn show_readouts(
    sliders: Query<(&SettingSlider, &SliderValue), Changed<SliderValue>>,
    mut readouts: Query<(&Readout, &mut Text)>,
) {
    for (readout, mut text) in &mut readouts {
        if let Ok((kind, value)) = sliders.get(readout.0) {
            let shown = kind.readout(value.0);
            if text.0 != shown {
                text.0 = shown;
            }
        }
    }
}

/// Keeps the status line under the CoD4 field in step with the indexed source.
pub(super) fn show_cod4_status(
    cod4: Option<Res<Cod4>>,
    mut status: Query<&mut Text, With<Cod4Status>>,
) {
    let shown = cod4.map_or_else(String::new, |cod4| cod4.status());
    for mut text in &mut status {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// Writes the screen's name, CoD4 path and slider values into the Settings.
fn commit(
    name: &EditableText,
    cod4_path: &EditableText,
    sliders: &Query<(&SettingSlider, &SliderValue)>,
    settings: &mut ResMut<Settings>,
) {
    let mut edited = settings.clone();
    let name = name.value().to_string();
    if !name.trim().is_empty() {
        edited.name = name.trim().to_owned();
    }
    edited.cod4_path = parse_path_input(&cod4_path.value().to_string());
    for (kind, value) in sliders {
        match kind {
            SettingSlider::Sensitivity => edited.mouse_sensitivity = value.0,
            SettingSlider::Fov => edited.fov_deg = value.0,
            SettingSlider::Volume => edited.master_volume = value.0,
        }
    }
    settings.set_if_neq(edited);
}
