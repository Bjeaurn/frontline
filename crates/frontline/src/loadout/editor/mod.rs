//! Create a Class, MW2 style: from the main menu (`MenuScreen::CreateAClass`) or from the
//! Loadout screen in game (`Overlay::ClassEditor`), three levels deep and never a scrolling form:
//!
//! 1. the class list: the Default classes (read-only) and the Profile's custom ones, with a
//!    summary of the highlighted class;
//! 2. the slot overview of one custom class: big tiles for Primary (with its Attachments and
//!    camo), Secondary, Lethal, Tactical, Perk 1 to 3 (with the Greed Perk inside while that
//!    Wildcard is in) and Wildcards, the points meter, the name and SAVE / BACK;
//! 3. a full-screen picker per slot: the options on the left (locked ones greyed with what
//!    unlocks them, ones the budget or a rule refuses marked in red), a preview with stat bars
//!    and costs on the right. The Primary leads on to its own screen with an Attachment tile per
//!    Attachment it may carry (three with Gunfighter) and the camo, each with its own picker.
//!
//! Arrows move the highlight, Enter selects, Esc backs out; the mouse hovers and clicks the same
//! things. Long pickers page instead of scrolling. `model` holds the rules (what each slot can
//! hold, costs, locks, refusals) and `view` draws the screens; this file is the state, input and
//! actions. Nothing touches the Profile until SAVE, and a class that breaks a rule or holds
//! something locked can't be saved.

mod model;
mod view;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use bevy::window::CursorMoved;

use self::model::{Choice, Dir, PAGE_SIZE, Slot, Spot};
use crate::data::GameData;
use crate::flow::{MenuScreen, Overlay};
use crate::gear::Loadout;
use crate::profile::{LoadoutChoice, Profile};
use crate::ui::{self, UiFonts};

/// Item keys of the buttons and the name field (the tiles and rows use their own index).
const KEY_NAME: usize = 20;
const KEY_SAVE: usize = 21;
const KEY_BACK: usize = 22;
const KEY_EDIT: usize = 30;
const KEY_PREV: usize = 1000;
const KEY_NEXT: usize = 1001;

pub(super) struct ClassEditorPlugin;

impl Plugin for ClassEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Act>()
            .add_observer(on_activate)
            .add_systems(OnEnter(MenuScreen::CreateAClass), open_from_menu)
            .add_systems(OnEnter(Overlay::ClassEditor), open_in_game)
            .add_systems(OnExit(MenuScreen::CreateAClass), close)
            .add_systems(OnExit(Overlay::ClassEditor), close)
            .add_systems(
                Update,
                (keyboard, hover, act, render, refresh)
                    .chain()
                    .run_if(editor_open)
                    .run_if(resource_exists::<Editor>),
            );
    }
}

/// The editor is showing, over the main menu or over the game.
fn editor_open(menu: Option<Res<State<MenuScreen>>>, overlay: Option<Res<State<Overlay>>>) -> bool {
    menu.is_some_and(|screen| *screen.get() == MenuScreen::CreateAClass)
        || overlay.is_some_and(|screen| *screen.get() == Overlay::ClassEditor)
}

/// Where the editor was opened from, which is where closing the class list goes back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// The main menu: back to the title.
    Menu,
    /// In game: back to the Loadout screen.
    Game,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    List,
    Overview,
    /// The Primary's Attachments and camo.
    Gun,
    Picker(Slot),
}

/// A line under the title: why something was refused, or that the class was saved.
struct Note {
    text: String,
    good: bool,
}

impl Note {
    fn bad(text: impl Into<String>) -> Option<Note> {
        Some(Note {
            text: text.into(),
            good: false,
        })
    }
}

/// The editor's state, there while it is open.
#[derive(Resource)]
struct Editor {
    origin: Origin,
    screen: Screen,
    /// The highlighted item's key on this screen (the option index in a picker).
    cursor: usize,
    /// The highlighted class of the list, as an index into defaults then customs.
    class: usize,
    /// How many classes the list has.
    classes: usize,
    /// The custom slot being edited and its working copy; nothing reaches the Profile before
    /// SAVE.
    slot: usize,
    draft: Loadout,
    /// The options of the open picker.
    options: Vec<Choice>,
    message: Option<Note>,
    /// Back was pressed with unsaved changes; pressing it again discards them.
    armed: bool,
    /// The screen's content has to be built again.
    rebuild: bool,
    /// How many times it was built, so previews know to follow.
    built: u32,
}

