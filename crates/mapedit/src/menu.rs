//! The Esc menu: load, save, save as, new Map, and the Map's settings; a dialog before changes
//! would be lost. Built with the game's own UI widgets (`frontline::ui`). While it is open, or a
//! text field has the keyboard, [`InputCapture`] tells every other keyboard system to stand back.

use bevy::ecs::message::Messages;
use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, TextInput};
use bevy::window::WindowCloseRequested;
use frontline::data::GameData;
use frontline::maps::{MapId, MapRegistry};
use frontline::ui::{
    ACCENT, AMBER, PANEL, PANEL_BORDER, TEXT, TEXT_DIM, UiFonts, backdrop, button, compact_button,
    label, text_field, title,
};

use crate::edit::{Command, Editor, LooseMap, SettingsChange, loose_maps};
use crate::overlay::MODE_KEYS;

const PAGE: usize = 9;
const PANEL_WIDTH: f32 = 620.0;

/// Which keys the editor's other systems may act on.
#[derive(Resource, Default)]
pub struct InputCapture {
    /// The Esc menu is open.
    pub menu: bool,
    /// A text field has the keyboard.
    pub typing: bool,
}

impl InputCapture {
    pub fn blocked(&self) -> bool {
        self.menu || self.typing
    }
}

/// Marks UI the cursor must not click through to the Map.
#[derive(Component)]
pub struct BlockPicking;

#[derive(Clone, Debug)]
pub enum Screen {
    Main,
    Load {
        page: usize,
    },
    SaveAs,
    New,
    Settings,
    /// Unsaved changes would be lost by this command.
    Confirm(Command),
}

/// What the Settings screen edits before Apply.
#[derive(Clone, Debug, Default)]
struct Draft {
    name: String,
    /// `None`: the Map's own look.
    look: Option<String>,
    modes: [bool; 4],
    factions: [String; 2],
    /// The id and name typed in the Save As and New Map fields.
    id_text: String,
    name_text: String,
}

#[derive(Resource)]
pub struct MenuState {
    pub open: bool,
    pub screen: Screen,
    draft: Draft,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: false,
            screen: Screen::Main,
            draft: Draft::default(),
        }
    }
}

impl MenuState {
    /// The open screen's name, or `None` when the menu is closed.
    pub fn screen_name(&self) -> Option<&'static str> {
        self.open.then_some(match self.screen {
            Screen::Main => "main",
            Screen::Load { .. } => "load",
            Screen::SaveAs => "save_as",
            Screen::New => "new",
            Screen::Settings => "settings",
            Screen::Confirm(_) => "confirm",
        })
    }

    pub fn close(&mut self) {
        self.open = false;
        self.screen = Screen::Main;
    }

    /// Asks before `command` throws unsaved changes away.
    pub fn confirm_discard(&mut self, command: Command) {
        self.open = true;
        self.screen = Screen::Confirm(command);
    }
}

#[derive(Message, Clone, Debug)]
enum MenuAction {
    Goto(Screen),
    Close,
    Page(i32),
    Open(MapId),
    OpenFile(LooseMap),
    Save,
    DoSaveAs,
    DoNew,
    Apply,
    CycleLook(i32),
    ToggleMode(usize),
    CycleFaction(usize, i32),
    /// Run a command that was waiting for the answer to "discard changes?".
    Discard(Command),
    SaveThen(Command),
    Playtest,
    Quit,
}

/// Opens (or closes) the menu on a screen by name: `main`, `load`, `save_as`, `new`, `settings`,
/// `close`. For the remote control, to put the menu in a screenshot.
pub fn show(world: &mut World, screen: &str) -> Result<(), String> {
    let screen = match screen {
        "close" => {
            world
                .resource_mut::<Messages<MenuAction>>()
                .write(MenuAction::Close);
            return Ok(());
        }
        "main" => Screen::Main,
        "load" => Screen::Load { page: 0 },
        "save_as" => Screen::SaveAs,
        "new" => Screen::New,
        "settings" => Screen::Settings,
        other => {
            return Err(format!(
                "unknown screen \"{other}\"; known: main, load, save_as, new, settings, close"
            ));
        }
    };
    world.resource_mut::<MenuState>().open = true;
    world
        .resource_mut::<Messages<MenuAction>>()
        .write(MenuAction::Goto(screen));
    Ok(())
}

