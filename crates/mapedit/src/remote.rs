//! Remote control: Bevy's remote protocol (JSON-RPC 2.0 over HTTP, localhost only, port 15702
//! unless `--remote-port`) with MapEdit's own methods on top of the generic `world.*` ones. The
//! stdio MCP server in `tools/mapedit-mcp` turns them into tools for AI agents.
//!
//! Methods (params are JSON objects, results JSON):
//!
//! - `mapedit/status`: Map, look, camera, overlays, entity counts, known Maps and looks, and the
//!   `editor` block: whether the Map can be edited (`open`, else `problem`), `dirty`, the selection,
//!   `revision` and `synced` (the live view shows the working copy: wait for it before a
//!   screenshot), undo/redo availability, the `generator` script when one writes this Map's file.
//! - `mapedit/load_map` `{id, discard?}`: opens a Map of `data/maps.ron` for editing; refused while
//!   there are unsaved changes unless `discard` is true.
//! - `mapedit/reload` re-reads the Map's file (same refusal; `{discard: true}` to force),
//!   `mapedit/set_look` `{name}` (null: the Map's own).
//! - `mapedit/set_camera` `{position:[x,y,z], yaw, pitch}` (degrees; yaw 0 looks north, -z), or
//!   `{position, look_at:[x,y,z]}`, or `{preset:"top"|"aerial"}`.
//! - `mapedit/set_overlay` `{mode, labels, through_walls, colliders, fog}`, each optional; mode is
//!   `all`, `tdm`, `dom`, `hq` or `sd`.
//! - `mapedit/entities`: every spawn point, Flag, HQ site, bomb site and ammo crate with its
//!   position in game metres.
//! - `mapedit/screenshot` `{path}`: queues a PNG; it is written a few frames later.
//! - `mapedit/quit` `{discard?}`: closes the viewer (refused with unsaved changes unless `discard`).
//!
//! Editing (the working copy is a parsed `.map`; nothing reaches the Map's file before `save`;
//! after each edit the view reloads within about half a second, `status.editor.synced` says when).
//! Positions are game space: metres, y up, -z forward; yaw/angle in degrees, 0 faces -z and
//! counts counter-clockwise seen from above. An `item` is `{"entity": 0, "brush": 12}` (brush 12
//! of entity 0, the worldspawn) or `{"entity": 5}` (a point entity). **The selection is a list**
//! of items; every edit takes `item` (that one), `items` (a list: it becomes the selection first)
//! or neither (the whole selection), and does the whole thing as ONE undo step. Item indices
//! shift when something is deleted. Every edit answers `{message, selection (a list), dirty, ...}`.
//!
//! - `mapedit/new_map` `{id, name?, discard?}`: creates `maps/<id>.map` (a 60 x 60 m floor, two
//!   spawns per Team, a worldspawn with a look), lists it in `data/maps.ron` for Team Deathmatch
//!   and opens it.
//! - `mapedit/save`: writes the working copy to the Map's file (and its `maps.ron` entry when the
//!   settings changed or it is new). The answer says when `tools/mapgen/<map>.py` generates this
//!   file: running that script overwrites the edits.
//! - `mapedit/save_as` `{id, name?}`: saves a copy as a new registered Map and opens it.
//! - `mapedit/map_settings` `{name?, look? (string, null = the Map's own), modes? (array of tdm dom
//!   hq sd; empty = every mode), factions? [alpha_id, bravo_id]}`: applies and answers the current
//!   settings (call with no fields to read them); `save` persists them.
//! - `mapedit/palette`: what can be placed: surfaces, primitives, entity classes with their
//!   properties (with their value kinds), prop models with bounds, looks, Game Modes, Factions.
//! - `mapedit/items` `{kind? ("brush"|"entity"|"all"), class?, near? [x,y,z], radius?, offset?,
//!   limit? (default 100)}`: the brushes and point entities of the working copy, with bounds,
//!   surface or class and properties, and `total`.
//! - `mapedit/select`: sets the selection (highlighted in the view). One of `item`, `items`,
//!   `ray: {origin, direction}`, `screen: [x, y]` (pixels of the window: the item there),
//!   `box: {min [x,y,z], max [x,y,z], flat?}` (the items wholly inside; `flat` ignores height),
//!   `screen_box: [x0, y0, x1, y1]` (the items wholly inside that window rectangle: a box
//!   drag), `all: true`, `kind` ("brush"|"entity"), `class` (`spawn_point`...), `surface`
//!   (`concrete`...), `like: item` (everything of that item's class or surface), or
//!   `clear: true`. `mode` is "replace" (default), "add" or "toggle" (shift-click).
//! - `mapedit/move` `{delta [x,y,z]}`: nudges the selection. `mapedit/resize` `{face, amount,
//!   snap?}` moves ONE face of one brush, every other face staying put: `face` is an index, a
//!   side ("+x", "-x", "+y", "-y", "+z", "-z": the face that looks that way, sliding along that
//!   axis, so a ramp's slope goes straight up) or a direction `[x, y, z]`; `amount` is metres,
//!   outward when positive; `snap` puts an axis-aligned face on the grid. `{min, max}` instead
//!   scales the whole brush to that bounding box. `mapedit/delete`; `mapedit/duplicate`
//!   `{offset? [x,y,z]}` (default 1 m in +x; the copies are selected); `mapedit/set_texture`
//!   `{surface}` (a surface name of the palette; a brush's faces get its real-world texture
//!   scale; entities in the selection are skipped).
//! - `mapedit/rotate` `{degrees, pivot? [x, z]}`: turns the selection counter-clockwise about the
//!   vertical axis through the pivot (default: the middle of its bounding box). Any angle; the
//!   keys use 15 degree steps. Brush planes turn in `f64`, textures turn with the faces (their
//!   Valve 220 axes and offsets turn too, so nothing swims), entities that face somewhere turn
//!   their `angle`. `mapedit/mirror` `{axis "x"|"z", at?, copy?}` mirrors about the plane
//!   perpendicular to the axis at that coordinate (default: the middle of the selection);
//!   `copy` keeps the originals and selects the mirror images. `mapedit/array` `{count, offset}`:
//!   `count` copies, each `offset` further than the one before (selected).
//! - `mapedit/copy` puts the selection on the clipboard; `mapedit/paste` `{position? | offset?}`
//!   pastes it with its anchor (the middle of its footprint at its lowest point) at `position`,
//!   or moved by `offset` (default 1 m in +x); the pasted items are selected.
//!   `mapedit/save_prefab` `{name, overwrite?}` saves the selection as `prefabs/<name>.map` of
//!   the game's assets (`content/private/prefabs/` for a private Map); `mapedit/prefabs` lists
//!   them; `mapedit/place_prefab` `{name, position, rotate?}` puts one with its anchor at
//!   `position`, turned by `rotate` degrees.
//! - `mapedit/get_entity` `{item?}`: an entity's class, position, angle, key/values, and what its
//!   class accepts (each property with kind, choices, default and current value).
//!   `mapedit/set_entity_property` `{key, value}` (a null or empty value removes the key), or
//!   `{properties: {key: value}}`, and/or `{position [x,y,z]}` and `{angle}`: checked against what
//!   the entity's class accepts (a prop's `model` must exist, `team` is alpha, bravo or any, a
//!   light's `color` is `r g b`...) and applied to every selected entity as one step.
//! - `mapedit/add_brush` `{shape ("box"|"wedge"|"wall"|"slab"|"ramp"), min [x,y,z], max [x,y,z]
//!   (or centre + size), surface?, rise? ("+x"|"-x"|"+z"|"-z", wedge and ramp)}`: adds a brush to
//!   the worldspawn and selects it. `wall`, `slab` and `ramp` are the palette primitives; with
//!   `position` instead of bounds they are placed with their default size, bottom centre there.
//! - `mapedit/add_entity` `{class, position, angle?, props? {key: value}}`: adds one of the palette's
//!   entity classes (`spawn_point` with `team`, `modes`; `dom_flag` `label`; `hq_site`; `bomb_site`
//!   `label`; `ammo_crate`; `light_point`; `prop` with `model`, `collider`).
//! - `mapedit/undo`, `mapedit/redo`.
//! - `mapedit/playtest`: writes the working copy next to the Map's file and starts the game on it
//!   (`frontline --host --map-file ...`, the executable next to MapEdit or `MAPEDIT_GAME`).
//! - `mapedit/ui` `{menu? ("main"|"load"|"save_as"|"new"|"settings"|"close"), palette? (bool),
//!   tab? ("surfaces"|"brushes"|"entities"|"props"|"prefabs")}`: shows the Esc menu or the palette
//!   panel, e.g. for a screenshot. The entity panel (left, bottom) shows itself while a single
//!   entity is selected.
use std::path::PathBuf;

