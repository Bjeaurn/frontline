//! Rebindable controls for the local player.

use bevy::prelude::*;

/// Keys that trigger one action; any of them counts.
#[derive(Clone, Debug)]
pub struct Binding(pub Vec<KeyCode>);

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

/// All local-player bindings, in one place so a settings menu can rebind them.
#[derive(Resource, Clone, Debug)]
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
    /// Grab the cursor for mouse look (when it got freed, e.g. by losing window focus).
    pub grab_cursor: MouseButton,
    /// Open the pause menu; steps back out of menus.
    pub pause: Binding,
    /// Hold to fire.
    pub fire: MouseButton,
    /// Hold to aim down sights.
    pub aim: MouseButton,
    pub reload: Binding,
    /// Open the team select screen.
    pub team_menu: Binding,
    /// Hold to show the Scoreboard.
    pub scoreboard: Binding,
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
            fire: MouseButton::Left,
            aim: MouseButton::Right,
            reload: Binding::keys(&[KeyCode::KeyR]),
            team_menu: Binding::keys(&[KeyCode::KeyT]),
            scoreboard: Binding::keys(&[KeyCode::Tab]),
        }
    }
}