#[derive(Component)]
struct MenuRoot;

/// A text field of the menu.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Field {
    Id,
    Name,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<InputCapture>()
            .add_message::<MenuAction>()
            .add_systems(PreUpdate, track_capture)
            .add_systems(
                Update,
                (menu_keys, close_requested, handle_actions, rebuild).chain(),
            );
    }
}

fn track_capture(
    menu: Res<MenuState>,
    focus: Res<InputFocus>,
    inputs: Query<(), With<TextInput>>,
    mut capture: ResMut<InputCapture>,
) {
    capture.menu = menu.open;
    capture.typing = focus.get().is_some_and(|entity| inputs.contains(entity));
}

/// Esc opens the menu, steps back, and closes it; Esc or Enter ends typing first.
fn menu_keys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    mut focus: ResMut<InputFocus>,
    mut menu: ResMut<MenuState>,
) {
    let end_key = keys.just_pressed(KeyCode::Escape)
        || keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter);
    if capture.typing {
        if end_key {
            focus.clear();
        }
        return;
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if !menu.open {
        menu.open = true;
        menu.screen = Screen::Main;
        return;
    }
    match menu.screen {
        Screen::Main => menu.close(),
        _ => menu.screen = Screen::Main,
    }
}

/// The window's close button asks before losing changes.
fn close_requested(
    mut requests: MessageReader<WindowCloseRequested>,
    editor: Res<Editor>,
    mut menu: ResMut<MenuState>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().count() == 0 {
        return;
    }
    if editor.dirty() {
        menu.confirm_discard(Command::Quit { discard: false });
    } else {
        exit.write(AppExit::Success);
    }
}

fn read_fields(fields: &Query<(&Field, &EditableText)>, draft: &mut Draft) {
    for (field, text) in fields {
        let value = text.value().to_string();
        match field {
            Field::Id => draft.id_text = value,
            Field::Name => draft.name_text = value,
        }
    }
}

