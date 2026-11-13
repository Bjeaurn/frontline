//! The Video tab: pickers for the display, quality and frame-rate settings, the brightness
//! slider, and the "Keep these settings?" prompt after APPLY changed the display. Everything
//! here edits the live `ActiveVideo` except the display mode and resolution, which are staged
//! in `StagedDisplay` until APPLY.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{Form, Origin, SettingSlider, setting_row, slider_row};
use crate::flow::{MenuScreen, Overlay, Settings};
use crate::menus::host_setup::picker;
use crate::player::input::Rebinding;
use crate::ui::{ArrowIcon, TEXT_DIM, UiFonts, compact_button, label};
use crate::video::{
    ActiveVideo, AntiAliasing, CurrentMonitor, DisplayChoice, DisplayMode, FPS_CAPS, PendingRevert,
    QualityPreset, ShadowQuality, SsaoQuality, StagedDisplay, VideoSettings, step,
};

const PICKER_WIDTH: f32 = 280.0;

/// A setting the Video tab edits with a picker.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum VideoField {
    DisplayMode,
    Resolution,
    Vsync,
    FpsCap,
    Preset,
    Shadows,
    Ssao,
    Bloom,
    AntiAliasing,
    ShowFps,
}

impl VideoField {
    /// In the order of the tab.
    const ALL: [VideoField; 10] = [
        VideoField::DisplayMode,
        VideoField::Resolution,
        VideoField::Vsync,
        VideoField::FpsCap,
        VideoField::Preset,
        VideoField::Shadows,
        VideoField::Ssao,
        VideoField::Bloom,
        VideoField::AntiAliasing,
        VideoField::ShowFps,
    ];

    fn caption(self) -> &'static str {
        match self {
            VideoField::DisplayMode => "Display mode",
            VideoField::Resolution => "Resolution",
            VideoField::Vsync => "Vsync",
            VideoField::FpsCap => "FPS cap",
            VideoField::Preset => "Quality",
            VideoField::Shadows => "Shadows",
            VideoField::Ssao => "Ambient occlusion",
            VideoField::Bloom => "Bloom",
            VideoField::AntiAliasing => "Anti-aliasing",
            VideoField::ShowFps => "Show FPS",
        }
    }

    /// What the picker shows: the staged display for the display mode and resolution, the
    /// live `video` for the rest.
    fn value(self, video: &VideoSettings, staged: &DisplayChoice) -> String {
        match self {
            VideoField::DisplayMode => staged.mode.label().to_owned(),
            VideoField::Resolution => match (staged.mode, staged.resolution) {
                // Borderless fullscreen always fills the monitor.
                (DisplayMode::BorderlessFullscreen, _) => "Monitor's".to_owned(),
                (_, None) => "Current".to_owned(),
                (_, Some((w, h))) => format!("{w} x {h}"),
            },
            VideoField::Vsync => on_off(video.vsync).to_owned(),
            VideoField::FpsCap => match video.fps_cap {
                0 => "Unlimited".to_owned(),
                cap => cap.to_string(),
            },
            VideoField::Preset => video.preset.label().to_owned(),
            VideoField::Shadows => video.quality.shadows.label().to_owned(),
            VideoField::Ssao => video.quality.ssao.label().to_owned(),
            VideoField::Bloom => on_off(video.quality.bloom).to_owned(),
            VideoField::AntiAliasing => video.quality.aa.label().to_owned(),
            VideoField::ShowFps => on_off(video.show_fps).to_owned(),
        }
    }

    /// Moves the setting `dir` places along what it offers (`resolutions`: the monitor's
    /// sizes). The display mode and resolution change `staged` only.
    fn step(
        self,
        video: &mut VideoSettings,
        staged: &mut DisplayChoice,
        resolutions: &[(u32, u32)],
        dir: i32,
    ) {
        match self {
            VideoField::DisplayMode => staged.step_mode(dir),
            VideoField::Resolution => staged.step_resolution(resolutions, dir),
            VideoField::Vsync => video.vsync = !video.vsync,
            VideoField::FpsCap => video.fps_cap = step(&FPS_CAPS, video.fps_cap, dir),
            VideoField::Preset => video.set_preset(step(&QualityPreset::ALL, video.preset, dir)),
            VideoField::Shadows => video.edit_quality(|quality| {
                quality.shadows = step(&ShadowQuality::ALL, quality.shadows, dir);
            }),
            VideoField::Ssao => video.edit_quality(|quality| {
                quality.ssao = step(&SsaoQuality::ALL, quality.ssao, dir);
            }),
            VideoField::Bloom => video.edit_quality(|quality| quality.bloom = !quality.bloom),
            VideoField::AntiAliasing => video.edit_quality(|quality| {
                quality.aa = step(&AntiAliasing::ALL, quality.aa, dir);
            }),
            VideoField::ShowFps => video.show_fps = !video.show_fps,
        }
    }
}

