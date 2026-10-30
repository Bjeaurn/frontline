//! Camera look and per-Map mood. Fixed for every Map: TonyMcMapface tonemapping (HDR), added to
//! every world-rendering `Camera3d` (gameplay eye, main menu orbit) when it spawns. Bloom, SSAO,
//! anti-aliasing and the sun's shadows follow Settings > Video (`video::Quality`) and apply
//! live. Per Map: a look preset from `data/looks.ron` (grade, fog, sun, ambient, sky), applied
//! when the Map is ready; the Video brightness setting offsets its exposure. Every world camera
//! also gets the look's procedural sky (`sky`: gradient, sun, clouds as a `Skybox`), the sky's
//! colours as hemispherical ambient light (cool from above, warm from the ground) and a
//! vignette. The viewmodel camera only shares the tone curve and grade (see `weapon::viewmodel`).
//! Client/Host only.
//!
//! SSAO needs `Msaa::Off`, so world cameras anti-alias in post: FXAA (cheapest) or SMAA, where
//! TAA would also need motion vectors and ghosts on fast flicks.

pub(crate) mod sky;

use std::collections::HashMap;

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::Exposure;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{DirectionalLightShadowMap, EnvironmentMapLight, Skybox};
use bevy::pbr::{
    DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use serde::Deserialize;

use crate::maps::MapReady;
use crate::video::{ActiveVideo, AntiAliasing, Quality, SsaoQuality};
use crate::weapon::ViewmodelCamera;

const LOOKS_FILE: &str = "looks.ron";
const BUILTIN_LOOKS: &str = include_str!("../../../data/looks.ron");

/// Share of a look's ambient light that arrives from the sky and ground (the hemispherical
/// environment light); the rest is flat fill so shadows never go black.
const ENVIRONMENT_SHARE: f32 = 0.75;
/// Vignette shape: a wide, soft falloff that only darkens the corners noticeably.
const VIGNETTE_RADIUS: f32 = 1.0;
const VIGNETTE_SMOOTHNESS: f32 = 1.6;

/// The look before a Map is ready, and for Maps that name none (or an unknown one).
const DEFAULT_LOOK: &str = "overcast_urban";

/// Tone curve shared by every camera, so the gun matches the world.
pub const TONEMAPPING: Tonemapping = Tonemapping::TonyMcMapface;

type Rgb = (f32, f32, f32);

/// The visual mood of a Map (IDEA.md: Look per Map). See `data/looks.ron` for the units.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LookPreset {
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub fog_color: Rgb,
    pub fog_visibility: f32,
    pub fog_sun_color: (f32, f32, f32, f32),
    pub sun_color: Rgb,
    pub sun_illuminance: f32,
    pub sun_direction: Rgb,
    pub ambient_color: Rgb,
    pub ambient_brightness: f32,
    /// Sky colour straight up (it meets the fog colour at the horizon) and cloud cover (0 clear,
    /// 1 overcast).
    pub sky_zenith: Rgb,
    #[serde(default)]
    pub sky_clouds: f32,
    /// Colour of the light bounced up from the ground; `sky_zenith` and `ambient_color` light
    /// from above and the sides.
    pub ambient_ground: Rgb,
    /// Strength (0..1) of the dark screen edges.
    #[serde(default)]
    pub vignette: f32,
    /// Scale (0 by day, 1 at night) of the Map's practical lights: lamp posts and the like.
    #[serde(default)]
    pub practical_lights: f32,
}

fn srgb((r, g, b): Rgb) -> Color {
    Color::srgb(r, g, b)
}

