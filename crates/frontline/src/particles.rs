//! Cheap particles for shot and blast effects: soft camera-facing puffs (dust, smoke, fire,
//! flash stars, shock rings), spark streaks and bullet-hole decals, all drawn as unlit quads or
//! boxes with procedural textures. Client/Host only. How many a spawner makes follows the Video
//! quality (`Detail`).

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::atmosphere::sky::fbm;
use crate::flow::AppState;
use crate::maps::MapReady;
use crate::player::EyeCamera;
use crate::video::{AntiAliasing, Quality, SsaoQuality};
use crate::weapon::WeaponRng;

/// How much particle work the Video quality allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Detail {
    Low,
    Medium,
    High,
}

impl Detail {
    /// Derived from the effects the player turned on: bloom, SSAO and the anti-aliasing, so the
    /// Low/Medium/High presets map to Low/Medium/High and a hand-made mix lands in between.
    pub fn of(quality: &Quality) -> Detail {
        let score = u8::from(quality.bloom)
            + u8::from(quality.ssao != SsaoQuality::Off)
            + u8::from(quality.ssao == SsaoQuality::High)
            + u8::from(quality.aa == AntiAliasing::Smaa);
        match score {
            0..=1 => Detail::Low,
            2..=3 => Detail::Medium,
            _ => Detail::High,
        }
    }

    /// `medium` particles at Medium detail, fewer at Low (never none of a non-zero count) and
    /// more at High.
    pub fn count(self, medium: u32) -> u32 {
        match self {
            Detail::Low => medium.div_ceil(2),
            Detail::Medium => medium,
            Detail::High => medium + medium / 2,
        }
    }

    /// Bullet holes kept in the world at once (the oldest go first): enough that a long
    /// fight's hits stay readable.
    fn hole_cap(self) -> usize {
        match self {
            Detail::Low => 120,
            Detail::Medium => 300,
            Detail::High => 600,
        }
    }
}

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BulletHoles>()
            .init_resource::<ParticleAssets>()
            .add_observer(clear_bullet_holes_on_new_map)
            .add_systems(OnExit(AppState::InGame), forget_bullet_holes)
            .add_systems(
                Update,
                (update_puffs, update_sparks).run_if(in_state(AppState::InGame)),
            );
    }
}

/// The shape of a puff's texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Round and lumpy, for dust, smoke and fire.
    Soft,
    /// Like `Soft` with a wide fully opaque core, for smoke that has to block the view.
    Dense,
    /// A bright core with spikes, for flashes.
    Star,
    /// A thin circle, for shock waves.
    Ring,
}

/// How a puff's alpha follows its life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fade {
    /// Up in `in_secs`, then down to nothing at the end of its life.
    Out { in_secs: f32 },
    /// Up over `in_secs`, held, down over the last `out_secs` (smoke clouds).
    Hold { in_secs: f32, out_secs: f32 },
}

impl Fade {
    fn alpha(self, age: f32, life: f32) -> f32 {
        match self {
            Fade::Out { in_secs } => {
                let rise = (age / in_secs.max(f32::EPSILON)).clamp(0.0, 1.0);
                let left = (1.0 - age / life).clamp(0.0, 1.0);
                rise * left * left.sqrt()
            }
            Fade::Hold { in_secs, out_secs } => {
                let rise = (age / in_secs.max(f32::EPSILON)).clamp(0.0, 1.0);
                let fall = ((life - age) / out_secs.max(f32::EPSILON)).clamp(0.0, 1.0);
                rise.min(fall)
            }
        }
    }
}

/// Everything about one puff.
#[derive(Clone, Copy, Debug)]
pub struct PuffSpec {
    pub at: Vec3,
    pub velocity: Vec3,
    /// Fraction of its speed a puff loses per second.
    pub drag: f32,
    /// Metres per second squared pulling it down (negative rises).
    pub gravity: f32,
    pub start_size: f32,
    pub end_size: f32,
    /// Seconds the growth takes (the puff keeps its end size afterwards).
    pub grow_secs: f32,
    pub life: f32,
    pub shape: Shape,
    /// Linear colour, may exceed 1 for glowing puffs (they bloom).
    pub color: LinearRgba,
    /// Peak opacity, 0..1.
    pub alpha: f32,
    pub additive: bool,
    pub fade: Fade,
    /// Radians per second.
    pub spin: f32,
    /// Starting angle.
    pub angle: f32,
}

