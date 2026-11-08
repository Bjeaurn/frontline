//! The viewing session around the Map: which Map and look are shown, the reload of the view (the
//! editor's `edit` module decides when: after an edit, and when the file changes on disk), the
//! HUD and screenshots (F12, or `--shot` for a scripted one).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use frontline::atmosphere::{LookOverride, Looks};
use frontline::maps::{
    AmmoCrate, BombSite, DomFlag, HqSite, IdleMap, MapId, MapReady, MapRegistry, MapRoot,
    SpawnPoint, SpawnTeam,
};

use crate::camera::FlyCamera;
use crate::edit::{Command, Editor, Interaction};
use crate::menu::{BlockPicking, InputCapture};
use crate::overlay::Overlay;

/// Frames a scripted screenshot waits after the Map is ready (textures and sky settle), and
/// frames the process lingers after taking it (the PNG is written on a worker thread).
const SHOT_DELAY_FRAMES: u32 = 150;
const SHOT_LINGER_FRAMES: u32 = 60;

/// Reload the shown Map from disk.
#[derive(Message, Clone, Copy, Debug)]
pub struct Reload;

#[derive(Resource)]
struct Session {
    ready_frames: u32,
    shot: Option<PathBuf>,
    shot_taken: Option<u32>,
    help: bool,
}

pub struct SessionPlugin {
    /// `--shot`: take this screenshot once the Map is ready, then exit.
    pub shot: Option<PathBuf>,
}

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Reload>()
            .insert_resource(Session {
                ready_frames: 0,
                shot: self.shot.clone(),
                shot_taken: None,
                help: true,
            })
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (keys, reload, scripted_shot, update_hud).chain());
    }
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    registry: Res<MapRegistry>,
    looks: Res<Looks>,
    idle: Res<IdleMap>,
    mut look: ResMut<LookOverride>,
    mut session: ResMut<Session>,
    mut reload: MessageWriter<Reload>,
    mut open: MessageWriter<Command>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::F12) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path = PathBuf::from(format!("screenshots/mapedit-{secs}.png"));
        shoot(&mut commands, path);
    }
    if capture.blocked() {
        return;
    }
    let step = |forward: bool, backward: bool| i32::from(forward) - i32::from(backward);
    let maps_step = step(
        keys.just_pressed(KeyCode::BracketRight),
        keys.just_pressed(KeyCode::BracketLeft),
    );
    if maps_step != 0 {
        let ids: Vec<&MapId> = registry.iter().map(|(id, _)| id).collect();
        if let Some(at) = ids.iter().position(|id| **id == idle.0) {
            let next = (at as i32 + maps_step).rem_euclid(ids.len() as i32) as usize;
            open.write(Command::Open {
                id: ids[next].clone(),
                discard: false,
            });
        }
    }
    let looks_step = step(
        keys.just_pressed(KeyCode::Period),
        keys.just_pressed(KeyCode::Comma),
    );
    if looks_step != 0 {
        // The Map's own look, then every preset by name.
        let names = looks.names();
        let at = look
            .0
            .as_deref()
            .and_then(|name| names.iter().position(|n| *n == name))
            .map_or(0, |i| i as i32 + 1);
        let next = (at + looks_step).rem_euclid(names.len() as i32 + 1);
        look.0 = (next > 0).then(|| names[next as usize - 1].to_owned());
        reload.write(Reload);
    }
    // Reads the Map's file again (asks first when there are unsaved edits).
    if keys.just_pressed(KeyCode::KeyR) {
        open.write(Command::Open {
            id: idle.0.clone(),
            discard: false,
        });
    }
    if keys.just_pressed(KeyCode::KeyH) {
        session.help = !session.help;
    }
}

fn shoot(commands: &mut Commands, path: PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    info!("Screenshot {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Unloads the Map and has the asset server read its file again; `maps::sync_map` then loads it.
fn reload(
    mut requests: MessageReader<Reload>,
    registry: Res<MapRegistry>,
    idle: Res<IdleMap>,
    roots: Query<Entity, With<MapRoot>>,
    asset_server: Res<AssetServer>,
    mut session: ResMut<Session>,
    mut commands: Commands,
) {
    if requests.read().count() == 0 {
        return;
    }
    session.ready_frames = 0;
    for root in &roots {
        commands.entity(root).despawn();
    }
    if let Some(info) = registry.get(&idle.0) {
        asset_server.reload(info.file.clone());
    }
}

fn scripted_shot(
    ready: Query<(), With<MapReady>>,
    mut session: ResMut<Session>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    if ready.is_empty() {
        session.ready_frames = 0;
        return;
    }
    session.ready_frames += 1;
    let Some(path) = session.shot.clone() else {
        return;
    };
    match session.shot_taken {
        None if session.ready_frames >= SHOT_DELAY_FRAMES => {
            shoot(&mut commands, path);
            session.shot_taken = Some(0);
        }
        Some(frames) => {
            session.shot_taken = Some(frames + 1);
            if frames >= SHOT_LINGER_FRAMES {
                exit.write(AppExit::Success);
            }
        }
        None => {}
    }
}

#[derive(Component)]
struct Hud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        BlockPicking,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));
}

