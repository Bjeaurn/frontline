//! The Video tab: pickers for the display, quality and frame-rate settings, the brightness
//! slider, and the "Keep these settings?" prompt after a display change. Everything here edits
//! the live `ActiveVideo`.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use super::{SettingSlider, setting_row, slider_row};
use crate::flow::Settings;
use crate::menus::host_setup::picker;
use crate::ui::{ArrowIcon, TEXT_DIM, UiFonts, compact_button, label};
use crate::video::{
    ActiveVideo, AntiAliasing, CurrentMonitor, DisplayMode, FPS_CAPS, PendingRevert, QualityPreset,
    ShadowQuality, SsaoQuality, VideoSettings, change_display, step,
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
}

impl VideoField {
    /// In the order of the tab.
    const ALL: [VideoField; 9] = [
        VideoField::DisplayMode,
        VideoField::Resolution,
        VideoField::Vsync,
        VideoField::FpsCap,
        VideoField::Preset,
        VideoField::Shadows,
        VideoField::Ssao,
        VideoField::Bloom,
        VideoField::AntiAliasing,
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
        }
    }

    /// What the picker shows.
    fn value(self, video: &VideoSettings) -> String {
        match self {
            VideoField::DisplayMode => video.display_mode.label().to_owned(),
            VideoField::Resolution => match (video.display_mode, video.resolution) {
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
        }
    }

    /// Moves the setting `dir` places along what it offers (`resolutions`: the monitor's sizes).
    fn step(self, video: &mut VideoSettings, resolutions: &[(u32, u32)], dir: i32) {
        match self {
            VideoField::DisplayMode => {
                video.display_mode = step(&DisplayMode::ALL, video.display_mode, dir);
            }
            // Nothing to pick while borderless: the monitor decides.
            VideoField::Resolution if video.display_mode == DisplayMode::BorderlessFullscreen => {}
            VideoField::Resolution => {
                let options: Vec<Option<(u32, u32)>> = std::iter::once(None)
                    .chain(resolutions.iter().copied().map(Some))
                    .collect();
                video.resolution = step(&options, video.resolution, dir);
            }
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
            &field.value(&video.0),
        );
        commands.entity(value).insert(FieldValue(field));
        for (arrow, dir) in [(left, -1), (right, 1)] {
            commands.entity(arrow).observe(
                move |_: On<Activate>,
                      mut video: ResMut<ActiveVideo>,
                      pending: Option<Res<PendingRevert>>,
                      monitor: CurrentMonitor,
                      mut commands: Commands| {
                    let resolutions = monitor.resolutions();
                    change_display(&mut video, pending.as_deref(), &mut commands, |video| {
                        field.step(video, &resolutions, dir);
                    });
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
pub(super) fn spawn_revert_prompt(commands: &mut Commands, fonts: &UiFonts, panel: Entity) {
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
    commands
        .spawn((ChildOf(row), compact_button(fonts, "Keep", 110.0, 22.0)))
        .observe(|_: On<Activate>, mut commands: Commands| {
            commands.remove_resource::<PendingRevert>();
        });
    commands
        .spawn((ChildOf(row), compact_button(fonts, "Revert", 110.0, 22.0)))
        .observe(
            |_: On<Activate>,
             pending: Option<Res<PendingRevert>>,
             mut video: ResMut<ActiveVideo>,
             mut commands: Commands| {
                if let Some(pending) = pending {
                    let mut reverted = video.0.clone();
                    reverted.set_display(pending.previous);
                    video.set_if_neq(ActiveVideo(reverted));
                }
                commands.remove_resource::<PendingRevert>();
            },
        );
}

/// Keeps every picker's text in step with the live video settings.
pub(super) fn show_values(video: Res<ActiveVideo>, mut values: Query<(&FieldValue, &mut Text)>) {
    for (field, mut text) in &mut values {
        let shown = field.0.value(&video.0).to_uppercase();
        if text.0 != shown {
            text.0 = shown;
        }
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
    use bevy::ecs::system::RunSystemOnce;

    const MODES: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (2560, 1440)];

    #[test]
    fn resolution_steps_through_the_monitors_modes_from_current() {
        let mut video = VideoSettings::default();
        VideoField::Resolution.step(&mut video, &MODES, 1);
        assert_eq!(video.resolution, Some((1280, 720)));
        VideoField::Resolution.step(&mut video, &MODES, 1);
        assert_eq!(video.resolution, Some((1920, 1080)));
        VideoField::Resolution.step(&mut video, &MODES, -1);
        VideoField::Resolution.step(&mut video, &MODES, -1);
        assert_eq!(video.resolution, None, "back to the window as it is");
        VideoField::Resolution.step(&mut video, &MODES, -1);
        assert_eq!(video.resolution, Some((2560, 1440)), "wraps");
    }

    #[test]
    fn borderless_fullscreen_leaves_the_resolution_alone() {
        let mut video = VideoSettings {
            display_mode: DisplayMode::BorderlessFullscreen,
            resolution: Some((1920, 1080)),
            ..default()
        };
        VideoField::Resolution.step(&mut video, &MODES, 1);
        assert_eq!(video.resolution, Some((1920, 1080)));
        assert_eq!(VideoField::Resolution.value(&video), "Monitor's");
    }

    #[test]
    fn picking_a_preset_sets_the_quality_and_editing_one_makes_it_custom() {
        let mut video = VideoSettings::default();
        VideoField::Preset.step(&mut video, &[], 1);
        assert_eq!(video.preset, QualityPreset::High);
        assert_eq!(Some(video.quality), QualityPreset::High.quality());
        VideoField::Bloom.step(&mut video, &[], 1);
        assert_eq!(video.preset, QualityPreset::Custom);
        assert_eq!(
            video.quality,
            Quality {
                bloom: false,
                ..QualityPreset::High.quality().unwrap()
            }
        );
    }

    #[test]
    fn only_a_display_or_resolution_change_starts_the_prompt() {
        let mut app = App::new();
        app.insert_resource(ActiveVideo(VideoSettings::default()));
        let prompt_after = |field: VideoField, dir: i32, app: &mut App| {
            app.world_mut()
                .run_system_once(
                    move |mut video: ResMut<ActiveVideo>, mut commands: Commands| {
                        change_display(&mut video, None, &mut commands, |video| {
                            field.step(video, &MODES, dir);
                        });
                    },
                )
                .unwrap();
            app.world().contains_resource::<PendingRevert>()
        };
        assert!(!prompt_after(VideoField::Vsync, 1, &mut app));
        assert!(!prompt_after(VideoField::Shadows, 1, &mut app));
        assert!(!prompt_after(VideoField::FpsCap, 1, &mut app));
        assert!(prompt_after(VideoField::Resolution, 1, &mut app));
    }
}