impl Editor {
    fn new(origin: Origin, profile: &Profile, data: &GameData) -> Editor {
        let defaults = data.default_loadouts.len();
        let class = match profile.chosen {
            LoadoutChoice::Default(index) => index,
            LoadoutChoice::Custom(slot) => defaults + slot,
        };
        Editor {
            origin,
            screen: Screen::List,
            cursor: class,
            class,
            classes: defaults + profile.custom.len(),
            slot: 0,
            draft: profile.custom[0].clone(),
            options: Vec::new(),
            message: None,
            armed: false,
            rebuild: true,
            built: 0,
        }
    }

    /// Moves to `screen` with the highlight on `cursor`.
    fn go(&mut self, screen: Screen, cursor: usize) {
        self.screen = screen;
        self.cursor = cursor;
        self.message = None;
        self.armed = false;
        self.rebuild = true;
    }

    /// Moves the highlight; a picker flips its page when the highlight leaves it.
    fn set_cursor(&mut self, cursor: usize) {
        if cursor == self.cursor {
            return;
        }
        if matches!(self.screen, Screen::Picker(_))
            && model::page_of(cursor) != model::page_of(self.cursor)
        {
            self.rebuild = true;
        }
        self.cursor = cursor;
        self.message = None;
        if self.screen == Screen::List && cursor < self.classes {
            self.class = cursor;
        }
    }

    /// The screen above this one, with the highlight on the item that led here.
    fn parent(&self) -> Option<(Screen, usize)> {
        match self.screen {
            Screen::List => None,
            Screen::Overview => Some((Screen::List, self.class)),
            Screen::Gun => Some((Screen::Overview, key_of(Slot::Primary))),
            Screen::Picker(slot) if slot.on_gun() => Some((Screen::Gun, key_of(slot))),
            Screen::Picker(slot) => Some((Screen::Overview, key_of(slot))),
        }
    }
}

/// The class list's row `index` (defaults first) as a Loadout choice.
fn class_choice(index: usize, defaults: usize) -> LoadoutChoice {
    if index < defaults {
        LoadoutChoice::Default(index)
    } else {
        LoadoutChoice::Custom(index - defaults)
    }
}

/// The key of the tile of `slot` on the overview (or on the Primary's screen).
fn key_of(slot: Slot) -> usize {
    use crate::class::EquipmentSlot;
    match slot {
        Slot::Primary => 0,
        Slot::Secondary => 1,
        Slot::Equipment(EquipmentSlot::Lethal) => 2,
        Slot::Equipment(EquipmentSlot::Tactical) => 3,
        Slot::Perk(index) => 4 + index,
        Slot::Wildcards => 7,
        Slot::Greed(index) => 8 + index,
        Slot::Attachment(index) => index,
        Slot::Camo => 3,
    }
}

// ----------------------------------------------------------------- components

/// The root of the screen under which `render` builds the current screen.
#[derive(Component)]
struct Content;

/// The panel whose contents follow the highlight.
#[derive(Component)]
struct PreviewPanel;

/// The line that shows `Editor::message` (or the class list's hint).
#[derive(Component)]
struct Status;

#[derive(Component)]
struct NameField;

/// The EDIT button, dimmed while the highlighted class can't be edited.
#[derive(Component)]
struct EditButton;

/// A selectable item: where the highlight is when `key` is the cursor, and where it sits for
/// the arrow keys.
#[derive(Component, Clone, Copy)]
struct Nav {
    key: usize,
    x: f32,
    y: f32,
    /// Of nested items under the mouse, the highest layer takes the highlight.
    layer: u8,
    /// Whether hovering it moves the highlight (not for things the keyboard can't reach).
    follows_mouse: bool,
}

impl Nav {
    fn new(key: usize, x: f32, y: f32) -> Nav {
        Nav {
            key,
            x,
            y,
            layer: 0,
            follows_mouse: true,
        }
    }

    fn layered(self) -> Nav {
        Nav { layer: 1, ..self }
    }

    fn mouse_only(self) -> Nav {
        Nav {
            follows_mouse: false,
            ..self
        }
    }
}

/// What activating an item does.
#[derive(Component, Clone, Copy)]
struct Action(Act);

/// Colours of an item, idle and highlighted.
#[derive(Component, Clone, Copy)]
struct Look {
    idle: Color,
    idle_border: Color,
    lit: Color,
    lit_border: Color,
}

/// Something the player asked for, by click or Enter.
#[derive(Message, Clone, Copy, Debug)]
enum Act {
    /// A class of the list (index into defaults then customs).
    Class(usize),
    Edit,
    /// Open the picker of a slot.
    Open(Slot),
    /// Take the option `n` of the open picker.
    Choose(usize),
    /// Page the picker by that many pages.
    Page(isize),
    Save,
    Back,
    FocusName,
}