use bevy::ecs::message::Messages;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::remote::http::RemoteHttpPlugin;
use bevy::remote::{BrpError, BrpResult, RemotePlugin, error_codes};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use frontline::atmosphere::{LookOverride, Looks};
use frontline::maps::{
    AmmoCrate, BombSite, DomFlag, HqSite, IdleMap, MapId, MapReady, MapRegistry, SpawnPoint,
    SpawnTeam,
};
use serde_json::{Value, json};

use frontline::data::GameData;

use crate::camera::{FlyCamera, GoTo, View};
use crate::edit::{Command, Editor, EntityChange, FaceRef, Failure, SettingsChange, execute};
use crate::mapfile::{Brush, Item, Rise};
use crate::overlay::{MODE_KEYS, Overlay};
use crate::palette;
use crate::selection::{Combine, Kind};
use crate::session::Reload;
use crate::transform::MirrorAxis;

pub struct ControlPlugin {
    pub port: u16,
}

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RemotePlugin::default()
                .with_method_main("mapedit/status", status)
                .with_method_main("mapedit/load_map", load_map)
                .with_method_main("mapedit/reload", reload)
                .with_method_main("mapedit/set_look", set_look)
                .with_method_main("mapedit/set_camera", set_camera)
                .with_method_main("mapedit/set_overlay", set_overlay)
                .with_method_main("mapedit/entities", entities)
                .with_method_main("mapedit/screenshot", screenshot)
                .with_method_main("mapedit/quit", quit)
                .with_method_main("mapedit/new_map", new_map)
                .with_method_main("mapedit/save", save)
                .with_method_main("mapedit/save_as", save_as)
                .with_method_main("mapedit/map_settings", map_settings)
                .with_method_main("mapedit/palette", palette)
                .with_method_main("mapedit/items", items)
                .with_method_main("mapedit/select", select)
                .with_method_main("mapedit/move", move_item)
                .with_method_main("mapedit/resize", resize)
                .with_method_main("mapedit/delete", delete)
                .with_method_main("mapedit/duplicate", duplicate)
                .with_method_main("mapedit/set_texture", set_texture)
                .with_method_main("mapedit/rotate", rotate)
                .with_method_main("mapedit/mirror", mirror)
                .with_method_main("mapedit/array", array)
                .with_method_main("mapedit/copy", copy)
                .with_method_main("mapedit/paste", paste)
                .with_method_main("mapedit/save_prefab", save_prefab)
                .with_method_main("mapedit/prefabs", prefabs)
                .with_method_main("mapedit/place_prefab", place_prefab)
                .with_method_main("mapedit/get_entity", get_entity)
                .with_method_main("mapedit/set_entity_property", set_entity_property)
                .with_method_main("mapedit/add_brush", add_brush)
                .with_method_main("mapedit/add_entity", add_entity)
                .with_method_main("mapedit/ui", ui)
                .with_method_main("mapedit/playtest", playtest)
                .with_method_main("mapedit/undo", undo)
                .with_method_main("mapedit/redo", redo),
            RemoteHttpPlugin::default().with_port(self.port),
        ));
    }
}