pub(super) fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// The text of a picker: shows `.0`'s current value.
#[derive(Component)]
pub(super) struct FieldValue(VideoField);

/// The "Keep these settings?" row; only showing while a display change awaits an answer.
#[derive(Component)]
pub(super) struct RevertPrompt;

/// The prompt's countdown text.
#[derive(Component)]
pub(super) struct RevertCountdown;

pub(super) fn spawn_video_tab(
    commands: &mut Commands,
    fonts: &UiFonts,
    icon: &ArrowIcon,
    tab: Entity,
    settings: &Settings,
    video: &ActiveVideo,
) {
    for field in VideoField::ALL {
        let row = setting_row(commands, fonts, tab, field.caption());
        let (left, right, value) = picker(
            commands,
            fonts,
            icon,
            row,
            PICKER_WIDTH,
            &field.value(&video.0, &video.0.display()),
        );
        commands.entity(value).insert(FieldValue(field));
        for (arrow, dir) in [(left, -1), (right, 1)] {
            commands.entity(arrow).observe(
                move |_: On<Activate>,
                      mut video: ResMut<ActiveVideo>,
                      mut staged: ResMut<StagedDisplay>,
                      monitor: CurrentMonitor| {
                    let mut edited = video.0.clone();
                    let mut picked = staged.0;
                    field.step(&mut edited, &mut picked, &monitor.resolutions(), dir);
                    video.set_if_neq(ActiveVideo(edited));
                    staged.set_if_neq(StagedDisplay(picked));
                },
            );
        }
    }
    slider_row(
        commands,
        fonts,
        tab,
        SettingSlider::Brightness,
        settings,
        video,
    );
}

/// The "Keep these settings?" row, under the tab bar.
pub(super) fn spawn_revert_prompt(
    commands: &mut Commands,
    fonts: &UiFonts,
    panel: Entity,
    origin: Origin,
) {
    let row = commands
        .spawn((
            ChildOf(panel),
            RevertPrompt,
            Node {
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                display: Display::None,
                ..default()
            },
        ))
        .id();
    commands.spawn((
        ChildOf(row),
        RevertCountdown,
        label(fonts, "", 22.0, TEXT_DIM),
        Node {
            width: px(430.0),
            ..default()
        },
    ));
    // KEEP confirms the display and saves everything, as APPLY does without a display change.
    commands
        .spawn((ChildOf(row), compact_button(fonts, "Keep", 110.0, 22.0)))
        .observe(
            move |_: On<Activate>,
                  mut form: Form,
                  mut menu: ResMut<NextState<MenuScreen>>,
                  mut overlay: ResMut<NextState<Overlay>>| {
                form.commit(true);
                origin.go_back(&mut menu, &mut overlay);
            },
        );
    commands
        .spawn((ChildOf(row), compact_button(fonts, "Revert", 110.0, 22.0)))
        .observe(
            |_: On<Activate>,
             pending: Option<Res<PendingRevert>>,
             mut video: ResMut<ActiveVideo>,
             mut staged: ResMut<StagedDisplay>,
             mut commands: Commands| {
                if let Some(pending) = pending {
                    video.revert_display(&pending, Some(&mut *staged));
                }
                commands.remove_resource::<PendingRevert>();
            },
        );
}

