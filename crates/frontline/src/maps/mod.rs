//! Maps: TrenchBroom `.map` files loaded with bevy_trenchbroom (IDEA.md: Maps (format)), in
//! every mode. A Dedicated Server loads the same files headless, for colliders and entities.
//!
//! - Units: 1 TrenchBroom unit = 1 inch ([`UNITS_PER_METRE`]), the crate's default and Call of
//!   Duty's own map unit, so Homage Map measurements carry over directly.
//! - Textures: a face's texture name picks a [`SurfaceKind`] from our material library
//!   (`SurfaceKind::from_texture_name`, e.g. `concrete/color`). TrenchBroom sees the same files
//!   under `assets/textures/`.
//! - Colliders: every brush becomes a static convex collider on `GameLayer::World`, the layer
//!   movement and shots query.
//! - Entities: see [`classes`]; spawn points become [`MapSpawns`].
//!
//! Which Map is loaded follows the Match: the server names it in `MatchState::map` (from
//! `--map`, or [`ChangeMap`]), which replicates, so every process loads the same file. Without a
//! Match (main menu) the idle Map shows. A Map is playable once [`MapReady`]: its colliders are
//! in the physics world.

mod classes;
mod registry;

use std::future::ready;
use std::path::PathBuf;
use std::sync::Arc;

use avian3d::prelude::*;
use bevy::asset::AssetPath;
use bevy::asset::io::file::FileAssetReader;
use bevy::gltf::GltfPlugin;
use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats, ImageLoader};
use bevy::prelude::*;
use bevy::world_serialization::{WorldInstanceReady, WorldSerializationPlugin};
use bevy_trenchbroom::config::{
    LoadLooseTextureFn, MapFileFormat, TextureLoadView, WriteTrenchBroomConfigOnStartPlugin,
};
use bevy_trenchbroom::fix_default_sampler::RepeatDefaultSamplerPlugin;
use bevy_trenchbroom::physics::SceneCollidersReady;
use bevy_trenchbroom::prelude::*;
use bevy_trenchbroom::special_textures::SpecialTexturesPlugin;

pub use classes::{
    AmmoCrate, BombSite, DomFlag, HqSite, Prop, PropCollider, SpawnPoint, SpawnTeam,
};
pub use registry::{DEFAULT_MAP, MapId, MapRegistry, private_dir, register_private_source};

use crate::cli::Mode;
use crate::cod4::{Cod4, Cod4Overrides, PreparedTexture};
use crate::data::{GameData, GameModeId};
use crate::flow::{AppState, ConnectionError, FlowCommand, MenuScreen};
use crate::materials::{SurfaceKind, SurfaceMaterials};
use crate::shared::{GameLayer, MatchState, Team};

/// TrenchBroom units per metre: 1 unit = 1 inch.
pub const UNITS_PER_METRE: f32 = 39.37008;

/// Loads Maps and keeps the right one loaded. Add after `WorldPlugin` (physics, assets) and, in
/// windowed modes, after `MaterialsPlugin`; insert `Mode` first.
pub struct MapsPlugin {
    /// The Map shown while there is no Match (main menu, or a server not yet started).
    pub idle: MapId,
    /// Write TrenchBroom's game config (FGD, textures) for this game on startup.
    pub write_trenchbroom_config: bool,
}

