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
//! Positions are game space: metres, y up, -z forward; yaw/angle in degrees, 0 faces -z. An
//! `item` is `{"entity": 0, "brush": 12}` (brush 12 of entity 0, the worldspawn) or
//! `{"entity": 5}` (a point entity); every method taking one defaults to the selection. Item
//! indices shift when something is deleted. Every edit answers `{message, selection, dirty, ...}`.
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
//!   properties, prop models with bounds, looks, Game Modes, Factions.
//! - `mapedit/items` `{kind? ("brush"|"entity"|"all"), class?, near? [x,y,z], radius?, offset?,
//!   limit? (default 100)}`: the brushes and point entities of the working copy, with bounds,
//!   surface or class and properties, and `total`.
//! - `mapedit/select` `{item}` | `{ray: {origin, direction}}` | `{screen: [x, y]}` (pixels of the
//!   window) | `{clear: true}`: sets the selection (highlighted in the view).
//! - `mapedit/move` `{item?, delta [x,y,z]}`, `mapedit/resize` `{item?, min, max}` (a brush's new
//!   bounding box) or `{item?, axis "x"|"y"|"z", side "+"|"-", amount}` (moves that face outward;
//!   negative inward), `mapedit/delete` `{item?}`, `mapedit/duplicate` `{item?, offset? [x,y,z]}`
//!   (default 1 m in +x; the copy is selected), `mapedit/set_texture` `{item?, surface}` (a surface
//!   name of the palette; a brush's faces get its real-world texture scale),
//!   `mapedit/set_property` `{item?, key, value}` (entity key/values such as `team`, `angle`;
//!   empty value removes the key).
//! - `mapedit/add_brush` `{shape ("box"|"wedge"|"wall"|"slab"|"ramp"), min [x,y,z], max [x,y,z]
//!   (or centre + size), surface?, rise? ("+x"|"-x"|"+z"|"-z", wedge and ramp)}`: adds a brush to
//!   the worldspawn and selects it. `wall`, `slab` and `ramp` are the palette primitives; with
//!   `position` instead of bounds they are placed with their default size, bottom centre there.
//! - `mapedit/add_entity` `{class, position, angle?, props? {key: value}}`: adds one of the palette's
//!   entity classes (`spawn_point` with `team`, `modes`; `dom_flag` `label`; `hq_site`; `bomb_site`
//!   `label`; `ammo_crate`; `light_point`; `prop` with `model`, `collider`).
//! - `mapedit/undo`, `mapedit/redo`.
//! - `mapedit/ui` `{menu? ("main"|"load"|"save_as"|"new"|"settings"|"close"), palette? (bool),
//!   tab? ("surfaces"|"brushes"|"entities"|"props")}`: shows the Esc menu or the palette panel,
//!   e.g. for a screenshot.

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
use crate::edit::{Command, Editor, Failure, SettingsChange, execute};
use crate::mapfile::{Brush, Item, Rise};
use crate::overlay::{MODE_KEYS, Overlay};
use crate::palette;
use crate::session::Reload;

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
                .with_method_main("mapedit/set_property", set_property)
                .with_method_main("mapedit/add_brush", add_brush)
                .with_method_main("mapedit/add_entity", add_entity)
                .with_method_main("mapedit/ui", ui)
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

fn item_arg(args: &Value, key: &str) -> BrpResult<Option<Item>> {
    let value = match args.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(value) => value,
    };
    let entity = value
        .get("entity")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("`item` is {\"entity\": n} or {\"entity\": n, \"brush\": m}"))?
        as usize;
    Ok(Some(match value.get("brush").and_then(Value::as_u64) {
        Some(brush) => Item::Brush {
            entity,
            brush: brush as usize,
        },
        None => Item::Entity(entity),
    }))
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

fn editor_status(world: &World) -> Value {
    let editor = world.resource::<Editor>();
    let (brushes, entities) = editor.map.as_ref().map_or((0, 0), |map| {
        (
            map.entities.iter().map(|e| e.brushes.len()).sum::<usize>(),
            map.entities.len().saturating_sub(1),
        )
    });
    json!({
        "open": editor.map.is_some(),
        "problem": editor.problem,
        "map": editor.id.0,
        "file": editor.source,
        "dirty": editor.dirty(),
        "settings_dirty": editor.settings_dirty,
        "revision": editor.revision,
        "synced": editor.synced(),
        "selection": editor.selection.map(item_json),
        "selection_text": editor.selection.map(|item| editor.describe(item)),
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
            "selection": editor.selection.map(item_json),
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

fn select(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let result = if flag(&args, "clear") {
        world.resource_mut::<Editor>().select(None)
    } else if let Some(item) = item_arg(&args, "item")? {
        world.resource_mut::<Editor>().select(Some(item))
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
                "give `item`, `ray` {origin, direction}, `screen` [x, y] or `clear: true`",
            ));
        };
        let mut editor = world.resource_mut::<Editor>();
        match editor.pick(origin, direction) {
            Some(hit) => editor.select(Some(hit.item)).map(|text| {
                format!(
                    "{text}; hit {:.2} m away on the face towards {:.0}, {:.0}, {:.0}",
                    hit.distance, hit.normal.x, hit.normal.y, hit.normal.z
                )
            }),
            None => {
                editor.selection = None;
                Err("the ray hit nothing; selection cleared".into())
            }
        }
    };
    let selected = world.resource::<Editor>().selection;
    match result {
        Ok(message) => Ok(json!({ "message": message, "selection": selected.map(item_json) })),
        Err(message) => Err(invalid(message)),
    }
}

fn move_item(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = item_arg(&args, "item")?;
    let delta = dvec3(&args, "delta")?;
    let result = world.resource_mut::<Editor>().move_item(item, delta);
    edit_reply(world, result)
}

fn resize(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = item_arg(&args, "item")?;
    let result = if args.get("min").is_some() {
        let (min, max) = (dvec3(&args, "min")?, dvec3(&args, "max")?);
        world.resource_mut::<Editor>().resize(item, min, max)
    } else {
        let axis = match args.get("axis").and_then(Value::as_str) {
            Some("x") => 0,
            Some("y") => 1,
            Some("z") => 2,
            _ => {
                return Err(invalid(
                    "give `min` and `max`, or `axis` (x, y, z), `side` (+, -) and `amount`",
                ));
            }
        };
        let positive = match args.get("side").and_then(Value::as_str) {
            Some("+") => true,
            Some("-") => false,
            _ => return Err(invalid("`side` is \"+\" or \"-\"")),
        };
        let amount = args
            .get("amount")
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid("`amount` (metres) is required"))?;
        world
            .resource_mut::<Editor>()
            .resize_face(item, axis, positive, amount)
    };
    edit_reply(world, result)
}

fn delete(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let item = item_arg(&params(args), "item")?;
    let result = world.resource_mut::<Editor>().delete(item);
    edit_reply(world, result)
}

fn duplicate(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = item_arg(&args, "item")?;
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
    let item = item_arg(&args, "item")?;
    let surface = args
        .get("surface")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`surface` is required (see mapedit/palette)"))?;
    let result = world.resource_mut::<Editor>().set_surface(item, surface);
    edit_reply(world, result)
}

fn set_property(In(args): In<Option<Value>>, world: &mut World) -> BrpResult {
    let args = params(args);
    let item = item_arg(&args, "item")?;
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`key` is required"))?;
    let value = match args.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => return Err(invalid("`value` is a string, number or boolean")),
    };
    let result = world
        .resource_mut::<Editor>()
        .set_property(item, key, &value);
    edit_reply(world, result)
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
