//! Create a Class, MW2 style: from the main menu (`MenuScreen::CreateAClass`) or from the
//! Loadout screen in game (`Overlay::ClassEditor`), three levels deep and never a scrolling form:
//!
//! 1. the class list: the Default classes (read-only) and the Profile's custom ones, with a
//!    summary of the highlighted class;
//! 2. the slot overview of one custom class: big tiles for Primary (with its Attachments and
//!    camo), Secondary, Lethal, Tactical, Perk 1 to 3 (with the Greed Perk inside while that
//!    Wildcard is in) and Wildcards, the points meter, the name and SAVE / BACK;
//! 3. a pick per slot. The small slots (Secondary, Lethal, Tactical, Perk 1 to 3 with their
//!    Greed Perk, Wildcards) open a dropdown list right under their tile (over it when the
//!    window has no room below): one entry per option with its name, a one-line description,
//!    its cost, the reason when it is locked or the budget or a rule refuses it, and a mark on
//!    what the slot holds, plus a preview card beside the list for the highlighted entry. A
//!    click or Enter on an entry picks it and closes the list; a click outside or Esc closes it
//!    without a change; a list too long for the window scrolls with the mouse wheel. The Primary
//!    keeps a full-screen picker (options on the left, stat bars and costs on the right) and
//!    leads on to its own screen with an Attachment tile per Attachment it may carry (three
//!    with Gunfighter) and the camo, each with the same full-screen picker.
//!
//! Arrows move the highlight (in a dropdown: up and down by entry, left and right by a list
//! height), Enter selects, Esc backs out; the mouse hovers and clicks the same things. The
//! full-screen pickers page instead of scrolling. `model` holds the rules (what each slot can
//! hold, costs, locks, refusals, where a list goes and how it scrolls) and `view` draws the
//! screens; this file is the state, input and actions. Nothing touches the Profile until SAVE,
//! and a class that breaks a rule or holds something locked can't be saved.

