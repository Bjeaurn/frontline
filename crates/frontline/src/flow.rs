//! Screen flow of a windowed game (Client/Host): main menu -> in game, with in-game overlays.
//! A Dedicated Server has no screens and never uses these states.
//!
//! [`FlowPlugin`] owns the states, the `Settings` file and the in-game overlay rules; netcode
//! carries out `FlowCommand`s (`net::session`) and `menus` draws the screens.

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::PathBuf;

use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;
use lightyear::prelude::{Client, Connected};
use serde::{Deserialize, Serialize};

use crate::cli::Mode;
use crate::data::GameModeId;
use crate::match_hud::PendingTeam;
use crate::player::input::Bindings;
use crate::rotation::HostSetup;
use crate::player::{ControllerSystems, MenuOpen};
use crate::shared::{LocalPlayerReplica, PlayerTeam};

/// Top-level screen.
#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    /// Title screen and its sub-screens (`MenuScreen`). No server or connection is running.
    #[default]
    MainMenu,
    /// Hosting or connected (or connecting) to a server.
    InGame,
}

/// Which page of the main menu is showing.
#[derive(SubStates, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[source(AppState = AppState::MainMenu)]
pub enum MenuScreen {
    #[default]
    Title,
    /// HOST GAME: pick the Rotation and server settings before starting.
    HostSetup,
    ServerBrowser,
    Settings,
}

/// Full-screen panel shown over the game. While it isn't `None`, the cursor is free and
/// gameplay input (look, move, fire) is ignored.
#[derive(SubStates, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[source(AppState = AppState::InGame)]
pub enum Overlay {
    #[default]
    None,
    TeamSelect,
    Loadout,
    Pause,
    Settings,
}

/// Requests that change what this process is doing; the flow owner reacts to them.
#[derive(Message, Clone, Copy, Debug)]
pub enum FlowCommand {
    /// Start a Host (server + local player) on the configured port.
    Host,
    /// Connect to a server.
    Join(SocketAddr),
    /// Disconnect (and stop hosting), back to the main menu.
    Leave,
    /// Close the game.
    Quit,
}

/// What a server announces to LAN discovery queries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    /// UDP port of the game server (the address comes from the reply's source).
    pub port: u16,
    pub mode: GameModeId,
    pub players: u32,
    pub max_players: u32,
}

/// A server found on the LAN, as shown in the Server Browser.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredServer {
    pub addr: SocketAddr,
    pub info: ServerInfo,
    /// Seconds since this server last answered; stale entries are dropped.
    pub age_secs: f32,
}

/// Servers currently visible on the LAN.
#[derive(Resource, Default, Debug)]
pub struct DiscoveredServers(pub Vec<DiscoveredServer>);

/// Player preferences, saved to the user's config directory and loaded at startup.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub name: String,
    /// Radians of view rotation per unit of mouse motion.
    pub mouse_sensitivity: f32,
    /// Horizontal field of view in degrees.
    pub fov_deg: f32,
    /// 0..1.
    pub master_volume: f32,
    /// Chosen Default Loadout, an index into `GameData::default_loadouts`.
    #[serde(default)]
    pub loadout: usize,
    /// The Host setup screen's choices.
    #[serde(default)]
    pub host: HostSetup,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: "Player".into(),
            mouse_sensitivity: 0.002,
            fov_deg: 80.0,
            master_volume: 0.8,
            loadout: 0,
            host: HostSetup::default(),
        }
    }
}

/// Why the last Join failed or the connection dropped; shown on the Server Browser.
#[derive(Resource, Default, Debug)]
pub struct ConnectionError(pub Option<String>);

