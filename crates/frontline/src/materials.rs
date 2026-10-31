//! The material library: one tileable PBR texture set (CC0, see `assets/textures/CREDITS.md`)
//! per [`SurfaceKind`], each with one shared, tinted material that Map faces use (`maps`).
//! Client/Host only; a Dedicated Server has no textures or materials.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::assets::EMBEDDED_ROOT;

/// What a surface is made of; picks its texture set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    Concrete,
    Plaster,
    /// Painted sheet metal.
    Metal,
    /// Corrugated steel, e.g. shipping containers.
    Corrugated,
    /// Plywood, e.g. crates.
    Wood,
    /// Dirt and gravel.
    Ground,
    Brick,
}

impl SurfaceKind {
    /// In declaration order, so `kind as usize` indexes it.
    pub const ALL: [SurfaceKind; 7] = [
        SurfaceKind::Concrete,
        SurfaceKind::Plaster,
        SurfaceKind::Metal,
        SurfaceKind::Corrugated,
        SurfaceKind::Wood,
        SurfaceKind::Ground,
        SurfaceKind::Brick,
    ];

    /// Canonical lowercase name; also the texture folder under `assets/textures/`.
    pub fn name(self) -> &'static str {
        match self {
            SurfaceKind::Concrete => "concrete",
            SurfaceKind::Plaster => "plaster",
            SurfaceKind::Metal => "metal",
            SurfaceKind::Corrugated => "corrugated",
            SurfaceKind::Wood => "wood",
            SurfaceKind::Ground => "ground",
            SurfaceKind::Brick => "brick",
        }
    }

    /// The kind a map texture name refers to, e.g. `"concrete/color"`, `"walls/brick_red"` or
    /// `"crate_plywood"`: the last path segment naming a kind wins, matched case-insensitively
    /// by keyword.
    pub fn from_texture_name(name: &str) -> Option<SurfaceKind> {
        const KEYWORDS: &[(&str, SurfaceKind)] = &[
            // Before "metal", which "corrugated metal" names also contain.
            ("corrugated", SurfaceKind::Corrugated),
            ("container", SurfaceKind::Corrugated),
            ("concrete", SurfaceKind::Concrete),
            ("plaster", SurfaceKind::Plaster),
            ("stucco", SurfaceKind::Plaster),
            ("metal", SurfaceKind::Metal),
            ("steel", SurfaceKind::Metal),
            ("wood", SurfaceKind::Wood),
            ("plank", SurfaceKind::Wood),
            ("plywood", SurfaceKind::Wood),
            ("crate", SurfaceKind::Wood),
            ("ground", SurfaceKind::Ground),
            ("dirt", SurfaceKind::Ground),
            ("gravel", SurfaceKind::Ground),
            ("sand", SurfaceKind::Ground),
            ("brick", SurfaceKind::Brick),
        ];
        name.rsplit(['/', '\\']).find_map(|segment| {
            let segment = segment.to_ascii_lowercase();
            KEYWORDS
                .iter()
                .find(|(keyword, _)| segment.contains(keyword))
                .map(|&(_, kind)| kind)
        })
    }

    /// Metres one repeat of the CC0 texture covers (horizontal, vertical); must match
    /// `SURFACES` in `tools/mapgen/mapgen.py`, which bakes it into the faces' scale. Replacement
    /// textures rescale their UVs against it.
    pub fn tile_metres(self) -> Vec2 {
        match self {
            SurfaceKind::Concrete => Vec2::new(2.2, 1.1),
            SurfaceKind::Plaster => Vec2::new(2.0, 1.0),
            SurfaceKind::Metal => Vec2::new(2.0, 2.0),
            SurfaceKind::Corrugated => Vec2::new(2.0, 2.0),
            SurfaceKind::Wood => Vec2::new(1.0, 1.0),
            SurfaceKind::Ground => Vec2::new(4.0, 4.0),
            SurfaceKind::Brick => Vec2::new(2.4, 1.2),
        }
    }

    /// Multiplied into the colour texture: a dirty, muted palette (khaki plaster, olive-drab
    /// metal, rusty containers) that the Map looks then grade.
    fn tint(self) -> Color {
        match self {
            SurfaceKind::Concrete => Color::srgb(1.0, 0.99, 0.96),
            SurfaceKind::Plaster => Color::srgb(0.90, 0.84, 0.72),
            SurfaceKind::Metal => Color::srgb(0.50, 0.56, 0.46),
            SurfaceKind::Corrugated => Color::srgb(0.72, 0.40, 0.28),
            SurfaceKind::Wood => Color::srgb(1.0, 0.94, 0.86),
            SurfaceKind::Ground => Color::srgb(0.96, 0.93, 0.88),
            SurfaceKind::Brick => Color::srgb(0.84, 0.72, 0.66),
        }
    }

    fn texture_path(self, map: &str) -> String {
        format!("{EMBEDDED_ROOT}textures/{}/{map}.jpg", self.name())
    }
}