mod model;
mod view;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use bevy::window::{CursorMoved, PrimaryWindow};

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
/// The "more above" and "more below" lines of a scrolling dropdown list.
const KEY_MORE_UP: usize = 1002;
const KEY_MORE_DOWN: usize = 1003;
/// The keys of a dropdown list's entries are this plus the option index.
const KEY_LIST: usize = 2000;

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
                (keyboard, wheel, hover, act, render, refresh)
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
    /// The options of the open picker or dropdown list.
    options: Vec<Choice>,
    /// The dropdown list of a small slot, over the overview.
    dropdown: Option<Dropdown>,
    message: Option<Note>,
    /// Back was pressed with unsaved changes; pressing it again discards them.
    armed: bool,
    /// The screen's content has to be built again.
    rebuild: bool,
    /// The dropdown list has to be built again (opened, scrolled or closed).
    redraw: bool,
    /// How many times it was built, so previews know to follow.
    built: u32,
    /// How many times the dropdown list was built, for the same reason.
    drawn: u32,
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
            dropdown: None,
            message: None,
            armed: false,
            rebuild: true,
            redraw: false,
            built: 0,
            drawn: 0,
        }
    }

    /// Moves to `screen` with the highlight on `cursor`.
    fn go(&mut self, screen: Screen, cursor: usize) {
        self.screen = screen;
        self.cursor = cursor;
        self.message = None;
        self.armed = false;
        self.rebuild = true;
        self.dismiss_list();
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

    /// Opens the dropdown list of `slot` from the tile at `anchor` in a `window` (width,
    /// height), the highlight on what the slot holds now.
    fn open_list(
        &mut self,
        slot: Slot,
        anchor: model::Rect,
        window: (f32, f32),
        profile: &Profile,
        data: &GameData,
    ) {
        self.cursor = key_of(slot);
        self.options = model::choices(&self.draft, slot, profile, data);
        let len = self.options.len();
        let cursor = self
            .options
            .iter()
            .position(|choice| choice.current)
            .unwrap_or(0);
        let place = model::place(anchor, window, len, card_height(slot));
        self.dropdown = Some(Dropdown {
            slot,
            cursor,
            first: model::reveal(cursor, 0, place.visible, len),
            place,
            wheel: 0.0,
        });
        self.message = None;
        self.armed = false;
        self.redraw = true;
    }

    /// Closes the dropdown list without picking; false when none was open.
    fn dismiss_list(&mut self) -> bool {
        let open = self.dropdown.take().is_some();
        self.redraw |= open;
        open
    }

    /// Moves the dropdown's highlight (the keys); the list scrolls to keep it in sight.
    fn move_list(&mut self, cursor: usize) {
        let len = self.options.len();
        let Some(list) = &mut self.dropdown else {
            return;
        };
        let cursor = cursor.min(len.saturating_sub(1));
        let first = model::reveal(cursor, list.first, list.place.visible, len);
        list.cursor = cursor;
        if first != list.first {
            list.first = first;
            self.redraw = true;
        }
        self.message = None;
    }

    /// Scrolls the dropdown list by `rows` (down is positive); the highlight stays on an entry
    /// that is shown.
    fn scroll_list(&mut self, rows: isize) {
        let len = self.options.len();
        let Some(list) = &mut self.dropdown else {
            return;
        };
        let first = model::scrolled(list.first, rows, list.place.visible, len);
        if first == list.first {
            return;
        }
        list.first = first;
        list.cursor = model::keep_shown(list.cursor, first, list.place.visible);
        self.redraw = true;
    }

    /// The mouse is over entry `index` of the dropdown list.
    fn hover_list(&mut self, index: usize) {
        if index < self.options.len()
            && let Some(list) = &mut self.dropdown
            && list.cursor != index
        {
            list.cursor = index;
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

/// An open dropdown list: what it lists is `Editor::options`.
struct Dropdown {
    slot: Slot,
    /// The highlighted option.
    cursor: usize,
    /// The first option shown.
    first: usize,
    place: model::Placement,
    /// Wheel turns not yet worth a whole row.
    wheel: f32,
}

/// Room the preview card of a dropdown list for `slot` needs: a Secondary shows stat bars.
fn card_height(slot: Slot) -> f32 {
    if slot == Slot::Secondary {
        440.0
    } else {
        240.0
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

/// The full-window root of the editor: what dropdown lists are placed against.
#[derive(Component)]
struct EditorRoot;

/// The click catcher, list and preview card of the open dropdown.
#[derive(Component)]
struct DropdownUi;

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
    /// Open the picker of a slot: a dropdown list under its tile, or the full-screen picker
    /// (Primary, Attachments, camo).
    Open(Slot),
    /// Take the option `n` of the open picker or dropdown list.
    Choose(usize),
    /// Page the full-screen picker by that many pages.
    Page(isize),
    /// Scroll the dropdown list by that many entries.
    Scroll(isize),
    /// Close the dropdown list without picking.
    CloseList,
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
        .spawn((
            Name::new("ClassEditor"),
            EditorRoot,
            ui::backdrop(),
            GlobalZIndex(10),
        ))
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
    if let Some((cursor, visible)) = editor
        .dropdown
        .as_ref()
        .map(|list| (list.cursor, list.place.visible))
    {
        if keys.just_pressed(KeyCode::Escape) {
            acts.write(Act::CloseList);
        } else if enter {
            acts.write(Act::Choose(cursor));
        }
        let rows = visible as isize;
        let delta = if keys.just_pressed(KeyCode::ArrowUp) {
            -1
        } else if keys.just_pressed(KeyCode::ArrowDown) {
            1
        } else if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::PageUp]) {
            -rows
        } else if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::PageDown]) {
            rows
        } else {
            0
        };
        let next = model::step(cursor, editor.options.len(), delta);
        if next != cursor {
            editor.move_list(next);
        }
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
    // While a dropdown list is open only its entries answer the mouse.
    if let Some(list) = &editor.dropdown {
        let under = items
            .iter()
            .find(|(nav, hovered)| hovered.0 && nav.key >= KEY_LIST)
            .map(|(nav, _)| nav.key - KEY_LIST);
        if let Some(index) = under
            && index != list.cursor
        {
            editor.hover_list(index);
        }
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

/// The mouse wheel scrolls a dropdown list that has more entries than fit.
fn wheel(mut wheel: MessageReader<MouseWheel>, mut editor: ResMut<Editor>) {
    let lines: f32 = wheel
        .read()
        .map(|turn| match turn.unit {
            MouseScrollUnit::Line => turn.y,
            MouseScrollUnit::Pixel => turn.y / model::ROW_H,
        })
        .sum();
    if lines == 0.0
        || !editor
            .dropdown
            .as_ref()
            .is_some_and(|list| list.place.scrolls(editor.options.len()))
    {
        return;
    }
    let rows = editor
        .dropdown
        .as_mut()
        .map_or(0, |list| model::wheel_rows(&mut list.wheel, lines));
    if rows != 0 {
        editor.scroll_list(rows);
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
    tiles: Query<(&Action, &ComputedNode, &UiGlobalTransform)>,
    windows: Query<&Window, With<PrimaryWindow>>,
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
            Act::Open(slot) if slot.listed() => {
                if let (Some(anchor), Ok(window)) = (tile_rect(&tiles, slot), windows.single()) {
                    // The name field gives the keys back to the list.
                    focus.clear();
                    let size = (window.width(), window.height());
                    editor.open_list(slot, anchor, size, &profile, &data);
                }
            }
            Act::Open(slot) => open_picker(&mut editor, slot, &profile, &data),
            Act::Choose(index) => choose(&mut editor, index, &data),
            Act::Scroll(rows) => editor.scroll_list(rows),
            Act::CloseList => {
                editor.dismiss_list();
            }
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

/// Where the tile that opens `slot` is, in the window's logical pixels. A Greed Perk hangs from
/// its Perk's tile.
fn tile_rect(
    tiles: &Query<(&Action, &ComputedNode, &UiGlobalTransform)>,
    slot: Slot,
) -> Option<model::Rect> {
    let tile = match slot {
        Slot::Greed(index) => Slot::Perk(index),
        other => other,
    };
    tiles
        .iter()
        .find(|(action, ..)| matches!(action.0, Act::Open(at) if at == tile))
        .map(|(_, node, transform)| {
            let scale = node.inverse_scale_factor();
            let size = node.size() * scale;
            let centre = transform.translation * scale;
            model::Rect {
                x: centre.x - size.x / 2.0,
                y: centre.y - size.y / 2.0,
                w: size.x,
                h: size.y,
            }
        })
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

/// Takes option `index` of the open dropdown list or full-screen picker and goes on (after a
/// Primary to its screen with the Attachments, otherwise back to the screen it was opened
/// from), unless it is locked or refused: then the list stays open with the reason on the
/// status line.
fn choose(editor: &mut Editor, index: usize, data: &GameData) {
    let slot = match (&editor.dropdown, editor.screen) {
        (Some(list), _) => list.slot,
        (None, Screen::Picker(slot)) => slot,
        _ => return,
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
        _ if slot.listed() => (Screen::Overview, key_of(slot)),
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

/// Builds the screen again when something moved to another one, and the dropdown list when it
/// opened, scrolled or closed.
fn render(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    data: Res<GameData>,
    profile: Res<Profile>,
    fonts: Res<UiFonts>,
    content: Query<Entity, With<Content>>,
    root: Query<Entity, With<EditorRoot>>,
    lists: Query<Entity, With<DropdownUi>>,
) {
    if !editor.rebuild && !editor.redraw {
        return;
    }
    let (Ok(content), Ok(root)) = (content.single(), root.single()) else {
        return;
    };
    if editor.rebuild {
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
    if editor.redraw {
        editor.redraw = false;
        editor.drawn += 1;
        for list in &lists {
            commands.entity(list).despawn();
        }
        let ctx = view::Ctx {
            fonts: &fonts,
            data: &data,
            profile: &profile,
            editor: &editor,
        };
        view::dropdown(&ctx, &mut commands, root);
    }
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
    mut shown: Local<Option<Shown>>,
) {
    let listed = editor.dropdown.as_ref().map(|list| KEY_LIST + list.cursor);
    for (nav, look, hovered, mut background, mut border) in &mut items {
        let lit = nav.key == editor.cursor
            || Some(nav.key) == listed
            || (!nav.follows_mouse && hovered.0);
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
    let key = Shown {
        built: editor.built,
        drawn: editor.drawn,
        screen: editor.screen,
        cursor: editor.cursor,
        class: editor.class,
        entry: editor.dropdown.as_ref().map(|list| list.cursor),
    };
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
        if let Some(list) = &editor.dropdown {
            if let Some(choice) = editor.options.get(list.cursor) {
                view::choice_preview(&ctx, &mut commands, panel, list.slot, model::CARD_W, choice);
            }
            continue;
        }
        match editor.screen {
            Screen::List => view::class_summary(&ctx, &mut commands, panel),
            Screen::Picker(slot) => {
                if let Some(choice) = editor.options.get(editor.cursor) {
                    view::choice_preview(&ctx, &mut commands, panel, slot, view::PREVIEW_W, choice);
                }
            }
            _ => {}
        }
    }
}

/// What the preview panels were last filled for.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    built: u32,
    drawn: u32,
    screen: Screen,
    cursor: usize,
    class: usize,
    /// The highlighted entry of the dropdown list, when one is open.
    entry: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::model::{Pick, Rect};
    use super::*;
    use crate::class::EquipmentSlot;
    use crate::gear::Perks;

    const WINDOW: (f32, f32) = (1280.0, 720.0);
    const TILE: Rect = Rect {
        x: 52.0,
        y: 130.0,
        w: 285.0,
        h: 206.0,
    };

    fn veteran(data: &GameData) -> Profile {
        let mut profile = Profile::fresh(data, 0);
        profile.xp = data.progression.levels.xp_to_reach(30);
        profile
    }

    /// The editor on the overview of custom class 1, with no Perks or Wildcards in it.
    fn editing(profile: &Profile, data: &GameData) -> Editor {
        let mut editor = Editor::new(Origin::Menu, profile, data);
        editor.draft.perks = Perks::default();
        editor.draft.wildcards.clear();
        editor.go(Screen::Overview, key_of(Slot::Perk(0)));
        editor
    }

    #[test]
    fn a_click_on_an_entry_picks_it_closes_the_list_and_keeps_the_tile_highlighted() {
        let data = GameData::load();
        let profile = veteran(&data);
        let mut editor = editing(&profile, &data);
        editor.open_list(Slot::Perk(0), TILE, WINDOW, &profile, &data);
        assert!(
            editor.options[0].current,
            "None is what the empty slot holds"
        );
        let at = editor
            .options
            .iter()
            .position(|choice| choice.usable() && matches!(choice.pick, Pick::Perk(_)))
            .expect("a Perk to take");
        let Pick::Perk(perk) = editor.options[at].pick else {
            unreachable!()
        };
        choose(&mut editor, at, &data);
        assert!(editor.dropdown.is_none() && editor.redraw && editor.rebuild);
        let tile = key_of(Slot::Perk(0));
        assert_eq!((editor.screen, editor.cursor), (Screen::Overview, tile));
        assert_eq!(editor.draft.perks.picks[0], Some(perk));
    }

    #[test]
    fn choosing_none_empties_an_optional_slot() {
        let data = GameData::load();
        let profile = veteran(&data);
        let mut editor = editing(&profile, &data);
        editor.draft.perks.picks[0] = Some(data.perks_in_slot(1).next().expect("a slot 1 Perk"));
        editor.open_list(Slot::Perk(0), TILE, WINDOW, &profile, &data);
        let list = editor.dropdown.as_ref().expect("open");
        assert_ne!(list.cursor, 0, "the highlight starts on the held Perk");
        choose(&mut editor, 0, &data);
        assert_eq!(editor.draft.perks.picks[0], None);
    }

    #[test]
    fn a_locked_entry_keeps_the_list_open_with_its_reason_and_the_class_unchanged() {
        let data = GameData::load();
        let fresh = Profile::fresh(&data, 0);
        let mut editor = editing(&fresh, &data);
        let slots = [
            Slot::Secondary,
            Slot::Perk(0),
            Slot::Wildcards,
            Slot::Equipment(EquipmentSlot::Lethal),
            Slot::Equipment(EquipmentSlot::Tactical),
        ];
        let (slot, at) = slots
            .into_iter()
            .find_map(|slot| {
                model::choices(&editor.draft, slot, &fresh, &data)
                    .iter()
                    .position(|choice| choice.lock.is_some())
                    .map(|at| (slot, at))
            })
            .expect("something is locked at Level 1");
        let before = editor.draft.clone();
        editor.open_list(slot, TILE, WINDOW, &fresh, &data);
        choose(&mut editor, at, &data);
        assert!(editor.dropdown.is_some());
        assert_eq!(editor.draft, before);
        assert!(editor.message.as_ref().is_some_and(|note| !note.good));
    }

    #[test]
    fn closing_the_list_changes_nothing() {
        let data = GameData::load();
        let profile = veteran(&data);
        let mut editor = editing(&profile, &data);
        let before = editor.draft.clone();
        editor.open_list(Slot::Secondary, TILE, WINDOW, &profile, &data);
        assert!(editor.dismiss_list());
        assert!(editor.dropdown.is_none() && editor.redraw);
        assert_eq!(editor.draft, before);
        assert!(!editor.dismiss_list(), "nothing left to close");
    }

    #[test]
    fn the_keys_and_the_wheel_scroll_a_list_that_does_not_fit() {
        let data = GameData::load();
        let profile = veteran(&data);
        let mut editor = editing(&profile, &data);
        // A window with room for one entry.
        editor.open_list(Slot::Wildcards, TILE, (1280.0, 400.0), &profile, &data);
        let len = editor.options.len();
        let list = editor.dropdown.as_ref().expect("open");
        let visible = list.place.visible;
        assert!(visible < len, "{visible} of {len} fit");
        assert_eq!((list.cursor, list.first), (0, 0));
        editor.move_list(len - 1);
        let list = editor.dropdown.as_ref().expect("open");
        assert_eq!((list.cursor, list.first), (len - 1, len - visible));
        editor.scroll_list(-100);
        let list = editor.dropdown.as_ref().expect("open");
        assert_eq!(list.first, 0);
        assert_eq!(list.cursor, visible - 1, "the highlight stays in sight");
    }

    #[test]
    fn the_primary_keeps_its_full_screen_picker_and_goes_on_to_its_attachments() {
        let data = GameData::load();
        let profile = veteran(&data);
        let mut editor = editing(&profile, &data);
        assert!(!Slot::Primary.listed());
        open_picker(&mut editor, Slot::Primary, &profile, &data);
        assert_eq!(editor.screen, Screen::Picker(Slot::Primary));
        assert!(editor.dropdown.is_none());
        let at = editor
            .options
            .iter()
            .position(|choice| choice.usable() && !choice.current)
            .expect("another Primary");
        choose(&mut editor, at, &data);
        assert_eq!(editor.screen, Screen::Gun);
    }
}