impl Default for PuffSpec {
    fn default() -> Self {
        PuffSpec {
            at: Vec3::ZERO,
            velocity: Vec3::ZERO,
            drag: 2.0,
            gravity: 0.0,
            start_size: 0.2,
            end_size: 0.6,
            grow_secs: 0.5,
            life: 0.6,
            shape: Shape::Soft,
            color: LinearRgba::WHITE,
            alpha: 0.5,
            additive: false,
            fade: Fade::Out { in_secs: 0.05 },
            spin: 0.0,
            angle: 0.0,
        }
    }
}

#[derive(Component, Debug)]
struct Puff {
    spec: PuffSpec,
    velocity: Vec3,
    age: f32,
    material: Handle<StandardMaterial>,
}

#[derive(Component, Debug)]
struct Spark {
    velocity: Vec3,
    age: f32,
    life: f32,
}

/// Meshes, textures and shared materials of the particles.
#[derive(Resource)]
pub struct ParticleAssets {
    quad: Handle<Mesh>,
    soft: Handle<Image>,
    dense: Handle<Image>,
    star: Handle<Image>,
    ring: Handle<Image>,
    spark_mesh: Handle<Mesh>,
    spark_material: Handle<StandardMaterial>,
    hole_material: Handle<StandardMaterial>,
    flame: Handle<Image>,
}

impl ParticleAssets {
    fn texture(&self, shape: Shape) -> Handle<Image> {
        match shape {
            Shape::Soft => self.soft.clone(),
            Shape::Dense => self.dense.clone(),
            Shape::Star => self.star.clone(),
            Shape::Ring => self.ring.clone(),
        }
    }

    pub fn quad(&self) -> Handle<Mesh> {
        self.quad.clone()
    }

    /// Star burst for muzzle flashes.
    pub fn star(&self) -> Handle<Image> {
        self.star.clone()
    }

    /// A cone of flame fading from a bright base (bottom of the image) to its tip (top).
    pub fn flame(&self) -> Handle<Image> {
        self.flame.clone()
    }
}

/// Bullet holes in the world, oldest first.
#[derive(Resource, Default)]
pub struct BulletHoles(VecDeque<Entity>);

fn image(size: u32, texel: impl Fn(Vec2) -> [f32; 4]) -> Image {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(
                (x as f32 + 0.5) / size as f32 * 2.0 - 1.0,
                (y as f32 + 0.5) / size as f32 * 2.0 - 1.0,
            );
            data.extend(texel(p).map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn soft_texture() -> Image {
    image(96, |p| {
        let lump = fbm(p * 2.6 + Vec2::splat(3.0)) - 0.5;
        let edge = p.length() + lump * 0.7;
        // Slightly darker underneath, so smoke reads as a volume.
        let shade = 1.0 - 0.18 * (p.y * 0.5 + 0.5);
        [shade, shade, shade, smoothstep(1.0, 0.1, edge)]
    })
}

/// The same lumpy outline as `soft_texture`, but opaque out to about 60% of the radius, so a
/// handful of puffs hide what is behind them instead of letting it show through.
fn dense_texture() -> Image {
    image(96, |p| {
        let lump = fbm(p * 2.6 + Vec2::splat(3.0)) - 0.5;
        let edge = p.length() + lump * 0.5;
        let shade = 1.0 - 0.18 * (p.y * 0.5 + 0.5);
        [shade, shade, shade, smoothstep(1.0, 0.6, edge)]
    })
}

fn star_texture() -> Image {
    image(96, |p| {
        let r = p.length().min(1.0);
        let angle = p.y.atan2(p.x);
        let falloff = (1.0 - r).powi(2);
        let core = (-r * r * 16.0).exp();
        let spikes = (2.0 * angle).cos().abs().powi(40) * falloff;
        let cross = (2.0 * angle + PI / 4.0).cos().abs().powi(60) * falloff * 0.45;
        let alpha = core + 0.9 * spikes + cross;
        [1.0, 0.92 - 0.2 * r, 0.7 - 0.3 * r, alpha]
    })
}

fn ring_texture() -> Image {
    image(128, |p| {
        let r = p.length();
        let alpha = (-((r - 0.86) / 0.07).powi(2)).exp() * smoothstep(1.0, 0.92, r);
        [1.0, 1.0, 1.0, alpha]
    })
}

/// Bright at the bottom (the base of the flame), narrowing and fading towards the top.
fn flame_texture() -> Image {
    image(64, |p| {
        let along = (1.0 - (p.y * 0.5 + 0.5)).clamp(0.0, 1.0); // 0 at the tip, 1 at the base
        let half_width = 0.18 + 0.82 * along.powf(0.7);
        let across = 1.0 - (p.x.abs() / half_width).min(1.0).powf(1.6);
        let alpha = across * along.powf(1.4);
        [
            1.0,
            0.85 - 0.25 * (1.0 - along),
            0.55 - 0.35 * (1.0 - along),
            alpha,
        ]
    })
}

/// A bullet hole: a black round crater with a ragged edge and a gray rim fading out, readable on
/// dark and light surfaces alike.
fn hole_texture() -> Image {
    image(64, |p| {
        let wobble = (fbm(p * 4.0 + Vec2::splat(11.0)) - 0.5) * 0.3;
        let edge = p.length() + wobble;
        let core = smoothstep(0.46, 0.40, edge);
        let rim = smoothstep(0.80, 0.55, edge);
        let shade = if core > 0.0 {
            0.02
        } else {
            0.36 - 0.1 * smoothstep(0.5, 0.8, edge)
        };
        [shade, shade, shade, core.max(rim * 0.85)]
    })
}

impl FromWorld for ParticleAssets {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        let hole = images.add(hole_texture());
        let soft = images.add(soft_texture());
        let dense = images.add(dense_texture());
        let star = images.add(star_texture());
        let ring = images.add(ring_texture());
        let flame = images.add(flame_texture());
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        let spark_mesh = meshes.add(Cuboid::from_size(Vec3::ONE));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        ParticleAssets {
            quad,
            soft,
            dense,
            star,
            ring,
            flame,
            spark_mesh,
            spark_material: materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.85, 0.5),
                emissive: LinearRgba::rgb(14.0, 8.0, 2.5),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            }),
            hole_material: materials.add(StandardMaterial {
                base_color_texture: Some(hole),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                depth_bias: 4.0,
                ..default()
            }),
        }
    }
}

