//! What can be placed: the surfaces, brush primitives, entity classes and prop models. The
//! catalogue (also served by the remote method `mapedit/palette`) comes first, then the panel
//! that shows it and places from it (key `P`).

use std::path::Path;

use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use frontline::ui::{
    ACCENT, AMBER, PANEL, PANEL_BORDER, TEXT_DIM, UiFonts, compact_button, label, rule,
};
use serde_json::{Value, json};

use crate::edit::{Editor, Place, PlaceRequest};
use crate::mapfile::{Item, MapEntity, PropBounds, Rise, SURFACES};
use crate::menu::{BlockPicking, InputCapture};

/// One property of an entity class.
pub struct Property {
    pub key: &'static str,
    pub help: &'static str,
    /// The values it accepts, when it is a choice (empty: free text or a number).
    pub choices: &'static [&'static str],
}

/// An entity class a Map can place (`crates/frontline/src/maps/classes.rs`).
pub struct ClassInfo {
    pub class: &'static str,
    pub purpose: &'static str,
    pub properties: &'static [Property],
    /// Key/values a new one starts with, after `classname` and `origin`.
    pub defaults: &'static [(&'static str, &'static str)],
    /// Metres above the clicked surface a new one is placed.
    pub lift: f64,
    /// Whether it faces somewhere (`angle`).
    pub faces: bool,
}

const TEAMS: &[&str] = &["alpha", "bravo", "any"];
const COLLIDERS: &[&str] = &["none", "box", "convex"];

pub const CLASSES: &[ClassInfo] = &[
    ClassInfo {
        class: "spawn_point",
        purpose: "Where players spawn: origin at the feet, angle the facing (0 faces -z).",
        properties: &[
            Property {
                key: "team",
                help: "Team allowed to spawn here",
                choices: TEAMS,
            },
            Property {
                key: "modes",
                help: "Game Mode keys (tdm dom hq sd) it serves; empty means every mode",
                choices: &[],
            },
            Property {
                key: "angle",
                help: "Facing in degrees",
                choices: &[],
            },
        ],
        defaults: &[("team", "any")],
        lift: 0.0,
        faces: true,
    },
    ClassInfo {
        class: "dom_flag",
        purpose: "Domination Flag; origin is the centre of its capture area on the ground.",
        properties: &[Property {
            key: "label",
            help: "Flag label",
            choices: &["A", "B", "C"],
        }],
        defaults: &[("label", "A")],
        lift: 0.0,
        faces: false,
    },
    ClassInfo {
        class: "hq_site",
        purpose: "Headquarters site; origin is the centre of the HQ on the ground.",
        properties: &[],
        defaults: &[],
        lift: 0.0,
        faces: false,
    },
    ClassInfo {
        class: "bomb_site",
        purpose: "Search & Destroy bomb site; origin is where the bomb is planted.",
        properties: &[Property {
            key: "label",
            help: "Site label",
            choices: &["A", "B"],
        }],
        defaults: &[("label", "A")],
        lift: 0.0,
        faces: false,
    },
    ClassInfo {
        class: "ammo_crate",
        purpose: "Team Deathmatch ammo crate; origin is the centre of its base on the ground.",
        properties: &[],
        defaults: &[],
        lift: 0.0,
        faces: false,
    },
    ClassInfo {
        class: "light_point",
        purpose: "An extra point light (the sun, sky and fog come from the look); targetname practical makes it a lamp the night looks switch on.",
        properties: &[
            Property {
                key: "color",
                help: "r g b, 0 to 1",
                choices: &[],
            },
            Property {
                key: "intensity",
                help: "Bevy light intensity",
                choices: &[],
            },
            Property {
                key: "range",
                help: "Metres",
                choices: &[],
            },
            Property {
                key: "shadows_enabled",
                help: "true or false",
                choices: &["true", "false"],
            },
            Property {
                key: "targetname",
                help: "practical makes it a lamp",
                choices: &[],
            },
        ],
        defaults: &[
            ("color", "1 0.92 0.8"),
            ("intensity", "220000"),
            ("range", "9.0"),
            ("shadows_enabled", "false"),
        ],
        lift: 2.9,
        faces: false,
    },
    ClassInfo {
        class: "prop",
        purpose: "A glTF model; box collides as one box around it, convex as a hull per mesh.",
        properties: &[
            Property {
                key: "model",
                help: "Asset path, e.g. models/downtown/Prop_Bollard.glb",
                choices: &[],
            },
            Property {
                key: "collider",
                help: "How it collides",
                choices: COLLIDERS,
            },
            Property {
                key: "angle",
                help: "Facing in degrees",
                choices: &[],
            },
        ],
        defaults: &[("collider", "box")],
        lift: 0.0,
        faces: true,
    },
];

