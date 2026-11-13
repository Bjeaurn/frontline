//! Rebindable controls for the local player.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Keys that trigger one action; any of them counts. Empty means unbound. Saved by key name
/// (`ShiftLeft`); a name this version doesn't know is dropped on load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<String>", into = "Vec<String>")]
pub struct Binding(pub Vec<KeyCode>);

impl From<Binding> for Vec<String> {
    fn from(binding: Binding) -> Self {
        binding
            .0
            .into_iter()
            .map(|key| format!("{key:?}"))
            .collect()
    }
}

impl From<Vec<String>> for Binding {
    fn from(names: Vec<String>) -> Self {
        Self(
            names
                .iter()
                .filter_map(|name| {
                    BINDABLE_KEYS
                        .iter()
                        .copied()
                        .find(|key| format!("{key:?}") == *name)
                })
                .collect(),
        )
    }
}

/// The keys the Controls list offers (and settings.ron can name): letters, digits, function,
/// arrow, numpad, modifier and punctuation keys. Escape is listed because it is the default
/// Pause key, but the list never binds it: it cancels a rebind.
const BINDABLE_KEYS: &[KeyCode] = &[
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::Space,
    KeyCode::Tab,
    KeyCode::Enter,
    KeyCode::Backspace,
    KeyCode::CapsLock,
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::Insert,
    KeyCode::Delete,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Backquote,
    KeyCode::Minus,
    KeyCode::Equal,
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Backslash,
    KeyCode::Semicolon,
    KeyCode::Quote,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::NumpadAdd,
    KeyCode::NumpadSubtract,
    KeyCode::NumpadMultiply,
    KeyCode::NumpadDivide,
    KeyCode::NumpadEnter,
    KeyCode::NumpadDecimal,
    KeyCode::Escape,
];

/// Whether the Controls list can bind `key` (Escape never: it cancels).
pub fn is_bindable(key: KeyCode) -> bool {
    key != KeyCode::Escape && BINDABLE_KEYS.contains(&key)
}

impl Binding {
    fn keys(keys: &[KeyCode]) -> Self {
        Self(keys.to_vec())
    }

    pub fn pressed(&self, input: &ButtonInput<KeyCode>) -> bool {
        input.any_pressed(self.0.iter().copied())
    }

    pub fn just_pressed(&self, input: &ButtonInput<KeyCode>) -> bool {
        input.any_just_pressed(self.0.iter().copied())
    }
}

/// A mouse button for one action; `None` means unbound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Option<SavedMouseButton>", into = "Option<SavedMouseButton>")]
pub struct MouseBinding(pub Option<MouseButton>);

/// `MouseButton` as saved in settings.ron.
#[derive(Clone, Copy, Serialize, Deserialize)]
enum SavedMouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

impl From<MouseBinding> for Option<SavedMouseButton> {
    fn from(binding: MouseBinding) -> Self {
        binding.0.map(|button| match button {
            MouseButton::Left => SavedMouseButton::Left,
            MouseButton::Right => SavedMouseButton::Right,
            MouseButton::Middle => SavedMouseButton::Middle,
            MouseButton::Back => SavedMouseButton::Back,
            MouseButton::Forward => SavedMouseButton::Forward,
            MouseButton::Other(n) => SavedMouseButton::Other(n),
        })
    }
}

impl From<Option<SavedMouseButton>> for MouseBinding {
    fn from(saved: Option<SavedMouseButton>) -> Self {
        Self(saved.map(|button| match button {
            SavedMouseButton::Left => MouseButton::Left,
            SavedMouseButton::Right => MouseButton::Right,
            SavedMouseButton::Middle => MouseButton::Middle,
            SavedMouseButton::Back => MouseButton::Back,
            SavedMouseButton::Forward => MouseButton::Forward,
            SavedMouseButton::Other(n) => MouseButton::Other(n),
        }))
    }
}

impl MouseBinding {
    pub fn pressed(&self, input: &ButtonInput<MouseButton>) -> bool {
        self.0.is_some_and(|button| input.pressed(button))
    }