impl Plugin for MapsPlugin {
    fn build(&self, app: &mut App) {
        let windowed = matches!(
            app.world().get_resource::<Mode>(),
            Some(Mode::Windowed { .. })
        );
        if !windowed {
            add_headless_asset_support(app);
        }
        let surfaces = match app.world().get_resource::<SurfaceMaterials>() {
            Some(library) => MapSurfaces::Library {
                library: Box::new(library.clone()),
                cod4: app.world().get_resource::<Cod4Overrides>().cloned(),
            },
            None => MapSurfaces::Headless(
                app.world_mut()
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(StandardMaterial::default()),
            ),
        };
        let registry = registry::load_registry(app.world().resource::<GameData>());
        app.add_plugins(
            TrenchBroomPlugins(trenchbroom_config(surfaces))
                .build()
                // Written on request only (below), not on every launch.
                .disable::<WriteTrenchBroomConfigOnStartPlugin>()
                // Quake sky/liquid materials for BSPs, and a repeating default sampler for
                // loose textures: we use neither (our materials set their own samplers).
                .disable::<SpecialTexturesPlugin>()
                .disable::<RepeatDefaultSamplerPlugin>(),
        )
        .override_class::<classes::Worldspawn>()
        .register_type::<SpawnPoint>()
        .register_type::<DomFlag>()
        .register_type::<HqSite>()
        .register_type::<BombSite>()
        .register_type::<AmmoCrate>()
        .register_type::<Prop>()
        .add_message::<ChangeMap>()
        .insert_resource(registry)
        .insert_resource(IdleMap(self.idle.clone()))
        .add_observer(mark_colliders_built)
        .add_observer(spawn_prop)
        .add_observer(prop_hulls_built)
        .add_observer(build_prop_box)
        .add_systems(Update, sync_map)
        .add_systems(FixedPostUpdate, promote_ready.after(PhysicsSystems::Last));
        if self.write_trenchbroom_config {
            app.add_systems(Startup, WriteTrenchBroomConfigOnStartPlugin::write);
        }
        if windowed {
            app.add_systems(
                Update,
                leave_on_unknown_map.run_if(in_state(AppState::InGame)),
            );
        }
    }
}

/// What a Dedicated Server (`MinimalPlugins`) lacks to load `.map` and glTF files without
/// rendering: images (bevy_trenchbroom reads texture sizes for UVs), standard materials, scenes.
fn add_headless_asset_support(app: &mut App) {
    app.add_plugins((
        ImagePlugin::default(),
        WorldSerializationPlugin,
        GltfPlugin::default(),
    ))
    .insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
    // Normally registered by the renderer once it knows the GPU's compressed formats.
    .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE))
    .init_asset::<StandardMaterial>();
}

/// Materials map faces get: the surface library when rendering, one plain material headless.
#[derive(Clone)]
enum MapSurfaces {
    /// `cod4`: decoded CoD4 textures for Homage Maps, when this process has that support.
    Library {
        library: Box<SurfaceMaterials>,
        cod4: Option<Cod4Overrides>,
    },
    Headless(Handle<StandardMaterial>),
}

fn trenchbroom_config(surfaces: MapSurfaces) -> TrenchBroomConfig {
    let mut exclusions = TrenchBroomConfig::default_texture_exclusions();
    // Each surface folder holds color/normal/roughness; only the colour map is a texture to
    // paint with.
    exclusions.extend(["*normal".to_owned(), "*roughness".to_owned()]);
    TrenchBroomConfig::new("frontline")
        .scale(UNITS_PER_METRE)
        // Valve 220, what `tools/mapgen` writes (the crate defaults to Quake 2's variant).
        .file_formats(vec![MapFileFormat::Valve])
        .assets_path(editor_assets_path())
        .material_root("textures")
        .texture_extensions(vec!["jpg".to_owned()])
        .texture_exclusions(exclusions)
        .default_solid_scene_hooks(|| {
            SceneHooks::new()
                .convex_collider()
                .with(CollisionLayers::new(GameLayer::World, LayerMask::ALL))
        })
        .load_loose_texture_fn(move |_| loose_texture_hook(surfaces))
}

/// The game's asset folder for TrenchBroom's search path, relative to the working directory
/// (TrenchBroom's game path, set when the config is written) when it lies below it.
fn editor_assets_path() -> PathBuf {
    let assets = FileAssetReader::get_base_path().join("assets");
    std::env::current_dir()
        .ok()
        .and_then(|cwd| assets.strip_prefix(cwd).ok().map(PathBuf::from))
        .unwrap_or(assets)
}

