//! The entity panel: while a single point entity is selected, its class, position, facing and
//! key/values in a panel at the bottom left. Edit a value in its text field (Enter or a click
//! elsewhere applies it, Esc puts the old value back), click a choice to step through it, add a
//! key the class knows with its `+` button, remove one with `x`. Every change goes through
//! [`Editor::edit_entity`], so it is checked against what the entity's class accepts
//! (`palette::CLASSES`) and is one undo step; a refused value says why in the HUD and the field
//! shows the old value again.

use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use frontline::ui::{
    ACCENT, AMBER, PANEL, PANEL_BORDER, TEXT, TEXT_DIM, UiFonts, compact_button, label, rule,
    text_field,
};

use crate::edit::{Editor, EntityChange};
use crate::mapfile::{Item, format_number, round_to};
use crate::menu::BlockPicking;
use crate::palette::{self, ClassInfo, Property, ValueKind};

const PANEL_WIDTH: f32 = 380.0;

/// Which value a text field of the panel edits.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FieldId {
    /// The value of a key.
    Value(String),
    X,
    Y,
    Z,
    Angle,
    /// The key and value of the "add a key" row of an entity whose class is not known.
    NewKey,
    NewValue,
}

/// A text field of the panel, and what it was built for: a field of an older state of the Map
/// is ignored.
#[derive(Component, Clone, Debug)]
struct PanelField {
    id: FieldId,
    item: Item,
    revision: u64,
}

#[derive(Component)]
struct InspectorRoot;

#[derive(Message, Clone, Debug)]
enum InspectorAction {
    /// Steps a choice property to its next value.
    Cycle(String),
    /// Adds a key of the class, with an example value.
    Add(String),
    Remove(String),
    /// Adds the key and value typed in the free "add" row.
    AddFree,
}

#[derive(Resource, Default)]
struct InspectorState {
    /// The field to focus when the panel is built again (the one that was clicked next).
    refocus: Option<FieldId>,
    /// Build the panel again even when nothing changed (a refused value).
    redraw: bool,
}

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorState>()
            .add_message::<InspectorAction>()
            .add_systems(Update, (commit, act, rebuild).chain());
    }
}

/// The selected entity, when the selection is one point entity (not the worldspawn).
fn selected_entity(editor: &Editor) -> Option<usize> {
    let Some(Item::Entity(index)) = editor.selection.single() else {
        return None;
    };
    let entity = editor.map.as_ref()?.entities.get(index)?;
    (index > 0 && entity.brushes.is_empty()).then_some(index)
}

/// A value to start a newly added key with.
fn example(property: &Property, class: &ClassInfo) -> String {
    if let Some((_, value)) = class.defaults.iter().find(|(key, _)| *key == property.key) {
        return (*value).to_owned();
    }
    match property.kind {
        ValueKind::Choice => property.choices.first().copied().unwrap_or("").to_owned(),
        ValueKind::Angle => "0".to_owned(),
        ValueKind::Modes => "tdm".to_owned(),
        ValueKind::Colour => "1 1 1".to_owned(),
        ValueKind::Number(min) => format_number(min.max(1.0)),
        ValueKind::Model => "models/downtown/Prop_Bollard.glb".to_owned(),
        ValueKind::Text => {
            if property.key == "targetname" {
                "practical".to_owned()
            } else {
                "value".to_owned()
            }
        }
    }
}

/// The text a position component shows.
fn metres(value: f64) -> String {
    format_number(round_to(value, 1e3))
}

/// What the field `id` of an entity currently holds, as text.
fn current_text(editor: &Editor, index: usize, id: &FieldId) -> String {
    let Some(entity) = editor.map.as_ref().and_then(|m| m.entities.get(index)) else {
        return String::new();
    };
    let at = entity.origin().unwrap_or_default();
    match id {
        FieldId::Value(key) => entity.get(key).unwrap_or("").to_owned(),
        FieldId::X => metres(at.x),
        FieldId::Y => metres(at.y),
        FieldId::Z => metres(at.z),
        FieldId::Angle => entity.get("angle").map_or(String::new(), |_| {
            format_number(round_to(entity.angle(), 1e4))
        }),
        FieldId::NewKey | FieldId::NewValue => String::new(),
    }
}

