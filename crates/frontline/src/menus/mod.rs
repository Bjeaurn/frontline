//! The game's own screens, built from `ui`: title, Server Browser and Settings in the main
//! menu; the 'Connecting' panel, Pause and Settings over the game. TeamSelect and Loadout live
//! with their features (`match_hud`, `loadout`). Client/Host only.

mod browser;
mod connecting;
mod pause;
mod settings;
mod title;

use bevy::prelude::*;

use crate::flow::{AppState, MenuScreen, Overlay};
use crate::player::input::Bindings;

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), title::spawn_menu_camera)
            .add_systems(OnEnter(MenuScreen::Title), title::spawn_title)
            .add_systems(OnEnter(MenuScreen::ServerBrowser), browser::spawn_browser)
            .add_systems(OnEnter(MenuScreen::Settings), settings::spawn_menu_settings)
            .add_systems(OnEnter(Overlay::Pause), pause::spawn_pause)
            .add_systems(OnEnter(Overlay::Settings), settings::spawn_overlay_settings)
            .add_systems(
                Update,
                (
                    (title::orbit_menu_camera, back_to_title).run_if(in_state(AppState::MainMenu)),
                    (browser::refresh_server_list, browser::show_connection_error)
                        .run_if(in_state(MenuScreen::ServerBrowser)),
                    settings::show_readouts,
                    connecting::show_connecting.run_if(in_state(AppState::InGame)),
                ),
            );
    }
}

/// Esc on a main-menu page goes back to the title.
fn back_to_title(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    screen: Res<State<MenuScreen>>,
    mut next: ResMut<NextState<MenuScreen>>,
) {
    if *screen.get() != MenuScreen::Title && bindings.pause.just_pressed(&keys) {
        next.set(MenuScreen::Title);
    }
}