/// Resolves a face's texture name to a material of [`MapSurfaces`] instead of loading the file.
fn loose_texture_hook(surfaces: MapSurfaces) -> Arc<LoadLooseTextureFn> {
    texture_hook(move |mut view| {
        let material = match &surfaces {
            MapSurfaces::Headless(material) => material.clone(),
            MapSurfaces::Library { library, cod4 } => {
                match SurfaceKind::from_texture_name(view.name) {
                    Some(kind) => {
                        let map_file = view.load_context.path().without_label().to_string();
                        let textures = cod4.as_ref().and_then(|cod4| cod4.get(&map_file));
                        match textures.as_deref().and_then(|map| map.get(view.name)) {
                            Some(prepared) => cod4_material(&mut view, kind, prepared),
                            None => library.material(kind),
                        }
                    }
                    None => {
                        warn!(
                            "Map texture \"{}\" names no surface ({}); drawing it as missing",
                            view.name,
                            SurfaceKind::ALL.map(SurfaceKind::name).join(", ")
                        );
                        return Box::pin(ready(view.tb_server.missing_material.read().clone()));
                    }
                }
            }
        };
        let handle = view.load_context.add_labeled_asset(
            format!("GenericMaterial_{}", view.name),
            GenericMaterial::new(material),
        );
        Box::pin(ready(handle))
    })
}

/// A material of `kind`'s faces made from the player's own CoD4 image (docs/adr/0004). The
/// images live in the Map's asset, in memory only. UVs are rescaled so one repeat covers
/// `tile_m` metres of wall instead of the CC0 texture's size, which the faces' scale assumes.
fn cod4_material(
    view: &mut TextureLoadView,
    kind: SurfaceKind,
    prepared: &PreparedTexture,
) -> Handle<StandardMaterial> {
    let name = view.name;
    let (width, height) = (
        prepared.color.width() as f32,
        prepared.color.height() as f32,
    );
    let tile = Vec2::new(prepared.tile_m, prepared.tile_m * height / width);
    let color = view
        .load_context
        .add_labeled_asset(format!("Cod4Color_{name}"), prepared.color.clone());
    let normal = prepared.normal.as_ref().map(|image| {
        view.load_context
            .add_labeled_asset(format!("Cod4Normal_{name}"), image.clone())
    });
    view.add_material(StandardMaterial {
        base_color_texture: Some(color),
        normal_map_texture: normal,
        // CoD4 normal maps have Y pointing down.
        flip_normal_map_y: true,
        perceptual_roughness: 0.9,
        reflectance: 0.3,
        uv_transform: bevy::math::Affine2::from_scale(kind.tile_metres() / tile),
        ..default()
    })
}

/// Pins a closure to the higher-ranked signature of [`LoadLooseTextureFn`].
fn texture_hook<F>(hook: F) -> Arc<LoadLooseTextureFn>
where
    F: for<'a, 'b> Fn(
            TextureLoadView<'a, 'b>,
        ) -> bevy::tasks::BoxedFuture<'a, Handle<GenericMaterial>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(hook)
}

/// Ask the server to play another Map from now on (server only; the Rotation changes
/// `MatchState::map` itself at a Match boundary). Every process unloads the current Map and loads the new one; players on a Team
/// respawn on it once it is ready. Unknown ids are refused with an error.
#[derive(Message, Clone, Debug)]
pub struct ChangeMap(pub MapId);

/// The Map loaded while there is no Match.
#[derive(Resource, Clone, Debug)]
struct IdleMap(MapId);

/// Root entity of the loaded Map's scene; despawning it unloads the Map.
#[derive(Component, Debug)]
pub struct MapRoot {
    pub id: MapId,
}

/// The Map's brush colliders exist, but haven't been through a physics step yet.
#[derive(Component, Debug)]
struct CollidersBuilt;

/// On the [`MapRoot`] once the Map is playable: colliders are in the physics world and
/// [`MapSpawns`] is filled.
#[derive(Component, Debug)]
pub struct MapReady {
    pub name: String,
    /// Look preset: `maps.ron`'s, else the worldspawn's; `None` when neither names one.
    pub look: Option<String>,
}

/// A prop whose collider is still being built; holds back [`MapReady`].
#[derive(Component, Debug)]
struct PendingPropCollider;

/// A prop that gets one box collider around its model once the model has spawned.
#[derive(Component, Debug)]
struct PendingPropBox;

/// Run condition: the loaded Map is playable.
pub fn map_ready(ready: Query<(), With<MapReady>>) -> bool {
    !ready.is_empty()
}