fn invalid(message: impl Into<String>) -> BrpError {
    BrpError {
        code: error_codes::INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}

fn params(params: Option<Value>) -> Value {
    params.unwrap_or_else(|| json!({}))
}

fn vec3(value: &Value, name: &str) -> BrpResult<Vec3> {
    let numbers: Option<Vec<f32>> = value.get(name).and_then(Value::as_array).map(|array| {
        array
            .iter()
            .filter_map(|n| n.as_f64())
            .map(|n| n as f32)
            .collect()
    });
    match numbers.as_deref() {
        Some([x, y, z]) => Ok(Vec3::new(*x, *y, *z)),
        _ => Err(invalid(format!("`{name}` must be [x, y, z] in metres"))),
    }
}

fn status(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let map = world.resource::<IdleMap>().0.0.clone();
    let mut ready = world.query::<&MapReady>();
    let ready = ready
        .iter(world)
        .next()
        .map(|ready| json!({ "name": ready.name, "look": ready.look }));
    let count = |world: &mut World, team: Option<SpawnTeam>| {
        let mut spawns = world.query::<&SpawnPoint>();
        spawns
            .iter(world)
            .filter(|spawn| team.is_none_or(|team| spawn.team == team))
            .count()
    };
    let spawns = json!({
        "total": count(world, None),
        "alpha": count(world, Some(SpawnTeam::Alpha)),
        "bravo": count(world, Some(SpawnTeam::Bravo)),
        "any": count(world, Some(SpawnTeam::Any)),
    });
    let flags = world.query::<&DomFlag>().iter(world).count();
    let hq_sites = world.query::<&HqSite>().iter(world).count();
    let bomb_sites = world.query::<&BombSite>().iter(world).count();
    let ammo_crates = world.query::<&AmmoCrate>().iter(world).count();
    let camera = world
        .query::<(&FlyCamera, &Transform)>()
        .iter(world)
        .next()
        .map(|(fly, transform)| {
            let p = transform.translation;
            json!({
                "position": [p.x, p.y, p.z],
                "yaw": fly.yaw.to_degrees(),
                "pitch": fly.pitch.to_degrees(),
                "speed": fly.speed,
            })
        });
    let overlay = world.resource::<Overlay>();
    let overlay = json!({
        "mode": overlay.mode.unwrap_or("all"),
        "labels": overlay.labels,
        "through_walls": overlay.through_walls,
        "colliders": overlay.colliders,
        "fog": overlay.fog,
    });
    let maps: Vec<Value> = world
        .resource::<MapRegistry>()
        .iter()
        .map(|(id, info)| json!({ "id": id.0, "name": info.name, "file": info.file, "private": info.private }))
        .collect();
    let looks = world.resource::<Looks>().names();
    Ok(json!({
        "map": map,
        "ready": ready,
        "look_override": world.resource::<LookOverride>().0,
        "spawns": spawns,
        "flags": flags,
        "hq_sites": hq_sites,
        "bomb_sites": bomb_sites,
        "ammo_crates": ammo_crates,
        "camera": camera,
        "overlay": overlay,
        "maps": maps,
        "looks": looks,
        "editor": editor_status(world),
    }))
}

fn load_map(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`id` (a Map id) is required"))?;
    let command = Command::Open {
        id: MapId::new(id),
        discard: flag(&args, "discard"),
    };
    let message = run(world, command)?;
    Ok(json!({ "loading": id, "message": message, "editor": editor_status(world) }))
}

fn quit(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let discard = flag(&params(args), "discard");
    run(world, Command::Quit { discard })?;
    Ok(json!({ "quitting": true }))
}

fn request_reload(world: &mut World) {
    world.resource_mut::<Messages<Reload>>().write(Reload);
}

fn reload(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let id = world.resource::<IdleMap>().0.clone();
    let discard = flag(&params(args), "discard");
    let message = run(world, Command::Open { id, discard })?;
    Ok(json!({ "reloading": true, "message": message }))
}

fn set_look(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let name = match args.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.clone()),
        Some(_) => {
            return Err(invalid(
                "`name` must be a look name, or null for the Map's own",
            ));
        }
    };
    if let Some(name) = &name {
        let looks = world.resource::<Looks>();
        if !looks.contains(name) {
            return Err(invalid(format!(
                "unknown look \"{name}\"; known: {}",
                looks.known_names()
            )));
        }
    }
    world.resource_mut::<LookOverride>().0 = name.clone();
    request_reload(world);
    Ok(json!({ "look": name }))
}

fn set_camera(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let view = match args.get("preset").and_then(Value::as_str) {
        Some("top") => View::TOP,
        Some("aerial") => View::AERIAL,
        Some(other) => {
            return Err(invalid(format!(
                "unknown preset \"{other}\"; known: top, aerial"
            )));
        }
        None => {
            let position = vec3(&args, "position")?;
            if args.get("look_at").is_some() {
                let direction = vec3(&args, "look_at")? - position;
                if direction.length_squared() < 1e-6 {
                    return Err(invalid("`look_at` is the camera position"));
                }
                // Forward is -z at yaw 0, and turns with yaw about +y.
                View {
                    position,
                    yaw: (-direction.x).atan2(-direction.z),
                    pitch: (direction.y / direction.length()).asin(),
                }
            } else {
                let angle = |name: &str| {
                    args.get(name)
                        .and_then(Value::as_f64)
                        .map(|degrees| (degrees as f32).to_radians())
                        .ok_or_else(|| {
                            invalid(format!("`{name}` (degrees) is required without `look_at`"))
                        })
                };
                View {
                    position,
                    yaw: angle("yaw")?,
                    pitch: angle("pitch")?,
                }
            }
        }
    };
    world.resource_mut::<Messages<GoTo>>().write(GoTo(view));
    Ok(json!({
        "position": [view.position.x, view.position.y, view.position.z],
        "yaw": view.yaw.to_degrees(),
        "pitch": view.pitch.to_degrees(),
    }))
}

fn set_overlay(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let mut overlay = world.resource_mut::<Overlay>();
    if let Some(mode) = args.get("mode").and_then(Value::as_str) {
        overlay.mode = match mode {
            "all" => None,
            key => match MODE_KEYS.iter().find(|known| **known == key) {
                Some(known) => Some(*known),
                None => {
                    return Err(invalid(format!(
                        "unknown mode \"{key}\"; known: all, {}",
                        MODE_KEYS.join(", ")
                    )));
                }
            },
        };
    }
    let flag = |name: &str| args.get(name).and_then(Value::as_bool);
    if let Some(on) = flag("labels") {
        overlay.labels = on;
    }
    if let Some(on) = flag("through_walls") {
        overlay.through_walls = on;
    }
    if let Some(on) = flag("colliders") {
        overlay.colliders = on;
    }
    if let Some(on) = flag("fog") {
        overlay.fog = on;
    }
    Ok(json!({
        "mode": overlay.mode.unwrap_or("all"),
        "labels": overlay.labels,
        "through_walls": overlay.through_walls,
        "colliders": overlay.colliders,
        "fog": overlay.fog,
    }))
}