/// Applies what was typed into a field. `Ok(false)`: nothing changed.
fn apply(editor: &mut Editor, field: &PanelField, typed: &str) -> Result<bool, String> {
    let Item::Entity(index) = field.item else {
        return Ok(false);
    };
    let typed = typed.trim();
    if typed == current_text(editor, index, &field.id) {
        return Ok(false);
    }
    let item = Some(field.item);
    match &field.id {
        FieldId::Value(key) => {
            editor.set_property(item, key, typed)?;
        }
        FieldId::Angle => {
            if typed.is_empty() {
                editor.set_property(item, "angle", "")?;
            } else {
                let angle: f64 = typed
                    .parse()
                    .map_err(|_| "`angle` is a number of degrees".to_owned())?;
                editor.edit_entity(
                    item,
                    EntityChange {
                        angle: Some(angle),
                        ..EntityChange::default()
                    },
                )?;
            }
        }
        FieldId::X | FieldId::Y | FieldId::Z => {
            let value: f64 = typed
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())
                .ok_or("a position is numbers in metres")?;
            let mut position = editor
                .map
                .as_ref()
                .and_then(|m| m.entities.get(index))
                .and_then(|e| e.origin())
                .ok_or("the entity has no position")?;
            match field.id {
                FieldId::X => position.x = value,
                FieldId::Y => position.y = value,
                _ => position.z = value,
            }
            editor.edit_entity(
                item,
                EntityChange {
                    position: Some(position),
                    ..EntityChange::default()
                },
            )?;
        }
        FieldId::NewKey | FieldId::NewValue => return Ok(false),
    }
    Ok(true)
}

/// A text field that lost the keyboard applies its value (Esc takes it back).
fn commit(
    focus: Res<InputFocus>,
    keys: Res<ButtonInput<KeyCode>>,
    fields: Query<(&PanelField, &EditableText)>,
    mut editor: ResMut<Editor>,
    mut state: ResMut<InspectorState>,
    mut before: Local<Option<Entity>>,
) {
    let now = focus.get();
    if *before == now {
        return;
    }
    let left = std::mem::replace(&mut *before, now);
    let Some((field, text)) = left.and_then(|entity| fields.get(entity).ok()) else {
        return;
    };
    let next = now
        .and_then(|entity| fields.get(entity).ok())
        .map(|(next, _)| next.id.clone());
    if keys.just_pressed(KeyCode::Escape) {
        state.redraw = true;
        state.refocus = next;
        return;
    }
    if field.revision != editor.revision {
        return;
    }
    match apply(&mut editor, field, &text.value().to_string()) {
        Ok(false) => {}
        Ok(true) => {
            let message = editor.selection_text();
            editor.say(message);
            state.refocus = next;
        }
        Err(message) => {
            editor.complain(message);
            state.redraw = true;
            state.refocus = next;
        }
    }
}

fn act(
    mut actions: MessageReader<InspectorAction>,
    mut editor: ResMut<Editor>,
    fields: Query<(&PanelField, &EditableText)>,
) {
    for action in actions.read() {
        let Some(index) = selected_entity(&editor) else {
            continue;
        };
        let class = editor
            .map
            .as_ref()
            .and_then(|m| m.entities.get(index))
            .map(|e| e.classname().to_owned())
            .unwrap_or_default();
        let info = palette::class(&class);
        let result = match action {
            InspectorAction::Cycle(key) => {
                let next = info.and_then(|c| c.property(key)).map(|property| {
                    let now = current_text(&editor, index, &FieldId::Value(key.clone()));
                    let at = property.choices.iter().position(|c| *c == now);
                    let next = at.map_or(0, |i| (i + 1) % property.choices.len());
                    property.choices[next].to_owned()
                });
                match next {
                    Some(value) => editor.set_property(None, key, &value),
                    None => Err(format!("`{key}` is not a choice")),
                }
            }
            InspectorAction::Add(key) => match info.and_then(|c| Some((c, c.property(key)?))) {
                Some((class, property)) => {
                    let value = example(property, class);
                    editor.set_property(None, key, &value)
                }
                None => Err(format!("`{class}` has no property `{key}`")),
            },
            InspectorAction::Remove(key) => editor.set_property(None, key, ""),
            InspectorAction::AddFree => {
                let typed = |id: &FieldId| {
                    fields
                        .iter()
                        .find(|(field, _)| field.id == *id)
                        .map(|(_, text)| text.value().to_string())
                        .unwrap_or_default()
                };
                let (key, value) = (typed(&FieldId::NewKey), typed(&FieldId::NewValue));
                if key.trim().is_empty() || value.trim().is_empty() {
                    Err("type a key and a value".to_owned())
                } else {
                    editor.set_property(None, key.trim(), value.trim())
                }
            }
        };
        editor.report(&result);
    }
}