/// The texture set of one [`SurfaceKind`].
#[derive(Clone, Debug)]
pub struct SurfaceTextures {
    pub color: Handle<Image>,
    pub normal: Handle<Image>,
    pub roughness: Handle<Image>,
}

/// Textures and a tinted material per [`SurfaceKind`], loaded once at startup.
#[derive(Resource, Clone, Debug)]
pub struct SurfaceMaterials {
    textures: [SurfaceTextures; 7],
    materials: [Handle<StandardMaterial>; 7],
}

impl SurfaceMaterials {
    pub fn textures(&self, kind: SurfaceKind) -> &SurfaceTextures {
        &self.textures[kind as usize]
    }

    /// The shared material of `kind`, tinted with its palette colour.
    pub fn material(&self, kind: SurfaceKind) -> Handle<StandardMaterial> {
        self.materials[kind as usize].clone()
    }

    /// A new material of `kind` whose textures are multiplied by `tint`.
    fn tinted(&self, kind: SurfaceKind, tint: Color) -> StandardMaterial {
        let textures = self.textures(kind);
        StandardMaterial {
            base_color: tint,
            base_color_texture: Some(textures.color.clone()),
            normal_map_texture: Some(textures.normal.clone()),
            // Greyscale roughness maps expand to RGBA: green drives roughness. Blue would drive
            // metallic, but every kind is dielectric (paint, not bare metal), so it's zeroed.
            metallic_roughness_texture: Some(textures.roughness.clone()),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.3,
            ..default()
        }
    }
}

impl FromWorld for SurfaceMaterials {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>();
        let load = |kind: SurfaceKind, map: &str, srgb: bool| -> Handle<Image> {
            server
                .load_builder()
                .with_settings(move |settings: &mut ImageLoaderSettings| {
                    settings.is_srgb = srgb;
                    settings.sampler = ImageSampler::Descriptor(repeat_sampler());
                })
                .load(kind.texture_path(map))
        };
        let textures = SurfaceKind::ALL.map(|kind| SurfaceTextures {
            color: load(kind, "color", true),
            normal: load(kind, "normal", false),
            roughness: load(kind, "roughness", false),
        });
        let mut library = SurfaceMaterials {
            textures,
            materials: default(),
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        library.materials =
            SurfaceKind::ALL.map(|kind| materials.add(library.tinted(kind, kind.tint())));
        library
    }
}

/// Trilinear, anisotropic and tiling in both directions.
pub(crate) fn repeat_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        label: Some("surface".into()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    }
}

/// Loads the material library and gives its textures mipmaps. Client/Host only.
pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceMaterials>()
            .add_systems(Update, (mip_surface_textures, mip_prop_textures));
    }
}

/// The MegaKit's glTF props (the skyline buildings, facades, street tiles) carry JPEG textures
/// with no mip chain too, so every facade seen from a distance minifies by 4x to 16x without
/// filtering and sparkles when the camera moves. Each texture a material names gets a chain
/// (and trilinear, anisotropic sampling) the first time it shows up.
fn mip_prop_textures(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::LoadedWithDependencies { id }) = *event else {
            continue;
        };
        let Some(material) = materials.get(id) else {
            continue;
        };
        for handle in [
            &material.base_color_texture,
            &material.normal_map_texture,
            &material.metallic_roughness_texture,
            &material.occlusion_texture,
            &material.emissive_texture,
        ]
        .into_iter()
        .flatten()
        {
            let Some(mut image) = images.get_mut(handle.id()) else {
                continue;
            };
            if image.texture_descriptor.mip_level_count != 1 {
                continue;
            }
            generate_mips(&mut image);
            if image.texture_descriptor.mip_level_count == 1 {
                continue; // not a format it can mip
            }
            match &mut image.sampler {
                ImageSampler::Descriptor(sampler) => {
                    sampler.mag_filter = ImageFilterMode::Linear;
                    sampler.min_filter = ImageFilterMode::Linear;
                    sampler.mipmap_filter = ImageFilterMode::Linear;
                    sampler.anisotropy_clamp = 4;
                }
                sampler @ ImageSampler::Default => {
                    *sampler = ImageSampler::Descriptor(repeat_sampler());
                }
            }
            image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        }
    }
}