fn handle_actions(
    mut actions: MessageReader<MenuAction>,
    mut menu: ResMut<MenuState>,
    editor: Res<Editor>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
    fields: Query<(&Field, &EditableText)>,
    mut commands: MessageWriter<Command>,
) {
    for action in actions.read() {
        match action.clone() {
            MenuAction::Goto(screen) => {
                // Typed text survives a trip to another screen.
                read_fields(&fields, &mut menu.draft);
                match screen {
                    Screen::Settings => {
                        if let Some(info) = registry.get(&editor.id) {
                            let own_look = editor
                                .map
                                .as_ref()
                                .and_then(|m| m.worldspawn())
                                .and_then(|w| w.get("look"))
                                .map(str::to_owned);
                            menu.draft = Draft {
                                name: info.name.clone(),
                                look: info.look.clone().or(own_look),
                                modes: MODE_KEYS.map(|key| info.modes.iter().any(|m| m == key)),
                                factions: info.factions.clone(),
                                ..menu.draft.clone()
                            };
                        }
                    }
                    Screen::SaveAs => {
                        menu.draft.id_text = format!("{}_copy", editor.id);
                        menu.draft.name_text.clear();
                    }
                    Screen::New => {
                        menu.draft.id_text.clear();
                        menu.draft.name_text.clear();
                    }
                    _ => {}
                }
                menu.screen = screen;
            }
            MenuAction::Close => menu.close(),
            MenuAction::Page(step) => {
                if let Screen::Load { page } = menu.screen {
                    menu.screen = Screen::Load {
                        page: (page as i32 + step).max(0) as usize,
                    };
                }
            }
            MenuAction::Open(id) => {
                commands.write(Command::Open { id, discard: false });
            }
            MenuAction::OpenFile(loose) => {
                commands.write(Command::OpenFile {
                    stem: loose.stem,
                    private: loose.private,
                    discard: false,
                });
            }
            MenuAction::Save => {
                commands.write(Command::Save);
            }
            MenuAction::DoSaveAs => {
                read_fields(&fields, &mut menu.draft);
                commands.write(Command::SaveAs {
                    id: menu.draft.id_text.trim().to_owned(),
                    name: menu.draft.name_text.clone(),
                });
            }
            MenuAction::DoNew => {
                read_fields(&fields, &mut menu.draft);
                commands.write(Command::NewMap {
                    id: menu.draft.id_text.trim().to_owned(),
                    name: menu.draft.name_text.clone(),
                    discard: false,
                });
            }
            MenuAction::Apply => {
                read_fields(&fields, &mut menu.draft);
                let draft = menu.draft.clone();
                let name = if draft.name_text.trim().is_empty() {
                    draft.name.clone()
                } else {
                    draft.name_text.trim().to_owned()
                };
                commands.write(Command::Settings(SettingsChange {
                    name: Some(name),
                    look: Some(draft.look.clone()),
                    modes: Some(
                        MODE_KEYS
                            .iter()
                            .zip(draft.modes)
                            .filter(|(_, on)| *on)
                            .map(|(key, _)| (*key).to_owned())
                            .collect(),
                    ),
                    factions: Some(draft.factions.clone()),
                }));
            }
            MenuAction::CycleLook(step) => {
                read_fields(&fields, &mut menu.draft);
                let names = &editor.look_names;
                if names.is_empty() {
                    continue;
                }
                let at = menu
                    .draft
                    .look
                    .as_deref()
                    .and_then(|look| names.iter().position(|n| n == look))
                    .map_or(0, |i| i as i32);
                let next = (at + step).rem_euclid(names.len() as i32) as usize;
                menu.draft.look = Some(names[next].clone());
            }
            MenuAction::ToggleMode(index) => {
                read_fields(&fields, &mut menu.draft);
                menu.draft.modes[index] = !menu.draft.modes[index];
            }
            MenuAction::CycleFaction(slot, step) => {
                read_fields(&fields, &mut menu.draft);
                let ids: Vec<&String> = data.factions.iter().map(|f| &f.id).collect();
                let at = ids
                    .iter()
                    .position(|id| **id == menu.draft.factions[slot])
                    .unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(ids.len() as i32) as usize;
                menu.draft.factions[slot] = ids[next].clone();
            }
            MenuAction::Discard(command) => {
                commands.write(command.discarding());
            }
            MenuAction::SaveThen(command) => {
                commands.write(Command::SaveAnd(Box::new(command)));
            }
            MenuAction::Playtest => {
                commands.write(Command::Playtest);
            }
            MenuAction::Quit => {
                commands.write(Command::Quit { discard: false });
            }
        }
    }
}

// --------------------------------------------------------------------------------------- UI

struct Ui<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a UiFonts,
}

impl Ui<'_, '_, '_> {
    fn text(&mut self, parent: Entity, text: impl Into<String>, size: f32, color: Color) {
        self.commands.spawn((
            ChildOf(parent),
            label(self.fonts, &text.into(), size, color),
        ));
    }

    fn wrapped(&mut self, parent: Entity, text: impl Into<String>, size: f32, color: Color) {
        self.commands.spawn((
            ChildOf(parent),
            label(self.fonts, &text.into(), size, color),
            Node {
                max_width: px(PANEL_WIDTH - 50.0),
                ..default()
            },
        ));
    }

    fn big_button(&mut self, parent: Entity, text: &str, action: MenuAction) {
        self.commands
            .spawn((ChildOf(parent), button(self.fonts, text)))
            .observe(move |_: On<Activate>, mut out: MessageWriter<MenuAction>| {
                out.write(action.clone());
            });
    }

    fn small_button(&mut self, parent: Entity, text: &str, width: f32, action: MenuAction) {
        self.commands
            .spawn((
                ChildOf(parent),
                compact_button(self.fonts, text, width, 18.0),
            ))
            .observe(move |_: On<Activate>, mut out: MessageWriter<MenuAction>| {
                out.write(action.clone());
            });
    }

    fn row(&mut self, parent: Entity) -> Entity {
        self.commands
            .spawn((
                ChildOf(parent),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(10.0),
                    ..default()
                },
            ))
            .id()
    }

    fn field(&mut self, parent: Entity, which: Field, text: &str, max: usize, focus: bool) {
        let mut entity =
            self.commands
                .spawn((ChildOf(parent), which, text_field(self.fonts, text, max)));
        if focus {
            entity.insert(AutoFocus);
        }
    }
}