/// Keeps every picker's text in step with the video settings, the display mode and resolution
/// with what is staged.
pub(super) fn show_values(
    video: Res<ActiveVideo>,
    staged: Option<Res<StagedDisplay>>,
    mut values: Query<(&FieldValue, &mut Text)>,
) {
    let Some(staged) = staged else {
        return;
    };
    for (field, mut text) in &mut values {
        let shown = field.0.value(&video.0, &staged.0).to_uppercase();
        if text.0 != shown {
            text.0 = shown;
        }
    }
}

/// Esc while "Keep these settings?" is up reverts, like REVERT. (The Pause menu's Settings also
/// closes on Esc, which reverts as well.) A rebind in progress takes the Esc first.
pub(super) fn revert_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    rebinding: Res<Rebinding>,
    pending: Res<PendingRevert>,
    mut video: ResMut<ActiveVideo>,
    mut staged: Option<ResMut<StagedDisplay>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) && !rebinding.active() {
        video.revert_display(&pending, staged.as_deref_mut());
        commands.remove_resource::<PendingRevert>();
    }
}

/// Shows the prompt and its countdown while a display change awaits an answer.
pub(super) fn show_revert_prompt(
    pending: Option<Res<PendingRevert>>,
    mut prompt: Query<&mut Node, With<RevertPrompt>>,
    mut countdown: Query<&mut Text, With<RevertCountdown>>,
) {
    let display = if pending.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut prompt {
        if node.display != display {
            node.display = display;
        }
    }
    let shown = pending.map_or_else(String::new, |pending| {
        format!("KEEP THESE SETTINGS? REVERTING IN {}S", pending.secs_left())
    });
    for mut text in &mut countdown {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::Quality;

    const MODES: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (2560, 1440)];

    fn windowed() -> DisplayChoice {
        VideoSettings::default().display()
    }

    #[test]
    fn the_display_pickers_change_the_staged_display_and_not_the_live_settings() {
        let live = VideoSettings::default();
        for field in [VideoField::DisplayMode, VideoField::Resolution] {
            let mut video = live.clone();
            let mut staged = windowed();
            field.step(&mut video, &mut staged, &MODES, 1);
            assert_eq!(video, live, "the window keeps its settings until APPLY");
            assert_ne!(staged, windowed());
        }
    }

    #[test]
    fn the_other_pickers_change_the_live_settings_and_leave_the_staged_display() {
        for field in [
            VideoField::Vsync,
            VideoField::FpsCap,
            VideoField::Preset,
            VideoField::Shadows,
            VideoField::Ssao,
            VideoField::Bloom,
            VideoField::AntiAliasing,
            VideoField::ShowFps,
        ] {
            let mut video = VideoSettings::default();
            let mut staged = windowed();
            field.step(&mut video, &mut staged, &MODES, 1);
            assert_ne!(video, VideoSettings::default(), "{:?}", field.caption());
            assert_eq!(staged, windowed());
        }
    }

    #[test]
    fn the_pickers_show_the_staged_display() {
        let video = VideoSettings::default();
        let staged = DisplayChoice {
            mode: DisplayMode::ExclusiveFullscreen,
            resolution: Some((1920, 1080)),
        };
        assert_eq!(
            VideoField::DisplayMode.value(&video, &staged),
            "Exclusive fullscreen"
        );
        assert_eq!(VideoField::Resolution.value(&video, &staged), "1920 x 1080");
    }

    #[test]
    fn borderless_fullscreen_shows_the_monitors_resolution() {
        let staged = DisplayChoice {
            mode: DisplayMode::BorderlessFullscreen,
            resolution: Some((1920, 1080)),
        };
        assert_eq!(
            VideoField::Resolution.value(&VideoSettings::default(), &staged),
            "Monitor's"
        );
    }

    #[test]
    fn picking_a_preset_sets_the_quality_and_editing_one_makes_it_custom() {
        let mut video = VideoSettings::default();
        let mut staged = windowed();
        VideoField::Preset.step(&mut video, &mut staged, &[], 1);
        assert_eq!(video.preset, QualityPreset::High);
        assert_eq!(Some(video.quality), QualityPreset::High.quality());
        VideoField::Bloom.step(&mut video, &mut staged, &[], 1);
        assert_eq!(video.preset, QualityPreset::Custom);
        assert_eq!(
            video.quality,
            Quality {
                bloom: false,
                ..QualityPreset::High.quality().unwrap()
            }
        );
    }
}
