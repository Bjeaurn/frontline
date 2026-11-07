//! Settings, from the main menu or the pause menu, in four tabs:
//! - Video (`video`): display mode, resolution, vsync, FPS cap, quality, brightness. All but the
//!   display mode and resolution apply live; those two wait for APPLY, which then asks "Keep
//!   these settings?".
//! - Audio: master volume.
//! - Controls (`controls`): sensitivity, ADS and scope multipliers, invert Y and the key-rebinding list.
//! - Gameplay: name, field of view, Auto reload and the CoD4 folder for Homage Map textures
//!   (docs/adr/0004; CHECK applies it at once so the status line can say what was found).
//!
//! All four tabs exist at once (the others are hidden), so what was typed or dragged on one
//! survives a visit to another. APPLY saves everything (`flow` writes the file) and goes back,
//! unless the display mode or resolution changed: then it applies them, asks "Keep these
//! settings?" and saves on KEEP. BACK (or Esc) discards, including the live video changes.

mod controls;
mod video;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, SliderStep, SliderValue};

use super::host_setup::picker;
use super::rules_editor::{EditorOrigin, RulesOrigin};
use crate::cod4::{Cod4, parse_path_input};
use crate::flow::{Handedness, MenuScreen, Overlay, Settings};
use crate::player::input::{Bindings, Rebinding};
use crate::ui::{
    ACCENT, ArrowIcon, TEXT, TEXT_DIM, UiFonts, backdrop, button, compact_button, label, panel,
    slider, text_field, title,
};
use crate::video::{ActiveVideo, BRIGHTNESS_RANGE, PendingRevert, StagedDisplay};

const MAX_NAME_CHARS: usize = 24;
const MAX_PATH_CHARS: usize = 300;
const SENSITIVITY_RANGE: (f32, f32) = (0.0005, 0.006);
const ADS_SENSITIVITY_RANGE: (f32, f32) = (0.25, 1.5);
const SCOPE_SENSITIVITY_RANGE: (f32, f32) = (0.1, 1.5);
const FOV_RANGE: (f32, f32) = (65.0, 110.0);
const PANEL_WIDTH: f32 = 860.0;
/// Height of the tab area: the Controls tab is the tallest, the others keep its size so the
/// panel doesn't jump when switching.
const TAB_HEIGHT: f32 = 510.0;

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

pub(super) struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTab>()
            .add_systems(OnEnter(MenuScreen::Settings), spawn_menu_settings)
            .add_systems(OnEnter(Overlay::Settings), spawn_overlay_settings)
            .add_systems(OnExit(MenuScreen::Settings), leave)
            .add_systems(OnExit(Overlay::Settings), leave)
            .add_systems(
                Update,
                (
                    show_readouts,
                    show_cod4_status,
                    show_tabs,
                    apply_brightness,
                    video::show_values,
                    video::show_revert_prompt,
                    video::revert_on_escape.run_if(resource_exists::<PendingRevert>),
                    controls::show_values,
                    show_auto_reload,
                    show_handedness,
                    controls::capture_rebind.run_if(resource_exists::<Draft>),
                ),
            )
            .add_systems(Last, controls::advance_rebinding);
    }
}

/// The tabs, in the order of the tab bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Video,
    Audio,
    Controls,
    Gameplay,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Video, Tab::Audio, Tab::Controls, Tab::Gameplay];

    fn label(self) -> &'static str {
        match self {
            Tab::Video => "Video",
            Tab::Audio => "Audio",
            Tab::Controls => "Controls",
            Tab::Gameplay => "Gameplay",
        }
    }
}

/// The tab showing. Outlives the screen, so coming back from the Match Rules editor lands on
/// the tab it was opened from.
#[derive(Resource)]
struct ActiveTab(Tab);

impl Default for ActiveTab {
    fn default() -> Self {
        Self(Tab::Video)
    }
}

#[derive(Component)]
struct TabButton(Tab);

#[derive(Component)]
struct TabPanel(Tab);

