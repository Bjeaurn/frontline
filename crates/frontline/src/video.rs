//! Video settings (Settings > Video): the saved [`VideoSettings`], the live [`ActiveVideo`] the
//! Settings screen edits, and what carries them out: the window (display mode, resolution,
//! vsync) and a sleep-based frame limiter here, camera and light quality plus brightness in
//! `atmosphere`. Client/Host only.
//!
//! Most video changes apply live. The display mode and resolution are only staged in the Video
//! tab ([`StagedDisplay`]) until APPLY ([`ActiveVideo::apply_display`]), which starts a
//! [`PendingRevert`]: the Settings screen asks "Keep these settings?" and puts the old ones back
//! after [`KEEP_SECS`] unless they are kept.
//!
//! There is no render scale: Bevy 0.20 has no supported way to render the main pass below the
//! window size (`MainPassResolutionOverride` is render-world only, meant for DLSS, and leaves
//! post-processing at full size).

use std::thread;
use std::time::{Duration, Instant};

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::system::SystemParam;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder};
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow,
    VideoModeSelection, WindowMode,
};
use serde::{Deserialize, Serialize};

use crate::flow::Settings;

/// Seconds the "Keep these settings?" prompt waits before reverting.
pub const KEEP_SECS: f32 = 15.0;

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
    /// 1024 px sun shadow map, 2 cascades out to 60 m.
    Low,
    /// 2048 px sun shadow map, 3 cascades out to 120 m.
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

    /// How the sun splits its shadow over distance. Every cascade draws every shadow caster
    /// again, so this is most of the shadow cost; Bevy's default (4 cascades to 150 m) is more
    /// than the Maps' fog lets anyone see.
    pub fn cascades(self) -> CascadeShadowConfig {
        let (num_cascades, maximum_distance) = match self {
            ShadowQuality::Off | ShadowQuality::Low => (2, 60.0),
            ShadowQuality::High => (3, 120.0),
        };
        CascadeShadowConfigBuilder {
            num_cascades,
            maximum_distance,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build()
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
    /// SSAO is the costliest effect here (it brings depth and normal prepasses along), so the
    /// default keeps it at Low.
    const MEDIUM: Quality = Quality {
        shadows: ShadowQuality::High,
        ssao: SsaoQuality::Low,
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

/// The display part of the settings: what the pickers stage and the revert prompt puts back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayChoice {
    pub mode: DisplayMode,
    pub resolution: Option<(u32, u32)>,
}

impl DisplayChoice {
    /// Moves the display mode `dir` places along the three it offers.
    pub fn step_mode(&mut self, dir: i32) {
        self.mode = step(&DisplayMode::ALL, self.mode, dir);
    }

    /// Moves the resolution `dir` places along "as it is" and the monitor's `resolutions`.
    /// Nothing to pick while borderless: the monitor decides.
    pub fn step_resolution(&mut self, resolutions: &[(u32, u32)], dir: i32) {
        if self.mode == DisplayMode::BorderlessFullscreen {
            return;
        }
        let options: Vec<Option<(u32, u32)>> = std::iter::once(None)
            .chain(resolutions.iter().copied().map(Some))
            .collect();
        self.resolution = step(&options, self.resolution, dir);
    }

    /// Whether `other` shows differently on the window. Borderless fullscreen always fills the
    /// monitor, so its resolution doesn't count.
    pub fn differs_on_screen(&self, other: &DisplayChoice) -> bool {
        self.mode != other.mode
            || (self.mode != DisplayMode::BorderlessFullscreen
                && self.resolution != other.resolution)
    }
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
    /// Frame rate and frame time in the top-right corner.
    pub show_fps: bool,
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
            show_fps: false,
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

/// The video settings in effect. The Settings screen edits them live, except the display mode
/// and resolution, which wait in [`StagedDisplay`] until APPLY. APPLY copies them into
/// `Settings` (which saves them), BACK sets them back to the saved ones.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ActiveVideo(pub VideoSettings);

/// The display mode and resolution the Video tab shows: the active ones when Settings opens,
/// then whatever was picked. Only [`ActiveVideo::apply_display`] puts them on the window.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct StagedDisplay(pub DisplayChoice);

impl ActiveVideo {
    /// APPLY of the Video tab: makes `staged` the active display. When that changes what the
    /// window shows, returns the "Keep these settings?" prompt to start; a prompt already up
    /// keeps the settings from before its first change.
    pub fn apply_display(
        &mut self,
        staged: DisplayChoice,
        pending: Option<&PendingRevert>,
    ) -> Option<PendingRevert> {
        let before = self.0.display();
        if staged == before {
            return None;
        }
        self.0.set_display(staged);
        before
            .differs_on_screen(&staged)
            .then(|| PendingRevert::begin(pending, before))
    }

    /// Puts back the display settings the prompt `pending` was keeping, and the pickers with
    /// them.
    pub fn revert_display(&mut self, pending: &PendingRevert, staged: Option<&mut StagedDisplay>) {
        if self.0.display() != pending.previous {
            self.0.set_display(pending.previous);
        }
        if let Some(staged) = staged
            && staged.0 != pending.previous
        {
            staged.0 = pending.previous;
        }
    }

    /// The settings to save. A display still waiting for its "Keep these settings?" is left
    /// out (the saved one stays) unless `keep_display`.
    pub fn to_save(&self, pending: Option<&PendingRevert>, keep_display: bool) -> VideoSettings {
        let mut video = self.0.clone();
        if !keep_display && let Some(pending) = pending {
            video.set_display(pending.previous);
        }
        video
    }
}

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
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        // Profiling aid: `FRONTLINE_PERF_LOG=1` logs the frame time and every render pass's GPU
        // time and pipeline statistics every few seconds.
        if std::env::var_os("FRONTLINE_PERF_LOG").is_some() {
            app.add_plugins((
                bevy::render::diagnostic::RenderDiagnosticsPlugin,
                bevy::diagnostic::LogDiagnosticsPlugin {
                    wait_duration: Duration::from_secs(3),
                    ..default()
                },
            ));
        }
        app.add_systems(Startup, spawn_fps_counter)
            .add_systems(Update, show_fps_counter);
    }
}

/// The Show FPS readout.
#[derive(Component)]
struct FpsCounter;

fn spawn_fps_counter(fonts: Res<crate::ui::UiFonts>, mut commands: Commands) {
    commands.spawn((
        Name::new("FpsCounter"),
        FpsCounter,
        crate::ui::hud_text(&fonts.regular, "", 16.0, Color::srgb(0.6, 1.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: px(4.0),
            right: px(8.0),
            ..default()
        },
        GlobalZIndex(100),
        Visibility::Hidden,
    ));
}

/// Shows the smoothed frame rate and frame time a few times a second while Show FPS is on.
fn show_fps_counter(
    video: Res<ActiveVideo>,
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
    mut since_update: Local<f32>,
    mut counter: Query<(&mut Text, &mut Visibility), With<FpsCounter>>,
) {
    let Ok((mut text, mut visibility)) = counter.single_mut() else {
        return;
    };
    let shown = if video.0.show_fps {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    visibility.set_if_neq(shown);
    *since_update += time.delta_secs();
    if !video.0.show_fps || *since_update < 0.25 {
        return;
    }
    *since_update = 0.0;
    let smoothed = |path| {
        diagnostics
            .get(path)
            .and_then(|diagnostic| diagnostic.smoothed())
    };
    if let (Some(fps), Some(ms)) = (
        smoothed(&FrameTimeDiagnosticsPlugin::FPS),
        smoothed(&FrameTimeDiagnosticsPlugin::FRAME_TIME),
    ) {
        text.0 = format!("{fps:.0} FPS  {ms:.1} ms");
    }
}

/// Puts the old display settings back when nobody answered the prompt in time.
fn revert_when_time_is_up(
    time: Res<Time<Real>>,
    mut pending: ResMut<PendingRevert>,
    mut video: ResMut<ActiveVideo>,
    mut staged: Option<ResMut<StagedDisplay>>,
    mut commands: Commands,
) {
    if pending.tick(time.delta_secs()) {
        info!("Display settings not kept; going back");
        video.revert_display(&pending, staged.as_deref_mut());
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
        at(mode, None)
    }

    #[test]
    fn the_revert_prompt_counts_down_and_expires() {
        let mut prompt = PendingRevert::begin(None, display(DisplayMode::Windowed));
        assert_eq!(prompt.secs_left(), 15);
        assert!(!prompt.tick(6.0));
        assert_eq!(prompt.secs_left(), 9);
        assert!(!prompt.tick(8.5));
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
        assert_eq!(second.secs_left(), 15);
    }

    const MODES: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (2560, 1440)];

    fn at(mode: DisplayMode, resolution: Option<(u32, u32)>) -> DisplayChoice {
        DisplayChoice { mode, resolution }
    }

    fn active(display: DisplayChoice) -> ActiveVideo {
        let mut video = VideoSettings::default();
        video.set_display(display);
        ActiveVideo(video)
    }

    #[test]
    fn resolution_steps_through_the_monitors_modes_from_current() {
        let mut choice = at(DisplayMode::Windowed, None);
        choice.step_resolution(&MODES, 1);
        assert_eq!(choice.resolution, Some((1280, 720)));
        choice.step_resolution(&MODES, 1);
        assert_eq!(choice.resolution, Some((1920, 1080)));
        choice.step_resolution(&MODES, -1);
        choice.step_resolution(&MODES, -1);
        assert_eq!(choice.resolution, None, "back to the window as it is");
        choice.step_resolution(&MODES, -1);
        assert_eq!(choice.resolution, Some((2560, 1440)), "wraps");
    }

    #[test]
    fn borderless_fullscreen_leaves_the_resolution_alone() {
        let mut choice = at(DisplayMode::BorderlessFullscreen, Some((1920, 1080)));
        choice.step_resolution(&MODES, 1);
        assert_eq!(choice.resolution, Some((1920, 1080)));
        // Its resolution doesn't show on the window either.
        assert!(!choice.differs_on_screen(&at(DisplayMode::BorderlessFullscreen, None)));
        assert!(choice.differs_on_screen(&at(DisplayMode::Windowed, Some((1920, 1080)))));
    }

    #[test]
    fn applying_changes_the_display_and_starts_a_prompt_for_the_old_one() {
        let windowed = at(DisplayMode::Windowed, None);
        let mut video = active(windowed);
        let wanted = at(DisplayMode::Windowed, Some((1920, 1080)));
        let prompt = video.apply_display(wanted, None).expect("a prompt");
        assert_eq!(video.0.display(), wanted);
        assert_eq!(prompt.previous, windowed);
        assert_eq!(prompt.secs_left(), 15);
    }

    #[test]
    fn applying_what_is_already_active_starts_no_prompt() {
        let wanted = at(DisplayMode::Windowed, Some((1920, 1080)));
        let mut video = active(wanted);
        assert_eq!(video.apply_display(wanted, None), None);
        // Borderless fills the monitor whatever resolution was staged with it.
        let mut video = active(at(DisplayMode::BorderlessFullscreen, None));
        let staged = at(DisplayMode::BorderlessFullscreen, Some((1280, 720)));
        assert_eq!(video.apply_display(staged, None), None);
        assert_eq!(video.0.display(), staged, "the pickers can't drift from it");
    }

    #[test]
    fn applying_again_during_the_prompt_still_reverts_to_the_first_display() {
        let windowed = at(DisplayMode::Windowed, None);
        let mut video = active(windowed);
        let first = video
            .apply_display(at(DisplayMode::Windowed, Some((1280, 720))), None)
            .unwrap();
        let second = video
            .apply_display(at(DisplayMode::ExclusiveFullscreen, None), Some(&first))
            .unwrap();
        assert_eq!(second.previous, windowed);
    }

    #[test]
    fn reverting_restores_the_old_display_and_the_pickers() {
        let windowed = at(DisplayMode::Windowed, None);
        let mut video = active(windowed);
        let prompt = video
            .apply_display(at(DisplayMode::BorderlessFullscreen, None), None)
            .unwrap();
        // The user picks something else while the prompt is up.
        let mut staged = StagedDisplay(at(DisplayMode::ExclusiveFullscreen, Some((1280, 720))));
        video.revert_display(&prompt, Some(&mut staged));
        assert_eq!(video.0.display(), windowed);
        assert_eq!(staged.0, windowed);
    }

    #[test]
    fn only_a_kept_display_is_saved() {
        let windowed = at(DisplayMode::Windowed, None);
        let mut video = active(windowed);
        let wanted = at(DisplayMode::BorderlessFullscreen, None);
        let prompt = video.apply_display(wanted, None).unwrap();
        video.0.vsync = false;
        // Saving while the prompt is up (CHECK, leaving for the rules editor) keeps the old
        // display; the live settings go along.
        let saved = video.to_save(Some(&prompt), false);
        assert_eq!(saved.display(), windowed);
        assert!(!saved.vsync);
        // KEEP.
        assert_eq!(video.to_save(Some(&prompt), true).display(), wanted);
        // Nothing awaiting an answer.
        assert_eq!(video.to_save(None, false).display(), wanted);
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