fn entities(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut found: Vec<Value> = Vec::new();
    let at = |transform: &GlobalTransform| {
        let p = transform.translation();
        [p.x, p.y, p.z]
    };
    let mut spawns = world.query::<(&SpawnPoint, &GlobalTransform)>();
    for (spawn, transform) in spawns.iter(world) {
        let forward = transform.compute_transform().rotation * Vec3::NEG_Z;
        found.push(json!({
            "kind": "spawn_point",
            "team": format!("{:?}", spawn.team).to_lowercase(),
            "modes": spawn.mode_keys(),
            "position": at(transform),
            "facing_yaw": (-forward.x).atan2(-forward.z).to_degrees(),
        }));
    }
    let mut flags = world.query::<(&DomFlag, &GlobalTransform)>();
    for (flag, transform) in flags.iter(world) {
        found.push(json!({ "kind": "dom_flag", "label": flag.label.letter().to_string(), "position": at(transform) }));
    }
    let mut hqs = world.query_filtered::<&GlobalTransform, With<HqSite>>();
    for transform in hqs.iter(world) {
        found.push(json!({ "kind": "hq_site", "position": at(transform) }));
    }
    let mut bombs = world.query::<(&BombSite, &GlobalTransform)>();
    for (bomb, transform) in bombs.iter(world) {
        found.push(json!({ "kind": "bomb_site", "label": bomb.label.letter().to_string(), "position": at(transform) }));
    }
    let mut crates = world.query_filtered::<&GlobalTransform, With<AmmoCrate>>();
    for transform in crates.iter(world) {
        found.push(json!({ "kind": "ammo_crate", "position": at(transform) }));
    }
    Ok(json!({ "entities": found }))
}

fn screenshot(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`path` (a .png file) is required"))?;
    let path = PathBuf::from(path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    info!("Screenshot {} (remote)", path.display());
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    Ok(json!({ "path": path, "written_after_frames": 2 }))
}

// ------------------------------------------------------------------------------- editing

fn flag(args: &Value, name: &str) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// Runs a file command and turns a refusal into an error answer.
fn run(world: &mut World, command: Command) -> BrpResult<String> {
    execute(world, command).map_err(|failure| match failure {
        Failure::Dirty => invalid("there are unsaved changes: save first, or pass `discard: true`"),
        Failure::Message(message) => invalid(message),
    })
}

fn item_json(item: Item) -> Value {
    match item {
        Item::Brush { entity, brush } => json!({ "entity": entity, "brush": brush }),
        Item::Entity(entity) => json!({ "entity": entity }),
    }
}

fn items_json(items: &[Item]) -> Value {
    Value::Array(items.iter().copied().map(item_json).collect())
}

fn item_of(value: &Value) -> BrpResult<Item> {
    let entity = value
        .get("entity")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("an item is {\"entity\": n} or {\"entity\": n, \"brush\": m}"))?
        as usize;
    Ok(match value.get("brush").and_then(Value::as_u64) {
        Some(brush) => Item::Brush {
            entity,
            brush: brush as usize,
        },
        None => Item::Entity(entity),
    })
}

fn item_arg(args: &Value, key: &str) -> BrpResult<Option<Item>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => item_of(value).map(Some),
    }
}

fn items_arg(args: &Value, key: &str) -> BrpResult<Option<Vec<Item>>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(list)) => list.iter().map(item_of).collect::<BrpResult<_>>().map(Some),
        Some(_) => Err(invalid(format!(
            "`{key}` is a list of items: [{{\"entity\": 0, \"brush\": 3}}, ...]"
        ))),
    }
}

/// What an edit acts on: `item`, or `items` (which become the selection first), else the
/// selection itself (`None`).
fn target_arg(world: &mut World, args: &Value) -> BrpResult<Option<Item>> {
    if let Some(items) = items_arg(args, "items")? {
        world
            .resource_mut::<Editor>()
            .select_items(&items, Combine::Replace)
            .map_err(invalid)?;
        return Ok(None);
    }
    item_arg(args, "item")
}

fn dvec3(value: &Value, name: &str) -> BrpResult<DVec3> {
    let numbers: Option<Vec<f64>> = value
        .get(name)
        .and_then(Value::as_array)
        .map(|array| array.iter().filter_map(Value::as_f64).collect());
    match numbers.as_deref() {
        Some([x, y, z]) => Ok(DVec3::new(*x, *y, *z)),
        _ => Err(invalid(format!("`{name}` must be [x, y, z] in metres"))),
    }
}

fn dvec3_json(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn number(args: &Value, name: &str) -> BrpResult<f64> {
    args.get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid(format!("`{name}` (a number) is required")))
}

/// A key/value of an entity as text: strings, numbers and booleans.
fn text_value(value: &Value, what: &str) -> BrpResult<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(invalid(format!("`{what}` is a string, number or boolean"))),
    }
}

fn editor_status(world: &World) -> Value {
    let editor = world.resource::<Editor>();
    let (brushes, entities) = editor.map.as_ref().map_or((0, 0), |map| {
        (
            map.entities.iter().map(|e| e.brushes.len()).sum::<usize>(),
            map.entities.len().saturating_sub(1),
        )
    });
    let bounds = editor.selection_bounds();
    json!({
        "open": editor.map.is_some(),
        "problem": editor.problem,
        "map": editor.id.0,
        "file": editor.source,
        "dirty": editor.dirty(),
        "settings_dirty": editor.settings_dirty,
        "revision": editor.revision,
        "synced": editor.synced(),
        "selection": items_json(editor.selection.items()),
        "selection_count": editor.selection.len(),
        "selection_text": (!editor.selection.is_empty()).then(|| editor.describe_many(editor.selection.items())),
        "selection_min": bounds.map(|(min, _)| dvec3_json(min)),
        "selection_max": bounds.map(|(_, max)| dvec3_json(max)),
        "selection_size": bounds.map(|(min, max)| dvec3_json(max - min)),
        "clipboard": editor.clipboard.as_ref().map(|c| c.items().len()),
        "grid": editor.grid,
        "surface": editor.surface.name,
        "can_undo": editor.can_undo(),
        "can_redo": editor.can_redo(),
        "brushes": brushes,
        "entities": entities,
        "generator": editor.generator(),
        "notice": editor.notice.as_ref().map(|n| n.text.clone()),
        "menu": world.resource::<crate::menu::MenuState>().screen_name(),
        "palette": world.resource::<palette::PaletteState>().open,
    })
}

