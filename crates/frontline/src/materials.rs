//! The material library: one tileable PBR texture set (CC0, see `assets/textures/CREDITS.md`)
//! per [`SurfaceKind`], drawn with a tinted material that Map faces use (`maps`). A surface's
//! textures load when the first Map that uses it does, and go with the last one, so a Map only
//! pays for the surfaces on its walls. Client/Host only; a Dedicated Server has no textures or
//! materials.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::assets::EMBEDDED_ROOT;

/// Declares the surfaces: one line each with the folder under `assets/textures/`, the metres one
/// repeat covers (horizontal, vertical), the image size in pixels and the tint multiplied into
/// the colour texture (a muted palette that the Map looks then grade).
macro_rules! surfaces {
    ($(
        $(#[$doc:meta])*
        $variant:ident = $name:literal, tile ($u:literal, $v:literal), px ($w:literal, $h:literal),
        tint ($r:literal, $g:literal, $b:literal);
    )*) => {
        /// What a surface is made of; picks its texture set.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum SurfaceKind {
            $($(#[$doc])* $variant,)*
        }

        impl SurfaceKind {
            /// In declaration order.
            pub const ALL: &'static [SurfaceKind] = &[$(SurfaceKind::$variant,)*];

            /// Canonical lowercase name; also the texture folder under `assets/textures/`.
            pub const fn name(self) -> &'static str {
                match self {
                    $(SurfaceKind::$variant => $name,)*
                }
            }

            /// Metres one repeat of the texture covers (horizontal, vertical). `tools/mapgen`
            /// and MapEdit bake it into the faces' scale; `SURFACES` in `mapgen.py` must match
            /// (a test compares them).
            pub const fn tile(self) -> (f64, f64) {
                match self {
                    $(SurfaceKind::$variant => ($u, $v),)*
                }
            }

            /// Size of the colour image in pixels.
            pub const fn image_px(self) -> (u32, u32) {
                match self {
                    $(SurfaceKind::$variant => ($w, $h),)*
                }
            }

            fn tint(self) -> Color {
                match self {
                    $(SurfaceKind::$variant => Color::srgb($r, $g, $b),)*
                }
            }
        }
    };
}

surfaces! {
    Concrete = "concrete", tile (2.2, 1.1), px (1024, 512), tint (1.0, 0.99, 0.96);
    /// Khaki painted plaster.
    Plaster = "plaster", tile (2.0, 1.0), px (1024, 512), tint (0.90, 0.84, 0.72);
    /// Painted sheet metal, olive drab.
    Metal = "metal", tile (2.0, 2.0), px (1024, 1024), tint (0.50, 0.56, 0.46);
    /// Corrugated steel, e.g. shipping containers.
    Corrugated = "corrugated", tile (2.0, 2.0), px (1024, 1024), tint (0.72, 0.40, 0.28);
    /// Plywood, e.g. crates.
    Wood = "wood", tile (1.0, 1.0), px (1024, 1024), tint (1.0, 0.94, 0.86);
    /// Dry stony earth.
    Ground = "ground", tile (4.0, 4.0), px (1024, 1024), tint (0.96, 0.93, 0.88);
    Brick = "brick", tile (2.4, 1.2), px (1024, 512), tint (0.76, 0.69, 0.65);
    /// Lawns, hedges and foliage.
    Grass = "grass", tile (3.0, 3.0), px (1024, 1024), tint (0.92, 0.98, 0.86);

    // Walls: weathered paint over plaster, the brick showing through where it has come off.
    PlasterYellow = "plaster_yellow", tile (2.0, 2.0), px (1024, 1024), tint (0.90, 0.88, 0.84);
    PlasterOchre = "plaster_ochre", tile (2.0, 2.0), px (1024, 1024), tint (0.88, 0.84, 0.78);
    PlasterBlue = "plaster_blue", tile (2.0, 2.0), px (1024, 1024), tint (0.95, 0.95, 0.94);
    PlasterWhite = "plaster_white", tile (2.0, 2.0), px (1024, 1024), tint (0.96, 0.94, 0.90);
    PlasterGreen = "plaster_green", tile (2.0, 2.0), px (1024, 1024), tint (0.96, 0.96, 0.92);
    /// Old, dirty red brick with wide mortar.
    BrickOld = "brick_old", tile (1.7, 1.7), px (1024, 1024), tint (0.92, 0.88, 0.84);
    /// Perforated clay blocks: unfinished and cheap construction.
    BrickHollow = "brick_hollow", tile (2.0, 2.0), px (1024, 1024), tint (0.86, 0.80, 0.74);
    /// Sandstone blocks.
    Stone = "stone", tile (2.2, 2.2), px (1024, 1024), tint (0.96, 0.94, 0.90);
    /// Dark, stained, broken concrete.
    ConcreteDamaged = "concrete_damaged", tile (3.0, 3.0), px (1024, 1024), tint (1.0, 0.99, 0.96);
    /// Peeling blue paint on boards: doors, shutters, fences.
    WoodPainted = "wood_painted", tile (1.5, 1.5), px (1024, 1024), tint (0.95, 0.95, 0.95);
    /// Grey weathered horizontal boards.
    WoodSiding = "wood_siding", tile (1.4, 1.4), px (1024, 1024), tint (1.0, 0.98, 0.94);

    // Floors and ground.
    Asphalt = "asphalt", tile (5.0, 5.0), px (1024, 1024), tint (1.0, 1.0, 1.0);
    Sand = "sand", tile (3.5, 3.5), px (1024, 1024), tint (0.96, 0.93, 0.88);
    Gravel = "gravel", tile (1.6, 1.6), px (1024, 1024), tint (0.92, 0.90, 0.86);
    /// Grey concrete pavers.
    Paving = "paving", tile (2.0, 2.0), px (1024, 1024), tint (0.76, 0.76, 0.74);
    /// Clinker bricks laid as a street.
    PavingBrick = "paving_brick", tile (1.3, 1.3), px (1024, 1024), tint (0.92, 0.90, 0.88);
    /// Dirty white floor tiles.
    Tiles = "tiles", tile (2.0, 2.0), px (1024, 1024), tint (0.90, 0.89, 0.86);
    /// Dark, worn floorboards.
    WoodFloor = "wood_floor", tile (1.9, 1.9), px (1024, 1024), tint (1.0, 0.97, 0.92);

    // Roofs.
    RoofTiles = "roof_tiles", tile (2.9, 2.9), px (1024, 1024), tint (0.90, 0.86, 0.82);
    RoofSlate = "roof_slate", tile (2.9, 2.9), px (1024, 1024), tint (1.0, 1.0, 1.0);

    // Metal.
    /// Rusted steel plates.
    MetalRust = "metal_rust", tile (1.6, 1.6), px (1024, 1024), tint (0.90, 0.88, 0.86);
    /// Green paint flaking to rust: gates, doors, dumpsters.
    MetalGreen = "metal_green", tile (2.0, 2.0), px (1024, 1024), tint (0.82, 0.84, 0.80);
    /// White paint flaking to rust.
    MetalWhite = "metal_white", tile (2.0, 2.0), px (1024, 1024), tint (0.92, 0.91, 0.88);
    CorrugatedRust = "corrugated_rust", tile (2.0, 2.0), px (1024, 1024), tint (0.92, 0.90, 0.88);
    CorrugatedBlue = "corrugated_blue", tile (2.0, 2.0), px (1024, 1024), tint (0.90, 0.92, 0.92);
}

impl SurfaceKind {
    /// The surface a map texture name refers to. A path segment that is a surface's own name
    /// (`"plaster_blue/color"`, the names `tools/mapgen` and MapEdit write) picks it; otherwise
    /// a keyword in the segment picks one of the base surfaces (`"walls/brick_red"`,
    /// `"crate_plywood"`). The last segment naming one wins, case-insensitively.
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
            ("grass", SurfaceKind::Grass),
            ("lawn", SurfaceKind::Grass),
        ];
        name.rsplit(['/', '\\']).find_map(|segment| {
            let segment = segment.to_ascii_lowercase();
            SurfaceKind::ALL
                .iter()
                .copied()
                .find(|kind| kind.name() == segment)
                .or_else(|| {
                    KEYWORDS
                        .iter()
                        .find(|(keyword, _)| segment.contains(keyword))
                        .map(|&(_, kind)| kind)
                })
        })
    }

    /// [`SurfaceKind::tile`] as a vector. Replacement textures rescale their UVs against it.
    pub fn tile_metres(self) -> Vec2 {
        let (u, v) = self.tile();
        Vec2::new(u as f32, v as f32)
    }

    /// The material of this surface, tinted with its palette colour. `load` loads one of its
    /// texture files: the asset path and whether it holds colour (sRGB) or data.
    pub fn material(self, mut load: impl FnMut(String, bool) -> Handle<Image>) -> StandardMaterial {
        StandardMaterial {
            base_color: self.tint(),
            base_color_texture: Some(load(self.texture_path("color"), true)),
            normal_map_texture: Some(load(self.texture_path("normal"), false)),
            // Greyscale roughness maps expand to RGBA: green drives roughness. Blue would drive
            // metallic, but every surface is dielectric (paint and rust, not bare metal), so
            // it's zeroed.
            metallic_roughness_texture: Some(load(self.texture_path("roughness"), false)),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.3,
            ..default()
        }
    }

    fn texture_path(self, map: &str) -> String {
        format!("{EMBEDDED_ROOT}textures/{}/{map}.jpg", self.name())
    }
}