fn on_activate(activate: On<Activate>, actions: Query<&Action>, mut acts: MessageWriter<Act>) {
    if let Ok(action) = actions.get(activate.entity) {
        acts.write(action.0);
    }
}

// -------------------------------------------------------------------- opening

fn open_from_menu(commands: Commands, profile: Res<Profile>, data: Res<GameData>) {
    open(commands, Origin::Menu, &profile, &data);
}

fn open_in_game(commands: Commands, profile: Res<Profile>, data: Res<GameData>) {
    open(commands, Origin::Game, &profile, &data);
}

fn open(mut commands: Commands, origin: Origin, profile: &Profile, data: &GameData) {
    commands.insert_resource(Editor::new(origin, profile, data));
    let root = commands
        .spawn((Name::new("ClassEditor"), ui::backdrop(), GlobalZIndex(10)))
        .insert(BackgroundColor(match origin {
            Origin::Game => ui::BACKDROP_SOLID,
            Origin::Menu => ui::BACKDROP,
        }))
        .insert(Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(24.0), px(22.0)),
            ..default()
        })
        .id();
    match origin {
        Origin::Menu => commands
            .entity(root)
            .insert(DespawnOnExit(MenuScreen::CreateAClass)),
        Origin::Game => commands
            .entity(root)
            .insert(DespawnOnExit(Overlay::ClassEditor)),
    };
    commands.spawn((
        Content,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10.0),
            ..default()
        },
        ChildOf(root),
    ));
}

fn close(mut commands: Commands) {
    commands.remove_resource::<Editor>();
}

// ---------------------------------------------------------------------- input

/// Arrows, Enter and Esc. Ignored while the name field has the keys (Esc and Enter end the
/// typing; the press that ended it isn't acted on twice).
fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(), With<NameField>>,
    mut was_typing: Local<bool>,
    mut editor: ResMut<Editor>,
    spots: Query<(&Nav, &Action)>,
    mut acts: MessageWriter<Act>,
) {
    let typing = focus.get().is_some_and(|entity| fields.contains(entity));
    let blocked = typing || *was_typing;
    *was_typing = typing;
    let enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    if typing && enter {
        focus.clear();
    }
    if blocked {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        acts.write(Act::Back);
    } else if enter
        && let Some((_, action)) = spots.iter().find(|(nav, _)| nav.key == editor.cursor)
    {
        acts.write(action.0);
    }
    let dir = [
        (KeyCode::ArrowUp, Dir::Up),
        (KeyCode::ArrowDown, Dir::Down),
        (KeyCode::ArrowLeft, Dir::Left),
        (KeyCode::ArrowRight, Dir::Right),
    ]
    .into_iter()
    .find(|(key, _)| keys.just_pressed(*key))
    .map(|(_, dir)| dir);
    let page = if keys.just_pressed(KeyCode::PageUp) {
        Some(Dir::Left)
    } else if keys.just_pressed(KeyCode::PageDown) {
        Some(Dir::Right)
    } else {
        None
    };
    let Some(dir) = dir.or(page) else {
        return;
    };
    let next = if matches!(editor.screen, Screen::Picker(_)) {
        let delta = match dir {
            Dir::Up => -1,
            Dir::Down => 1,
            Dir::Left => -(PAGE_SIZE as isize),
            Dir::Right => PAGE_SIZE as isize,
        };
        model::step(editor.cursor, editor.options.len(), delta)
    } else {
        let spots: Vec<Spot> = spots
            .iter()
            .map(|(nav, _)| Spot {
                key: nav.key,
                x: nav.x,
                y: nav.y,
            })
            .collect();
        model::neighbour(&spots, editor.cursor, dir)
    };
    if next != editor.cursor {
        editor.set_cursor(next);
    }
}

/// The mouse moving over an item highlights it (and a nested item over the tile it sits in).
fn hover(
    mut moved: MessageReader<CursorMoved>,
    items: Query<(&Nav, &Hovered)>,
    mut editor: ResMut<Editor>,
) {
    if moved.read().count() == 0 {
        return;
    }
    let under = items
        .iter()
        .filter(|(nav, hovered)| hovered.0 && nav.follows_mouse)
        .max_by_key(|(nav, _)| nav.layer);
    if let Some((nav, _)) = under
        && nav.key != editor.cursor
    {
        editor.set_cursor(nav.key);
    }
}

// -------------------------------------------------------------------- actions