/// The holes went with the game (`DespawnOnExit`); drop their entities from the list.
fn forget_bullet_holes(mut holes: ResMut<BulletHoles>) {
    holes.0.clear();
}

/// A new Map is ready (a Rotation moved on, or the Match started): the old Map's holes would
/// hang in the air, so they go.
fn clear_bullet_holes_on_new_map(
    _: On<Add<MapReady>>,
    mut holes: ResMut<BulletHoles>,
    mut commands: Commands,
) {
    for hole in holes.0.drain(..) {
        commands.entity(hole).try_despawn();
    }
}

/// Spawns a puff.
pub fn spawn_puff(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &ParticleAssets,
    spec: PuffSpec,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::from(spec.color.with_alpha(0.0)),
        base_color_texture: Some(assets.texture(spec.shape)),
        unlit: true,
        alpha_mode: if spec.additive {
            AlphaMode::Add
        } else {
            AlphaMode::Blend
        },
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Name::new("Puff"),
        Puff {
            spec,
            velocity: spec.velocity,
            age: 0.0,
            material: material.clone(),
        },
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(spec.at).with_scale(Vec3::splat(spec.start_size)),
        NotShadowCaster,
        DespawnOnExit(AppState::InGame),
    ));
}

/// Ages, moves, grows and fades the puffs, and turns them to face the eye camera.
fn update_puffs(
    time: Res<Time>,
    eye: Query<&GlobalTransform, With<EyeCamera>>,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let facing = eye
        .iter()
        .next()
        .map_or(Quat::IDENTITY, |eye| eye.rotation());
    for (entity, mut puff, mut transform) in &mut puffs {
        puff.age += dt;
        let spec = puff.spec;
        if puff.age >= spec.life {
            commands.entity(entity).despawn();
            continue;
        }
        let slow = (-spec.drag * dt).exp();
        puff.velocity *= slow;
        puff.velocity.y -= spec.gravity * dt;
        transform.translation += puff.velocity * dt;
        let grown = (puff.age / spec.grow_secs.max(f32::EPSILON)).min(1.0);
        let size =
            spec.start_size + (spec.end_size - spec.start_size) * (1.0 - (1.0 - grown).powi(2));
        transform.scale = Vec3::splat(size);
        transform.rotation = facing * Quat::from_rotation_z(spec.angle + spec.spin * puff.age);
        if let Some(mut material) = materials.get_mut(&puff.material) {
            let alpha = spec.alpha * spec.fade.alpha(puff.age, spec.life);
            material.base_color = Color::from(spec.color.with_alpha(alpha));
        }
    }
}