impl LookPreset {
    /// The look's grade with `brightness_ev` (Settings > Video) added to its exposure.
    pub fn color_grading(&self, brightness_ev: f32) -> ColorGrading {
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: self.exposure + brightness_ev,
                temperature: self.temperature,
                tint: self.tint,
                post_saturation: self.saturation,
                ..default()
            },
            midtones: ColorGradingSection {
                contrast: self.contrast,
                ..default()
            },
            ..default()
        }
    }

    fn fog(&self) -> DistanceFog {
        let (r, g, b, a) = self.fog_sun_color;
        DistanceFog {
            color: srgb(self.fog_color),
            directional_light_color: Color::srgba(r, g, b, a),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::from_visibility_squared(self.fog_visibility),
        }
    }

    fn sun(&self, shadows: bool) -> (DirectionalLight, Transform) {
        let light = DirectionalLight {
            color: srgb(self.sun_color),
            illuminance: self.sun_illuminance,
            shadow_maps_enabled: shadows,
            ..default()
        };
        let direction = Vec3::from(self.sun_direction);
        (light, Transform::default().looking_to(direction, Vec3::Y))
    }

    fn sky(&self) -> sky::SkyParams {
        let linear = |(r, g, b): Rgb| {
            let color = Color::srgb(r, g, b).to_linear();
            Vec3::new(color.red, color.green, color.blue)
        };
        sky::SkyParams {
            zenith: linear(self.sky_zenith),
            horizon: linear(self.fog_color),
            ground: linear(self.fog_color) * 0.6,
            to_sun: -Vec3::from(self.sun_direction).normalize(),
            sun_color: linear(self.sun_color),
            clouds: self.sky_clouds,
        }
    }

    fn vignette(&self) -> Vignette {
        Vignette {
            intensity: self.vignette,
            radius: VIGNETTE_RADIUS,
            smoothness: VIGNETTE_SMOOTHNESS,
            ..default()
        }
    }

    /// The sky and the light it casts: a hemispherical ambient (sky colour above, ambient colour
    /// around, ground bounce below).
    fn environment(&self, images: &mut Assets<Image>) -> ActiveSky {
        let skybox = images.add(sky::cubemap(&self.sky()));
        let light = EnvironmentMapLight::hemispherical_gradient(
            images,
            srgb(self.sky_zenith),
            srgb(self.ambient_color),
            srgb(self.ambient_ground),
        )
        .with_intensity(self.ambient_brightness * ENVIRONMENT_SHARE);
        ActiveSky { skybox, light }
    }
}

/// The Map's sun, the one light whose shadows Settings > Video switches.
#[derive(Component)]
struct Sun;

/// Every look preset by name, from `data/looks.ron`.
#[derive(Resource, Debug)]
pub struct Looks(HashMap<String, LookPreset>);

impl Looks {
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Preset names in alphabetical order, for error messages.
    pub fn known_names(&self) -> String {
        let mut names: Vec<&str> = self.0.keys().map(String::as_str).collect();
        names.sort_unstable();
        names.join(", ")
    }
}

/// `--look`: a look preset that replaces every Map's own for this session.
#[derive(Resource, Debug)]
struct LookOverride(Option<String>);

/// The look in effect; cameras spawned later pick it up.
#[derive(Resource, Debug)]
pub struct ActiveLook(LookPreset);

impl ActiveLook {
    /// The colour of kicked-up dust under this look: the haze tinted by the bounced ground light.
    pub fn dust(&self) -> LinearRgba {
        let look = &self.0;
        let mix = |a: f32, b: f32| a * 0.6 + b * 0.4;
        srgb((
            mix(look.fog_color.0, look.ambient_ground.0),
            mix(look.fog_color.1, look.ambient_ground.1),
            mix(look.fog_color.2, look.ambient_ground.2),
        ))
        .to_linear()
    }
}

/// The look's sky: the skybox cubemap and the hemispherical ambient it gives; cameras spawned
/// later pick it up.
#[derive(Resource)]
struct ActiveSky {
    skybox: Handle<Image>,
    light: EnvironmentMapLight,
}

impl ActiveSky {
    fn skybox(&self) -> Skybox {
        Skybox {
            image: Some(self.skybox.clone()),
            // The skybox is scaled by the camera's exposure like a light; undo that so its texels
            // read as display colour, in the units fog uses.
            brightness: 1.0 / Exposure::default().exposure(),
            ..default()
        }
    }
}

pub struct AtmospherePlugin {
    /// Look preset name from `--look`; checked against [`Looks`] by the caller.
    pub look_override: Option<String>,
}

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        let looks: HashMap<String, LookPreset> = crate::data::load(LOOKS_FILE, BUILTIN_LOOKS);
        let Some(default_look) = looks.get(DEFAULT_LOOK).cloned() else {
            panic!("{LOOKS_FILE} must define the \"{DEFAULT_LOOK}\" look");
        };
        let sky = default_look.environment(&mut app.world_mut().resource_mut::<Assets<Image>>());
        app.insert_resource(Looks(looks))
            .insert_resource(LookOverride(self.look_override.clone()))
            .insert_resource(ActiveLook(default_look))
            .insert_resource(sky)
            .add_observer(add_atmosphere)
            .add_observer(apply_map_look)
            .add_systems(
                Update,
                (
                    scale_practical_lights,
                    apply_video.run_if(resource_changed::<ActiveVideo>),
                ),
            );
    }
}

fn bloom() -> Bloom {
    Bloom {
        intensity: 0.08,
        ..Bloom::NATURAL
    }
}