#[allow(clippy::too_many_arguments)]
fn act(
    mut acts: MessageReader<Act>,
    mut editor: ResMut<Editor>,
    data: Res<GameData>,
    mut profile: ResMut<Profile>,
    name: Query<(Entity, &EditableText), With<NameField>>,
    mut focus: ResMut<InputFocus>,
    mut menu: Option<ResMut<NextState<MenuScreen>>>,
    mut overlay: Option<ResMut<NextState<Overlay>>>,
) {
    if acts.is_empty() {
        return;
    }
    // Keep what was typed in the name field.
    if let Ok((_, field)) = name.single() {
        let typed = field.value().to_string();
        if typed != editor.draft.name {
            editor.draft.name = typed;
        }
    }
    for act in acts.read().copied() {
        match act {
            Act::Class(index) => {
                editor.set_cursor(index);
                open_class(&mut editor, &profile, &data);
            }
            Act::Edit => open_class(&mut editor, &profile, &data),
            Act::Open(slot) => open_picker(&mut editor, slot, &profile, &data),
            Act::Choose(index) => choose(&mut editor, index, &data),
            Act::Page(delta) => {
                let step = delta * PAGE_SIZE as isize;
                let next = model::step(editor.cursor, editor.options.len(), step);
                editor.set_cursor(next);
            }
            Act::Save => save(&mut editor, &mut profile, &data),
            Act::Back => back(&mut editor, &profile, &mut menu, &mut overlay),
            Act::FocusName => {
                if let Ok((entity, _)) = name.single() {
                    focus.set(entity, FocusCause::Navigated);
                }
            }
        }
    }
}

/// Why the highlighted class can't be edited, if it can't.
fn edit_block(editor: &Editor, profile: &Profile, data: &GameData) -> Option<String> {
    match class_choice(editor.class, data.default_loadouts.len()) {
        LoadoutChoice::Default(_) => {
            Some("Default classes are read-only. Pick a custom class to edit it.".to_owned())
        }
        LoadoutChoice::Custom(_) if !profile.custom_unlocked(data) => Some(format!(
            "Custom classes unlock at Level {}",
            data.progression
                .unlock_level(crate::progression::UnlockItem::CustomLoadouts)
        )),
        LoadoutChoice::Custom(_) => None,
    }
}

/// Selecting a custom class opens its slot overview.
fn open_class(editor: &mut Editor, profile: &Profile, data: &GameData) {
    if let Some(reason) = edit_block(editor, profile, data) {
        editor.message = Note::bad(reason);
        return;
    }
    let LoadoutChoice::Custom(slot) = class_choice(editor.class, data.default_loadouts.len())
    else {
        return;
    };
    editor.slot = slot;
    editor.draft = profile.custom[slot].clone();
    editor.go(Screen::Overview, 0);
}

fn open_picker(editor: &mut Editor, slot: Slot, profile: &Profile, data: &GameData) {
    editor.options = model::choices(&editor.draft, slot, profile, data);
    let at = editor
        .options
        .iter()
        .position(|choice| choice.current)
        .unwrap_or(0);
    editor.go(Screen::Picker(slot), at);
}

/// Takes option `index` of the open picker and goes on (to the Primary's screen after a
/// Primary, otherwise back where the picker was opened from), unless it is locked or refused.
fn choose(editor: &mut Editor, index: usize, data: &GameData) {
    let Screen::Picker(slot) = editor.screen else {
        return;
    };
    let Some(choice) = editor.options.get(index) else {
        return;
    };
    if let Some(reason) = choice.lock.as_ref().or(choice.refusal.as_ref()) {
        editor.message = Note::bad(format!("{}: {reason}", choice.name));
        return;
    }
    let after = choice.after.clone();
    info!(
        "Class {}: {} -> {} ({} / {} points)",
        editor.slot + 1,
        slot.label(),
        choice.name,
        data.class_cost(&after),
        data.class_points.budget
    );
    editor.draft = after;
    let (screen, cursor) = match slot {
        Slot::Primary => (Screen::Gun, key_of(Slot::Attachment(0))),
        _ => editor.parent().expect("a picker has a parent"),
    };
    editor.go(screen, cursor);
}

fn save(editor: &mut Editor, profile: &mut Profile, data: &GameData) {
    let mut loadout = editor.draft.clone();
    if loadout.name.trim().is_empty() {
        loadout.name = format!("Custom {}", editor.slot + 1);
    }
    let loadout = loadout.validated(data);
    let problems = model::problems(&loadout, profile, data);
    if !problems.is_empty() {
        let text = problems.join("; ");
        info!("Can't save class {}: {text}", editor.slot + 1);
        editor.message = Note::bad(format!("Can't save yet: {text}"));
        return;
    }
    info!(
        "Saved custom class {}: \"{}\" ({} / {} points)",
        editor.slot + 1,
        loadout.name,
        data.class_cost(&loadout),
        data.class_points.budget
    );
    if profile.custom[editor.slot] != loadout {
        profile.custom[editor.slot] = loadout.clone();
    }
    editor.draft = loadout;
    let class = editor.class;
    editor.go(Screen::List, class);
    editor.message = Some(Note {
        text: "Class saved".into(),
        good: true,
    });
}