fn rebuild(
    mut commands: Commands,
    menu: Res<MenuState>,
    editor: Res<Editor>,
    registry: Res<MapRegistry>,
    data: Res<GameData>,
    fonts: Option<Res<UiFonts>>,
    roots: Query<Entity, With<MenuRoot>>,
) {
    if !menu.is_changed() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    let (true, Some(fonts)) = (menu.open, fonts) else {
        return;
    };
    let root = commands
        .spawn((MenuRoot, BlockPicking, backdrop(), GlobalZIndex(10)))
        .id();
    let panel = commands
        .spawn((
            ChildOf(root),
            BlockPicking,
            Node {
                width: px(PANEL_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24.0)),
                row_gap: px(10.0),
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
    let mut ui = Ui {
        commands: &mut commands,
        fonts: &fonts,
    };
    let name = registry
        .get(&editor.id)
        .map_or(String::new(), |info| info.name.clone());
    match &menu.screen {
        Screen::Main => main_screen(&mut ui, panel, &editor, &name),
        Screen::Load { page } => load_screen(&mut ui, panel, &editor, &registry, *page),
        Screen::SaveAs => {
            ui.commands
                .spawn((ChildOf(panel), title(&fonts, "Save as a new Map", 34.0)));
            ui.text(
                panel,
                "Map id (file name: letters, digits, _)",
                18.0,
                TEXT_DIM,
            );
            ui.field(panel, Field::Id, &menu.draft.id_text, 40, true);
            ui.text(panel, "Name (optional)", 18.0, TEXT_DIM);
            ui.field(panel, Field::Name, &menu.draft.name_text, 40, false);
            ui.wrapped(
                panel,
                "Writes maps/<id>.map, lists it in data/maps.ron with these settings, and opens the copy. The Map you are leaving stays as it was last saved.",
                16.0,
                TEXT_DIM,
            );
            ui.big_button(panel, "Save copy", MenuAction::DoSaveAs);
            ui.big_button(panel, "Back", MenuAction::Goto(Screen::Main));
        }
        Screen::New => {
            ui.commands
                .spawn((ChildOf(panel), title(&fonts, "New Map", 34.0)));
            ui.text(
                panel,
                "Map id (file name: letters, digits, _)",
                18.0,
                TEXT_DIM,
            );
            ui.field(panel, Field::Id, &menu.draft.id_text, 40, true);
            ui.text(panel, "Name (optional)", 18.0, TEXT_DIM);
            ui.field(panel, Field::Name, &menu.draft.name_text, 40, false);
            ui.wrapped(
                panel,
                "Creates maps/<id>.map with a 60 x 60 m floor and two spawns per Team, lists it in data/maps.ron for Team Deathmatch (add Flags, HQ and bomb sites, then set its Game Modes in Map settings) and opens it.",
                16.0,
                TEXT_DIM,
            );
            ui.big_button(panel, "Create", MenuAction::DoNew);
            ui.big_button(panel, "Back", MenuAction::Goto(Screen::Main));
        }
        Screen::Settings => settings_screen(&mut ui, panel, &menu.draft, &data, &editor),
        Screen::Confirm(command) => {
            ui.commands
                .spawn((ChildOf(panel), title(&fonts, "Unsaved changes", 34.0)));
            ui.wrapped(
                panel,
                format!(
                    "{} has changes that are not saved. Saving writes them to the Map's file.",
                    editor.id
                ),
                20.0,
                TEXT,
            );
            ui.big_button(
                panel,
                "Save and continue",
                MenuAction::SaveThen(command.clone()),
            );
            ui.big_button(
                panel,
                "Discard changes",
                MenuAction::Discard(command.clone()),
            );
            ui.big_button(panel, "Cancel", MenuAction::Goto(Screen::Main));
        }
    }
}

fn main_screen(ui: &mut Ui, panel: Entity, editor: &Editor, name: &str) {
    ui.commands
        .spawn((ChildOf(panel), title(ui.fonts, "MapEdit", 38.0)));
    let dirty = if editor.dirty() {
        "   * unsaved changes"
    } else {
        ""
    };
    ui.text(
        panel,
        format!("{}  \"{name}\"{dirty}", editor.id),
        20.0,
        if editor.dirty() { AMBER } else { TEXT },
    );
    if let Some(problem) = &editor.problem {
        ui.wrapped(
            panel,
            format!("View only: {problem}"),
            16.0,
            Color::srgb(1.0, 0.45, 0.4),
        );
    }
    if let Some(script) = editor.generator() {
        ui.wrapped(
            panel,
            format!(
                "{} generates this Map: running it overwrites what you save here.",
                script.display()
            ),
            16.0,
            AMBER,
        );
    }
    ui.big_button(panel, "Resume", MenuAction::Close);
    ui.big_button(
        panel,
        "Load Map...",
        MenuAction::Goto(Screen::Load { page: 0 }),
    );
    ui.big_button(panel, "Save", MenuAction::Save);
    ui.big_button(panel, "Save As...", MenuAction::Goto(Screen::SaveAs));
    ui.big_button(panel, "New Map...", MenuAction::Goto(Screen::New));
    ui.big_button(panel, "Map settings...", MenuAction::Goto(Screen::Settings));
    ui.big_button(panel, "Playtest (F5)", MenuAction::Playtest);
    ui.big_button(panel, "Quit MapEdit", MenuAction::Quit);
    ui.text(
        panel,
        "Esc closes. P palette. Click selects (Shift/Ctrl+click adds, drag a box); arrows, PgUp/PgDn move the selection; Alt+arrows move a face; J/K turn; Tab cycles the surface; Delete; Ctrl+D duplicate, Ctrl+C / Ctrl+V copy / paste, Ctrl+Z / Ctrl+Y undo / redo, Ctrl+S save, F5 playtest.",
        14.0,
        TEXT_DIM,
    );
}

fn load_screen(ui: &mut Ui, panel: Entity, editor: &Editor, registry: &MapRegistry, page: usize) {
    ui.commands
        .spawn((ChildOf(panel), title(ui.fonts, "Load Map", 34.0)));
    enum Entry {
        Registered(MapId, String, bool),
        Loose(LooseMap),
    }
    let mut entries: Vec<Entry> = registry
        .iter()
        .map(|(id, info)| Entry::Registered(id.clone(), info.name.clone(), info.private))
        .collect();
    entries.extend(
        loose_maps(&editor.paths, registry)
            .into_iter()
            .map(Entry::Loose),
    );
    let pages = entries.len().div_ceil(PAGE).max(1);
    let page = page.min(pages - 1);
    for entry in entries.iter().skip(page * PAGE).take(PAGE) {
        match entry {
            Entry::Registered(id, name, private) => {
                let current = if *id == editor.id { "   (open)" } else { "" };
                let private = if *private { "   private" } else { "" };
                ui.small_button(
                    panel,
                    &format!("{id}   {name}{private}{current}"),
                    PANEL_WIDTH - 50.0,
                    MenuAction::Open(id.clone()),
                );
            }
            Entry::Loose(loose) => {
                let private = if loose.private { "private/" } else { "" };
                ui.small_button(
                    panel,
                    &format!("file {private}{}.map   (not in maps.ron)", loose.stem),
                    PANEL_WIDTH - 50.0,
                    MenuAction::OpenFile(loose.clone()),
                );
            }
        }
    }
    let row = ui.row(panel);
    if page > 0 {
        ui.small_button(row, "Previous", 120.0, MenuAction::Page(-1));
    }
    ui.text(row, format!("page {} of {pages}", page + 1), 16.0, TEXT_DIM);
    if page + 1 < pages {
        ui.small_button(row, "Next", 120.0, MenuAction::Page(1));
    }
    ui.big_button(panel, "Back", MenuAction::Goto(Screen::Main));
}

fn settings_screen(ui: &mut Ui, panel: Entity, draft: &Draft, data: &GameData, editor: &Editor) {
    ui.commands
        .spawn((ChildOf(panel), title(ui.fonts, "Map settings", 34.0)));
    ui.text(panel, format!("Map {}", editor.id), 16.0, TEXT_DIM);
    ui.text(panel, "Name", 18.0, TEXT_DIM);
    ui.field(panel, Field::Name, &draft.name, 40, false);
    ui.text(panel, "Look", 18.0, TEXT_DIM);
    let row = ui.row(panel);
    ui.small_button(row, "prev", 70.0, MenuAction::CycleLook(-1));
    ui.text(
        row,
        draft
            .look
            .clone()
            .unwrap_or_else(|| "(the Map's own)".into()),
        20.0,
        ACCENT,
    );
    ui.small_button(row, "next", 70.0, MenuAction::CycleLook(1));
    ui.text(
        panel,
        "Game Modes (none selected: every mode)",
        18.0,
        TEXT_DIM,
    );
    let row = ui.row(panel);
    for (index, key) in MODE_KEYS.iter().enumerate() {
        let mark = if draft.modes[index] { "[x]" } else { "[ ]" };
        ui.small_button(
            row,
            &format!("{mark} {key}"),
            110.0,
            MenuAction::ToggleMode(index),
        );
    }
    ui.text(panel, "Factions (Alpha, Bravo)", 18.0, TEXT_DIM);
    for slot in 0..2 {
        let row = ui.row(panel);
        let name = data
            .faction_by_id(&draft.factions[slot])
            .map_or(draft.factions[slot].clone(), |f| {
                format!("{} ({})", f.name, f.id)
            });
        ui.small_button(row, "prev", 70.0, MenuAction::CycleFaction(slot, -1));
        ui.text(row, name, 20.0, ACCENT);
        ui.small_button(row, "next", 70.0, MenuAction::CycleFaction(slot, 1));
    }
    ui.wrapped(
        panel,
        "Apply changes the open Map; Save writes the name, look, Game Modes and Factions to data/maps.ron, and the name and look into the worldspawn.",
        16.0,
        TEXT_DIM,
    );
    ui.big_button(panel, "Apply", MenuAction::Apply);
    ui.big_button(panel, "Back", MenuAction::Goto(Screen::Main));
}

/// Helpers for tests that press the UI's buttons.
#[cfg(test)]
pub mod testing {
    use super::*;
    use frontline::ui::MenuButton;

    /// The button whose caption is `caption` (the game's buttons uppercase it); a caption that
    /// is only part of one (a Map's row) matches when no button says exactly that.
    pub fn button_named(app: &mut App, caption: &str) -> Entity {
        let want = caption.to_uppercase();
        let world = app.world_mut();
        let mut buttons = world.query_filtered::<(Entity, &Children), With<MenuButton>>();
        let captions: Vec<(Entity, String)> = buttons
            .iter(world)
            .filter_map(|(entity, children)| {
                children
                    .iter()
                    .find_map(|child| world.get::<Text>(child).map(|t| t.0.clone()))
                    .map(|text| (entity, text))
            })
            .collect();
        let exact: Vec<Entity> = captions
            .iter()
            .filter(|(_, t)| *t == want)
            .map(|(e, _)| *e)
            .collect();
        let found = if exact.is_empty() {
            captions
                .iter()
                .filter(|(_, t)| t.contains(&want))
                .map(|(e, _)| *e)
                .collect()
        } else {
            exact
        };
        assert_eq!(
            found.len(),
            1,
            "one button \"{caption}\", found {}",
            found.len()
        );
        found[0]
    }

    pub fn click(app: &mut App, caption: &str) {
        let entity = button_named(app, caption);
        app.world_mut().trigger(Activate { entity });
        app.update();
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{button_named, click};
    use super::*;
    use crate::edit::Paths;
    use crate::mapfile::MapFile;
    use bevy::ecs::message::Messages;
    use frontline::maps::MapInfo;
    use frontline::ui::MenuButton;
    use std::path::PathBuf;

    fn app() -> App {
        let mut app = App::new();
        let mut editor = Editor::new(
            Paths {
                assets: PathBuf::from("/nonexistent"),
                registry: PathBuf::new(),
                private: None,
            },
            Vec::new(),
        );
        editor.id = MapId::new("arena");
        editor.look_names = vec!["a_look".into(), "b_look".into()];
        editor.map = Some(MapFile::with_worldspawn("Arena", "a_look"));
        let mut registry = MapRegistry::default();
        for id in ["arena", "crossroads"] {
            registry.insert(
                MapId::new(id),
                MapInfo {
                    name: id.to_uppercase(),
                    file: format!("maps/{id}.map"),
                    look: None,
                    modes: vec!["tdm".into()],
                    factions: ["ranger".into(), "harmattan".into()],
                    private: false,
                },
            );
        }
        app.insert_resource(editor)
            .insert_resource(registry)
            .insert_resource(GameData::load())
            .insert_resource(UiFonts {
                regular: Handle::default(),
                bold: Handle::default(),
                display: Handle::default(),
            })
            .init_resource::<InputFocus>()
            .init_resource::<MenuState>()
            .add_message::<MenuAction>()
            .add_message::<Command>()
            .add_systems(Update, (handle_actions, rebuild).chain());
        app
    }

    fn commands(app: &mut App) -> Vec<Command> {
        app.world_mut()
            .resource_mut::<Messages<Command>>()
            .drain()
            .collect()
    }

    fn screen(app: &App) -> Option<&'static str> {
        app.world().resource::<MenuState>().screen_name()
    }

    #[test]
    fn the_menu_walks_through_its_screens_and_sends_commands() {
        let mut app = app();
        app.update();
        assert_eq!(screen(&app), None, "closed to begin with");
        assert!(
            app.world_mut()
                .query::<&MenuRoot>()
                .iter(app.world())
                .next()
                .is_none()
        );

        app.world_mut().resource_mut::<MenuState>().open = true;
        app.update();
        assert_eq!(screen(&app), Some("main"));
        for caption in [
            "Resume",
            "Load Map...",
            "Save",
            "Save As...",
            "New Map...",
            "Map settings...",
            "Quit MapEdit",
        ] {
            button_named(&mut app, caption);
        }

        // Save and Quit are commands.
        click(&mut app, "Save");
        click(&mut app, "Quit MapEdit");
        let sent = commands(&mut app);
        assert!(matches!(sent[0], Command::Save));
        assert!(matches!(sent[1], Command::Quit { discard: false }));

        // Load lists the registry and sends Open for the pick.
        click(&mut app, "Load Map...");
        assert_eq!(screen(&app), Some("load"));
        click(&mut app, "crossroads");
        let sent = commands(&mut app);
        assert!(
            matches!(&sent[0], Command::Open { id, discard: false } if id.0 == "crossroads"),
            "{sent:?}"
        );
        click(&mut app, "Back");
        assert_eq!(screen(&app), Some("main"));

        // New Map reads its fields.
        click(&mut app, "New Map...");
        assert_eq!(screen(&app), Some("new"));
        app.world_mut().resource_mut::<MenuState>().draft.id_text = "typed_id".into();
        app.world_mut().resource_mut::<MenuState>().set_changed();
        app.update();
        click(&mut app, "Create");
        let sent = commands(&mut app);
        assert!(
            matches!(&sent[0], Command::NewMap { id, discard: false, .. } if id == "typed_id"),
            "{sent:?}"
        );

        // Save As starts from "<id>_copy".
        app.world_mut().resource_mut::<MenuState>().screen = Screen::Main;
        app.update();
        click(&mut app, "Save As...");
        click(&mut app, "Save copy");
        let sent = commands(&mut app);
        assert!(
            matches!(&sent[0], Command::SaveAs { id, .. } if id == "arena_copy"),
            "{sent:?}"
        );

        // Settings: cycle the look, toggle a mode, apply.
        app.world_mut().resource_mut::<MenuState>().screen = Screen::Main;
        app.update();
        click(&mut app, "Map settings...");
        assert_eq!(screen(&app), Some("settings"));
        assert_eq!(
            app.world().resource::<MenuState>().draft.look.as_deref(),
            Some("a_look")
        );
        let nexts: Vec<Entity> = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<(Entity, &Children), With<MenuButton>>();
            q.iter(world)
                .filter(|(_, c)| {
                    c.iter()
                        .any(|ch| world.get::<Text>(ch).is_some_and(|t| t.0 == "NEXT"))
                })
                .map(|(e, _)| e)
                .collect()
        };
        assert_eq!(nexts.len(), 3, "next for the look and for each Faction");
        app.world_mut().trigger(Activate { entity: nexts[0] });
        app.update();
        assert_eq!(
            app.world().resource::<MenuState>().draft.look.as_deref(),
            Some("b_look")
        );
        click(&mut app, "[ ] dom");
        click(&mut app, "[x] tdm");
        assert_eq!(
            app.world().resource::<MenuState>().draft.modes,
            [false, true, false, false]
        );
        click(&mut app, "Apply");
        let sent = commands(&mut app);
        let Command::Settings(change) = &sent[0] else {
            panic!("{sent:?}")
        };
        assert_eq!(change.look, Some(Some("b_look".to_owned())));
        assert_eq!(change.modes.as_deref(), Some(&["dom".to_owned()][..]));
        assert_eq!(change.name.as_deref(), Some("ARENA"), "the registry's name");

        // Unsaved changes: the dialog offers save-and-continue, discard and cancel.
        app.world_mut()
            .resource_mut::<MenuState>()
            .confirm_discard(Command::Open {
                id: MapId::new("crossroads"),
                discard: false,
            });
        app.update();
        assert_eq!(screen(&app), Some("confirm"));
        click(&mut app, "Discard changes");
        let sent = commands(&mut app);
        assert!(
            matches!(&sent[0], Command::Open { discard: true, .. }),
            "{sent:?}"
        );
        click(&mut app, "Save and continue");
        let sent = commands(&mut app);
        let Command::SaveAnd(then) = &sent[0] else {
            panic!("{sent:?}")
        };
        assert!(matches!(**then, Command::Open { .. }));
        click(&mut app, "Cancel");
        assert_eq!(screen(&app), Some("main"));
        click(&mut app, "Resume");
        assert_eq!(screen(&app), None);
        assert!(
            app.world_mut()
                .query::<&MenuRoot>()
                .iter(app.world())
                .next()
                .is_none(),
            "the menu is gone"
        );
    }

    #[test]
    fn escape_opens_steps_back_and_closes_and_ends_typing_first() {
        let mut app = app();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputCapture>()
            .add_systems(Update, menu_keys.before(handle_actions));
        let press = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        };
        press(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Some("main"));
        click(&mut app, "Load Map...");
        press(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Some("main"), "Esc steps back");
        // A focused text field takes the Esc that ends the typing.
        click(&mut app, "New Map...");
        app.world_mut().resource_mut::<InputCapture>().typing = true;
        press(&mut app, KeyCode::Escape);
        assert_eq!(
            screen(&app),
            Some("new"),
            "the press that ended typing did not also close the screen"
        );
        app.world_mut().resource_mut::<InputCapture>().typing = false;
        press(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Some("main"));
        press(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), None, "Esc closes the menu");
    }
}