/// The answer to an edit: what happened and where the editor stands.
fn edit_reply(world: &mut World, result: Result<String, String>) -> BrpResult {
    let mut editor = world.resource_mut::<Editor>();
    editor.report(&result);
    match result {
        Ok(message) => Ok(json!({
            "message": message,
            "selection": items_json(editor.selection.items()),
            "dirty": editor.dirty(),
            "revision": editor.revision,
            "synced": editor.synced(),
        })),
        Err(message) => Err(invalid(message)),
    }
}

fn new_map(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`id` is required: lowercase letters, digits and _"))?;
    let name = args.get("name").and_then(Value::as_str).unwrap_or("");
    let message = run(
        world,
        Command::NewMap {
            id: id.to_owned(),
            name: name.to_owned(),
            discard: flag(&args, "discard"),
        },
    )?;
    Ok(json!({ "message": message, "editor": editor_status(world) }))
}

fn save(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let message = run(world, Command::Save)?;
    let generator = world.resource::<Editor>().generator();
    Ok(json!({
        "message": message,
        "generator": generator,
        "dirty": world.resource::<Editor>().dirty(),
    }))
}

fn save_as(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`id` is required: lowercase letters, digits and _"))?;
    let name = args.get("name").and_then(Value::as_str).unwrap_or("");
    let message = run(
        world,
        Command::SaveAs {
            id: id.to_owned(),
            name: name.to_owned(),
        },
    )?;
    Ok(json!({ "message": message, "editor": editor_status(world) }))
}

fn settings_json(world: &World) -> Value {
    let editor = world.resource::<Editor>();
    let info = world.resource::<MapRegistry>().get(&editor.id);
    let world_look = editor
        .map
        .as_ref()
        .and_then(|m| m.worldspawn())
        .and_then(|w| w.get("look"));
    json!({
        "id": editor.id.0,
        "name": info.map(|i| i.name.clone()),
        "look": info.and_then(|i| i.look.clone()),
        "worldspawn_look": world_look,
        "modes": info.map(|i| i.modes.clone()),
        "factions": info.map(|i| i.factions.clone()),
        "file": editor.source,
        "settings_dirty": editor.settings_dirty,
    })
}

fn map_settings(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let mut change = SettingsChange::default();
    if let Some(name) = args.get("name") {
        change.name = Some(
            name.as_str()
                .ok_or_else(|| invalid("`name` is a string"))?
                .to_owned(),
        );
    }
    match args.get("look") {
        None => {}
        Some(Value::Null) => change.look = Some(None),
        Some(Value::String(look)) => change.look = Some(Some(look.clone())),
        Some(_) => return Err(invalid("`look` is a look name, or null for the Map's own")),
    }
    if let Some(modes) = args.get("modes") {
        let list = modes
            .as_array()
            .ok_or_else(|| invalid("`modes` is an array of Game Mode keys"))?;
        change.modes = Some(
            list.iter()
                .filter_map(|m| m.as_str().map(str::to_owned))
                .collect(),
        );
    }
    if let Some(factions) = args.get("factions") {
        let list: Vec<String> = factions
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let [alpha, bravo] = <[String; 2]>::try_from(list)
            .map_err(|_| invalid("`factions` is [alpha faction id, bravo faction id]"))?;
        change.factions = Some([alpha, bravo]);
    }
    let changed = change.name.is_some()
        || change.look.is_some()
        || change.modes.is_some()
        || change.factions.is_some();
    let message = if changed {
        Some(run(world, Command::Settings(change))?)
    } else {
        None
    };
    Ok(json!({ "message": message, "settings": settings_json(world) }))
}

fn palette(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let looks: Vec<String> = world
        .resource::<Looks>()
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let looks: Vec<&str> = looks.iter().map(String::as_str).collect();
    let factions: Vec<(String, String)> = world
        .resource::<GameData>()
        .factions
        .iter()
        .map(|f| (f.id.clone(), f.name.clone()))
        .collect();
    let editor = world.resource::<Editor>();
    Ok(palette::to_json(&editor.props, &looks, &factions))
}

fn items(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("all");
    let class = args.get("class").and_then(Value::as_str);
    let near = if args.get("near").is_some() {
        Some((
            dvec3(&args, "near")?,
            args.get("radius").and_then(Value::as_f64).unwrap_or(5.0),
        ))
    } else {
        None
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
    let editor = world.resource::<Editor>();
    let map = editor
        .map
        .as_ref()
        .ok_or_else(|| invalid("no Map is open for editing"))?;
    let mut found = Vec::new();
    for item in map.items() {
        let entity = map.entity(item).expect("listed items exist");
        let is_brush = matches!(item, Item::Brush { .. });
        if (kind == "brush" && !is_brush) || (kind == "entity" && is_brush) {
            continue;
        }
        if class.is_some_and(|c| !is_brush && entity.classname() != c)
            || class.is_some_and(|_| is_brush)
        {
            continue;
        }
        let bounds = map.bounds(item, &editor.prop_bounds);
        if let Some((centre, radius)) = near {
            let Some((min, max)) = bounds else { continue };
            let nearest = centre.clamp(min, max);
            if nearest.distance(centre) > radius {
                continue;
            }
        }
        let mut entry = json!({ "item": item_json(item) });
        if let Some((min, max)) = bounds {
            entry["min"] = dvec3_json(min);
            entry["max"] = dvec3_json(max);
        }
        match item {
            Item::Brush { entity: e, brush } => {
                let b: &Brush = map.brush(e, brush).expect("listed");
                entry["kind"] = json!("brush");
                entry["entity_class"] = json!(entity.classname());
                entry["surface"] = json!(b.surface().map(|s| s.name));
                entry["faces"] = json!(b.faces.len());
            }
            Item::Entity(_) => {
                entry["kind"] = json!("entity");
                entry["class"] = json!(entity.classname());
                if let Some(origin) = entity.origin() {
                    entry["position"] = dvec3_json(origin);
                }
                let props: serde_json::Map<String, Value> = entity
                    .props
                    .iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "classname" | "origin"))
                    .map(|(k, v)| (k.clone(), json!(v)))
                    .collect();
                entry["props"] = Value::Object(props);
            }
        }
        found.push(entry);
    }
    let total = found.len();
    let page: Vec<Value> = found.into_iter().skip(offset).take(limit).collect();
    Ok(json!({ "total": total, "offset": offset, "items": page }))
}