/// One level up; out of the class list it leaves the editor (to the title, or in game to the
/// Loadout screen). Leaving a class with unsaved changes asks once.
fn back(
    editor: &mut Editor,
    profile: &Profile,
    menu: &mut Option<ResMut<NextState<MenuScreen>>>,
    overlay: &mut Option<ResMut<NextState<Overlay>>>,
) {
    if editor.screen == Screen::Overview
        && !editor.armed
        && editor.draft != profile.custom[editor.slot]
    {
        editor.armed = true;
        editor.message = Note::bad("Unsaved changes: Back again discards them, or SAVE");
        return;
    }
    match editor.parent() {
        Some((screen, cursor)) => editor.go(screen, cursor),
        None => match (editor.origin, menu, overlay) {
            (Origin::Menu, Some(menu), _) => menu.set(MenuScreen::Title),
            (Origin::Game, _, Some(overlay)) => overlay.set(Overlay::Loadout),
            _ => {}
        },
    }
}

// ------------------------------------------------------------------ rendering

/// Builds the screen again when something moved to another one.
fn render(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    data: Res<GameData>,
    profile: Res<Profile>,
    fonts: Res<UiFonts>,
    content: Query<Entity, With<Content>>,
) {
    if !editor.rebuild {
        return;
    }
    let Ok(content) = content.single() else {
        return;
    };
    editor.rebuild = false;
    editor.built += 1;
    commands.entity(content).despawn_children();
    let ctx = view::Ctx {
        fonts: &fonts,
        data: &data,
        profile: &profile,
        editor: &editor,
    };
    view::build(&ctx, &mut commands, content);
}

/// Highlights, the status line, the EDIT button and the preview panel.
#[allow(clippy::too_many_arguments)]
fn refresh(
    mut commands: Commands,
    editor: Res<Editor>,
    data: Res<GameData>,
    profile: Res<Profile>,
    fonts: Res<UiFonts>,
    mut items: Query<(
        &Nav,
        &Look,
        &Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut status: Query<(&mut Text, &mut TextColor), With<Status>>,
    edit: Query<&Children, With<EditButton>>,
    mut colors: Query<&mut TextColor, Without<Status>>,
    panels: Query<Entity, With<PreviewPanel>>,
    mut shown: Local<Option<(u32, Screen, usize, usize)>>,
) {
    for (nav, look, hovered, mut background, mut border) in &mut items {
        let lit = nav.key == editor.cursor || (!nav.follows_mouse && hovered.0);
        let (fill, edge) = if lit {
            (look.lit, look.lit_border)
        } else {
            (look.idle, look.idle_border)
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
    }
    if !editor.is_changed() {
        return;
    }
    let blocked = edit_block(&editor, &profile, &data);
    let (line, tint) = match (&editor.message, editor.screen) {
        (Some(note), _) => (
            note.text.clone(),
            if note.good { ui::ACCENT } else { view::RED },
        ),
        (None, Screen::List) => (blocked.clone().unwrap_or_default(), ui::TEXT_DIM),
        (None, _) => (String::new(), view::RED),
    };
    for (mut text, mut color) in &mut status {
        if text.0 != line {
            text.0.clone_from(&line);
        }
        color.set_if_neq(TextColor(tint));
    }
    for children in &edit {
        for child in children {
            if let Ok(mut color) = colors.get_mut(*child) {
                color.set_if_neq(TextColor(if blocked.is_some() {
                    ui::TEXT_DIM
                } else {
                    ui::TEXT
                }));
            }
        }
    }
    let key = (editor.built, editor.screen, editor.cursor, editor.class);
    if *shown == Some(key) {
        return;
    }
    *shown = Some(key);
    let ctx = view::Ctx {
        fonts: &fonts,
        data: &data,
        profile: &profile,
        editor: &editor,
    };
    for panel in &panels {
        commands.entity(panel).despawn_children();
        match editor.screen {
            Screen::List => view::class_summary(&ctx, &mut commands, panel),
            Screen::Picker(_) => {
                if let Some(choice) = editor.options.get(editor.cursor) {
                    view::choice_preview(&ctx, &mut commands, panel, choice);
                }
            }
            _ => {}
        }
    }
}
