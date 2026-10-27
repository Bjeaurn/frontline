//! Camera look and per-Map mood. Fixed for every Map: TonyMcMapface tonemapping (HDR), mild
//! bloom and SSAO, added to every world-rendering `Camera3d` (gameplay eye, main menu orbit) when
//! it spawns. Per Map: a look preset from `data/looks.ron` (grade, fog, sun, ambient, sky),
//! applied when the Map is ready. The viewmodel camera only shares the tone curve and grade (see
//! `weapon::viewmodel`). Client/Host only.
//!
//! SSAO (Medium) needs `Msaa::Off`, so world cameras anti-alias with FXAA instead: the cheapest
//! option, where TAA would also need motion vectors and ghosts on fast flicks.

use std::collections::HashMap;

use bevy::anti_alias::fxaa::Fxaa;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{
    DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use serde::Deserialize;

use crate::maps::MapReady;
use crate::weapon::ViewmodelCamera;

const LOOKS_FILE: &str = "looks.ron";
const BUILTIN_LOOKS: &str = include_str!("../../../data/looks.ron");

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
    pub sky_color: Rgb,
}

fn srgb((r, g, b): Rgb) -> Color {
    Color::srgb(r, g, b)
}

impl LookPreset {
    pub fn color_grading(&self) -> ColorGrading {
        ColorGrading {
            global: ColorGradingGlobal {
                exposure: self.exposure,
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

    fn sun(&self) -> (DirectionalLight, Transform) {
        let light = DirectionalLight {
            color: srgb(self.sun_color),
            illuminance: self.sun_illuminance,
            shadow_maps_enabled: true,
            ..default()
        };
        let direction = Vec3::from(self.sun_direction);
        (light, Transform::default().looking_to(direction, Vec3::Y))
    }
}

/// Every look preset by name, from `data/looks.ron`.
#[derive(Resource, Debug)]
struct Looks(HashMap<String, LookPreset>);

/// The look in effect; cameras spawned later pick it up.
#[derive(Resource, Debug)]
struct ActiveLook(LookPreset);

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        let looks: HashMap<String, LookPreset> = crate::data::load(LOOKS_FILE, BUILTIN_LOOKS);
        let Some(default_look) = looks.get(DEFAULT_LOOK).cloned() else {
            panic!("{LOOKS_FILE} must define the \"{DEFAULT_LOOK}\" look");
        };
        app.insert_resource(Looks(looks))
            .insert_resource(ActiveLook(default_look))
            .add_observer(add_atmosphere)
            .add_observer(apply_map_look);
    }
}

fn add_atmosphere(
    add: On<Add<Camera3d>>,
    viewmodels: Query<(), With<ViewmodelCamera>>,
    look: Res<ActiveLook>,
    mut commands: Commands,
) {
    let grading = look.0.color_grading();
    if viewmodels.contains(add.entity) {
        commands.entity(add.entity).insert(grading);
        return;
    }
    commands.entity(add.entity).insert((
        TONEMAPPING,
        grading,
        Bloom {
            intensity: 0.08,
            ..Bloom::NATURAL
        },
        Msaa::Off,
        Fxaa::default(),
        ScreenSpaceAmbientOcclusion {
            quality_level: ScreenSpaceAmbientOcclusionQualityLevel::Medium,
            ..default()
        },
        look.0.fog(),
    ));
}

/// Applies the ready Map's look: sky, ambient, a sun that goes with the Map, and every camera's
/// grade and fog.
fn apply_map_look(
    add: On<Add<MapReady>>,
    maps: Query<&MapReady>,
    looks: Res<Looks>,
    mut active: ResMut<ActiveLook>,
    mut cameras: Query<(&mut ColorGrading, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok(map) = maps.get(add.entity) else {
        return;
    };
    let name = map.look.as_deref().unwrap_or(DEFAULT_LOOK);
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
    commands.insert_resource(ClearColor(srgb(look.sky_color)));
    commands.insert_resource(GlobalAmbientLight {
        color: srgb(look.ambient_color),
        brightness: look.ambient_brightness,
        ..default()
    });
    let (sun, transform) = look.sun();
    commands.spawn((Name::new("Sun"), sun, transform, ChildOf(add.entity)));
    for (mut grading, fog) in &mut cameras {
        *grading = look.color_grading();
        if let Some(mut fog) = fog {
            *fog = look.fog();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_looks_parse_and_include_the_presets() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        for name in [DEFAULT_LOOK, "overcast_urban", "desert_dust", "dawn_cold"] {
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
    fn preset_drives_the_grade() {
        let looks: HashMap<String, LookPreset> = ron::from_str(BUILTIN_LOOKS).unwrap();
        let look = &looks["desert_dust"];
        let grading = look.color_grading();
        assert_eq!(grading.global.exposure, look.exposure);
        assert_eq!(grading.global.post_saturation, look.saturation);
        assert_eq!(grading.midtones.contrast, look.contrast);
    }
}
