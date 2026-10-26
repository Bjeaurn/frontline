//! Settings, from the main menu or the pause menu: name, mouse sensitivity, field of view and
//! master volume. APPLY saves them (`flow` writes the file) and goes back; BACK discards.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderStep, SliderValue};

use crate::flow::{MenuScreen, Overlay, Settings};
use crate::ui::{
    ACCENT, TEXT_DIM, UiFonts, backdrop, button, label, panel, slider, text_field, title,
};

const MAX_NAME_CHARS: usize = 24;
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
                  sliders: Query<(&SettingSlider, &SliderValue)>,
                  mut settings: ResMut<Settings>,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                let mut edited = settings.clone();
                let name = name.value().to_string();
                if !name.trim().is_empty() {
                    edited.name = name.trim().to_owned();
                }
                for (kind, value) in &sliders {
                    match kind {
                        SettingSlider::Sensitivity => edited.mouse_sensitivity = value.0,
                        SettingSlider::Fov => edited.fov_deg = value.0,
                        SettingSlider::Volume => edited.master_volume = value.0,
                    }
                }
                settings.set_if_neq(edited);
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