/// The kind of items a `select` call names, when it names one.
fn kind_arg(args: &Value) -> BrpResult<Option<Kind>> {
    if flag(args, "all") {
        return Ok(Some(Kind::Everything));
    }
    if let Some(kind) = args.get("kind").and_then(Value::as_str) {
        return match kind {
            "all" => Ok(Some(Kind::Everything)),
            "brush" | "brushes" => Ok(Some(Kind::Brushes)),
            "entity" | "entities" => Ok(Some(Kind::Entities)),
            other => Err(invalid(format!(
                "unknown `kind` \"{other}\"; known: all, brush, entity (or give `class` or `surface`)"
            ))),
        };
    }
    if let Some(class) = args.get("class").and_then(Value::as_str) {
        return Ok(Some(Kind::Class(class.to_owned())));
    }
    if let Some(name) = args.get("surface").and_then(Value::as_str) {
        if crate::mapfile::surface(name).is_none() {
            return Err(invalid(format!(
                "unknown surface \"{name}\"; see mapedit/palette"
            )));
        }
        return Ok(Some(Kind::Surface(name.to_owned())));
    }
    Ok(item_arg(args, "like")?.map(Kind::Like))
}

fn select(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let how = match args.get("mode").and_then(Value::as_str) {
        None => Combine::Replace,
        Some(key) => Combine::from_key(key)
            .ok_or_else(|| invalid("`mode` is \"replace\", \"add\" or \"toggle\""))?,
    };
    let kind = kind_arg(&args)?;
    let result = if flag(&args, "clear") {
        world.resource_mut::<Editor>().select(None)
    } else if let Some(items) = items_arg(&args, "items")? {
        world.resource_mut::<Editor>().select_items(&items, how)
    } else if let Some(item) = item_arg(&args, "item")? {
        world.resource_mut::<Editor>().select_items(&[item], how)
    } else if let Some(area) = args.get("box") {
        let (min, max) = (dvec3(area, "min")?, dvec3(area, "max")?);
        let flat = flag(area, "flat");
        let mut editor = world.resource_mut::<Editor>();
        editor
            .find_in_box(min, max, flat)
            .and_then(|items| editor.select_items(&items, how))
    } else if let Some(rect) = args.get("screen_box").and_then(Value::as_array) {
        let numbers: Vec<f32> = rect
            .iter()
            .filter_map(Value::as_f64)
            .map(|n| n as f32)
            .collect();
        let [x0, y0, x1, y1] = numbers[..] else {
            return Err(invalid("`screen_box` is [x0, y0, x1, y1] in window pixels"));
        };
        let (low, high) = (
            Vec2::new(x0.min(x1), y0.min(y1)),
            Vec2::new(x0.max(x1), y0.max(y1)),
        );
        let mut cameras = world.query_filtered::<(&Camera, &GlobalTransform), With<FlyCamera>>();
        let (camera, transform) = cameras.single(world).map_err(|_| invalid("no camera"))?;
        let (camera, transform) = (camera.clone(), *transform);
        let mut editor = world.resource_mut::<Editor>();
        editor
            .find_where(|min, max| {
                crate::selection::corners_inside(
                    min,
                    max,
                    |p| camera.world_to_viewport(&transform, p.as_vec3()).ok(),
                    low,
                    high,
                )
            })
            .and_then(|items| editor.select_items(&items, how))
    } else if let Some(kind) = kind {
        let mut editor = world.resource_mut::<Editor>();
        editor
            .find_kind(&kind)
            .and_then(|items| editor.select_items(&items, how))
    } else {
        let (origin, direction) = if let Some(ray) = args.get("ray") {
            (dvec3(ray, "origin")?, dvec3(ray, "direction")?)
        } else if let Some(screen) = args.get("screen").and_then(Value::as_array) {
            let (Some(x), Some(y)) = (
                screen.first().and_then(Value::as_f64),
                screen.get(1).and_then(Value::as_f64),
            ) else {
                return Err(invalid("`screen` is [x, y] in window pixels"));
            };
            let mut cameras =
                world.query_filtered::<(&Camera, &GlobalTransform), With<FlyCamera>>();
            let (camera, transform) = cameras.single(world).map_err(|_| invalid("no camera"))?;
            let ray = camera
                .viewport_to_world(transform, Vec2::new(x as f32, y as f32))
                .map_err(|err| invalid(format!("that pixel is not in the view: {err}")))?;
            (ray.origin.as_dvec3(), ray.direction.as_dvec3())
        } else {
            return Err(invalid(
                "give `item`, `items`, `box`, `screen_box`, `all`, `kind`, `class`, `surface`, `like`, `ray` {origin, direction}, `screen` [x, y] or `clear: true`",
            ));
        };
        let mut editor = world.resource_mut::<Editor>();
        match editor.pick(origin, direction) {
            Some(hit) => editor.select_items(&[hit.item], how).map(|text| {
                format!(
                    "{text}; hit {:.2} m away on the face towards {:.0}, {:.0}, {:.0}",
                    hit.distance, hit.normal.x, hit.normal.y, hit.normal.z
                )
            }),
            None => {
                if how == Combine::Replace {
                    editor.selection.clear();
                }
                Err("the ray hit nothing".into())
            }
        }
    };
    let editor = world.resource::<Editor>();
    match result {
        Ok(message) => Ok(json!({
            "message": message,
            "selection": items_json(editor.selection.items()),
            "selection_count": editor.selection.len(),
        })),
        Err(message) => Err(invalid(message)),
    }
}