/// Run condition: the Map the Match names is the one loaded, and it is playable. (Right after
/// the Rotation moves on, the previous Map is still loaded for a frame.)
pub fn match_map_ready(
    roots: Query<&MapRoot, With<MapReady>>,
    matches: Query<&MatchState>,
) -> bool {
    matches
        .iter()
        .next()
        .is_some_and(|state| roots.iter().any(|root| root.id == state.map))
}

/// One spawn point of the loaded Map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapSpawn {
    /// Feet position.
    pub position: Vec3,
    pub yaw: f32,
    pub team: SpawnTeam,
    /// Game Mode keys; empty means every mode.
    pub modes: Vec<String>,
}

/// The loaded Map's spawn points; present while a Map is [`MapReady`].
#[derive(Resource, Clone, Debug, Default)]
pub struct MapSpawns(pub Vec<MapSpawn>);

impl MapSpawns {
    /// Spawns a player of `team` may use in `mode` (any Team's when `team` is `None`). When the
    /// Map has none for them, every spawn point, so a sparse Map stays playable.
    pub fn usable(&self, team: Option<Team>, mode: GameModeId) -> Vec<&MapSpawn> {
        let usable: Vec<&MapSpawn> = self
            .0
            .iter()
            .filter(|spawn| registry::mode_listed(&spawn.modes, mode))
            .filter(|spawn| match (team, spawn.team) {
                (None, _) | (_, SpawnTeam::Any) => true,
                (Some(Team::Alpha), team) => team == SpawnTeam::Alpha,
                (Some(Team::Bravo), team) => team == SpawnTeam::Bravo,
            })
            .collect();
        if usable.is_empty() {
            warn!("No spawn points for {team:?} in {mode:?}; using all of them");
            return self.0.iter().collect();
        }
        usable
    }
}

/// Loads the Map the Match names (or the idle Map without one), unloading any other first.
fn sync_map(
    matches: Query<&MatchState>,
    idle: Res<IdleMap>,
    roots: Query<(Entity, &MapRoot)>,
    registry: Res<MapRegistry>,
    asset_server: Res<AssetServer>,
    mut cod4: Option<ResMut<Cod4>>,
    mut unknown: Local<Option<MapId>>,
    mut commands: Commands,
) {
    let wanted = matches.iter().next().map_or(&idle.0, |state| &state.map);
    if roots.iter().any(|(_, root)| &root.id == wanted) {
        return;
    }
    for (entity, root) in &roots {
        info!("Unloading Map \"{}\"", root.id);
        commands.entity(entity).despawn();
        if let (Some(cod4), Some(info)) = (&cod4, registry.get(&root.id)) {
            cod4.release(&info.file);
        }
    }
    commands.remove_resource::<MapSpawns>();
    let Some(info) = registry.get(wanted) else {
        if unknown.as_ref() != Some(wanted) {
            error!(
                "Can't load unknown Map \"{wanted}\"; known: {}",
                registry.known_ids()
            );
            *unknown = Some(wanted.clone());
        }
        return;
    };
    *unknown = None;
    // A Homage Map with CoD4 textures waits for them to be decoded (docs/adr/0004).
    if let Some(cod4) = cod4.as_mut()
        && info.private
        && !cod4.prepare_map(wanted, &info.file)
    {
        return;
    }
    info!("Loading Map \"{wanted}\" from {}", info.file);
    let scene = asset_server.load(AssetPath::from(info.file.clone()).with_label("Scene"));
    commands.spawn((
        Name::new(format!("Map {wanted}")),
        MapRoot { id: wanted.clone() },
        WorldAssetRoot(scene),
        Transform::IDENTITY,
    ));
}

fn mark_colliders_built(
    ready: On<SceneCollidersReady>,
    roots: Query<&MapRoot>,
    colliders: Query<(&Collider, &GlobalTransform)>,
    mut commands: Commands,
) {
    let Ok(root) = roots.get(ready.scene_root_entity) else {
        return;
    };
    let mut brushes = 0;
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (collider, transform) in ready
        .collider_entities
        .iter()
        .filter_map(|&entity| colliders.get(entity).ok())
    {
        brushes += collider
            .shape()
            .as_compound()
            .map_or(1, |compound| compound.shapes().len());
        let aabb = collider.aabb(transform.translation(), transform.rotation(), 0.0);
        low = low.min(aabb.min);
        high = high.max(aabb.max);
    }
    let size = high - low;
    info!(
        "Map \"{}\": {brushes} brush colliders on {} entities, extents {:.0} x {:.0} m \
         (x {:.0}..{:.0}, z {:.0}..{:.0}, up to {:.0} m high; all brushes, perimeter and skyline \
         bodies included)",
        root.id,
        ready.collider_entities.len(),
        size.x,
        size.z,
        low.x,
        high.x,
        low.z,
        high.z,
        high.y,
    );
    commands
        .entity(ready.scene_root_entity)
        .insert(CollidersBuilt);
}