/// What the open screen edits that isn't held by a widget of its own. APPLY writes it into the
/// Settings; BACK drops it.
#[derive(Resource)]
struct Draft {
    invert_y: bool,
    auto_reload: bool,
    handedness: Handedness,
    bindings: Bindings,
}

/// The Auto reload picker's text.
#[derive(Component)]
struct AutoReloadValue;

/// The Handedness picker's text.
#[derive(Component)]
struct HandednessValue;

#[derive(Component)]
struct NameField;

/// The CoD4 path text field.
#[derive(Component)]
struct Cod4Field;

/// The line under the CoD4 field saying what was found.
#[derive(Component)]
pub(super) struct Cod4Status;

/// Which setting a slider edits.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SettingSlider {
    Sensitivity,
    AdsSensitivity,
    ScopeSensitivity,
    Fov,
    Volume,
    Brightness,
}

impl SettingSlider {
    fn caption(self) -> &'static str {
        match self {
            SettingSlider::Sensitivity => "Mouse sensitivity",
            SettingSlider::AdsSensitivity => "ADS sensitivity",
            SettingSlider::ScopeSensitivity => "Scope sensitivity",
            SettingSlider::Fov => "Field of view",
            SettingSlider::Volume => "Master volume",
            SettingSlider::Brightness => "Brightness",
        }
    }

    fn range(self) -> (f32, f32) {
        match self {
            SettingSlider::Sensitivity => SENSITIVITY_RANGE,
            SettingSlider::AdsSensitivity => ADS_SENSITIVITY_RANGE,
            SettingSlider::ScopeSensitivity => SCOPE_SENSITIVITY_RANGE,
            SettingSlider::Fov => FOV_RANGE,
            SettingSlider::Volume => (0.0, 1.0),
            SettingSlider::Brightness => BRIGHTNESS_RANGE,
        }
    }

    fn step(self) -> f32 {
        match self {
            SettingSlider::Sensitivity => 0.0001,
            SettingSlider::AdsSensitivity => 0.05,
            SettingSlider::ScopeSensitivity => 0.05,
            SettingSlider::Fov => 1.0,
            SettingSlider::Volume => 0.05,
            SettingSlider::Brightness => 0.05,
        }
    }

    fn value(self, settings: &Settings, video: &ActiveVideo) -> f32 {
        match self {
            SettingSlider::Sensitivity => settings.mouse_sensitivity,
            SettingSlider::AdsSensitivity => settings.ads_sensitivity,
            SettingSlider::ScopeSensitivity => settings.scope_sensitivity,
            SettingSlider::Fov => settings.fov_deg,
            SettingSlider::Volume => settings.master_volume,
            SettingSlider::Brightness => video.0.brightness,
        }
    }

    fn readout(self, value: f32) -> String {
        match self {
            // Shown in thousandths: the default 0.002 reads "2.0".
            SettingSlider::Sensitivity => format!("{:.1}", value * 1000.0),
            SettingSlider::AdsSensitivity => format!("{value:.2}x"),
            SettingSlider::ScopeSensitivity => format!("{value:.2}x"),
            SettingSlider::Fov => format!("{value:.0}°"),
            SettingSlider::Volume | SettingSlider::Brightness => {
                format!("{:.0}%", value * 100.0)
            }
        }
    }
}

/// Text showing the value of the slider `.0`.
#[derive(Component)]
struct Readout(Entity);

fn spawn_menu_settings(
    commands: Commands,
    fonts: Res<UiFonts>,
    icon: Res<ArrowIcon>,
    settings: Res<Settings>,
    video: Res<ActiveVideo>,
    tab: Res<ActiveTab>,
) {
    spawn_settings(
        commands,
        &fonts,
        &icon,
        &settings,
        &video,
        tab.0,
        Origin::MainMenu,
    );
}

fn spawn_overlay_settings(
    commands: Commands,
    fonts: Res<UiFonts>,
    icon: Res<ArrowIcon>,
    settings: Res<Settings>,
    video: Res<ActiveVideo>,
    tab: Res<ActiveTab>,
) {
    spawn_settings(
        commands,
        &fonts,
        &icon,
        &settings,
        &video,
        tab.0,
        Origin::Pause,
    );
}