pub fn class(name: &str) -> Option<&'static ClassInfo> {
    CLASSES.iter().find(|class| class.class == name)
}

impl ClassInfo {
    /// A new entity of this class standing at `position`, with `overrides` replacing defaults.
    pub fn instantiate(
        &self,
        position: DVec3,
        angle: Option<f64>,
        overrides: &[(String, String)],
    ) -> Result<MapEntity, String> {
        let mut entity = MapEntity::point(self.class, position, None, &[]);
        let angle = angle.or(self.faces.then_some(0.0));
        if let Some(angle) = angle {
            entity.set("angle", &format!("{}", angle.rem_euclid(360.0)));
        }
        for (key, value) in self.defaults {
            entity.set(key, value);
        }
        for (key, value) in overrides {
            self.check(key, value)?;
            entity.set(key, value);
        }
        if self.class == "prop" && entity.get("model").is_none() {
            return Err("a prop needs a `model`, e.g. models/downtown/Prop_Bollard.glb".into());
        }
        Ok(entity)
    }

    /// Whether `value` is acceptable for `key` of this class.
    pub fn check(&self, key: &str, value: &str) -> Result<(), String> {
        if key == "origin" || key == "classname" {
            return Err(format!("`{key}` cannot be set here"));
        }
        let Some(property) = self.properties.iter().find(|p| p.key == key) else {
            // Unknown keys are legal in a .map (the loader ignores them); say so rather than
            // silently writing a typo.
            let known: Vec<&str> = self.properties.iter().map(|p| p.key).collect();
            return Err(format!(
                "`{}` has no property `{key}`; known: {}",
                self.class,
                known.join(", ")
            ));
        };
        if !property.choices.is_empty() && !property.choices.contains(&value) {
            return Err(format!(
                "`{key}` of `{}` is one of: {}",
                self.class,
                property.choices.join(", ")
            ));
        }
        if key == "modes" {
            for mode in value.split(|c: char| c == ',' || c.is_whitespace()) {
                if !mode.is_empty() && !crate::overlay::MODE_KEYS.contains(&mode) {
                    return Err(format!(
                        "unknown Game Mode `{mode}`; known: {}",
                        crate::overlay::MODE_KEYS.join(" ")
                    ));
                }
            }
        }
        if key == "angle" && value.trim().parse::<f64>().is_err() {
            return Err("`angle` is a number of degrees".into());
        }
        Ok(())
    }
}

/// A brush primitive with a default size (metres). For a ramp, `size.x` runs along the climb.
pub struct Primitive {
    pub key: &'static str,
    pub title: &'static str,
    pub size: DVec3,
    pub ramp: bool,
}

pub const PRIMITIVES: &[Primitive] = &[
    Primitive {
        key: "box",
        title: "Box 2 x 2 x 2 m",
        size: DVec3::new(2.0, 2.0, 2.0),
        ramp: false,
    },
    Primitive {
        key: "wall",
        title: "Wall 4 x 3 x 0.4 m",
        size: DVec3::new(4.0, 3.0, 0.4),
        ramp: false,
    },
    Primitive {
        key: "slab",
        title: "Slab 4 x 0.2 x 4 m",
        size: DVec3::new(4.0, 0.2, 4.0),
        ramp: false,
    },
    Primitive {
        key: "ramp",
        title: "Ramp 4 long, 1.5 high, 2 wide",
        size: DVec3::new(4.0, 1.5, 2.0),
        ramp: true,
    },
];

pub fn primitive(key: &str) -> Option<&'static Primitive> {
    PRIMITIVES.iter().find(|p| p.key == key)
}