    pub fn just_pressed(&self, input: &ButtonInput<MouseButton>) -> bool {
        self.0.is_some_and(|button| input.just_pressed(button))
    }
}

/// All local-player bindings, in one place so the settings menu can rebind them. Part of the
/// saved `Settings`; the `Bindings` resource is its copy in effect.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub forward: Binding,
    pub back: Binding,
    pub left: Binding,
    pub right: Binding,
    pub sprint: Binding,
    /// Jump when standing; stand up when crouched or prone.
    pub jump: Binding,
    /// Toggle crouch.
    pub crouch: Binding,
    /// Toggle prone.
    pub prone: Binding,
    /// Grab the cursor for mouse look (when it got freed, e.g. by losing window focus). Not
    /// rebindable in the menu (clicking the game always takes the mouse back), so not saved.
    #[serde(skip)]
    pub grab_cursor: MouseButton,
    /// Open the pause menu; steps back out of menus. Not rebindable in the menu: Esc is also
    /// what cancels a rebind, so it can never be taken away.
    pub pause: Binding,
    /// Hold to fire.
    pub fire: MouseBinding,
    /// Hold to aim down sights.
    pub aim: MouseBinding,
    pub reload: Binding,
    /// Switch to the Weapon's next fire mode.
    pub fire_mode: Binding,
    /// Hold to ready the lethal / the tactical equipment, let go to throw it.
    pub lethal: Binding,
    pub tactical: Binding,
    /// Knife whoever is in front of you (melee), as V does in CoD4.
    pub melee: Binding,
    /// Switch the Primary into and out of the underslung grenade launcher (fire it with the
    /// fire button). 3, CoD4's attachment slot.
    pub launcher: Binding,
    /// Take the Primary / the Secondary (the mouse wheel switches to the other Weapon).
    pub primary_weapon: Binding,
    pub secondary_weapon: Binding,
    /// Open the team select screen.
    pub team_menu: Binding,
    /// Hold to show the Scoreboard.
    pub scoreboard: Binding,
    /// Start typing a chat message to everyone / to the Team.
    pub chat_all: Binding,
    pub chat_team: Binding,
    /// Hold to plant or defuse the bomb.
    pub use_objective: Binding,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            forward: Binding::keys(&[KeyCode::KeyW]),
            back: Binding::keys(&[KeyCode::KeyS]),
            left: Binding::keys(&[KeyCode::KeyA]),
            right: Binding::keys(&[KeyCode::KeyD]),
            sprint: Binding::keys(&[KeyCode::ShiftLeft]),
            jump: Binding::keys(&[KeyCode::Space]),
            crouch: Binding::keys(&[KeyCode::KeyC]),
            prone: Binding::keys(&[KeyCode::ControlLeft, KeyCode::KeyZ]),
            grab_cursor: MouseButton::Left,
            pause: Binding::keys(&[KeyCode::Escape]),
            fire: MouseBinding(Some(MouseButton::Left)),
            aim: MouseBinding(Some(MouseButton::Right)),
            reload: Binding::keys(&[KeyCode::KeyR]),
            fire_mode: Binding::keys(&[KeyCode::KeyX]),
            lethal: Binding::keys(&[KeyCode::KeyG]),
            tactical: Binding::keys(&[KeyCode::KeyQ]),
            melee: Binding::keys(&[KeyCode::KeyV]),
            launcher: Binding::keys(&[KeyCode::Digit3]),
            primary_weapon: Binding::keys(&[KeyCode::Digit1]),
            secondary_weapon: Binding::keys(&[KeyCode::Digit2]),
            team_menu: Binding::keys(&[KeyCode::KeyT]),
            scoreboard: Binding::keys(&[KeyCode::Tab]),
            chat_all: Binding::keys(&[KeyCode::KeyY]),
            chat_team: Binding::keys(&[KeyCode::KeyU]),
            use_objective: Binding::keys(&[KeyCode::KeyF]),
        }
    }
}

/// An action the Settings > Controls list can rebind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Sprint,
    Jump,
    Crouch,
    Prone,
    Fire,
    Aim,
    Reload,
    FireMode,
    Lethal,
    Tactical,
    Melee,
    Launcher,
    PrimaryWeapon,
    SecondaryWeapon,
    UseObjective,
    Scoreboard,
    TeamMenu,
    ChatAll,
    ChatTeam,
}

