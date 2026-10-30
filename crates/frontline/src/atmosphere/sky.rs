//! The procedural sky of a look: a cubemap (gradient from horizon haze to zenith, a sun glow and
//! disc where the Map's sun stands, and noise clouds) drawn as the world camera's `Skybox`.
//! Generated on the CPU when a Map's look is applied; nothing is shipped or loaded.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

/// Edge length of a cubemap face. Sky detail is soft, so this is plenty.
const FACE_SIZE: u32 = 256;

/// What the sky of one look is made of. Colours are linear and in the units fog uses, so the
/// horizon meets the distance fog exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyParams {
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    /// Unit vector from the viewer towards the sun.
    pub to_sun: Vec3,
    pub sun_color: Vec3,
    /// Cloud cover, 0 (clear) to 1 (overcast).
    pub clouds: f32,
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 0x0100_0000 as f32
}

fn value_noise(p: Vec2) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (x, y) = (cell.x as i32, cell.y as i32);
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let top = hash(x, y).lerp(hash(x + 1, y), u.x);
    let bottom = hash(x, y + 1).lerp(hash(x + 1, y + 1), u.x);
    top.lerp(bottom, u.y)
}

pub(crate) fn fbm(mut p: Vec2) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 0.5, 0.0);
    for _ in 0..5 {
        sum += amplitude * value_noise(p);
        total += amplitude;
        p = p * 2.03 + Vec2::new(17.1, 9.7);
        amplitude *= 0.5;
    }
    sum / total
}

/// The sky's linear colour looking along unit vector `dir` (y is up).
pub fn sky_color(sky: &SkyParams, dir: Vec3) -> Vec3 {
    let height = dir.y;
    let mut color = if height >= 0.0 {
        // Warm haze and blue sky blend through a muddy purple; greying the middle of the
        // gradient keeps it reading as a hazy sky.
        let t = height.powf(0.4);
        let blend = sky.horizon.lerp(sky.zenith, t);
        let grey = Vec3::splat(blend.dot(Vec3::new(0.2126, 0.7152, 0.0722)));
        blend.lerp(grey, 0.5 * (t * std::f32::consts::PI).sin())
    } else {
        sky.horizon.lerp(sky.ground, (-height * 5.0).min(1.0))
    };

    let sun = dir.dot(sky.to_sun).max(0.0);
    // Wide haze glow around the sun, a tight one, then the disc itself.
    let glow = sun.powi(5) * 0.25 + sun.powi(48) * 0.6;
    color += sky.sun_color * glow;
    color += sky.sun_color * 14.0 * smoothstep(0.9997, 0.99985, sun);

    if sky.clouds > 0.0 && height > 0.0 {
        let plane = Vec2::new(dir.x, dir.z) / (height + 0.14) * 1.7;
        let density_noise = fbm(plane);
        let cover = sky.clouds;
        let density = smoothstep(0.0, 1.0, (density_noise + cover - 0.62) / 0.34);
        // Lit from the sun's side: compare density a step towards the sun.
        let towards_sun = Vec2::new(sky.to_sun.x, sky.to_sun.z).normalize_or_zero() * 0.12;
        let lit = (density_noise - fbm(plane + towards_sun)).clamp(-0.3, 0.3) * 1.6;
        let base = sky.horizon.lerp(Vec3::ONE * sky.horizon.max_element().max(0.3), 0.45);
        let shade = (0.78 + lit - 0.22 * density_noise).clamp(0.35, 1.25);
        let cloud = base * shade + sky.sun_color * sun.powi(8) * 0.25;
        // Clouds thin into the haze at the horizon.
        let visible = density * smoothstep(0.0, 0.22, height);
        color = color.lerp(cloud, visible * (0.55 + 0.4 * cover));
    }
    color
}

/// The direction (in the cubemap's own, left-handed sample space) of texel `(x, y)` of `face`
/// (+X, -X, +Y, -Y, +Z, -Z).
fn sample_direction(face: usize, x: u32, y: u32) -> Vec3 {
    let u = (x as f32 + 0.5) / FACE_SIZE as f32 * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / FACE_SIZE as f32 * 2.0 - 1.0;
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// `f32` as IEEE half-float bits (round to nearest; overflow clamps, tiny values flush to 0).
fn f16_bits(value: f32) -> u16 {
    let bits = value.clamp(0.0, 65504.0).to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exponent <= 0 {
        return 0;
    }
    let mantissa = bits & 0x007f_ffff;
    let half = ((exponent as u32) << 10) | (mantissa >> 13);
    // Round to nearest even; a mantissa carry correctly bumps the exponent.
    let round = (mantissa >> 12) & 1;
    (half + round).min(0x7bff) as u16
}

/// The sky as a cubemap image for `Skybox`. Bevy samples the cube with its z flipped (left-handed
/// cubemaps), so texel directions are mirrored back to world space here.
pub fn cubemap(sky: &SkyParams) -> Image {
    let face_texels = (FACE_SIZE * FACE_SIZE) as usize;
    let mut data = Vec::with_capacity(6 * face_texels * 8);
    for face in 0..6 {
        for y in 0..FACE_SIZE {
            for x in 0..FACE_SIZE {
                let sample = sample_direction(face, x, y);
                let world = Vec3::new(sample.x, sample.y, -sample.z);
                let color = sky_color(sky, world);
                for value in [color.x, color.y, color.z, 1.0] {
                    data.extend_from_slice(&f16_bits(value).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: FACE_SIZE,
            height: FACE_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_day() -> SkyParams {
        SkyParams {
            zenith: Vec3::new(0.1, 0.3, 0.8),
            horizon: Vec3::new(0.6, 0.7, 0.8),
            ground: Vec3::splat(0.2),
            to_sun: Vec3::new(0.5, 0.7, 0.5).normalize(),
            sun_color: Vec3::new(1.0, 0.9, 0.7),
            clouds: 0.0,
        }
    }

    #[test]
    fn half_float_bits_match_known_values() {
        assert_eq!(f16_bits(0.0), 0x0000);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(2.0), 0x4000);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(1.5), 0x3e00);
        assert_eq!(f16_bits(1.0e9), 0x7bff);
    }

    #[test]
    fn the_sky_goes_from_horizon_haze_to_zenith_and_the_sun_is_brightest() {
        let sky = clear_day();
        let horizon = sky_color(&sky, Vec3::new(-0.7, 0.0, 0.7).normalize());
        let zenith = sky_color(&sky, Vec3::Y);
        assert!((horizon - sky.horizon).length() < 0.05, "{horizon}");
        assert!((zenith - sky.zenith).length() < 0.15, "{zenith}");
        assert!(sky_color(&sky, sky.to_sun).max_element() > 5.0);
    }

    #[test]
    fn every_texel_direction_stays_on_its_cube_face() {
        for face in 0..6 {
            let axis = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z][face];
            for (x, y) in [(0, 0), (FACE_SIZE - 1, 0), (FACE_SIZE / 2, FACE_SIZE / 2)] {
                assert!(sample_direction(face, x, y).dot(axis) > 0.55, "face {face}");
            }
        }
    }

    #[test]
    fn clouds_change_the_sky_overhead_but_not_the_horizon() {
        let clear = clear_day();
        let cloudy = SkyParams {
            clouds: 1.0,
            ..clear
        };
        let up = Vec3::new(0.3, 0.9, 0.1).normalize();
        assert!((sky_color(&clear, up) - sky_color(&cloudy, up)).length() > 0.05);
        let flat = Vec3::new(1.0, 0.0, 0.0);
        assert!((sky_color(&clear, flat) - sky_color(&cloudy, flat)).length() < 1e-4);
    }
}