fn move_item(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let delta = dvec3(&args, "delta")?;
    let result = world.resource_mut::<Editor>().move_item(item, delta);
    edit_reply(world, result)
}

/// A face of a brush, as a call names it: its index, a side (`+x`, `-y`...), or a direction.
fn face_arg(value: &Value) -> BrpResult<FaceRef> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .map(|i| FaceRef::Index(i as usize))
            .ok_or_else(|| invalid("a face index is a whole number")),
        Value::String(side) => {
            let (sign, axis) = side.split_at(side.len().min(1));
            let sign = match sign {
                "+" => 1.0,
                "-" => -1.0,
                _ => {
                    return Err(invalid(
                        "`face` is an index, a side (\"+x\", \"-x\", \"+y\", \"-y\", \"+z\", \"-z\") or a direction [x, y, z]",
                    ));
                }
            };
            let direction = match axis {
                "x" => DVec3::X,
                "y" => DVec3::Y,
                "z" => DVec3::Z,
                _ => {
                    return Err(invalid(
                        "a side is \"+x\", \"-x\", \"+y\", \"-y\", \"+z\" or \"-z\"",
                    ));
                }
            };
            Ok(FaceRef::Towards(direction * sign))
        }
        Value::Array(_) => {
            let direction = dvec3(&json!({ "face": value }), "face")?;
            if direction.length() < 1e-9 {
                return Err(invalid("a direction cannot be [0, 0, 0]"));
            }
            Ok(FaceRef::Towards(direction))
        }
        _ => Err(invalid(
            "`face` is an index, a side or a direction [x, y, z]",
        )),
    }
}

fn resize(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let result = if args.get("min").is_some() {
        let (min, max) = (dvec3(&args, "min")?, dvec3(&args, "max")?);
        world.resource_mut::<Editor>().resize(item, min, max)
    } else if let Some(face) = args.get("face") {
        let face = face_arg(face)?;
        let amount = number(&args, "amount")?;
        world
            .resource_mut::<Editor>()
            .move_face(item, face, amount, flag(&args, "snap"))
    } else {
        return Err(invalid(
            "give `face` (index, \"+x\".. or [x, y, z]) and `amount` to move one face, or `min` and `max` to scale the brush to a box",
        ));
    };
    edit_reply(world, result)
}

fn delete(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let result = world.resource_mut::<Editor>().delete(item);
    edit_reply(world, result)
}

fn duplicate(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let offset = if args.get("offset").is_some() {
        dvec3(&args, "offset")?
    } else {
        DVec3::X
    };
    let result = world.resource_mut::<Editor>().duplicate(item, offset);
    edit_reply(world, result)
}

fn set_texture(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let surface = args
        .get("surface")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`surface` is required (see mapedit/palette)"))?;
    let result = world.resource_mut::<Editor>().set_surface(item, surface);
    edit_reply(world, result)
}

fn rotate(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let degrees = number(&args, "degrees")?;
    let pivot = match args.get("pivot").and_then(Value::as_array) {
        None => None,
        Some(list) => {
            let numbers: Vec<f64> = list.iter().filter_map(Value::as_f64).collect();
            match numbers[..] {
                [x, z] => Some(DVec3::new(x, 0.0, z)),
                [x, y, z] => Some(DVec3::new(x, y, z)),
                _ => return Err(invalid("`pivot` is [x, z] (or [x, y, z]) in metres")),
            }
        }
    };
    let result = world.resource_mut::<Editor>().turn(item, degrees, pivot);
    edit_reply(world, result)
}

fn mirror(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let axis = args
        .get("axis")
        .and_then(Value::as_str)
        .and_then(MirrorAxis::from_key)
        .ok_or_else(|| invalid("`axis` is \"x\" or \"z\" (the plane is perpendicular to it)"))?;
    let at = args.get("at").and_then(Value::as_f64);
    let result = world
        .resource_mut::<Editor>()
        .mirror(item, axis, at, flag(&args, "copy"));
    edit_reply(world, result)
}

fn array(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let count = args
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("`count` (a whole number of copies) is required"))?
        as usize;
    let offset = dvec3(&args, "offset")?;
    let result = world.resource_mut::<Editor>().array(item, count, offset);
    edit_reply(world, result)
}

fn copy(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let result = world.resource_mut::<Editor>().copy(item);
    edit_reply(world, result)
}

fn paste(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let mut editor = world.resource_mut::<Editor>();
    let result = if args.get("position").is_some() {
        // The clipboard's anchor (the middle of its footprint, at its lowest point) goes there.
        let point = dvec3(&args, "position")?;
        match editor.clipboard_anchor() {
            Some(anchor) => editor.paste(point - anchor),
            None => Err("the clipboard is empty (copy first)".into()),
        }
    } else if args.get("offset").is_some() {
        editor.paste(dvec3(&args, "offset")?)
    } else {
        editor.paste(DVec3::X)
    };
    edit_reply(world, result)
}

fn save_prefab(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`name` is required: letters, digits, _ and -"))?;
    let result = world
        .resource_mut::<Editor>()
        .save_prefab(item, name, flag(&args, "overwrite"));
    edit_reply(world, result)
}

fn prefabs(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let editor = world.resource::<Editor>();
    let list: Vec<Value> = editor
        .prefabs()
        .into_iter()
        .map(|p| json!({ "name": p.name, "private": p.private }))
        .collect();
    Ok(json!({
        "prefabs": list,
        "folder": editor.paths.prefab_folder(false).ok(),
        "private_folder": editor.paths.prefab_folder(true).ok(),
    }))
}

fn place_prefab(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`name` is required (see mapedit/prefabs)"))?;
    let position = dvec3(&args, "position")?;
    let degrees = args.get("rotate").and_then(Value::as_f64).unwrap_or(0.0);
    let result = world
        .resource_mut::<Editor>()
        .place_prefab(name, position, degrees);
    edit_reply(world, result)
}

