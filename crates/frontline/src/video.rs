//! Video settings (Settings > Video): the saved [`VideoSettings`], the live [`ActiveVideo`] the
//! Settings screen edits, and what carries them out: the window (display mode, resolution,
//! vsync) and a sleep-based frame limiter here, camera and light quality plus brightness in
//! `atmosphere`. Client/Host only.
//!
//! Video changes apply live. A change of display mode or resolution starts a [`PendingRevert`]:
//! the Settings screen asks "Keep these settings?" and puts the old ones back after
//! [`KEEP_SECS`] unless they are kept.
//!
//! There is no render scale: Bevy 0.20 has no supported way to render the main pass below the
//! window size (`MainPassResolutionOverride` is render-world only, meant for DLSS, and leaves
//! post-processing at full size).

use std::thread;
use std::time::{Duration, Instant};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow,
    VideoModeSelection, WindowMode,
};
use serde::{Deserialize, Serialize};

use crate::flow::Settings;

/// Seconds the "Keep these settings?" prompt waits before reverting.
pub const KEEP_SECS: f32 = 10.0;

/// FPS caps the picker offers; 0 is no cap.
pub const FPS_CAPS: [u32; 8] = [0, 30, 60, 90, 120, 144, 165, 240];

/// Brightness range of the slider; 1.0 is the Map's own look.
pub const BRIGHTNESS_RANGE: (f32, f32) = (0.5, 1.5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    #[default]
    Windowed,
    BorderlessFullscreen,
    ExclusiveFullscreen,
}

impl DisplayMode {
    pub const ALL: [DisplayMode; 3] = [
        DisplayMode::Windowed,
        DisplayMode::BorderlessFullscreen,
        DisplayMode::ExclusiveFullscreen,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Windowed => "Windowed",
            DisplayMode::BorderlessFullscreen => "Borderless fullscreen",
            DisplayMode::ExclusiveFullscreen => "Exclusive fullscreen",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    Off,
    /// 1024 px sun shadow map.
    Low,
    /// 2048 px sun shadow map.
    High,
}

impl ShadowQuality {
    pub const ALL: [ShadowQuality; 3] =
        [ShadowQuality::Off, ShadowQuality::Low, ShadowQuality::High];

    pub fn label(self) -> &'static str {
        match self {
            ShadowQuality::Off => "Off",
            ShadowQuality::Low => "Low",
            ShadowQuality::High => "High",
        }
    }

    pub fn enabled(self) -> bool {
        self != ShadowQuality::Off
    }

    /// Width and height of the sun's shadow map cascades.
    pub fn map_size(self) -> usize {
        match self {
            ShadowQuality::Off | ShadowQuality::Low => 1024,
            ShadowQuality::High => 2048,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaoQuality {
    Off,
    Low,
    Medium,
    High,
}

impl SsaoQuality {
    pub const ALL: [SsaoQuality; 4] = [
        SsaoQuality::Off,
        SsaoQuality::Low,
        SsaoQuality::Medium,
        SsaoQuality::High,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SsaoQuality::Off => "Off",
            SsaoQuality::Low => "Low",
            SsaoQuality::Medium => "Medium",
            SsaoQuality::High => "High",
        }
    }
}

/// Anti-aliasing. SSAO needs `Msaa::Off`, so these are all post-process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Smaa,
}

impl AntiAliasing {
    pub const ALL: [AntiAliasing; 3] = [AntiAliasing::Off, AntiAliasing::Fxaa, AntiAliasing::Smaa];

    pub fn label(self) -> &'static str {
        match self {
            AntiAliasing::Off => "Off",
            AntiAliasing::Fxaa => "FXAA",
            AntiAliasing::Smaa => "SMAA",
        }
    }
}

/// The four settings a quality preset drives. Loads with the Medium values for whatever a
/// saved file lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Quality {
    pub shadows: ShadowQuality,
    pub ssao: SsaoQuality,
    pub bloom: bool,
    pub aa: AntiAliasing,
}

impl Quality {
    const LOW: Quality = Quality {
        shadows: ShadowQuality::Low,
        ssao: SsaoQuality::Off,
        bloom: false,
        aa: AntiAliasing::Off,
    };
    /// What the game looked like before the video settings existed.
    const MEDIUM: Quality = Quality {
        shadows: ShadowQuality::High,
        ssao: SsaoQuality::Medium,
        bloom: true,
        aa: AntiAliasing::Fxaa,
    };
    const HIGH: Quality = Quality {
        shadows: ShadowQuality::High,
        ssao: SsaoQuality::High,
        bloom: true,
        aa: AntiAliasing::Smaa,
    };
}