/// After the physics step that took in the Map's colliders (and once every prop collider is
/// built), marks the Map ready and collects its spawn points.
fn promote_ready(
    built: Query<(Entity, &MapRoot), (With<CollidersBuilt>, Without<MapReady>)>,
    pending_props: Query<(), With<PendingPropCollider>>,
    children: Query<&Children>,
    spawn_points: Query<(&SpawnPoint, &Transform)>,
    flags: Query<(), With<DomFlag>>,
    hq_sites: Query<(), With<HqSite>>,
    bomb_sites: Query<(), With<BombSite>>,
    ammo_crates: Query<(), With<AmmoCrate>>,
    props: Query<(), With<Prop>>,
    worldspawns: Query<&classes::Worldspawn>,
    registry: Res<MapRegistry>,
    mut commands: Commands,
) {
    if !pending_props.is_empty() {
        return;
    }
    for (entity, root) in &built {
        let entities: Vec<Entity> = children.iter_descendants(entity).collect();
        let count = |found: &dyn Fn(Entity) -> bool| entities.iter().filter(|&&e| found(e)).count();
        // Map entities are direct children of the root, which sits at the origin.
        let spawns: Vec<MapSpawn> = entities
            .iter()
            .filter_map(|&e| spawn_points.get(e).ok())
            .map(|(spawn, transform)| MapSpawn {
                position: transform.translation,
                yaw: transform.rotation.to_euler(EulerRot::YXZ).0,
                team: spawn.team,
                modes: spawn.mode_keys(),
            })
            .collect();
        let info = registry.get(&root.id);
        let worldspawn = entities.iter().find_map(|&e| worldspawns.get(e).ok());
        let look = info
            .and_then(|info| info.look.clone())
            .or_else(|| worldspawn.map(|w| w.look.clone()))
            .filter(|look| !look.is_empty());
        let name = info.map_or_else(|| root.id.0.clone(), |info| info.name.clone());
        info!(
            "Map \"{}\" ready: {} spawn points, {} Flags, {} HQ sites, {} bomb sites, {} ammo crates, \
             {} props, look {}",
            root.id,
            spawns.len(),
            count(&|e| flags.contains(e)),
            count(&|e| hq_sites.contains(e)),
            count(&|e| bomb_sites.contains(e)),
            count(&|e| ammo_crates.contains(e)),
            count(&|e| props.contains(e)),
            look.as_deref().unwrap_or("(default)"),
        );
        commands.insert_resource(MapSpawns(spawns));
        commands.entity(entity).insert(MapReady { name, look });
    }
}

/// Spawns a prop's glTF scene and sets up its collider.
fn spawn_prop(
    add: On<Add<Prop>>,
    props: Query<&Prop>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(prop) = props.get(add.entity) else {
        return;
    };
    let scene = asset_server.load(AssetPath::from(prop.model.clone()).with_label("Scene0"));
    let mut entity = commands.entity(add.entity);
    entity.insert(WorldAssetRoot(scene));
    let layers = CollisionLayers::new(GameLayer::World, LayerMask::ALL);
    match prop.collider {
        PropCollider::None => {}
        PropCollider::Box => {
            entity.insert((RigidBody::Static, PendingPropCollider, PendingPropBox));
        }
        PropCollider::Convex => {
            entity.insert((
                RigidBody::Static,
                PendingPropCollider,
                ColliderConstructorHierarchy::new(ColliderConstructor::ConvexHullFromMesh)
                    .with_default_layers(layers),
            ));
        }
    }
}

