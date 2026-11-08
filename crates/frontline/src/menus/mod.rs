//! The game's own screens, built from `ui`: the first-boot name prompt, title, Host setup, Server
//! Browser, Settings and the Match Rules editor in the main menu (Create a Class lives with
//! `loadout`); the 'Connecting' panel, Pause, Settings and the Match Rules editor over the game.
//! TeamSelect and Loadout live with their features (`match_hud`, `loadout`). Client/Host only.

mod browser;
mod connecting;
mod first_boot;
mod host_setup;
mod pause;
mod rules_editor;
mod settings;
mod title;

use bevy::prelude::*;

use crate::flow::{AppState, MenuScreen, Overlay};
use crate::player::input::{Bindings, Rebinding};
use crate::profile::has_active_profile;

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((settings::SettingsPlugin, first_boot::FirstBootPlugin))
            .add_systems(OnEnter(AppState::MainMenu), title::spawn_menu_camera)
            .add_systems(
                OnEnter(MenuScreen::Title),
                title::spawn_title.run_if(has_active_profile),
            )
            .add_systems(OnEnter(MenuScreen::HostSetup), host_setup::spawn_host_setup)
            .add_systems(OnEnter(MenuScreen::ServerBrowser), browser::spawn_browser)
            .add_systems(OnEnter(MenuScreen::Rules), rules_editor::spawn_menu_rules)
            .add_systems(OnEnter(Overlay::Pause), pause::spawn_pause)
            .add_systems(OnEnter(Overlay::Rules), rules_editor::spawn_overlay_rules)
            .add_systems(
                Update,
                (
                    (title::orbit_menu_camera, back_to_title).run_if(in_state(AppState::MainMenu)),
                    (browser::refresh_server_list, browser::show_connection_error)
                        .run_if(in_state(MenuScreen::ServerBrowser)),
                    (
                        host_setup::refresh,
                        host_setup::read_sliders,
                        host_setup::show_readouts,
                    )
                        .chain()
                        .run_if(in_state(MenuScreen::HostSetup))
                        .run_if(resource_exists::<host_setup::Draft>),
                    (
                        rules_editor::refresh,
                        rules_editor::read_sliders,
                        rules_editor::esc_back,
                    )
                        .chain()
                        .run_if(rules_editor_open)
                        .run_if(resource_exists::<rules_editor::Editor>),
                    connecting::show_connecting.run_if(in_state(AppState::InGame)),
                ),
            );
    }
}

/// Esc on a main-menu page goes back to the title (the rules editor steps back to where it
/// was opened from instead, Create a Class steps up its own screens, and the name prompt has
/// nowhere to go back to).
fn back_to_title(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    rebinding: Res<Rebinding>,
    screen: Res<State<MenuScreen>>,
    mut next: ResMut<NextState<MenuScreen>>,
) {
    if rebinding.active() {
        return;
    }
    if !matches!(
        *screen.get(),
        MenuScreen::Title | MenuScreen::FirstBoot | MenuScreen::Rules | MenuScreen::CreateAClass
    ) && bindings.pause.just_pressed(&keys)
    {
        next.set(MenuScreen::Title);
    }
}

/// The Match Rules editor is showing, over the main menu or over the game.
fn rules_editor_open(
    menu: Option<Res<State<MenuScreen>>>,
    overlay: Option<Res<State<Overlay>>>,
) -> bool {
    menu.is_some_and(|screen| *screen.get() == MenuScreen::Rules)
        || overlay.is_some_and(|screen| *screen.get() == Overlay::Rules)
}
