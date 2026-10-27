//! The material library: one tileable PBR texture set (CC0, see `assets/textures/CREDITS.md`)
//! per [`SurfaceKind`], plus [`textured_box`] meshes whose UVs follow world size so textures
//! never stretch. Client/Host only; a Dedicated Server has no textures or materials.

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::mesh::{Indices, PrimitiveTopology};
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

    /// The kind a map texture name refers to, e.g. `"concrete"`, `"walls/brick_red"` or
    /// `"crate_plywood"`: the last path segment, matched case-insensitively by keyword.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "for the TrenchBroom map loader")
    )]
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
        let name = name.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        KEYWORDS
            .iter()
            .find(|(keyword, _)| name.contains(keyword))
            .map(|&(_, kind)| kind)
    }

    /// Metres one texture repeat covers, (horizontal, vertical) on walls.
    fn texture_metres(self) -> Vec2 {
        match self {
            SurfaceKind::Concrete => Vec2::new(2.2, 1.1),
            SurfaceKind::Plaster => Vec2::new(2.0, 1.0),
            SurfaceKind::Metal | SurfaceKind::Corrugated => Vec2::splat(2.0),
            SurfaceKind::Wood => Vec2::splat(1.0),
            SurfaceKind::Ground => Vec2::splat(4.0),
            SurfaceKind::Brick => Vec2::new(2.4, 1.2),
        }
    }

    /// Texture repeats per metre, for [`textured_box`].
    pub fn uv_scale(self) -> Vec2 {
        self.texture_metres().recip()
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

/// Textures and an untinted material per [`SurfaceKind`], loaded once at startup.
#[derive(Resource, Clone, Debug)]
pub struct SurfaceMaterials {
    textures: [SurfaceTextures; 7],
    materials: [Handle<StandardMaterial>; 7],
}

impl SurfaceMaterials {
    pub fn textures(&self, kind: SurfaceKind) -> &SurfaceTextures {
        &self.textures[kind as usize]
    }

    /// The shared, untinted material of `kind`.
    #[expect(dead_code, reason = "for the TrenchBroom map loader")]
    pub fn material(&self, kind: SurfaceKind) -> Handle<StandardMaterial> {
        self.materials[kind as usize].clone()
    }

    /// A new material of `kind` whose textures are multiplied by `tint`.
    pub fn tinted(&self, kind: SurfaceKind, tint: Color) -> StandardMaterial {
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
            reflectance: 0.4,
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
            SurfaceKind::ALL.map(|kind| materials.add(library.tinted(kind, Color::WHITE)));
        library
    }
}

/// Trilinear, anisotropic and tiling in both directions.
fn repeat_sampler() -> ImageSamplerDescriptor {
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
            .add_systems(Update, mip_surface_textures);
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
fn generate_mips(image: &mut Image) {
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

/// A box of full extents `size`, centred on the origin, whose UVs repeat `uv_scale` times per
/// metre (see [`SurfaceKind::uv_scale`]). Side faces keep texture "up" along +Y; with
/// tangents for normal mapping.
pub fn textured_box(size: Vec3, uv_scale: Vec2) -> Mesh {
    // (normal, u direction, v direction); v runs down the texture, so "up" is -v.
    const FACES: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
        (Vec3::Z, Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
    ];
    let half = size / 2.0;
    let extent = |axis: Vec3| axis.abs().dot(size);
    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut uvs = Vec::with_capacity(24);
    let mut tangents = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, u, v) in FACES {
        let base = positions.len() as u32;
        let uv_size = Vec2::new(extent(u), extent(v)) * uv_scale;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let position = (normal + u * su + v * sv) * half;
            positions.push(position.to_array());
            normals.push(normal.to_array());
            uvs.push([(su + 1.0) / 2.0 * uv_size.x, (sv + 1.0) / 2.0 * uv_size.y]);
            // Bitangent = normal x tangent = texture "up", the OpenGL normal-map convention.
            tangents.push(u.extend(1.0).to_array());
        }
        indices.extend([0, 2, 1, 0, 3, 2].map(|i| base + i));
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

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
        ];
        for (name, kind) in cases {
            assert_eq!(SurfaceKind::from_texture_name(name), kind, "{name}");
        }
        for kind in SurfaceKind::ALL {
            assert_eq!(SurfaceKind::from_texture_name(kind.name()), Some(kind));
        }
    }

    #[test]
    fn box_uvs_follow_world_size() {
        let mesh = textured_box(Vec3::new(4.0, 1.0, 0.5), Vec2::splat(0.5));
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("box has no UVs");
        };
        assert_eq!(uvs.len(), 24);
        // +Z face: 4 m wide and 1 m tall at 0.5 repeats per metre.
        let front = &uvs[8..12];
        let max = front
            .iter()
            .fold(Vec2::ZERO, |max, &uv| max.max(Vec2::from(uv)));
        assert_eq!(max, Vec2::new(2.0, 0.5));
    }

    #[test]
    fn box_faces_wind_outward() {
        let mesh = textured_box(Vec3::new(2.0, 3.0, 4.0), Vec2::ONE);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("box has no positions");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("box has no indices");
        };
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize]));
            let centroid = (a + b + c) / 3.0;
            // Counter-clockwise from outside: the face normal points away from the centre.
            assert!((b - a).cross(c - a).dot(centroid) > 0.0);
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
}