impl Default for Quality {
    fn default() -> Self {
        Quality::MEDIUM
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityPreset {
    Low,
    /// What the game looked like before the video settings existed.
    #[default]
    Medium,
    High,
    /// Any of the four settings was edited by hand.
    Custom,
}

impl QualityPreset {
    pub const ALL: [QualityPreset; 4] = [
        QualityPreset::Low,
        QualityPreset::Medium,
        QualityPreset::High,
        QualityPreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QualityPreset::Low => "Low",
            QualityPreset::Medium => "Medium",
            QualityPreset::High => "High",
            QualityPreset::Custom => "Custom",
        }
    }

    /// What the preset sets; `None` for Custom, which keeps the current values.
    pub fn quality(self) -> Option<Quality> {
        match self {
            QualityPreset::Low => Some(Quality::LOW),
            QualityPreset::Medium => Some(Quality::MEDIUM),
            QualityPreset::High => Some(Quality::HIGH),
            QualityPreset::Custom => None,
        }
    }
}

/// The display part of the settings: what the revert prompt puts back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayChoice {
    pub mode: DisplayMode,
    pub resolution: Option<(u32, u32)>,
}

/// Everything in Settings > Video. Loads with defaults for whatever an older settings.ron lacks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub display_mode: DisplayMode,
    /// Physical size of the window (windowed) or of the video mode (exclusive fullscreen);
    /// `None` leaves the window as it is. Borderless fullscreen always uses the monitor's.
    pub resolution: Option<(u32, u32)>,
    pub vsync: bool,
    /// Frames per second the limiter allows; 0 is unlimited.
    pub fps_cap: u32,
    pub preset: QualityPreset,
    pub quality: Quality,
    /// 1.0 is the Map's own look; scales the exposure.
    pub brightness: f32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            display_mode: DisplayMode::Windowed,
            resolution: None,
            vsync: true,
            fps_cap: 0,
            preset: QualityPreset::Medium,
            quality: Quality::MEDIUM,
            brightness: 1.0,
        }
    }
}

impl VideoSettings {
    pub fn display(&self) -> DisplayChoice {
        DisplayChoice {
            mode: self.display_mode,
            resolution: self.resolution,
        }
    }

    pub fn set_display(&mut self, display: DisplayChoice) {
        self.display_mode = display.mode;
        self.resolution = display.resolution;
    }

    /// Takes a preset's values (Custom keeps the current ones).
    pub fn set_preset(&mut self, preset: QualityPreset) {
        self.preset = preset;
        if let Some(quality) = preset.quality() {
            self.quality = quality;
        }
    }

    /// Edits one of the quality settings by hand, which makes the preset Custom.
    pub fn edit_quality(&mut self, edit: impl FnOnce(&mut Quality)) {
        edit(&mut self.quality);
        self.preset = QualityPreset::Custom;
    }

    /// Exposure offset in EV that `brightness` adds to a look.
    pub fn exposure_offset(&self) -> f32 {
        self.brightness.max(0.01).log2()
    }
}

/// The entry after (`dir` > 0) or before `current` in `options`, wrapping around. `current`
/// that isn't listed counts as the first entry.
pub fn step<T: Copy + PartialEq>(options: &[T], current: T, dir: i32) -> T {
    let at = options
        .iter()
        .position(|option| *option == current)
        .unwrap_or(0) as i32;
    options[(at + dir.signum()).rem_euclid(options.len() as i32) as usize]
}

/// The video settings in effect. The Settings screen edits them live; APPLY copies them into
/// `Settings` (which saves them), BACK sets them back to the saved ones.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ActiveVideo(pub VideoSettings);

/// A display change awaiting "Keep these settings?".
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct PendingRevert {
    /// What to go back to.
    pub previous: DisplayChoice,
    remaining: f32,
}

impl PendingRevert {
    /// The prompt after a display change from `previous`. A change while a prompt is already
    /// up keeps the settings from before the first one and restarts the countdown.
    pub fn begin(pending: Option<&PendingRevert>, previous: DisplayChoice) -> Self {
        Self {
            previous: pending.map_or(previous, |pending| pending.previous),
            remaining: KEEP_SECS,
        }
    }

    /// Counts down; true once the time is up.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.remaining = (self.remaining - dt).max(0.0);
        self.remaining <= 0.0
    }

    /// Whole seconds left, rounded up, for the prompt.
    pub fn secs_left(&self) -> u32 {
        self.remaining.ceil() as u32
    }
}

/// The monitor the window is on (the primary one before the window knows it).
#[derive(SystemParam)]
pub struct CurrentMonitor<'w, 's> {
    window: Query<'w, 's, &'static OnMonitor, With<PrimaryWindow>>,
    monitors: Query<'w, 's, &'static Monitor>,
    primary: Query<'w, 's, &'static Monitor, With<PrimaryMonitor>>,
}