fn update_hud(
    session: Res<Session>,
    editor: Res<Editor>,
    interaction: Res<Interaction>,
    overlay: Res<Overlay>,
    idle: Res<IdleMap>,
    look: Res<LookOverride>,
    ready: Query<&MapReady>,
    camera: Query<(&FlyCamera, &Transform)>,
    spawns: Query<&SpawnPoint>,
    flags: Query<(), With<DomFlag>>,
    hqs: Query<(), With<HqSite>>,
    bombs: Query<(), With<BombSite>>,
    crates: Query<(), With<AmmoCrate>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let mut lines = Vec::new();
    match ready.iter().next() {
        Some(map) => {
            let shown = look
                .0
                .as_deref()
                .or(map.look.as_deref())
                .unwrap_or("(default)");
            let forced = if look.0.is_some() { " (forced)" } else { "" };
            let dirty = if editor.dirty() { "  * UNSAVED" } else { "" };
            lines.push(format!(
                "MapEdit  {}  \"{}\"  look {shown}{forced}{dirty}",
                idle.0.0, map.name
            ));
        }
        None => lines.push(format!("MapEdit  {}  loading...", idle.0.0)),
    }
    let count = |team: SpawnTeam| spawns.iter().filter(|s| s.team == team).count();
    lines.push(format!(
        "spawns {} (alpha {}, bravo {}, any {})  flags {}  hq {}  bombs {}  crates {}",
        spawns.iter().count(),
        count(SpawnTeam::Alpha),
        count(SpawnTeam::Bravo),
        count(SpawnTeam::Any),
        flags.iter().count(),
        hqs.iter().count(),
        bombs.iter().count(),
        crates.iter().count(),
    ));
    if let Ok((fly, transform)) = camera.single() {
        let p = transform.translation;
        lines.push(format!(
            "camera x {:.1} y {:.1} z {:.1}  yaw {:.0} pitch {:.0}  speed {:.0} m/s   mode filter: {}",
            p.x,
            p.y,
            p.z,
            fly.yaw.to_degrees(),
            fly.pitch.to_degrees(),
            fly.speed,
            overlay.mode.unwrap_or("all"),
        ));
    }
    if !editor.selection.is_empty() {
        lines.push(format!(
            "selected {}",
            editor.describe_many(editor.selection.items())
        ));
        if let Some((min, max)) = editor.selection_bounds() {
            // The size in metres and in cells of the grid, and where it stands.
            let (size, g) = (max - min, editor.grid);
            lines.push(format!(
                "size {:.2} x {:.2} x {:.2} m = {:.2} x {:.2} x {:.2} grid cells of {g} m   x {:.2}..{:.2}  y {:.2}..{:.2}  z {:.2}..{:.2}",
                size.x,
                size.y,
                size.z,
                size.x / g,
                size.y / g,
                size.z / g,
                min.x,
                max.x,
                min.y,
                max.y,
                min.z,
                max.z,
            ));
        }
    }
    if let Some(readout) = &interaction.readout {
        lines.push(format!("dragging {readout}"));
    }
    lines.push(format!(
        "grid {} m  new brushes {}  undo {}  redo {}{}",
        editor.grid,
        editor.surface.name,
        if editor.can_undo() { "yes" } else { "-" },
        if editor.can_redo() { "yes" } else { "-" },
        if editor.synced() {
            ""
        } else {
            "  (updating view)"
        },
    ));
    if let Some(problem) = &editor.problem {
        lines.push(format!("VIEW ONLY, cannot edit: {problem}"));
    }
    if editor.dirty()
        && let Some(script) = editor.generator()
    {
        lines.push(format!(
            "WARNING {} generates this Map: running it overwrites saved edits",
            script.display()
        ));
    }
    if let Some(notice) = &editor.notice {
        lines.push(format!(
            "{}{}",
            if notice.error { "! " } else { "> " },
            notice.text
        ));
    }
    if session.help {
        lines.push(
            "RMB look  WASD QE move  Shift fast  Ctrl slow  wheel speed  T top view  Home reset"
                .into(),
        );
        lines.push("[ ] map  , . look  R reload from disk  1-5 mode filter  C colliders  X through walls  L labels  F12 shot  H help".into());
        lines.push("Esc menu  P palette  click select (Shift/Ctrl+click adds)  drag a box (top view, or Alt+drag)  Ctrl+A all (+Shift: same kind)  Alt+drag a face of the selected brush".into());
        lines.push("arrows PgUp/PgDn move (Shift 4x)  Alt+arrows move a face (+Shift in)  J/K turn 15 (Shift 90)  M / Shift+M mirror  Tab texture  Del delete  B box  Shift+B ramp".into());
        lines.push("Ctrl+D copy  Ctrl+Shift+D array of 3  Ctrl+C / Ctrl+V copy, paste at cursor  Ctrl+Z/Y undo/redo  Ctrl+S save  F5 playtest  G grid".into());
    }
    **text = lines.join("\n");
}