/// What an action is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Mouse,
}

impl Action {
    pub const ALL: [Action; 23] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Sprint,
        Action::Jump,
        Action::Crouch,
        Action::Prone,
        Action::Fire,
        Action::Aim,
        Action::Reload,
        Action::FireMode,
        Action::Lethal,
        Action::Tactical,
        Action::Melee,
        Action::Launcher,
        Action::PrimaryWeapon,
        Action::SecondaryWeapon,
        Action::UseObjective,
        Action::Scoreboard,
        Action::TeamMenu,
        Action::ChatAll,
        Action::ChatTeam,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "Move forward",
            Action::Back => "Move back",
            Action::Left => "Move left",
            Action::Right => "Move right",
            Action::Sprint => "Sprint",
            Action::Jump => "Jump / stand",
            Action::Crouch => "Crouch",
            Action::Prone => "Prone",
            Action::Fire => "Fire",
            Action::Aim => "Aim (ADS)",
            Action::Reload => "Reload",
            Action::FireMode => "Fire mode",
            Action::Lethal => "Lethal",
            Action::Tactical => "Tactical",
            Action::Melee => "Knife",
            Action::Launcher => "Launcher",
            Action::PrimaryWeapon => "Primary",
            Action::SecondaryWeapon => "Secondary",
            Action::UseObjective => "Plant/defuse",
            Action::Scoreboard => "Scoreboard",
            Action::TeamMenu => "Team menu",
            Action::ChatAll => "Chat: all",
            Action::ChatTeam => "Chat: team",
        }
    }

    pub fn device(self) -> Device {
        match self {
            Action::Fire | Action::Aim => Device::Mouse,
            _ => Device::Keyboard,
        }
    }
}

impl Bindings {
    fn keys_mut(&mut self, action: Action) -> Option<&mut Binding> {
        Some(match action {
            Action::Forward => &mut self.forward,
            Action::Back => &mut self.back,
            Action::Left => &mut self.left,
            Action::Right => &mut self.right,
            Action::Sprint => &mut self.sprint,
            Action::Jump => &mut self.jump,
            Action::Crouch => &mut self.crouch,
            Action::Prone => &mut self.prone,
            Action::Reload => &mut self.reload,
            Action::FireMode => &mut self.fire_mode,
            Action::Lethal => &mut self.lethal,
            Action::Tactical => &mut self.tactical,
            Action::Melee => &mut self.melee,
            Action::Launcher => &mut self.launcher,
            Action::PrimaryWeapon => &mut self.primary_weapon,
            Action::SecondaryWeapon => &mut self.secondary_weapon,
            Action::UseObjective => &mut self.use_objective,
            Action::Scoreboard => &mut self.scoreboard,
            Action::TeamMenu => &mut self.team_menu,
            Action::ChatAll => &mut self.chat_all,
            Action::ChatTeam => &mut self.chat_team,
            Action::Fire | Action::Aim => return None,
        })
    }

    /// Every keyboard binding, for removing a key from all but one.
    fn all_keys_mut(&mut self) -> [&mut Binding; 22] {
        [
            &mut self.forward,
            &mut self.back,
            &mut self.left,
            &mut self.right,
            &mut self.sprint,
            &mut self.jump,
            &mut self.crouch,
            &mut self.prone,
            &mut self.pause,
            &mut self.reload,
            &mut self.fire_mode,
            &mut self.lethal,
            &mut self.tactical,
            &mut self.melee,
            &mut self.launcher,
            &mut self.primary_weapon,
            &mut self.secondary_weapon,
            &mut self.team_menu,
            &mut self.scoreboard,
            &mut self.chat_all,
            &mut self.chat_team,
            &mut self.use_objective,
        ]
    }