impl CurrentMonitor<'_, '_> {
    fn monitor(&self) -> Option<&Monitor> {
        self.window
            .single()
            .ok()
            .and_then(|on| self.monitors.get(on.0).ok())
            .or_else(|| self.primary.single().ok())
    }

    /// The distinct physical sizes of the monitor's video modes, smallest first.
    pub fn resolutions(&self) -> Vec<(u32, u32)> {
        let Some(monitor) = self.monitor() else {
            return Vec::new();
        };
        let mut sizes: Vec<(u32, u32)> = monitor
            .video_modes
            .iter()
            .map(|mode| (mode.physical_size.x, mode.physical_size.y))
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }

    /// The video mode for `resolution`: its fastest refresh rate, then deepest colour.
    fn video_mode(&self, resolution: (u32, u32)) -> Option<bevy::window::VideoMode> {
        self.monitor()?
            .video_modes
            .iter()
            .filter(|mode| (mode.physical_size.x, mode.physical_size.y) == resolution)
            .max_by_key(|mode| (mode.refresh_rate_millihertz, mode.bit_depth))
            .copied()
    }
}

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        let video = app.world().resource::<Settings>().video.clone();
        app.insert_resource(ActiveVideo(video)).add_systems(
            Update,
            (
                apply_window.run_if(resource_changed::<ActiveVideo>),
                revert_when_time_is_up.run_if(resource_exists::<PendingRevert>),
            ),
        );
        app.add_systems(Last, limit_frame_rate);
    }
}

/// Edits the live video settings and, when that changes the display mode or resolution, starts
/// (or restarts) the "Keep these settings?" prompt.
pub fn change_display(
    video: &mut ActiveVideo,
    pending: Option<&PendingRevert>,
    commands: &mut Commands,
    edit: impl FnOnce(&mut VideoSettings),
) {
    let before = video.0.display();
    let mut edited = video.0.clone();
    edit(&mut edited);
    let after = edited.display();
    if edited != video.0 {
        video.0 = edited;
    }
    if after != before {
        commands.insert_resource(PendingRevert::begin(pending, before));
    }
}

/// Puts the old display settings back when nobody answered the prompt in time.
fn revert_when_time_is_up(
    time: Res<Time<Real>>,
    mut pending: ResMut<PendingRevert>,
    mut video: ResMut<ActiveVideo>,
    mut commands: Commands,
) {
    if pending.tick(time.delta_secs()) {
        info!("Display settings not kept; going back");
        video.0.set_display(pending.previous);
        commands.remove_resource::<PendingRevert>();
    }
}

/// What was last handed to the window, so a brightness drag doesn't undo a manual resize.
#[derive(Clone, Copy, PartialEq)]
struct AppliedWindow {
    display: DisplayChoice,
    vsync: bool,
}

/// The windowed size to come back to, and the size this system last set.
#[derive(Default)]
struct WindowedSize {
    /// The window's size before this system last changed it (a manual resize counts).
    base: Option<UVec2>,
    set: Option<UVec2>,
}

/// Puts the display mode, resolution and vsync on the window.
fn apply_window(
    video: Res<ActiveVideo>,
    monitor: CurrentMonitor,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut applied: Local<Option<AppliedWindow>>,
    mut windowed: Local<WindowedSize>,
) {
    let video = &video.0;
    let wanted = AppliedWindow {
        display: video.display(),
        vsync: video.vsync,
    };
    if *applied == Some(wanted) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    *applied = Some(wanted);

    let present_mode = if video.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    if window.present_mode != present_mode {
        window.present_mode = present_mode;
    }

    let mode = match video.display_mode {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::BorderlessFullscreen => {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        }
        DisplayMode::ExclusiveFullscreen => WindowMode::Fullscreen(
            MonitorSelection::Current,
            video
                .resolution
                .and_then(|resolution| monitor.video_mode(resolution))
                .map_or(VideoModeSelection::Current, VideoModeSelection::Specific),
        ),
    };
    let size_now = window.resolution.physical_size();
    if window.mode == WindowMode::Windowed && Some(size_now) != windowed.set {
        windowed.base = Some(size_now);
    }
    if window.mode != mode {
        window.mode = mode;
    }
    if video.display_mode == DisplayMode::Windowed
        && let Some(size) = video
            .resolution
            .map(|(w, h)| UVec2::new(w, h))
            .or(windowed.base)
        && size_now != size
    {
        window.resolution.set_physical_resolution(size.x, size.y);
        windowed.set = Some(size);
    }
}