/// Puts the quality settings' camera effects on a world camera, and takes the off ones away.
/// SSAO brings its depth and normal prepasses along, so they go with it.
fn set_quality(camera: &mut EntityCommands, quality: &Quality) {
    if quality.bloom {
        camera.insert(bloom());
    } else {
        camera.remove::<Bloom>();
    }
    let ssao_level = match quality.ssao {
        SsaoQuality::Off => None,
        SsaoQuality::Low => Some(ScreenSpaceAmbientOcclusionQualityLevel::Low),
        SsaoQuality::Medium => Some(ScreenSpaceAmbientOcclusionQualityLevel::Medium),
        SsaoQuality::High => Some(ScreenSpaceAmbientOcclusionQualityLevel::High),
    };
    match ssao_level {
        Some(quality_level) => {
            camera.insert(ScreenSpaceAmbientOcclusion {
                quality_level,
                ..default()
            });
        }
        None => {
            camera.remove::<(ScreenSpaceAmbientOcclusion, DepthPrepass, NormalPrepass)>();
        }
    }
    match quality.aa {
        AntiAliasing::Off => {
            camera.remove::<(Fxaa, Smaa)>();
        }
        AntiAliasing::Fxaa => {
            camera.insert(Fxaa::default()).remove::<Smaa>();
        }
        AntiAliasing::Smaa => {
            camera.insert(Smaa::default()).remove::<Fxaa>();
        }
    }
}

fn add_atmosphere(
    add: On<Add<Camera3d>>,
    viewmodels: Query<(), With<ViewmodelCamera>>,
    look: Res<ActiveLook>,
    sky: Res<ActiveSky>,
    video: Res<ActiveVideo>,
    mut commands: Commands,
) {
    let grading = look.0.color_grading(video.0.exposure_offset());
    let mut camera = commands.entity(add.entity);
    if viewmodels.contains(add.entity) {
        camera.insert(grading);
        return;
    }
    camera.insert((
        TONEMAPPING,
        grading,
        Msaa::Off,
        look.0.fog(),
        look.0.vignette(),
        sky.skybox(),
        sky.light.clone(),
    ));
    set_quality(&mut camera, &video.0.quality);
}

/// Carries out a change of the Video settings on the cameras and the sun: exposure (brightness)
/// everywhere, the quality effects on the world cameras, shadows on the sun.
fn apply_video(
    video: Res<ActiveVideo>,
    look: Res<ActiveLook>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut cameras: Query<(Entity, &mut ColorGrading, Has<ViewmodelCamera>), With<Camera3d>>,
    mut suns: Query<&mut DirectionalLight, With<Sun>>,
    mut commands: Commands,
    mut applied: Local<Option<Quality>>,
) {
    let video = &video.0;
    let exposure = look.0.exposure + video.exposure_offset();
    // Cameras keep their effects through a brightness drag; only a quality change touches them.
    let quality_changed = *applied != Some(video.quality);
    *applied = Some(video.quality);
    for (entity, mut grading, viewmodel) in &mut cameras {
        if grading.global.exposure != exposure {
            grading.global.exposure = exposure;
        }
        if !viewmodel && quality_changed {
            set_quality(&mut commands.entity(entity), &video.quality);
        }
    }
    let size = video.quality.shadows.map_size();
    if shadow_map.size != size {
        shadow_map.size = size;
    }
    let shadows = video.quality.shadows.enabled();
    for mut sun in &mut suns {
        if sun.shadow_maps_enabled != shadows {
            sun.shadow_maps_enabled = shadows;
        }
    }
}

/// Applies the ready Map's look: sky, ambient, a sun that goes with the Map, and every camera's
/// grade, fog, sky and vignette.
fn apply_map_look(
    add: On<Add<MapReady>>,
    maps: Query<&MapReady>,
    looks: Res<Looks>,
    look_override: Res<LookOverride>,
    video: Res<ActiveVideo>,
    mut active: ResMut<ActiveLook>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<(Entity, Has<ViewmodelCamera>), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok(map) = maps.get(add.entity) else {
        return;
    };
    let name = look_override
        .0
        .as_deref()
        .or(map.look.as_deref())
        .unwrap_or(DEFAULT_LOOK);
    let (name, look) = match looks.0.get(name) {
        Some(look) => (name, look),
        None => {
            warn!(
                "Map \"{}\" names unknown look \"{name}\"; using \"{DEFAULT_LOOK}\"",
                map.name
            );
            (DEFAULT_LOOK, &looks.0[DEFAULT_LOOK])
        }
    };
    info!("Look \"{name}\" for Map \"{}\"", map.name);
    active.0 = look.clone();
    commands.insert_resource(ClearColor(srgb(look.fog_color)));
    commands.insert_resource(GlobalAmbientLight {
        color: srgb(look.ambient_color),
        brightness: look.ambient_brightness * (1.0 - ENVIRONMENT_SHARE),
        ..default()
    });
    let (sun, transform) = look.sun(video.0.quality.shadows.enabled());
    commands.spawn((Name::new("Sun"), Sun, sun, transform, ChildOf(add.entity)));
    let sky = look.environment(&mut images);
    let grading = look.color_grading(video.0.exposure_offset());
    for (entity, viewmodel) in &cameras {
        let mut camera = commands.entity(entity);
        camera.insert(grading.clone());
        if !viewmodel {
            camera.insert((look.fog(), look.vignette(), sky.skybox(), sky.light.clone()));
        }
    }
    commands.insert_resource(sky);
}