    /// Settings saved before the knife existed have the launcher on V and no knife entry, so
    /// the knife's default V lands on the launcher's key: the launcher moves to its new default
    /// (3, when nothing else has it; else it is left unbound). Run once on load.
    pub fn migrate(&mut self) {
        let (knife, launcher) = (KeyCode::KeyV, KeyCode::Digit3);
        if self.melee.0 != [knife] || !self.launcher.0.contains(&knife) {
            return;
        }
        self.launcher.0.retain(|key| *key != knife);
        let taken = self.all_keys_mut().iter().any(|b| b.0.contains(&launcher));
        if self.launcher.0.is_empty() && !taken {
            self.launcher = Binding(vec![launcher]);
        }
    }

    /// Binds the keyboard action to `key` alone. Any other action that had the key loses it
    /// (and is left unbound if it was its only one).
    pub fn rebind_key(&mut self, action: Action, key: KeyCode) {
        if self.keys_mut(action).is_none() {
            return;
        }
        for other in self.all_keys_mut() {
            other.0.retain(|held| *held != key);
        }
        if let Some(slot) = self.keys_mut(action) {
            *slot = Binding(vec![key]);
        }
    }

    /// Binds the mouse action to `button`; the other mouse action loses it if it had it.
    pub fn rebind_mouse(&mut self, action: Action, button: MouseButton) {
        let (slot, other) = match action {
            Action::Fire => (&mut self.fire, &mut self.aim),
            Action::Aim => (&mut self.aim, &mut self.fire),
            _ => return,
        };
        slot.0 = Some(button);
        if other.0 == Some(button) {
            other.0 = None;
        }
    }

    /// What the action is bound to, for the Controls list.
    pub fn describe(&self, action: Action) -> String {
        match action {
            Action::Fire => describe_mouse(self.fire),
            Action::Aim => describe_mouse(self.aim),
            _ => {
                let binding = match action {
                    Action::Forward => &self.forward,
                    Action::Back => &self.back,
                    Action::Left => &self.left,
                    Action::Right => &self.right,
                    Action::Sprint => &self.sprint,
                    Action::Jump => &self.jump,
                    Action::Crouch => &self.crouch,
                    Action::Prone => &self.prone,
                    Action::Reload => &self.reload,
                    Action::FireMode => &self.fire_mode,
                    Action::Lethal => &self.lethal,
                    Action::Tactical => &self.tactical,
                    Action::Melee => &self.melee,
                    Action::Launcher => &self.launcher,
                    Action::PrimaryWeapon => &self.primary_weapon,
                    Action::SecondaryWeapon => &self.secondary_weapon,
                    Action::UseObjective => &self.use_objective,
                    Action::Scoreboard => &self.scoreboard,
                    Action::TeamMenu => &self.team_menu,
                    Action::ChatAll => &self.chat_all,
                    Action::ChatTeam => &self.chat_team,
                    Action::Fire | Action::Aim => unreachable!("handled above"),
                };
                if binding.0.is_empty() {
                    "Unbound".to_owned()
                } else {
                    binding
                        .0
                        .iter()
                        .map(|key| key_name(*key))
                        .collect::<Vec<_>>()
                        .join(" / ")
                }
            }
        }
    }
}

fn describe_mouse(binding: MouseBinding) -> String {
    match binding.0 {
        None => "Unbound".to_owned(),
        Some(MouseButton::Left) => "Mouse left".to_owned(),
        Some(MouseButton::Right) => "Mouse right".to_owned(),
        Some(MouseButton::Middle) => "Mouse middle".to_owned(),
        Some(MouseButton::Back) => "Mouse back".to_owned(),
        Some(MouseButton::Forward) => "Mouse forward".to_owned(),
        Some(MouseButton::Other(n)) => format!("Mouse {n}"),
    }
}

/// A key's name for the Controls list: "W", "1", "Shift Left", "Space".
pub fn key_name(key: KeyCode) -> String {
    let debug = format!("{key:?}");
    let name = ["Key", "Digit"]
        .iter()
        .find_map(|prefix| debug.strip_prefix(prefix))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&debug);
    // "ShiftLeft" -> "Shift Left": a space before each capital after the first letter.
    let name = name.replace("Control", "Ctrl");
    let mut spaced = String::with_capacity(name.len() + 2);
    for (at, c) in name.chars().enumerate() {
        if at > 0 && c.is_uppercase() {
            spaced.push(' ');
        }
        spaced.push(c);
    }
    spaced
}