/// Leaving the screen by any way but APPLY puts the live video settings back to the saved ones
/// and drops the edits and any rebind or prompt in progress.
fn leave(
    settings: Res<Settings>,
    mut video: ResMut<ActiveVideo>,
    mut rebinding: ResMut<Rebinding>,
    mut commands: Commands,
) {
    video.set_if_neq(ActiveVideo(settings.video.clone()));
    rebinding.cancel();
    commands.remove_resource::<PendingRevert>();
    commands.remove_resource::<StagedDisplay>();
    commands.remove_resource::<Draft>();
}

fn spawn_settings(
    mut commands: Commands,
    fonts: &UiFonts,
    icon: &ArrowIcon,
    settings: &Settings,
    video: &ActiveVideo,
    active_tab: Tab,
    origin: Origin,
) {
    commands.insert_resource(StagedDisplay(video.0.display()));
    commands.insert_resource(Draft {
        invert_y: settings.invert_y,
        auto_reload: settings.auto_reload,
        handedness: settings.handedness,
        bindings: settings.bindings.clone(),
    });
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
    commands.entity(panel).insert(Node {
        width: px(PANEL_WIDTH),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(24.0)),
        row_gap: px(10.0),
        border: UiRect::all(px(1.0)),
        ..default()
    });
    commands.spawn((ChildOf(panel), title(fonts, "Settings", 44.0)));

    let bar = commands
        .spawn((
            ChildOf(panel),
            Node {
                column_gap: px(8.0),
                ..default()
            },
        ))
        .id();
    for tab in Tab::ALL {
        commands
            .spawn((
                ChildOf(bar),
                TabButton(tab),
                compact_button(fonts, tab.label(), 150.0, 26.0),
            ))
            .observe(
                move |_: On<Activate>,
                      mut active: ResMut<ActiveTab>,
                      mut rebinding: ResMut<Rebinding>| {
                    active.0 = tab;
                    rebinding.cancel();
                },
            );
    }
    video::spawn_revert_prompt(&mut commands, fonts, panel, origin);

    let tabs = commands
        .spawn((
            ChildOf(panel),
            Node {
                height: px(TAB_HEIGHT),
                ..default()
            },
        ))
        .id();
    let tab_panels = Tab::ALL.map(|tab| {
        commands
            .spawn((
                ChildOf(tabs),
                TabPanel(tab),
                Name::new(tab.label()),
                Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10.0),
                    display: if tab == active_tab {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    ..default()
                },
            ))
            .id()
    });
    let [video_tab, audio_tab, controls_tab, gameplay_tab] = tab_panels;

    video::spawn_video_tab(&mut commands, fonts, icon, video_tab, settings, video);

    slider_row(
        &mut commands,
        fonts,
        audio_tab,
        SettingSlider::Volume,
        settings,
        video,
    );

    slider_row(
        &mut commands,
        fonts,
        controls_tab,
        SettingSlider::Sensitivity,
        settings,
        video,
    );
    slider_row(
        &mut commands,
        fonts,
        controls_tab,
        SettingSlider::AdsSensitivity,
        settings,
        video,
    );
    slider_row(
        &mut commands,
        fonts,
        controls_tab,
        SettingSlider::ScopeSensitivity,
        settings,
        video,
    );
    controls::spawn_controls_tab(&mut commands, fonts, icon, controls_tab, settings);

    spawn_gameplay_tab(
        &mut commands,
        fonts,
        icon,
        gameplay_tab,
        settings,
        video,
        origin,
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
                  mut form: Form,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                if form.apply() {
                    origin.go_back(&mut menu, &mut overlay);
                }
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

fn spawn_gameplay_tab(
    commands: &mut Commands,
    fonts: &UiFonts,
    icon: &ArrowIcon,
    tab: Entity,
    settings: &Settings,
    video: &ActiveVideo,
    origin: Origin,
) {
    let name_row = setting_row(commands, fonts, tab, "Name");
    commands.spawn((
        ChildOf(name_row),
        NameField,
        text_field(fonts, &settings.name, MAX_NAME_CHARS),
    ));

    slider_row(commands, fonts, tab, SettingSlider::Fov, settings, video);

    let reload_row = setting_row(commands, fonts, tab, "Auto reload");
    let (left, right, value) = picker(
        commands,
        fonts,
        icon,
        reload_row,
        140.0,
        video::on_off(settings.auto_reload),
    );
    commands.entity(value).insert(AutoReloadValue);
    for arrow in [left, right] {
        commands
            .entity(arrow)
            .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
                draft.auto_reload = !draft.auto_reload;
            });
    }
    let hint_row = setting_row(commands, fonts, tab, "");
    commands.spawn((
        ChildOf(hint_row),
        label(
            fonts,
            "Off: an empty magazine clicks and R reloads. On: pulling the trigger reloads it.",
            18.0,
            TEXT_DIM,
        ),
    ));

    let hand_row = setting_row(commands, fonts, tab, "Handedness");
    let (left, right, value) = picker(
        commands,
        fonts,
        icon,
        hand_row,
        140.0,
        settings.handedness.label(),
    );
    commands.entity(value).insert(HandednessValue);
    for arrow in [left, right] {
        commands
            .entity(arrow)
            .observe(|_: On<Activate>, mut draft: ResMut<Draft>| {
                draft.handedness = draft.handedness.toggled();
            });
    }
    let hand_hint_row = setting_row(commands, fonts, tab, "");
    commands.spawn((
        ChildOf(hand_hint_row),
        label(
            fonts,
            "Left mirrors your view. Other players still see you right-handed.",
            18.0,
            TEXT_DIM,
        ),
    ));

    let cod4_row = setting_row(commands, fonts, tab, "CoD4 textures");
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
        width: px(340.0),
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
        .observe(|_: On<Activate>, mut form: Form| {
            form.commit(false);
        });
    let status_row = setting_row(commands, fonts, tab, "");
    commands.spawn((
        ChildOf(status_row),
        Cod4Status,
        label(fonts, "", 20.0, TEXT_DIM),
    ));

    // The Custom Match Rules editor; the settings edited so far are applied on the way, so
    // coming back shows them.
    let rules_row = setting_row(commands, fonts, tab, "Match rules");
    commands
        .spawn((
            ChildOf(rules_row),
            compact_button(fonts, "Custom rule sets", 240.0, 22.0),
        ))
        .observe(
            move |_: On<Activate>,
                  mut form: Form,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>,
                  mut commands: Commands| {
                form.commit(false);
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
}

/// A row with a caption on the left; returns it for the control(s).
fn setting_row(commands: &mut Commands, fonts: &UiFonts, parent: Entity, caption: &str) -> Entity {
    let row = commands
        .spawn((
            ChildOf(parent),
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

/// A row with a slider for `kind` and its readout.
fn slider_row(
    commands: &mut Commands,
    fonts: &UiFonts,
    parent: Entity,
    kind: SettingSlider,
    settings: &Settings,
    video: &ActiveVideo,
) {
    let row = setting_row(commands, fonts, parent, kind.caption());
    let value = kind.value(settings, video);
    let (min, max) = kind.range();
    let slider = commands
        .spawn((
            ChildOf(row),
            kind,
            slider(value, min, max),
            SliderStep(kind.step()),
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

/// Keeps each readout in step with its slider.
fn show_readouts(
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

/// Keeps the Auto reload picker in step with the draft.
fn show_auto_reload(draft: Option<Res<Draft>>, mut value: Query<&mut Text, With<AutoReloadValue>>) {
    let Some(draft) = draft else {
        return;
    };
    let shown = video::on_off(draft.auto_reload).to_uppercase();
    for mut text in &mut value {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// Keeps the Handedness picker in step with the draft.
fn show_handedness(draft: Option<Res<Draft>>, mut value: Query<&mut Text, With<HandednessValue>>) {
    let Some(draft) = draft else {
        return;
    };
    let shown = draft.handedness.label().to_uppercase();
    for mut text in &mut value {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// Keeps the status line under the CoD4 field in step with the indexed source.
fn show_cod4_status(cod4: Option<Res<Cod4>>, mut status: Query<&mut Text, With<Cod4Status>>) {
    let shown = cod4.map_or_else(String::new, |cod4| cod4.status());
    for mut text in &mut status {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// Shows the active tab's panel and lights its button.
fn show_tabs(
    active: Res<ActiveTab>,
    mut panels: Query<(&TabPanel, &mut Node)>,
    buttons: Query<(&TabButton, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    for (panel, mut node) in &mut panels {
        let shown = if panel.0 == active.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != shown {
            node.display = shown;
        }
    }
    for (button, children) in &buttons {
        let color = if button.0 == active.0 { ACCENT } else { TEXT };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child)
                && text.0 != color
            {
                text.0 = color;
            }
        }
    }
}

/// The brightness slider applies as it is dragged, so the Map shows what it will look like.
fn apply_brightness(
    sliders: Query<(&SettingSlider, &SliderValue), Changed<SliderValue>>,
    mut video: ResMut<ActiveVideo>,
) {
    for (kind, value) in &sliders {
        if *kind == SettingSlider::Brightness && video.0.brightness != value.0 {
            video.0.brightness = value.0;
        }
    }
}

/// Everything APPLY saves: the text fields, the sliders, the draft and the video settings (the
/// staged display is what APPLY puts on the window first).
#[derive(SystemParam)]
struct Form<'w, 's> {
    name: Single<'w, 's, &'static EditableText, With<NameField>>,
    cod4_path: Single<'w, 's, &'static EditableText, With<Cod4Field>>,
    sliders: Query<'w, 's, (&'static SettingSlider, &'static SliderValue)>,
    draft: Res<'w, Draft>,
    video: ResMut<'w, ActiveVideo>,
    staged: Res<'w, StagedDisplay>,
    pending: Option<Res<'w, PendingRevert>>,
    settings: ResMut<'w, Settings>,
    commands: Commands<'w, 's>,
}

impl Form<'_, '_> {
    /// APPLY. Puts the staged display on the window; when that changes it, the "Keep these
    /// settings?" prompt starts and nothing is saved until KEEP. Otherwise writes the screen's
    /// values into the Settings (with a prompt already up, that keeps its display). True when
    /// they were saved, so the screen can go back.
    fn apply(&mut self) -> bool {
        let staged = self.staged.0;
        if let Some(prompt) = self.video.apply_display(staged, self.pending.as_deref()) {
            self.commands.insert_resource(prompt);
            return false;
        }
        self.commit(true);
        true
    }

    /// Writes the screen's values into the Settings. A display change still waiting for its
    /// "Keep these settings?" is kept when `keep_display`, otherwise left unsaved.
    fn commit(&mut self, keep_display: bool) {
        let mut edited = self.settings.clone();
        let name = self.name.value().to_string();
        if !name.trim().is_empty() {
            edited.name = name.trim().to_owned();
        }
        edited.cod4_path = parse_path_input(&self.cod4_path.value().to_string());
        for (kind, value) in &self.sliders {
            match kind {
                SettingSlider::Sensitivity => edited.mouse_sensitivity = value.0,
                SettingSlider::AdsSensitivity => edited.ads_sensitivity = value.0,
                SettingSlider::ScopeSensitivity => edited.scope_sensitivity = value.0,
                SettingSlider::Fov => edited.fov_deg = value.0,
                SettingSlider::Volume => edited.master_volume = value.0,
                // Applied live into the video settings, which are copied below.
                SettingSlider::Brightness => {}
            }
        }
        edited.invert_y = self.draft.invert_y;
        edited.auto_reload = self.draft.auto_reload;
        edited.handedness = self.draft.handedness;
        edited.bindings = self.draft.bindings.clone();
        edited.video = self.video.to_save(self.pending.as_deref(), keep_display);
        if keep_display {
            self.commands.remove_resource::<PendingRevert>();
        }
        self.settings.set_if_neq(edited);
    }
}