fn get_entity(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let editor = world.resource::<Editor>();
    let item = editor.resolve(item).map_err(invalid)?;
    let Item::Entity(index) = item else {
        return Err(invalid(
            "that is a brush; use mapedit/items for brushes (entities have properties)",
        ));
    };
    let map = editor.map.as_ref().expect("resolved");
    let entity = &map.entities[index];
    let class = palette::class(entity.classname());
    let properties: serde_json::Map<String, Value> = entity
        .props
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "classname" | "origin"))
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    // What the class accepts: each property with its kind, choices, default and current value.
    let schema = class.map(|class| {
        class
            .properties
            .iter()
            .map(|p| {
                json!({
                    "key": p.key,
                    "help": p.help,
                    "kind": p.kind.name(),
                    "choices": p.choices,
                    "default": class.defaults.iter().find(|(k, _)| *k == p.key).map(|(_, v)| *v),
                    "value": entity.get(p.key),
                })
            })
            .collect::<Vec<_>>()
    });
    let bounds = map.bounds(item, &editor.prop_bounds);
    Ok(json!({
        "item": item_json(item),
        "class": entity.classname(),
        "known_class": class.is_some(),
        "purpose": class.map(|c| c.purpose),
        "position": entity.origin().map(dvec3_json),
        "angle": entity.get("angle").map(|_| entity.angle()),
        "properties": properties,
        "schema": schema,
        "min": bounds.map(|(min, _)| dvec3_json(min)),
        "max": bounds.map(|(_, max)| dvec3_json(max)),
    }))
}

fn set_entity_property(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = target_arg(world, &args)?;
    let mut change = EntityChange::default();
    // `value: null` (or "") takes the key off.
    let mut set = |key: &str, value: &Value| -> BrpResult<()> {
        let text = match value {
            Value::Null => String::new(),
            other => text_value(other, key)?,
        };
        change.set.push((key.to_owned(), text));
        Ok(())
    };
    if let Some(key) = args.get("key").and_then(Value::as_str) {
        set(key, args.get("value").unwrap_or(&Value::Null))?;
    }
    if let Some(object) = args.get("properties").and_then(Value::as_object) {
        for (key, value) in object {
            set(key, value)?;
        }
    }
    if args.get("position").is_some() {
        change.position = Some(dvec3(&args, "position")?);
    }
    if let Some(angle) = args.get("angle").or_else(|| args.get("yaw")) {
        change.angle = Some(
            angle
                .as_f64()
                .ok_or_else(|| invalid("`angle` is a number of degrees"))?,
        );
    }
    if change.set.is_empty() && change.position.is_none() && change.angle.is_none() {
        return Err(invalid(
            "give `key` and `value`, `properties` {key: value}, `position` or `angle`",
        ));
    }
    let result = world.resource_mut::<Editor>().edit_entity(item, change);
    edit_reply(world, result)
}

fn playtest(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let message = run(world, Command::Playtest)?;
    Ok(json!({ "message": message }))
}
fn add_brush(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let shape = args
        .get("shape")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`shape` is required: box, wedge, wall, slab or ramp"))?;
    let rise = match args.get("rise").and_then(Value::as_str) {
        None => Rise::PosX,
        Some(key) => Rise::from_key(key)
            .ok_or_else(|| invalid("`rise` is \"+x\", \"-x\", \"+z\" or \"-z\""))?,
    };
    let primitive = match shape {
        "box" | "wedge" => None,
        key => Some(
            palette::primitive(key)
                .ok_or_else(|| invalid("`shape` is box, wedge, wall, slab or ramp"))?,
        ),
    };
    let (min, max) = if args.get("min").is_some() {
        (dvec3(&args, "min")?, dvec3(&args, "max")?)
    } else if args.get("centre").is_some() || args.get("center").is_some() {
        let key = if args.get("centre").is_some() {
            "centre"
        } else {
            "center"
        };
        let (centre, size) = (dvec3(&args, key)?, dvec3(&args, "size")?);
        (centre - size / 2.0, centre + size / 2.0)
    } else if let (Some(primitive), true) = (primitive, args.get("position").is_some()) {
        let size = primitive.extents(rise);
        let position = dvec3(&args, "position")?;
        let min = position - DVec3::new(size.x / 2.0, 0.0, size.z / 2.0);
        (min, min + size)
    } else {
        return Err(invalid(
            "give `min` and `max`, or `centre` and `size`, or (wall, slab, ramp) `position`",
        ));
    };
    let mut editor = world.resource_mut::<Editor>();
    let surface = match args.get("surface").and_then(Value::as_str) {
        None => editor.surface,
        Some(name) => crate::mapfile::surface(name)
            .ok_or_else(|| invalid(format!("unknown surface \"{name}\"; see mapedit/palette")))?,
    };
    let ramp = shape == "wedge" || primitive.is_some_and(|p| p.ramp);
    let result = if ramp {
        editor.add_ramp(min, max, rise, surface)
    } else {
        editor.add_box(min, max, surface)
    };
    edit_reply(world, result)
}

fn add_entity(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let class = args
        .get("class")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`class` is required (see mapedit/palette)"))?;
    let position = dvec3(&args, "position")?;
    let angle = args
        .get("angle")
        .or_else(|| args.get("yaw"))
        .and_then(Value::as_f64);
    let mut props = Vec::new();
    if let Some(object) = args.get("props").and_then(Value::as_object) {
        for (key, value) in object {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => {
                    return Err(invalid(format!(
                        "`props.{key}` is a string, number or boolean"
                    )));
                }
            };
            props.push((key.clone(), text));
        }
    }
    let result = world
        .resource_mut::<Editor>()
        .add_entity(class, position, angle, &props);
    edit_reply(world, result)
}

fn undo(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let result = world.resource_mut::<Editor>().undo();
    edit_reply(world, result)
}

fn redo(In(_): In<Option<Value>>, world: &mut World) -> BrpResult {
    let result = world.resource_mut::<Editor>().redo();
    edit_reply(world, result)
}

fn ui(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    if let Some(screen) = args.get("menu").and_then(Value::as_str) {
        crate::menu::show(world, screen).map_err(invalid)?;
    }
    if args.get("palette").is_some() || args.get("tab").is_some() {
        let open = args.get("palette").and_then(Value::as_bool).unwrap_or(true);
        palette::show(world, open, args.get("tab").and_then(Value::as_str)).map_err(invalid)?;
    }
    Ok(json!({ "ok": true }))
}