/// Name bevy_trenchbroom gives a `light_point` whose `targetname` is `practical`.
const PRACTICAL_LIGHT_NAME: &str = "light_point (practical)";

/// A Map's practical light and the intensity it was authored with.
#[derive(Component)]
struct PracticalLight {
    lumens: f32,
}

/// Scales the Map's practical point lights (`targetname` `practical`) by the look's
/// `practical_lights`: they take it when they appear and again whenever the look changes.
fn scale_practical_lights(
    look: Res<ActiveLook>,
    mut lights: Query<(Entity, &Name, &mut PointLight, Option<&PracticalLight>)>,
    mut commands: Commands,
) {
    for (entity, name, mut light, practical) in &mut lights {
        let lumens = match practical {
            Some(practical) if look.is_changed() => practical.lumens,
            Some(_) => continue,
            None if name.as_str() == PRACTICAL_LIGHT_NAME => {
                let lumens = light.intensity;
                commands.entity(entity).insert(PracticalLight { lumens });
                lumens
            }
            None => continue,
        };
        light.intensity = lumens * look.0.practical_lights;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_looks_parse_and_include_the_presets() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        for name in [
            DEFAULT_LOOK,
            "overcast_urban",
            "desert_dust",
            "dawn_cold",
            "jungle_humid",
            "night_ops",
        ] {
            let look = &looks[name];
            assert!(
                look.fog_visibility > 0.0 && look.sun_illuminance > 0.0,
                "{name}"
            );
            assert!(
                Vec3::from(look.sun_direction).y < 0.0,
                "{name}: sun must shine down"
            );
        }
    }

    #[test]
    fn looks_have_a_sane_sky_ambient_and_vignette() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        for (name, look) in &looks {
            let unit = |c: Rgb| [c.0, c.1, c.2].iter().all(|v| (0.0..=1.0).contains(v));
            assert!(
                unit(look.sky_zenith) && unit(look.ambient_ground) && unit(look.fog_color),
                "{name}: colours are sRGB 0..1"
            );
            assert!((0.0..=1.0).contains(&look.sky_clouds), "{name}");
            assert!((0.0..=1.0).contains(&look.vignette), "{name}");
            assert!(look.ambient_brightness > 0.0, "{name}");
        }
    }

    #[test]
    fn the_sky_meets_the_fog_at_the_horizon_and_the_sun_is_where_the_light_comes_from() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        let look = &looks["desert_dust"];
        let sky = look.sky();
        let fog = Color::srgb(look.fog_color.0, look.fog_color.1, look.fog_color.2).to_linear();
        assert!((sky.horizon - Vec3::new(fog.red, fog.green, fog.blue)).length() < 1e-5);
        // The light travels down and away from the sun, so the sun stands up and opposite.
        assert!(sky.to_sun.y > 0.0);
        assert!(sky.to_sun.dot(Vec3::from(look.sun_direction).normalize()) < -0.99);
    }

    #[test]
    fn only_night_looks_switch_the_practical_lights_on() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        for (name, look) in &looks {
            assert_eq!(look.practical_lights > 0.0, name == "night_ops", "{name}");
        }
    }

    #[test]
    fn preset_drives_the_grade() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        let look = &looks["desert_dust"];
        let grading = look.color_grading(0.0);
        assert_eq!(grading.global.exposure, look.exposure);
        assert_eq!(grading.global.post_saturation, look.saturation);
        assert_eq!(grading.midtones.contrast, look.contrast);
    }

    #[test]
    fn brightness_offsets_the_looks_exposure() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        let look = &looks["night_ops"];
        let brighter = look.color_grading(1.0);
        assert_eq!(brighter.global.exposure, look.exposure + 1.0);
        assert_eq!(brighter.global.temperature, look.temperature);
    }
}