/// Sleeps out the rest of each frame when an FPS cap is set.
fn limit_frame_rate(video: Res<ActiveVideo>, mut next_frame: Local<Option<Instant>>) {
    let now = Instant::now();
    if video.0.fps_cap == 0 {
        *next_frame = None;
        return;
    }
    let frame = Duration::from_secs_f64(1.0 / f64::from(video.0.fps_cap));
    let deadline = next_frame.map_or(now, |last| last + frame);
    if now < deadline {
        thread::sleep(deadline - now);
        *next_frame = Some(deadline);
    } else {
        *next_frame = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_set_the_four_quality_settings() {
        let mut video = VideoSettings::default();
        video.set_preset(QualityPreset::Low);
        assert_eq!(video.preset, QualityPreset::Low);
        assert_eq!(
            video.quality,
            Quality {
                shadows: ShadowQuality::Low,
                ssao: SsaoQuality::Off,
                bloom: false,
                aa: AntiAliasing::Off,
            }
        );
        video.set_preset(QualityPreset::High);
        assert_eq!(video.quality.ssao, SsaoQuality::High);
        assert_eq!(video.quality.aa, AntiAliasing::Smaa);
    }

    #[test]
    fn the_default_is_the_medium_preset() {
        let video = VideoSettings::default();
        assert_eq!(video.preset, QualityPreset::Medium);
        assert_eq!(Some(video.quality), QualityPreset::Medium.quality());
    }

    #[test]
    fn editing_any_quality_setting_makes_the_preset_custom() {
        for edit in [
            (|q: &mut Quality| q.shadows = ShadowQuality::Off) as fn(&mut Quality),
            |q| q.ssao = SsaoQuality::Low,
            |q| q.bloom = false,
            |q| q.aa = AntiAliasing::Smaa,
        ] {
            let mut video = VideoSettings::default();
            video.edit_quality(edit);
            assert_eq!(video.preset, QualityPreset::Custom);
        }
        // Custom keeps the values it had.
        let mut video = VideoSettings::default();
        video.set_preset(QualityPreset::Custom);
        assert_eq!(Some(video.quality), QualityPreset::Medium.quality());
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let options = [1, 2, 3];
        assert_eq!(step(&options, 3, 1), 1);
        assert_eq!(step(&options, 1, -1), 3);
        assert_eq!(step(&options, 2, 1), 3);
        assert_eq!(step(&options, 9, 1), 2);
    }

    #[test]
    fn brightness_is_an_exposure_offset_in_ev() {
        let mut video = VideoSettings::default();
        assert_eq!(video.exposure_offset(), 0.0);
        video.brightness = 0.5;
        assert_eq!(video.exposure_offset(), -1.0);
        video.brightness = 2.0;
        assert_eq!(video.exposure_offset(), 1.0);
    }

    fn display(mode: DisplayMode) -> DisplayChoice {
        DisplayChoice {
            mode,
            resolution: None,
        }
    }

    #[test]
    fn the_revert_prompt_counts_down_and_expires() {
        let mut prompt = PendingRevert::begin(None, display(DisplayMode::Windowed));
        assert_eq!(prompt.secs_left(), 10);
        assert!(!prompt.tick(4.0));
        assert_eq!(prompt.secs_left(), 6);
        assert!(!prompt.tick(5.5));
        assert_eq!(prompt.secs_left(), 1);
        assert!(prompt.tick(0.5));
        assert_eq!(prompt.secs_left(), 0);
    }

    #[test]
    fn another_change_keeps_the_first_previous_and_restarts_the_countdown() {
        let mut first = PendingRevert::begin(None, display(DisplayMode::Windowed));
        first.tick(7.0);
        let second = PendingRevert::begin(Some(&first), display(DisplayMode::BorderlessFullscreen));
        assert_eq!(second.previous, display(DisplayMode::Windowed));
        assert_eq!(second.secs_left(), 10);
    }

    #[test]
    fn an_old_settings_file_without_video_loads_with_defaults() {
        let video: VideoSettings = ron::from_str("(vsync: false, fps_cap: 144)").unwrap();
        assert!(!video.vsync);
        assert_eq!(video.fps_cap, 144);
        assert_eq!(video.display_mode, DisplayMode::Windowed);
        assert_eq!(video.preset, QualityPreset::Medium);
        assert_eq!(video.brightness, 1.0);
    }

    #[test]
    fn video_settings_round_trip_through_ron() {
        let mut video = VideoSettings {
            display_mode: DisplayMode::ExclusiveFullscreen,
            resolution: Some((1920, 1080)),
            fps_cap: 90,
            brightness: 1.25,
            ..default()
        };
        video.edit_quality(|q| q.aa = AntiAliasing::Smaa);
        let text = ron::to_string(&video).unwrap();
        assert_eq!(ron::from_str::<VideoSettings>(&text).unwrap(), video);
    }
}