impl Primitive {
    /// The extents of this primitive when it rises towards `rise` (a ramp's length turns).
    pub fn extents(&self, rise: Rise) -> DVec3 {
        if self.ramp && matches!(rise, Rise::PosZ | Rise::NegZ) {
            DVec3::new(self.size.z, self.size.y, self.size.x)
        } else {
            self.size
        }
    }
}

/// A prop model the Map can place.
#[derive(Clone, Debug)]
pub struct PropModel {
    /// Asset path, e.g. `models/downtown/Prop_Bollard.glb`.
    pub model: String,
    /// Model bounds relative to its origin (game space): `bounds.json`.
    pub bounds: Option<(DVec3, DVec3)>,
}

/// The `.glb` files of `assets/models/*/`, with their bounds from each folder's `bounds.json`.
pub fn load_props(assets: &Path) -> Vec<PropModel> {
    let mut props = Vec::new();
    let Ok(packs) = std::fs::read_dir(assets.join("models")) else {
        return props;
    };
    let mut packs: Vec<_> = packs.filter_map(Result::ok).collect();
    packs.sort_by_key(|pack| pack.file_name());
    for pack in packs {
        let dir = pack.path();
        if !dir.is_dir() {
            continue;
        }
        let bounds: Value = std::fs::read_to_string(dir.join("bounds.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<String> = files
            .filter_map(Result::ok)
            .filter_map(|f| f.file_name().into_string().ok())
            .filter(|name| name.ends_with(".glb"))
            .collect();
        files.sort();
        for file in files {
            let stem = file.trim_end_matches(".glb");
            let corner = |key: &str| -> Option<DVec3> {
                let list = bounds.get(stem)?.get(key)?.as_array()?;
                Some(DVec3::new(
                    list.first()?.as_f64()?,
                    list.get(1)?.as_f64()?,
                    list.get(2)?.as_f64()?,
                ))
            };
            props.push(PropModel {
                model: format!("models/{}/{file}", pack.file_name().to_string_lossy()),
                bounds: corner("min").zip(corner("max")),
            });
        }
    }
    props
}

/// The prop bounds the picker uses, by model file stem.
pub fn prop_bounds(props: &[PropModel]) -> PropBounds {
    props
        .iter()
        .filter_map(|prop| {
            let stem = prop.model.rsplit('/').next()?.trim_end_matches(".glb");
            Some((stem.to_owned(), prop.bounds?))
        })
        .collect()
}

fn vec_json(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// Everything placeable, for the remote method `mapedit/palette`.
pub fn to_json(props: &[PropModel], looks: &[&str], factions: &[(String, String)]) -> Value {
    json!({
        "surfaces": SURFACES.iter().map(|s| json!({
            "name": s.name,
            "texture": s.texture(),
            "tile_metres": [s.tile_metres.0, s.tile_metres.1],
            "swatch": format!("textures/{}/color.jpg", s.name),
        })).collect::<Vec<_>>(),
        "primitives": PRIMITIVES.iter().map(|p| json!({
            "key": p.key,
            "title": p.title,
            "size": vec_json(p.size),
            "rises": p.ramp.then(|| Rise::ALL.map(Rise::key)),
        })).collect::<Vec<_>>(),
        "entity_classes": CLASSES.iter().map(|c| json!({
            "class": c.class,
            "purpose": c.purpose,
            "defaults": c.defaults.iter().map(|(k, v)| json!({ *k: *v })).collect::<Vec<_>>(),
            "properties": c.properties.iter().map(|p| json!({
                "key": p.key,
                "help": p.help,
                "choices": p.choices,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "props": props.iter().map(|p| json!({
            "model": p.model,
            "bounds": p.bounds.map(|(min, max)| json!({ "min": vec_json(min), "max": vec_json(max) })),
        })).collect::<Vec<_>>(),
        "game_modes": crate::overlay::MODE_KEYS,
        "looks": looks,
        "factions": factions.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
    })
}

// -------------------------------------------------------------------------------------- panel

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Surfaces,
    Brushes,
    Entities,
    Props,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Surfaces, Tab::Brushes, Tab::Entities, Tab::Props];

    fn title(self) -> &'static str {
        match self {
            Tab::Surfaces => "Surfaces",
            Tab::Brushes => "Brushes",
            Tab::Entities => "Entities",
            Tab::Props => "Props",
        }
    }
}

/// The palette panel, toggled with `P`.
#[derive(Resource)]
pub struct PaletteState {
    pub open: bool,
    tab: Tab,
}

/// Shows or hides the panel, optionally on a tab (`surfaces`, `brushes`, `entities`, `props`).
pub fn show(world: &mut World, open: bool, tab: Option<&str>) -> Result<(), String> {
    let tab = match tab {
        None => None,
        Some(name) => Some(
            Tab::ALL
                .into_iter()
                .find(|tab| tab.title().eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    format!("unknown tab \"{name}\"; known: surfaces, brushes, entities, props")
                })?,
        ),
    };
    let mut state = world.resource_mut::<PaletteState>();
    state.open = open;
    if let Some(tab) = tab {
        state.tab = tab;
    }
    Ok(())
}

#[derive(Message, Clone, Debug)]
enum PaletteAction {
    Tab(Tab),
    Surface(&'static str),
    Place(Place),
}

#[derive(Component)]
struct PaletteRoot;

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PaletteState {
            open: false,
            tab: Tab::Surfaces,
        })
        .add_message::<PaletteAction>()
        .add_systems(Update, (toggle, handle, rebuild).chain());
    }
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    mut state: ResMut<PaletteState>,
) {
    if !capture.blocked() && keys.just_pressed(KeyCode::KeyP) {
        state.open = !state.open;
    }
}

fn handle(
    mut actions: MessageReader<PaletteAction>,
    mut state: ResMut<PaletteState>,
    mut editor: ResMut<Editor>,
    mut places: MessageWriter<PlaceRequest>,
) {
    for action in actions.read() {
        match action.clone() {
            PaletteAction::Tab(tab) => state.tab = tab,
            PaletteAction::Surface(name) => {
                // Paints the selected brush; new brushes use it from now on.
                let on_brush = matches!(editor.selection, Some(Item::Brush { .. }));
                let result = if on_brush {
                    editor.set_surface(None, name)
                } else {
                    crate::mapfile::surface(name).map_or_else(
                        || Err(format!("unknown surface {name}")),
                        |surface| {
                            editor.surface = surface;
                            Ok(format!("New brushes are {name}"))
                        },
                    )
                };
                editor.report(&result);
            }
            PaletteAction::Place(place) => {
                places.write(PlaceRequest {
                    place,
                    at_cursor: false,
                });
            }
        }
    }
}

fn action_button(
    commands: &mut Commands,
    fonts: &UiFonts,
    parent: Entity,
    text: &str,
    width: f32,
    action: PaletteAction,
) -> Entity {
    commands
        .spawn((ChildOf(parent), compact_button(fonts, text, width, 16.0)))
        .observe(
            move |_: On<Activate>, mut out: MessageWriter<PaletteAction>| {
                out.write(action.clone());
            },
        )
        .id()
}

fn note(
    commands: &mut Commands,
    fonts: &UiFonts,
    parent: Entity,
    text: String,
    size: f32,
    color: Color,
) {
    commands.spawn((
        ChildOf(parent),
        label(fonts, &text, size, color),
        Node {
            max_width: px(PANEL_W - 28.0),
            ..default()
        },
    ));
}

const PANEL_W: f32 = 420.0;

fn rebuild(
    mut commands: Commands,
    state: Res<PaletteState>,
    editor: Res<Editor>,
    fonts: Option<Res<UiFonts>>,
    assets: Res<AssetServer>,
    roots: Query<Entity, With<PaletteRoot>>,
    mut shown_surface: Local<&'static str>,
) {
    let surface = editor.surface.name;
    if !state.is_changed() && *shown_surface == surface {
        return;
    }
    *shown_surface = surface;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let (true, Some(fonts)) = (state.open, fonts) else {
        return;
    };
    let root = commands
        .spawn((
            PaletteRoot,
            BlockPicking,
            GlobalZIndex(5),
            Node {
                position_type: PositionType::Absolute,
                right: px(10.0),
                top: px(10.0),
                width: px(PANEL_W),
                max_height: percent(94.0),
                overflow: Overflow::clip(),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14.0)),
                row_gap: px(6.0),
                border: UiRect {
                    top: px(3.0),
                    ..UiRect::all(px(1.0))
                },
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor {
                top: AMBER,
                ..BorderColor::all(PANEL_BORDER)
            },
        ))
        .id();
    let tabs = commands
        .spawn((
            ChildOf(root),
            Node {
                column_gap: px(6.0),
                ..default()
            },
        ))
        .id();
    for tab in Tab::ALL {
        action_button(
            &mut commands,
            &fonts,
            tabs,
            tab.title(),
            90.0,
            PaletteAction::Tab(tab),
        );
    }
    note(
        &mut commands,
        &fonts,
        root,
        state.tab.title().to_uppercase(),
        18.0,
        ACCENT,
    );
    commands.spawn((ChildOf(root), rule(PANEL_BORDER)));
    match state.tab {
        Tab::Surfaces => {
            note(
                &mut commands,
                &fonts,
                root,
                "Click to paint the selected brush; new brushes use the marked surface (*).".into(),
                14.0,
                TEXT_DIM,
            );
            for s in &SURFACES {
                let row = commands
                    .spawn((
                        ChildOf(root),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(8.0),
                            ..default()
                        },
                    ))
                    .id();
                commands.spawn((
                    ChildOf(row),
                    ImageNode::new(assets.load(format!("textures/{}/color.jpg", s.name))),
                    Node {
                        width: px(30.0),
                        height: px(30.0),
                        ..default()
                    },
                ));
                let mark = if s.name == surface { "* " } else { "" };
                action_button(
                    &mut commands,
                    &fonts,
                    row,
                    &format!("{mark}{}", s.name),
                    140.0,
                    PaletteAction::Surface(s.name),
                );
                note(
                    &mut commands,
                    &fonts,
                    row,
                    format!("repeats every {} x {} m", s.tile_metres.0, s.tile_metres.1),
                    14.0,
                    TEXT_DIM,
                );
            }
        }
        Tab::Brushes => {
            note(
                &mut commands,
                &fonts,
                root,
                format!(
                    "Placed where the camera looks, in {surface}. Key B: a box at the cursor; Shift+B: a ramp."
                ),
                14.0,
                TEXT_DIM,
            );
            for p in PRIMITIVES {
                action_button(
                    &mut commands,
                    &fonts,
                    root,
                    p.title,
                    380.0,
                    PaletteAction::Place(Place::Primitive(p.key)),
                );
            }
            note(&mut commands, &fonts, root, "A ramp climbs away from you. Resize with Alt+arrows; a brush is a plain convex solid with real-world texture scale.".into(), 14.0, TEXT_DIM);
        }
        Tab::Entities => {
            note(
                &mut commands,
                &fonts,
                root,
                "Placed where the camera looks (spawns face the same way).".into(),
                14.0,
                TEXT_DIM,
            );
            let line =
                |c: &mut Commands, text: &str, class: &'static str, props: &[(&str, &str)]| {
                    let props = props
                        .iter()
                        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                        .collect();
                    action_button(
                        c,
                        &fonts,
                        root,
                        text,
                        380.0,
                        PaletteAction::Place(Place::Entity { class, props }),
                    );
                };
            line(
                &mut commands,
                "Spawn point, Alpha",
                "spawn_point",
                &[("team", "alpha")],
            );
            line(
                &mut commands,
                "Spawn point, Bravo",
                "spawn_point",
                &[("team", "bravo")],
            );
            line(
                &mut commands,
                "Spawn point, either Team",
                "spawn_point",
                &[("team", "any")],
            );
            line(
                &mut commands,
                "Domination Flag A",
                "dom_flag",
                &[("label", "A")],
            );
            line(
                &mut commands,
                "Domination Flag B",
                "dom_flag",
                &[("label", "B")],
            );
            line(
                &mut commands,
                "Domination Flag C",
                "dom_flag",
                &[("label", "C")],
            );
            line(&mut commands, "HQ site", "hq_site", &[]);
            line(&mut commands, "Bomb site A", "bomb_site", &[("label", "A")]);
            line(&mut commands, "Bomb site B", "bomb_site", &[("label", "B")]);
            line(&mut commands, "Ammo crate", "ammo_crate", &[]);
            line(&mut commands, "Point light", "light_point", &[]);
            commands.spawn((ChildOf(root), rule(PANEL_BORDER)));
            for class in CLASSES {
                note(
                    &mut commands,
                    &fonts,
                    root,
                    class.class.to_owned(),
                    15.0,
                    ACCENT,
                );
                let mut text = String::new();
                for p in class.properties {
                    text.push_str(p.key);
                    if !p.choices.is_empty() {
                        text.push_str(&format!(" ({})", p.choices.join("|")));
                    }
                    text.push_str("  ");
                }
                if !text.is_empty() {
                    note(
                        &mut commands,
                        &fonts,
                        root,
                        format!("  {}", text.trim_end()),
                        13.0,
                        TEXT_DIM,
                    );
                }
            }
        }
        Tab::Props => {
            note(
                &mut commands,
                &fonts,
                root,
                "A glTF model with a box collider, placed where the camera looks.".into(),
                14.0,
                TEXT_DIM,
            );
            for prop in &editor.props {
                let name = prop
                    .model
                    .rsplit('/')
                    .next()
                    .unwrap_or(&prop.model)
                    .trim_end_matches(".glb");
                let size = prop.bounds.map_or(String::new(), |(min, max)| {
                    let s = max - min;
                    format!("{:.1} x {:.1} x {:.1} m", s.x, s.y, s.z)
                });
                let row = commands
                    .spawn((
                        ChildOf(root),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(8.0),
                            ..default()
                        },
                    ))
                    .id();
                action_button(
                    &mut commands,
                    &fonts,
                    row,
                    name,
                    250.0,
                    PaletteAction::Place(Place::Prop(prop.model.clone())),
                );
                note(&mut commands, &fonts, row, size, 13.0, TEXT_DIM);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_instantiate_with_defaults_and_check_values() {
        let spawn = class("spawn_point").unwrap();
        let e = spawn
            .instantiate(
                DVec3::new(1.0, 0.0, 2.0),
                Some(90.0),
                &[("team".into(), "bravo".into())],
            )
            .unwrap();
        assert_eq!(e.get("team"), Some("bravo"));
        assert_eq!(e.get("angle"), Some("90"));
        assert!(
            spawn
                .instantiate(DVec3::ZERO, None, &[("team".into(), "red".into())])
                .is_err()
        );
        assert!(
            spawn
                .instantiate(DVec3::ZERO, None, &[("modes".into(), "tdm sd".into())])
                .is_ok()
        );
        assert!(
            spawn
                .instantiate(DVec3::ZERO, None, &[("modes".into(), "ctf".into())])
                .is_err()
        );
        assert!(
            spawn
                .instantiate(DVec3::ZERO, None, &[("origin".into(), "0 0 0".into())])
                .is_err()
        );
        assert!(
            class("prop")
                .unwrap()
                .instantiate(DVec3::ZERO, None, &[])
                .is_err(),
            "no model"
        );
        assert!(
            class("hq_site")
                .unwrap()
                .instantiate(DVec3::ZERO, None, &[("x".into(), "y".into())])
                .is_err()
        );
    }

    #[test]
    fn every_game_class_is_listed() {
        // The classes of crates/frontline/src/maps/classes.rs (and the light the maps use).
        for name in [
            "spawn_point",
            "dom_flag",
            "hq_site",
            "bomb_site",
            "ammo_crate",
            "prop",
            "light_point",
        ] {
            assert!(class(name).is_some(), "{name}");
        }
    }

    #[test]
    fn props_load_with_their_bounds() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontline/assets");
        let props = load_props(&assets);
        assert!(props.len() >= 10, "{} props", props.len());
        let bollard = props
            .iter()
            .find(|p| p.model == "models/downtown/Prop_Bollard.glb")
            .expect("the bollard");
        assert!(bollard.bounds.is_some());
        assert!(prop_bounds(&props).contains_key("Prop_Bollard"));
    }

    #[test]
    fn ramps_turn_their_length_with_the_rise() {
        let ramp = primitive("ramp").unwrap();
        assert_eq!(ramp.extents(Rise::PosX), DVec3::new(4.0, 1.5, 2.0));
        assert_eq!(ramp.extents(Rise::NegZ), DVec3::new(2.0, 1.5, 4.0));
        assert_eq!(
            primitive("box").unwrap().extents(Rise::NegZ),
            DVec3::new(2.0, 2.0, 2.0)
        );
    }

    // ---- the panel

    use crate::mapfile::MapFile;
    use crate::menu::testing::click;
    use bevy::ecs::message::Messages;

    fn panel_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let mut map = MapFile::with_worldspawn("T", "x");
        map.add_brush(crate::mapfile::Brush::cuboid(
            DVec3::ZERO,
            DVec3::ONE,
            crate::mapfile::surface("concrete").unwrap(),
        ));
        let mut editor = Editor::for_tests(
            map,
            vec![PropModel {
                model: "models/downtown/Prop_Bollard.glb".into(),
                bounds: Some((DVec3::new(-0.2, 0.0, -0.2), DVec3::new(0.2, 1.0, 0.2))),
            }],
        );
        editor.selection = Some(Item::Brush {
            entity: 0,
            brush: 0,
        });
        app.insert_resource(editor)
            .insert_resource(UiFonts {
                regular: Handle::default(),
                bold: Handle::default(),
                display: Handle::default(),
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputCapture>()
            .add_message::<PlaceRequest>()
            .add_plugins(PalettePlugin);
        app
    }

    fn places(app: &mut App) -> Vec<PlaceRequest> {
        app.world_mut()
            .resource_mut::<Messages<PlaceRequest>>()
            .drain()
            .collect()
    }

    #[test]
    fn the_panel_paints_and_places() {
        let mut app = panel_app();
        app.update();
        assert!(
            app.world_mut()
                .query::<&PaletteRoot>()
                .iter(app.world())
                .next()
                .is_none(),
            "P opens it"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyP);
        app.update();
        assert!(
            app.world_mut()
                .query::<&PaletteRoot>()
                .iter(app.world())
                .next()
                .is_some()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();

        // Surfaces: paints the selected brush.
        click(&mut app, "brick");
        let editor = app.world().resource::<Editor>();
        assert_eq!(
            editor
                .map
                .as_ref()
                .unwrap()
                .brush(0, 0)
                .unwrap()
                .surface()
                .unwrap()
                .name,
            "brick"
        );
        assert_eq!(editor.surface.name, "brick");
        // Without a brush selected, it only picks the surface of new brushes.
        app.world_mut().resource_mut::<Editor>().selection = None;
        click(&mut app, "wood");
        let editor = app.world().resource::<Editor>();
        assert_eq!(
            editor
                .map
                .as_ref()
                .unwrap()
                .brush(0, 0)
                .unwrap()
                .surface()
                .unwrap()
                .name,
            "brick"
        );
        assert_eq!(editor.surface.name, "wood");

        // Brushes.
        click(&mut app, "Brushes");
        click(&mut app, "Ramp 4 long");
        let sent = places(&mut app);
        assert!(
            matches!(&sent[0].place, Place::Primitive("ramp")) && !sent[0].at_cursor,
            "{sent:?}"
        );
        // Entities.
        click(&mut app, "Entities");
        click(&mut app, "Spawn point, Bravo");
        click(&mut app, "Domination Flag C");
        let sent = places(&mut app);
        assert!(
            matches!(&sent[0].place, Place::Entity { class: "spawn_point", props } if props == &[("team".to_owned(), "bravo".to_owned())]),
            "{sent:?}"
        );
        assert!(matches!(
            &sent[1].place,
            Place::Entity {
                class: "dom_flag",
                ..
            }
        ));
        // Props.
        click(&mut app, "Props");
        click(&mut app, "Prop_Bollard");
        let sent = places(&mut app);
        assert!(
            matches!(&sent[0].place, Place::Prop(m) if m == "models/downtown/Prop_Bollard.glb"),
            "{sent:?}"
        );
    }
}