/// States, `FlowCommand`, `Settings` (loaded now, saved on change) and in-game overlay rules.
/// Windowed builds only; add after `DefaultPlugins` and after inserting `Mode`.
pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        let Some(Mode::Windowed { start, name, .. }) = app.world().get_resource::<Mode>().cloned()
        else {
            panic!("FlowPlugin needs Mode::Windowed inserted first");
        };
        let mut settings = load_settings();
        if let Some(name) = name {
            settings.name = name;
        }
        match start {
            Some(_) => app.insert_state(AppState::InGame),
            None => app.init_state::<AppState>(),
        };
        if let Some(command) = start {
            app.add_systems(Startup, move |mut flow: MessageWriter<FlowCommand>| {
                flow.write(command);
            });
        }
        app.add_sub_state::<MenuScreen>()
            .add_sub_state::<Overlay>()
            .add_message::<FlowCommand>()
            .insert_resource(GlobalVolume::from(Volume::Linear(settings.master_volume)))
            .insert_resource(settings)
            .init_resource::<DiscoveredServers>()
            .init_resource::<ConnectionError>()
            .add_systems(
                Update,
                (
                    quit,
                    (save_settings, apply_volume)
                        .run_if(resource_changed::<Settings>)
                        .run_if(not(resource_added::<Settings>)),
                    (switch_overlays, open_team_select)
                        .chain()
                        .run_if(in_state(AppState::InGame)),
                    sync_menu_open.before(ControllerSystems::Look),
                ),
            );
    }
}

/// `<config dir>/frontline/settings.ron`.
fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("frontline").join("settings.ron"))
}

fn load_settings() -> Settings {
    let Some(path) = settings_path() else {
        warn!("No user config directory; using default settings");
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match ron::from_str(&text) {
            Ok(settings) => {
                info!("Loaded settings from {}", path.display());
                settings
            }
            Err(err) => {
                warn!("Ignoring {}: {err}", path.display());
                Settings::default()
            }
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Settings::default(),
        Err(err) => {
            warn!("Can't read {}: {err}", path.display());
            Settings::default()
        }
    }
}

fn save_settings(settings: Res<Settings>) {
    let Some(path) = settings_path() else {
        return;
    };
    let saved = ron::ser::to_string_pretty(&*settings, ron::ser::PrettyConfig::default())
        .map_err(|err| err.to_string())
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
            }
            std::fs::write(&path, text).map_err(|err| err.to_string())
        });
    match saved {
        Ok(()) => info!("Saved settings to {}", path.display()),
        Err(err) => warn!("Can't save settings to {}: {err}", path.display()),
    }
}

fn apply_volume(settings: Res<Settings>, mut volume: ResMut<GlobalVolume>) {
    volume.volume = Volume::Linear(settings.master_volume);
}

fn quit(mut flow: MessageReader<FlowCommand>, mut exit: MessageWriter<AppExit>) {
    if flow
        .read()
        .any(|command| matches!(command, FlowCommand::Quit))
    {
        exit.write(AppExit::Success);
    }
}

/// The cursor is free and gameplay input ignored outside the game, while connecting, and
/// while any overlay is open.
fn sync_menu_open(
    app_state: Res<State<AppState>>,
    overlay: Option<Res<State<Overlay>>>,
    connected: Query<(), (With<Client>, With<Connected>)>,
    mut menu: ResMut<MenuOpen>,
) {
    let playing = *app_state.get() == AppState::InGame
        && overlay.is_some_and(|overlay| *overlay.get() == Overlay::None)
        && !connected.is_empty();
    if menu.0 == playing {
        menu.0 = !playing;
    }
}

/// Esc opens Pause during play and steps back out of Pause and Settings; TeamSelect and
/// Loadout handle Esc themselves. The team key opens TeamSelect during play.
fn switch_overlays(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    overlay: Res<State<Overlay>>,
    connected: Query<(), (With<Client>, With<Connected>)>,
    mut next: ResMut<NextState<Overlay>>,
) {
    if connected.is_empty() {
        return;
    }
    let target = match overlay.get() {
        Overlay::None if bindings.pause.just_pressed(&keys) => Overlay::Pause,
        Overlay::None if bindings.team_menu.just_pressed(&keys) => Overlay::TeamSelect,
        Overlay::Pause if bindings.pause.just_pressed(&keys) => Overlay::None,
        Overlay::Settings if bindings.pause.just_pressed(&keys) => Overlay::Pause,
        _ => return,
    };
    next.set(target);
}

/// Puts TeamSelect up during play while the local player has no Team and hasn't asked for one.
fn open_team_select(
    overlay: Res<State<Overlay>>,
    pending: Res<PendingTeam>,
    replica: Query<&PlayerTeam, With<LocalPlayerReplica>>,
    mut next: ResMut<NextState<Overlay>>,
) {
    if *overlay.get() == Overlay::None
        && pending.0.is_none()
        && replica.single().is_ok_and(|team| team.0.is_none())
    {
        next.set(Overlay::TeamSelect);
    }
}