/// The Controls list's rebind in progress: the action waiting for a key press. While it is
/// set, menu Esc handling stands down (Esc cancels the rebind instead).
#[derive(Resource, Default, Debug)]
pub struct Rebinding {
    action: Option<Action>,
    stage: RebindStage,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum RebindStage {
    /// Just started: the click or key that started it must not count as the new binding.
    #[default]
    Fresh,
    Listening,
    /// Answered this frame; clears at the end of it, so the key that answered (Esc included)
    /// isn't also read by menu handling that runs after the answer.
    Answered,
}

impl Rebinding {
    pub fn active(&self) -> bool {
        self.action.is_some()
    }

    /// The action being rebound, whether or not it is listening yet.
    pub fn action(&self) -> Option<Action> {
        self.action
    }

    pub fn start(&mut self, action: Action) {
        self.action = Some(action);
        self.stage = RebindStage::Fresh;
    }

    /// The action to bind a key press to now.
    pub fn listening(&self) -> Option<Action> {
        self.action.filter(|_| self.stage == RebindStage::Listening)
    }

    /// The press came; `listening` is `None` for the rest of the frame.
    pub fn answer(&mut self) {
        self.stage = RebindStage::Answered;
    }

    /// End of a frame: a fresh rebind starts listening, an answered one ends.
    pub fn advance(&mut self) {
        match self.stage {
            RebindStage::Fresh => self.stage = RebindStage::Listening,
            RebindStage::Answered => *self = Rebinding::default(),
            RebindStage::Listening => {}
        }
    }