/// JPEGs carry no mip chain, and unmipped tiling textures shimmer at a distance, so each surface
/// texture gets one generated once it has loaded. The CPU copy is then dropped after upload.
fn mip_surface_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    library: Res<SurfaceMaterials>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = *event else {
            continue;
        };
        let ours = library.textures.iter().any(|set| {
            [&set.color, &set.normal, &set.roughness]
                .into_iter()
                .any(|handle| handle.id() == id)
        });
        if !ours {
            continue;
        }
        if let Some(mut image) = images.get_mut(id) {
            generate_mips(&mut image);
            image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        }
    }
}

/// Appends a box-filtered mip chain to an uncompressed 8-bit RGBA 2D image without one
/// (averaging sRGB colour in linear space).
pub(crate) fn generate_mips(image: &mut Image) {
    let srgb = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        _ => return,
    };
    let size = image.texture_descriptor.size;
    if image.texture_descriptor.mip_level_count != 1 || size.depth_or_array_layers != 1 {
        return;
    }
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let to_linear: [f32; 256] = std::array::from_fn(|i| Srgba::gamma_function(i as f32 / 255.0));
    let (mut width, mut height) = (size.width as usize, size.height as usize);
    data.reserve(data.len() / 3 + 4);
    let mut level_start = 0;
    let mut levels = 1;
    while width > 1 || height > 1 {
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        for y in 0..next_height {
            let rows = [2 * y, (2 * y + 1).min(height - 1)];
            for x in 0..next_width {
                let columns = [2 * x, (2 * x + 1).min(width - 1)];
                for channel in 0..4 {
                    let texel = |row: usize, column: usize| {
                        data[level_start + (row * width + column) * 4 + channel]
                    };
                    let samples = [
                        texel(rows[0], columns[0]),
                        texel(rows[0], columns[1]),
                        texel(rows[1], columns[0]),
                        texel(rows[1], columns[1]),
                    ];
                    let value = if srgb && channel < 3 {
                        let linear = samples.iter().map(|&s| to_linear[s as usize]).sum::<f32>();
                        (Srgba::gamma_function_inverse(linear / 4.0) * 255.0).round() as u8
                    } else {
                        let sum = samples.iter().map(|&s| u32::from(s)).sum::<u32>();
                        ((sum + 2) / 4) as u8
                    };
                    data.push(value);
                }
            }
        }
        level_start += width * height * 4;
        (width, height) = (next_width, next_height);
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_names_map_to_kinds() {
        let cases = [
            ("concrete", Some(SurfaceKind::Concrete)),
            ("walls/Brick_Red", Some(SurfaceKind::Brick)),
            ("corrugated_metal_blue", Some(SurfaceKind::Corrugated)),
            ("painted_metal", Some(SurfaceKind::Metal)),
            ("crate_plywood", Some(SurfaceKind::Wood)),
            ("ground/dirt01", Some(SurfaceKind::Ground)),
            ("plaster_white", Some(SurfaceKind::Plaster)),
            ("__TB_empty", None),
            // TrenchBroom names: folder per kind, `color` the map to paint with.
            ("concrete/color", Some(SurfaceKind::Concrete)),
            ("textures/brick/color", Some(SurfaceKind::Brick)),
        ];
        for (name, kind) in cases {
            assert_eq!(SurfaceKind::from_texture_name(name), kind, "{name}");
        }
        for kind in SurfaceKind::ALL {
            assert_eq!(SurfaceKind::from_texture_name(kind.name()), Some(kind));
        }
    }

    #[test]
    fn mips_halve_down_to_one_texel() {
        let mut image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 4,
                height: 2,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 0, 128, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        generate_mips(&mut image);
        assert_eq!(image.texture_descriptor.mip_level_count, 3);
        let data = image.data.as_ref().unwrap();
        // 4x2 + 2x1 + 1x1 texels.
        assert_eq!(data.len(), (8 + 2 + 1) * 4);
        assert_eq!(&data[data.len() - 4..], &[255, 0, 128, 255]);
    }

    #[test]
    fn prop_textures_get_mips_and_trilinear_sampling() {
        let mut app = App::new();
        app.add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .add_systems(Update, mip_prop_textures);
        let texture = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_fill(
                bevy::render::render_resource::Extent3d {
                    width: 8,
                    height: 8,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                &[200, 100, 50, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ));
        let _material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color_texture: Some(texture.clone()),
                ..default()
            });
        // The asset events are written after `Update`, so the system reads them on the next frame.
        app.update();
        app.update();
        let images = app.world().resource::<Assets<Image>>();
        let image = images.get(&texture).unwrap();
        assert_eq!(image.texture_descriptor.mip_level_count, 4);
        match &image.sampler {
            ImageSampler::Descriptor(sampler) => {
                assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
                assert_eq!(sampler.anisotropy_clamp, 4);
            }
            ImageSampler::Default => panic!("the sampler should be explicit"),
        }
    }
}