// --------------------------------------------------------------------------------------- UI

struct Build<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a UiFonts,
    refocus: Option<FieldId>,
    revision: u64,
    item: Item,
}

impl Build<'_, '_, '_> {
    fn text(&mut self, parent: Entity, text: &str, size: f32, color: Color, width: Option<f32>) {
        let mut node = Node {
            max_width: px(PANEL_WIDTH - 28.0),
            ..default()
        };
        if let Some(width) = width {
            node.width = px(width);
            node.max_width = px(width);
        }
        self.commands
            .spawn((ChildOf(parent), label(self.fonts, text, size, color), node));
    }

    fn row(&mut self, parent: Entity) -> Entity {
        self.commands
            .spawn((
                ChildOf(parent),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(6.0),
                    ..default()
                },
            ))
            .id()
    }

    fn field(&mut self, parent: Entity, id: FieldId, text: &str, width: f32) {
        let refocus = self.refocus.as_ref() == Some(&id);
        let mut entity = self.commands.spawn((
            ChildOf(parent),
            PanelField {
                id,
                item: self.item,
                revision: self.revision,
            },
            text_field(self.fonts, text, 200),
        ));
        entity.insert((
            Node {
                width: px(width),
                padding: UiRect::axes(px(6.0), px(2.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            TextFont {
                font: self.fonts.regular.clone().into(),
                font_size: FontSize::Px(16.0),
                ..default()
            },
        ));
        if refocus {
            entity.insert(AutoFocus);
        }
    }

    fn button(&mut self, parent: Entity, text: &str, width: f32, action: InspectorAction) {
        self.commands
            .spawn((
                ChildOf(parent),
                compact_button(self.fonts, text, width, 15.0),
            ))
            .observe(
                move |_: On<Activate>, mut out: MessageWriter<InspectorAction>| {
                    out.write(action.clone());
                },
            );
    }
}

fn rebuild(
    mut commands: Commands,
    editor: Res<Editor>,
    mut state: ResMut<InspectorState>,
    fonts: Option<Res<UiFonts>>,
    roots: Query<Entity, With<InspectorRoot>>,
    mut shown: Local<Option<(usize, u64)>>,
) {
    let wanted = selected_entity(&editor).map(|index| (index, editor.revision));
    if *shown == wanted && !state.redraw {
        return;
    }
    *shown = wanted;
    state.redraw = false;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let (Some((index, _)), Some(fonts)) = (wanted, fonts) else {
        state.refocus = None;
        return;
    };
    let Some(entity) = editor.map.as_ref().and_then(|map| map.entities.get(index)) else {
        return;
    };
    let class = palette::class(entity.classname());
    let root = commands
        .spawn((
            InspectorRoot,
            BlockPicking,
            GlobalZIndex(5),
            Node {
                position_type: PositionType::Absolute,
                left: px(10.0),
                bottom: px(10.0),
                width: px(PANEL_WIDTH),
                max_height: percent(62.0),
                overflow: Overflow::clip(),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12.0)),
                row_gap: px(5.0),
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
    let mut ui = Build {
        commands: &mut commands,
        fonts: &fonts,
        refocus: state.refocus.take(),
        revision: editor.revision,
        item: Item::Entity(index),
    };
    ui.text(
        root,
        &format!("{}  (entity {index})", entity.classname()),
        18.0,
        ACCENT,
        None,
    );
    match class {
        Some(class) => ui.text(root, class.purpose, 13.0, TEXT_DIM, None),
        None => ui.text(
            root,
            "A class MapEdit does not know: its keys are not checked.",
            13.0,
            AMBER,
            None,
        ),
    }
    ui.commands.spawn((ChildOf(root), rule(PANEL_BORDER)));

    // Position and facing.
    let row = ui.row(root);
    for (name, id) in [("x", FieldId::X), ("y", FieldId::Y), ("z", FieldId::Z)] {
        ui.text(row, name, 15.0, TEXT_DIM, None);
        let text = current_text(&editor, index, &id);
        ui.field(row, id, &text, 82.0);
    }
    let faces = class.is_none_or(|c| c.faces) || entity.get("angle").is_some();
    if faces {
        let row = ui.row(root);
        ui.text(row, "angle", 15.0, TEXT_DIM, Some(52.0));
        let text = current_text(&editor, index, &FieldId::Angle);
        ui.field(row, FieldId::Angle, &text, 82.0);
        ui.text(
            row,
            "0 faces -z, turns counter-clockwise",
            12.0,
            TEXT_DIM,
            None,
        );
    }

    // The class's own keys, set or not.
    let shown_keys: Vec<&Property> = class
        .map(|c| c.properties.iter().filter(|p| p.key != "angle").collect())
        .unwrap_or_default();
    for property in &shown_keys {
        let current = entity.get(property.key);
        let row = ui.row(root);
        ui.text(row, property.key, 15.0, TEXT, Some(104.0));
        let removable = class.is_some_and(|c| c.check_removal(property.key).is_ok());
        match (current, property.kind) {
            (Some(value), ValueKind::Choice) => {
                ui.button(
                    row,
                    value,
                    150.0,
                    InspectorAction::Cycle(property.key.to_owned()),
                );
                if removable {
                    ui.button(
                        row,
                        "x",
                        28.0,
                        InspectorAction::Remove(property.key.to_owned()),
                    );
                }
            }
            (Some(value), _) => {
                ui.field(row, FieldId::Value(property.key.to_owned()), value, 190.0);
                if removable {
                    ui.button(
                        row,
                        "x",
                        28.0,
                        InspectorAction::Remove(property.key.to_owned()),
                    );
                }
            }
            (None, _) => {
                let default = class
                    .and_then(|c| c.defaults.iter().find(|(k, _)| *k == property.key))
                    .map(|(_, v)| format!("default {v}"))
                    .unwrap_or_else(|| "not set".to_owned());
                ui.text(row, &default, 14.0, TEXT_DIM, Some(150.0));
                ui.button(
                    row,
                    "+",
                    28.0,
                    InspectorAction::Add(property.key.to_owned()),
                );
            }
        }
        if !property.help.is_empty() && property.kind != ValueKind::Choice {
            ui.text(root, property.help, 12.0, TEXT_DIM, None);
        }
    }

    // Keys the class does not list (a hand-made Map's): editable and removable as they are.
    let listed = |key: &str| {
        matches!(key, "classname" | "origin" | "angle") || shown_keys.iter().any(|p| p.key == key)
    };
    let extras: Vec<(String, String)> = entity
        .props
        .iter()
        .filter(|(key, _)| !listed(key))
        .cloned()
        .collect();
    for (key, value) in extras {
        let row = ui.row(root);
        ui.text(row, &key, 15.0, TEXT, Some(104.0));
        ui.field(row, FieldId::Value(key.clone()), &value, 190.0);
        ui.button(row, "x", 28.0, InspectorAction::Remove(key));
    }

    // A class that is not known takes any key.
    if class.is_none() {
        ui.commands.spawn((ChildOf(root), rule(PANEL_BORDER)));
        let row = ui.row(root);
        ui.field(row, FieldId::NewKey, "", 120.0);
        ui.field(row, FieldId::NewValue, "", 130.0);
        ui.button(row, "Add", 60.0, InspectorAction::AddFree);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapfile::{MapEntity, MapFile};
    use crate::menu::testing::{button_named, click};
    use crate::selection::Selection;
    use bevy::ecs::message::Messages;
    use bevy::math::DVec3;

    fn map() -> MapFile {
        let mut map = MapFile::with_worldspawn("T", "x");
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(5.0, 0.0, 6.0),
            Some(90.0),
            &[("team", "alpha")],
        ));
        map.add_entity(MapEntity::point(
            "mystery",
            DVec3::new(1.0, 0.0, 1.0),
            None,
            &[("colour", "red")],
        ));
        map
    }

    fn app() -> App {
        let mut app = App::new();
        let mut editor = Editor::for_tests(map(), Vec::new());
        editor.selection = Selection::from(Some(Item::Entity(1)));
        app.insert_resource(editor)
            .insert_resource(UiFonts {
                regular: Handle::default(),
                bold: Handle::default(),
                display: Handle::default(),
            })
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(InspectorPlugin);
        app.update();
        app
    }

    fn fields(app: &mut App) -> Vec<(Entity, PanelField)> {
        app.world_mut()
            .query::<(Entity, &PanelField)>()
            .iter(app.world())
            .map(|(entity, field)| (entity, field.clone()))
            .collect()
    }

    /// Types into a field and takes the keyboard away from it, as Enter does.
    fn type_into(app: &mut App, id: &FieldId, text: &str) {
        let (entity, _) = fields(app)
            .into_iter()
            .find(|(_, field)| field.id == *id)
            .unwrap_or_else(|| panic!("no field {id:?}"));
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .editor_mut()
            .set_text(text);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, bevy::input_focus::FocusCause::Navigated);
        app.update();
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
    }

    fn entity(app: &App, index: usize) -> MapEntity {
        app.world()
            .resource::<Editor>()
            .map
            .as_ref()
            .unwrap()
            .entities[index]
            .clone()
    }

    #[test]
    fn the_panel_shows_position_facing_and_keys_of_the_selected_entity_only() {
        let mut app = app();
        let ids: Vec<FieldId> = fields(&mut app).into_iter().map(|(_, f)| f.id).collect();
        assert!(ids.contains(&FieldId::X) && ids.contains(&FieldId::Angle));
        assert!(
            !ids.contains(&FieldId::Value("modes".into())),
            "`modes` is not set, so it has a + and no field: {ids:?}"
        );
        // The team is a choice you click through, not a text field.
        assert!(!ids.contains(&FieldId::Value("team".into())));
        app.world_mut().resource_mut::<Editor>().selection.clear();
        app.update();
        assert!(fields(&mut app).is_empty());
        // Several entities: no panel either.
        app.world_mut().resource_mut::<Editor>().selection =
            Selection::from(vec![Item::Entity(1), Item::Entity(2)]);
        app.update();
        assert!(fields(&mut app).is_empty());
    }

    #[test]
    fn typed_values_are_checked_applied_and_one_undo_step_each() {
        let mut app = app();
        type_into(&mut app, &FieldId::X, "7.5");
        assert!((entity(&app, 1).origin().unwrap().x - 7.5).abs() < 1e-3);
        type_into(&mut app, &FieldId::Angle, "370");
        assert_eq!(entity(&app, 1).angle(), 10.0, "wrapped into 0..360");
        // A refused value changes nothing; the field shows the old one again.
        let revision = app.world().resource::<Editor>().revision;
        type_into(&mut app, &FieldId::Angle, "north");
        assert_eq!(app.world().resource::<Editor>().revision, revision);
        assert_eq!(entity(&app, 1).angle(), 10.0);
        assert!(
            app.world()
                .resource::<Editor>()
                .notice
                .as_ref()
                .unwrap()
                .error
        );
        let (id, _) = fields(&mut app)
            .into_iter()
            .find(|(_, f)| f.id == FieldId::Angle)
            .unwrap();
        assert_eq!(
            app.world()
                .get::<EditableText>(id)
                .unwrap()
                .value()
                .to_string(),
            "10"
        );
        app.world_mut().resource_mut::<Editor>().undo().unwrap();
        assert_eq!(entity(&app, 1).angle(), 90.0);
    }

    #[test]
    fn choices_step_and_keys_are_added_with_examples_and_removed() {
        let mut app = app();
        click(&mut app, "alpha");
        assert_eq!(entity(&app, 1).get("team"), Some("bravo"));
        click(&mut app, "bravo");
        assert_eq!(entity(&app, 1).get("team"), Some("any"));
        click(&mut app, "any");
        assert_eq!(entity(&app, 1).get("team"), Some("alpha"), "wraps round");
        // `modes` is not set: its `+` adds an example the class accepts, `x` takes it off.
        let plus = button_named(&mut app, "+");
        app.world_mut().trigger(Activate { entity: plus });
        app.update();
        assert_eq!(entity(&app, 1).get("modes"), Some("tdm"));
        app.world_mut()
            .resource_mut::<Messages<InspectorAction>>()
            .write(InspectorAction::Remove("modes".into()));
        app.update();
        assert_eq!(entity(&app, 1).get("modes"), None);
    }

    #[test]
    fn an_unknown_class_takes_any_key() {
        let mut app = app();
        app.world_mut().resource_mut::<Editor>().selection = Selection::from(Some(Item::Entity(2)));
        app.update();
        type_into(&mut app, &FieldId::NewKey, "mood");
        type_into(&mut app, &FieldId::NewValue, "calm");
        click(&mut app, "Add");
        assert_eq!(entity(&app, 2).get("mood"), Some("calm"));
        type_into(&mut app, &FieldId::Value("colour".into()), "blue");
        assert_eq!(entity(&app, 2).get("colour"), Some("blue"));
    }
}