/// How a surface texture is read: colour as sRGB, normal and roughness as data; tiling.
pub fn surface_image_settings(srgb: bool) -> impl Fn(&mut ImageLoaderSettings) + Send + Sync {
    move |settings: &mut ImageLoaderSettings| {
        settings.is_srgb = srgb;
        settings.sampler = ImageSampler::Descriptor(repeat_sampler());
    }
}

/// Present in a process that draws surfaces (Client/Host, MapEdit): Maps then give their faces
/// the surfaces' materials instead of one plain one.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SurfaceLibrary;

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

/// Marks the process as one that draws surfaces and gives their textures mipmaps. Client/Host
/// only.
pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceLibrary>()
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
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = *event else {
            continue;
        };
        let ours = server
            .get_path(id)
            .is_some_and(|path| is_surface_texture(&path.to_string()));
        if !ours {
            continue;
        }
        if let Some(mut image) = images.get_mut(id) {
            generate_mips(&mut image);
            image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        }
    }
}

/// Whether an asset path is one of the embedded surface textures.
fn is_surface_texture(path: &str) -> bool {
    path.strip_prefix(EMBEDDED_ROOT)
        .is_some_and(|rest| rest.starts_with("textures/"))
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
    fn texture_names_map_to_surfaces() {
        let cases = [
            ("concrete", Some(SurfaceKind::Concrete)),
            ("walls/Brick_Red", Some(SurfaceKind::Brick)),
            ("corrugated_metal_blue", Some(SurfaceKind::Corrugated)),
            ("painted_metal", Some(SurfaceKind::Metal)),
            ("crate_plywood", Some(SurfaceKind::Wood)),
            ("ground/dirt01", Some(SurfaceKind::Ground)),
            ("grass/color", Some(SurfaceKind::Grass)),
            ("walls/plaster_cream", Some(SurfaceKind::Plaster)),
            ("__TB_empty", None),
            // TrenchBroom names: a folder per surface, `color` the map to paint with.
            ("concrete/color", Some(SurfaceKind::Concrete)),
            ("textures/brick/color", Some(SurfaceKind::Brick)),
            // A surface's own name beats the keywords inside it.
            ("plaster_blue/color", Some(SurfaceKind::PlasterBlue)),
            ("Brick_Old/color", Some(SurfaceKind::BrickOld)),
            ("concrete_damaged/color", Some(SurfaceKind::ConcreteDamaged)),
            ("sand/color", Some(SurfaceKind::Sand)),
            ("corrugated_rust/color", Some(SurfaceKind::CorrugatedRust)),
        ];
        for (name, kind) in cases {
            assert_eq!(SurfaceKind::from_texture_name(name), kind, "{name}");
        }
        for &kind in SurfaceKind::ALL {
            assert_eq!(SurfaceKind::from_texture_name(kind.name()), Some(kind));
            let texture = format!("{}/color", kind.name());
            assert_eq!(SurfaceKind::from_texture_name(&texture), Some(kind));
        }
    }

    /// `tools/mapgen/mapgen.py` bakes the tile and image sizes into every face it writes: its
    /// table must list the same surfaces with the same numbers.
    #[test]
    fn mapgen_lists_the_same_surfaces_and_sizes() {
        let source = include_str!("../../../tools/mapgen/mapgen.py");
        let table = source
            .split_once("SURFACES = {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .map(|(table, _)| table)
            .expect("SURFACES table in mapgen.py");
        let mut listed = Vec::new();
        for line in table.lines().map(str::trim) {
            let Some((name, numbers)) = line.strip_prefix('"').and_then(|l| l.split_once("\":"))
            else {
                continue;
            };
            let numbers: Vec<f64> = numbers
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().unwrap())
                .collect();
            listed.push((name.to_owned(), numbers));
        }
        let ours: Vec<(String, Vec<f64>)> = SurfaceKind::ALL
            .iter()
            .map(|kind| {
                let ((u, v), (w, h)) = (kind.tile(), kind.image_px());
                (
                    kind.name().to_owned(),
                    vec![u, v, f64::from(w), f64::from(h)],
                )
            })
            .collect();
        assert_eq!(listed, ours);
    }

    #[test]
    fn only_embedded_surface_textures_get_surface_mips() {
        assert!(is_surface_texture(
            "embedded://frontline/textures/sand/normal.jpg"
        ));
        assert!(!is_surface_texture("embedded://frontline/camos/gold.png"));
        assert!(!is_surface_texture("textures/sand/color.jpg"));
    }

    /// A surface missing from `assets::EMBEDDED_SURFACES` would draw untextured in a shipped
    /// build.
    #[test]
    fn every_surface_has_its_three_textures_embedded() {
        for kind in SurfaceKind::ALL {
            for map in ["color", "normal", "roughness"] {
                let path = format!("textures/{}/{map}.jpg", kind.name());
                assert!(crate::assets::is_embedded(&path), "{path} is not embedded");
            }
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
