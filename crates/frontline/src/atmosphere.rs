//! Camera look: TonyMcMapface tonemapping (HDR), mild bloom, SSAO, a light warm haze and a
//! slightly warm, less saturated grade. Added to every world-rendering `Camera3d` (gameplay eye,
//! main menu orbit) when it spawns; the viewmodel camera only shares the tone curve and grade
//! (see `weapon::viewmodel`). Client/Host only.
//!
//! SSAO (Medium) needs `Msaa::Off`, so world cameras anti-alias with FXAA instead: the cheapest
//! option, where TAA would also need motion vectors and ghosts on fast flicks.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{
    DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};

use crate::weapon::ViewmodelCamera;

/// Tone curve shared by every camera, so the gun matches the world.
pub const TONEMAPPING: Tonemapping = Tonemapping::TonyMcMapface;

/// Slightly warmer, slightly less saturated, a touch more midtone contrast.
pub fn color_grading() -> ColorGrading {
    ColorGrading {
        global: ColorGradingGlobal {
            temperature: 0.01,
            post_saturation: 0.9,
            ..default()
        },
        midtones: ColorGradingSection {
            contrast: 1.06,
            ..default()
        },
        ..default()
    }
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(add_atmosphere);
    }
}

fn add_atmosphere(
    add: On<Add<Camera3d>>,
    viewmodels: Query<(), With<ViewmodelCamera>>,
    mut commands: Commands,
) {
    if viewmodels.contains(add.entity) {
        return;
    }
    commands.entity(add.entity).insert((
        TONEMAPPING,
        color_grading(),
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
        DistanceFog {
            color: Color::srgb(0.80, 0.76, 0.68),
            directional_light_color: Color::srgba(1.0, 0.86, 0.66, 0.35),
            directional_light_exponent: 24.0,
            // Clear at fighting range, hazy across the arena.
            falloff: FogFalloff::from_visibility_squared(350.0),
        },
    ));
}