    pub fn cancel(&mut self) {
        *self = Rebinding::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_taken_by_another_action_is_unbound_from_it() {
        let mut bindings = Bindings::default();
        bindings.rebind_key(Action::Reload, KeyCode::KeyW);
        assert_eq!(bindings.reload, Binding(vec![KeyCode::KeyW]));
        assert_eq!(bindings.forward, Binding(vec![]));
        assert_eq!(bindings.describe(Action::Forward), "Unbound");
    }

    #[test]
    fn a_shared_key_is_only_taken_from_the_actions_that_hold_it() {
        let mut bindings = Bindings::default();
        // Prone is Ctrl or Z: taking Z leaves it Ctrl.
        bindings.rebind_key(Action::Crouch, KeyCode::KeyZ);
        assert_eq!(bindings.crouch, Binding(vec![KeyCode::KeyZ]));
        assert_eq!(bindings.prone, Binding(vec![KeyCode::ControlLeft]));
        assert_eq!(bindings.forward, Binding(vec![KeyCode::KeyW]));
    }

    #[test]
    fn rebinding_replaces_the_actions_old_keys() {
        let mut bindings = Bindings::default();
        bindings.rebind_key(Action::Prone, KeyCode::KeyX);
        assert_eq!(bindings.prone, Binding(vec![KeyCode::KeyX]));
        // Rebinding to the key it already has changes nothing else.
        let before = bindings.clone();
        bindings.rebind_key(Action::Prone, KeyCode::KeyX);
        assert_eq!(bindings, before);
    }

    #[test]
    fn a_mouse_button_taken_by_the_other_action_is_unbound_from_it() {
        let mut bindings = Bindings::default();
        bindings.rebind_mouse(Action::Aim, MouseButton::Left);
        assert_eq!(bindings.aim, MouseBinding(Some(MouseButton::Left)));
        assert_eq!(bindings.fire, MouseBinding(None));
        // Moving to a free button leaves the other action alone.
        bindings.rebind_mouse(Action::Fire, MouseButton::Back);
        assert_eq!(bindings.fire, MouseBinding(Some(MouseButton::Back)));
        assert_eq!(bindings.aim, MouseBinding(Some(MouseButton::Left)));
    }

    #[test]
    fn keyboard_and_mouse_bindings_do_not_clash() {
        let mut bindings = Bindings::default();
        bindings.rebind_mouse(Action::Fire, MouseButton::Middle);
        bindings.rebind_key(Action::Reload, KeyCode::KeyE);
        assert_eq!(bindings.aim, MouseBinding(Some(MouseButton::Right)));
        assert_eq!(bindings.grab_cursor, MouseButton::Left);
    }

    #[test]
    fn a_rebind_answered_this_frame_still_counts_as_active_until_the_frame_ends() {
        let mut rebinding = Rebinding::default();
        rebinding.start(Action::Jump);
        assert_eq!(rebinding.listening(), None, "the click that started it");
        rebinding.advance();
        assert_eq!(rebinding.listening(), Some(Action::Jump));
        rebinding.answer();
        assert!(rebinding.active());
        assert_eq!(rebinding.listening(), None);
        rebinding.advance();
        assert!(!rebinding.active());
    }

    #[test]
    fn key_names_read_like_a_keyboard() {
        assert_eq!(key_name(KeyCode::KeyW), "W");
        assert_eq!(key_name(KeyCode::Digit1), "1");
        assert_eq!(key_name(KeyCode::ShiftLeft), "Shift Left");
        assert_eq!(key_name(KeyCode::Space), "Space");
        assert_eq!(key_name(KeyCode::ControlLeft), "Ctrl Left");
    }

    #[test]
    fn a_file_with_some_bindings_loads_with_the_defaults_for_the_rest() {
        let reload = ron::to_string(&Binding(vec![KeyCode::KeyE])).unwrap();
        let partial: Bindings = ron::from_str(&format!("(reload: {reload})")).unwrap();
        assert_eq!(partial.reload, Binding(vec![KeyCode::KeyE]));
        assert_eq!(partial.forward, Binding(vec![KeyCode::KeyW]));
        assert_eq!(partial.fire, MouseBinding(Some(MouseButton::Left)));
    }

    #[test]
    fn a_key_name_this_version_does_not_know_is_dropped_on_load() {
        let binding: Binding = ron::from_str(r#"["KeyQ", "Teleport"]"#).unwrap();
        assert_eq!(binding, Binding(vec![KeyCode::KeyQ]));
        assert!(!is_bindable(KeyCode::Escape));
        assert!(is_bindable(KeyCode::ShiftLeft));
    }

    #[test]
    fn the_knife_is_on_v_and_the_launcher_on_3_by_default() {
        let bindings = Bindings::default();
        assert_eq!(bindings.describe(Action::Melee), "V");
        assert_eq!(bindings.describe(Action::Launcher), "3");
    }

    #[test]
    fn old_settings_with_the_launcher_on_v_hand_v_to_the_knife() {
        // Saved before the knife: the launcher on V, no `melee` entry.
        let mut old: Bindings = ron::from_str(r#"(launcher: ["KeyV"])"#).unwrap();
        old.migrate();
        assert_eq!(old.melee, Binding(vec![KeyCode::KeyV]));
        assert_eq!(old.launcher, Binding(vec![KeyCode::Digit3]));

        // With 3 taken by something else the launcher is left unbound, not doubled up.
        let mut taken: Bindings =
            ron::from_str(r#"(launcher: ["KeyV"], reload: ["Digit3"])"#).unwrap();
        taken.migrate();
        assert_eq!(taken.launcher, Binding(vec![]));
        assert_eq!(taken.reload, Binding(vec![KeyCode::Digit3]));

        // A knife the player moved elsewhere leaves their launcher alone.
        let mut moved: Bindings =
            ron::from_str(r#"(launcher: ["KeyV"], melee: ["KeyE"])"#).unwrap();
        moved.migrate();
        assert_eq!(moved.launcher, Binding(vec![KeyCode::KeyV]));
    }

    #[test]
    fn bindings_round_trip_through_ron() {
        let mut bindings = Bindings::default();
        bindings.rebind_key(Action::Jump, KeyCode::KeyV);
        bindings.rebind_mouse(Action::Aim, MouseButton::Other(7));
        let text = ron::to_string(&bindings).unwrap();
        assert_eq!(ron::from_str::<Bindings>(&text).unwrap(), bindings);
    }
}