/// A random unit vector.
pub fn random_unit(rng: &mut WeaponRng) -> Vec3 {
    loop {
        let v = Vec3::new(rng.signed(), rng.signed(), rng.signed());
        let length = v.length();
        if (0.05..=1.0).contains(&length) {
            return v / length;
        }
    }
}

/// Spawns a glowing spark streak.
pub fn spawn_spark(
    commands: &mut Commands,
    assets: &ParticleAssets,
    at: Vec3,
    velocity: Vec3,
    life: f32,
) {
    commands.spawn((
        Name::new("Spark"),
        Spark {
            velocity,
            age: 0.0,
            life,
        },
        Mesh3d(assets.spark_mesh.clone()),
        MeshMaterial3d(assets.spark_material.clone()),
        Transform::from_translation(at).with_scale(Vec3::new(0.008, 0.008, 0.05)),
        NotShadowCaster,
        DespawnOnExit(AppState::InGame),
    ));
}

const SPARK_GRAVITY: f32 = 9.8;

fn update_sparks(
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += dt;
        if spark.age >= spark.life {
            commands.entity(entity).despawn();
            continue;
        }
        spark.velocity.y -= SPARK_GRAVITY * dt;
        transform.translation += spark.velocity * dt;
        let speed = spark.velocity.length();
        if let Ok(direction) = Dir3::new(spark.velocity) {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, *direction);
        }
        let left = 1.0 - spark.age / spark.life;
        transform.scale = Vec3::new(
            0.008 * left,
            0.008 * left,
            (speed * 0.012).clamp(0.03, 0.12) * left,
        );
    }
}

/// Puts a bullet hole on a surface at `at` facing `normal`, forgetting the oldest beyond the cap.
pub fn spawn_bullet_hole(
    commands: &mut Commands,
    assets: &ParticleAssets,
    holes: &mut BulletHoles,
    detail: Detail,
    at: Vec3,
    normal: Vec3,
    roll: f32,
) {
    let cap = detail.hole_cap();
    while holes.0.len() >= cap {
        if let Some(old) = holes.0.pop_front() {
            commands.entity(old).despawn();
        }
    }
    let size = 0.14 + 0.06 * (roll * 7.0).fract();
    let id = commands
        .spawn((
            Name::new("Bullet hole"),
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.hole_material.clone()),
            Transform::from_translation(at + normal * 0.004)
                .with_rotation(
                    Quat::from_rotation_arc(Vec3::Z, normal) * Quat::from_rotation_z(roll * TAU),
                )
                .with_scale(Vec3::splat(size)),
            NotShadowCaster,
            DespawnOnExit(AppState::InGame),
        ))
        .id();
    holes.0.push_back(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::QualityPreset;

    #[test]
    fn the_quality_presets_map_to_their_detail() {
        let detail = |preset: QualityPreset| Detail::of(&preset.quality().unwrap());
        assert_eq!(detail(QualityPreset::Low), Detail::Low);
        assert_eq!(detail(QualityPreset::Medium), Detail::Medium);
        assert_eq!(detail(QualityPreset::High), Detail::High);
    }

    #[test]
    fn counts_shrink_at_low_and_grow_at_high_but_never_vanish() {
        assert_eq!(Detail::Medium.count(6), 6);
        assert_eq!(Detail::Low.count(6), 3);
        assert_eq!(Detail::Low.count(1), 1);
        assert_eq!(Detail::High.count(6), 9);
        assert_eq!(Detail::Low.count(0), 0);
    }

    #[test]
    fn fades_start_and_end_clear() {
        let out = Fade::Out { in_secs: 0.1 };
        assert_eq!(out.alpha(0.0, 1.0), 0.0);
        assert!(out.alpha(0.2, 1.0) > 0.5);
        assert_eq!(out.alpha(1.0, 1.0), 0.0);
        let hold = Fade::Hold {
            in_secs: 1.0,
            out_secs: 2.0,
        };
        assert_eq!(hold.alpha(0.0, 10.0), 0.0);
        assert_eq!(hold.alpha(5.0, 10.0), 1.0);
        assert_eq!(hold.alpha(10.0, 10.0), 0.0);
    }
}