fn prop_hulls_built(ready: On<ColliderConstructorHierarchyReady>, mut commands: Commands) {
    if let Ok(mut entity) = commands.get_entity(ready.entity) {
        entity.remove::<PendingPropCollider>();
    }
}

/// Gives a box prop one cuboid collider around all meshes of its spawned model.
fn build_prop_box(
    ready: On<WorldInstanceReady>,
    props: Query<&Prop, With<PendingPropBox>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &ChildOf, Option<&Mesh3d>)>,
    meshes: Res<Assets<Mesh>>,
    mut commands: Commands,
) {
    let Ok(prop) = props.get(ready.entity) else {
        return;
    };
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for node in children.iter_descendants(ready.entity) {
        let Ok((_, _, Some(mesh))) = nodes.get(node) else {
            continue;
        };
        let Some(positions) = meshes
            .get(&mesh.0)
            .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
            .and_then(|positions| positions.as_float3())
        else {
            continue;
        };
        // The mesh's transform relative to the prop.
        let mut to_prop = Transform::IDENTITY;
        let mut current = node;
        while current != ready.entity {
            let Ok((transform, parent, _)) = nodes.get(current) else {
                break;
            };
            to_prop = *transform * to_prop;
            current = parent.parent();
        }
        for &position in positions {
            let point = to_prop.transform_point(Vec3::from(position));
            bounds = Some(bounds.map_or((point, point), |(min, max)| {
                (min.min(point), max.max(point))
            }));
        }
    }
    let mut entity = commands.entity(ready.entity);
    entity.remove::<(PendingPropBox, PendingPropCollider)>();
    let Some((min, max)) = bounds else {
        warn!("Prop {} has no meshes to box; it won't collide", prop.model);
        return;
    };
    let size = max - min;
    entity.insert((
        Collider::compound(vec![(
            (min + max) / 2.0,
            Quat::IDENTITY,
            Collider::cuboid(size.x, size.y, size.z),
        )]),
        CollisionLayers::new(GameLayer::World, LayerMask::ALL),
    ));
}

/// A server naming a Map this game doesn't have (e.g. a Homage Map without the private
/// content) can't be played: leave with an explanation on the Server Browser.
fn leave_on_unknown_map(
    matches: Query<&MatchState, Changed<MatchState>>,
    registry: Res<MapRegistry>,
    mut error: ResMut<ConnectionError>,
    mut flow: MessageWriter<FlowCommand>,
    mut next_menu: ResMut<NextState<MenuScreen>>,
) {
    for state in &matches {
        if registry.get(&state.map).is_none() {
            error.0 = Some(format!(
                "The server plays Map \"{}\", which this game doesn't have",
                state.map
            ));
            warn!("{}", error.0.as_deref().unwrap_or_default());
            flow.write(FlowCommand::Leave);
            next_menu.set(MenuScreen::ServerBrowser);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(team: SpawnTeam, modes: &[&str]) -> MapSpawn {
        MapSpawn {
            position: Vec3::ZERO,
            yaw: 0.0,
            team,
            modes: modes.iter().map(|&m| m.to_owned()).collect(),
        }
    }

    #[test]
    fn spawns_filter_by_team_and_mode() {
        let spawns = MapSpawns(vec![
            spawn(SpawnTeam::Any, &["tdm"]),
            spawn(SpawnTeam::Alpha, &["dom"]),
            spawn(SpawnTeam::Bravo, &[]),
            spawn(SpawnTeam::Alpha, &[]),
        ]);
        let tdm = GameModeId::TeamDeathmatch;
        let alpha = spawns.usable(Some(Team::Alpha), tdm);
        assert_eq!(alpha, [&spawns.0[0], &spawns.0[3]]);
        let bravo = spawns.usable(Some(Team::Bravo), tdm);
        assert_eq!(bravo, [&spawns.0[0], &spawns.0[2]]);
        assert_eq!(spawns.usable(None, tdm).len(), 3);
    }

    #[test]
    fn spawns_fall_back_to_all_when_none_fit() {
        let spawns = MapSpawns(vec![spawn(SpawnTeam::Bravo, &["dom"])]);
        assert_eq!(
            spawns
                .usable(Some(Team::Alpha), GameModeId::TeamDeathmatch)
                .len(),
            1
        );
    }
}
