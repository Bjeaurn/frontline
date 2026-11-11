//! The model viewer: one glTF model, or a soldier as remote players are drawn, on a plain ground
//! with the game's lighting and look, instead of a Map.
//!
//!     cargo run -p mapedit -- --model models/weapons/ak47.glb
//!     cargo run -p mapedit -- --model /tmp/barrel.glb --shot /tmp/barrel.png --cam 1.5 1.0 2.0 150 -15
//!     cargo run -p mapedit -- --soldier ranger --stance crouching --walk --aim 20
//!
//! A soldier is a real `RemotePlayer` entity (the same systems that draw other players in a Match
//! pose and dress it), standing, walking or sprinting along -Z with the camera following, so
//! what you see is what a Match shows. Drag with the left mouse button to orbit, the wheel
//! zooms, `Home` resets the view. The ground carries a 1 m grid and a 1.8 m ruler; the HUD lists
//! the triangles drawn and the bounds of the model in its rest pose.
//!
//! `--shot` takes a screenshot (after `--frames` frames, 150 by default, stepped at a fixed 60 Hz
//! so a walking soldier is at the same point of its stride every time) and exits.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::asset::{AssetPath, UnapprovedPathMode};
use bevy::camera::RenderTarget;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::world_serialization::WorldAsset;
use frontline::cli::Mode;
use frontline::data::{GameData, MovementTuning};
use frontline::flow::AppState;
use frontline::maps::{MapReady, MapRegistry};
use frontline::player::{LocalLife, RemoteDeath, SoldierVisualsPlugin};
use frontline::shared::{Health, PlayerPose, PlayerTeam, RemotePlayer, Stance, Team};
use frontline::video::{ActiveVideo, VideoSettings};
use frontline::{assets, atmosphere, data, materials, ui, world};

use crate::camera::View;

/// Frames between the subject showing and a scripted screenshot, unless `--frames` says.
const DEFAULT_SHOT_FRAMES: u32 = 150;
/// Frames the process lingers after the screenshot (the PNG is written on a worker thread).
const SHOT_LINGER_FRAMES: u32 = 60;
/// Screenshots tried before giving up on a renderer that only draws black.
const MAX_SHOT_ATTEMPTS: u32 = 40;
const ORBIT_SENSITIVITY: f32 = 0.006;
const ZOOM_STEP: f32 = 0.1;
const MIN_DISTANCE: f32 = 0.25;
/// Reference height drawn beside the model, in metres.
const RULER_HEIGHT: f32 = 1.8;
/// Half the side of the grid drawn round the model, in metres.
const GRID_HALF: i32 = 8;

/// How fast a soldier moves along its facing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Still,
    Walk,
    Sprint,
}

/// What `--soldier` shows.
pub struct SoldierArgs {
    /// A Faction id from `data/factions.ron`.
    pub faction: String,
    pub stance: Stance,
    pub pace: Pace,
    /// Aim pitch in degrees, up positive.
    pub aim_degrees: f32,
    /// Team patches and armbands in the enemy colour instead of the friendly one.
    pub enemy: bool,
    /// Show the body it leaves when it dies instead.
    pub corpse: bool,
}

pub enum Subject {
    /// An asset path under the game's assets folder, or a file.
    Model(String),
    Soldier(SoldierArgs),
}

pub struct ViewerArgs {
    pub subject: Subject,
    pub look: Option<String>,
    /// Start camera: x y z, yaw and pitch in degrees (like a Map's `--cam`).
    pub cam: Option<[f32; 5]>,
    pub shot: Option<PathBuf>,
    pub frames: Option<u32>,
    pub size: (u32, u32),
    pub assets_dir: PathBuf,
}

/// The entity the subject hangs from: the model's scene root or the soldier's player.
#[derive(Component)]
struct ViewSubject;

#[derive(Component)]
struct Ground;

#[derive(Component)]
struct Hud;

/// The orbit camera: looks at `target` from `distance` away, `yaw` round from the front (the
/// -Z side, where a soldier faces) towards +X and `pitch` above the horizon.
#[derive(Component, Clone, Copy, Debug)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
    /// Where the camera looks, relative to the subject's centre (a `--cam` view need not look
    /// at it).
    focus: Vec3,
}

impl Orbit {
    /// Camera position relative to the target.
    fn offset(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        ) * self.distance
    }

    /// The orbit that puts the camera at `position` looking at `target`, which is `focus` from
    /// the subject's centre.
    fn from_position(position: Vec3, target: Vec3, focus: Vec3) -> Self {
        let offset = position - target;
        let distance = offset.length().max(MIN_DISTANCE);
        Self {
            yaw: offset.x.atan2(-offset.z),
            pitch: (offset.y / distance).clamp(-1.0, 1.0).asin(),
            distance,
            focus,
        }
    }
}

/// What the viewer shows and has measured.
#[derive(Resource)]
struct Viewer {
    title: String,
    soldier: Option<SoldierArgs>,
    /// The middle of the subject, which the camera looks at (plus `Orbit::focus`).
    centre: Vec3,
    /// The first view, which `Home` returns to.
    home: Orbit,
    pace: Pace,
    shot: Option<PathBuf>,
    shot_frames: u32,
    size: (u32, u32),
    /// Frames since something was drawn.
    ready_frames: u32,
    shot_taken: Option<u32>,
    /// Whether the last screenshot showed anything (once it has come back from the GPU).
    shot_drawn: Option<bool>,
    shot_attempts: u32,
    triangles: usize,
    meshes: usize,
    bounds: Option<(Vec3, Vec3)>,
    /// Where a soldier has walked to, so the ground and grid follow.
    travelled: Vec3,
    /// Whether the orbit target still needs the model's bounds (a model with no `--cam`).
    needs_framing: bool,
}

/// A model file named on the command line: an asset path under `assets_dir`, or any file.
fn resolve_model(model: &str, assets_dir: &Path) -> Result<String, String> {
    if model.starts_with("embedded://") {
        return Ok(model.to_owned());
    }
    let file = Path::new(model);
    if file.is_file() {
        let absolute = file.canonicalize().map_err(|error| error.to_string())?;
        return Ok(absolute.to_string_lossy().into_owned());
    }
    if assets_dir.join(model).is_file() {
        return Ok(model.to_owned());
    }
    Err(format!(
        "no model \"{model}\" (not a file, and not under {})",
        assets_dir.display()
    ))
}

pub fn run(args: ViewerArgs) -> AppExit {
    let (width, height) = args.size;
    let scene_path = match &args.subject {
        Subject::Model(model) => match resolve_model(model, &args.assets_dir) {
            Ok(path) => Some(path),
            Err(error) => {
                eprintln!("{error} (--model)");
                return AppExit::error();
            }
        },
        Subject::Soldier(_) => None,
    };

    let mut app = App::new();
    app.insert_resource(Mode::Windowed {
        port: 0,
        start: None,
        name: None,
        cod4: None,
    });
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Frontline model viewer".into(),
                    resolution: (width, height).into(),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: args.assets_dir.to_string_lossy().into_owned(),
                // Files named on the command line may be anywhere.
                unapproved_path_mode: UnapprovedPathMode::Allow,
                ..default()
            }),
    );
    app.add_plugins((
        data::DataPlugin,
        world::WorldPlugin,
        assets::EmbeddedAssetsPlugin,
        materials::MaterialsPlugin,
        ui::UiPlugin,
    ));
    app.init_state::<AppState>();
    app.insert_resource(ActiveVideo(VideoSettings::default()));
    app.add_plugins(atmosphere::AtmospherePlugin {
        look_override: args.look.clone(),
    });
    if let Some(look) = &args.look {
        let looks = app.world().resource::<atmosphere::Looks>();
        if !looks.contains(look) {
            eprintln!(
                "Unknown look \"{look}\" (--look); known: {}",
                looks.known_names()
            );
            return AppExit::error();
        }
    }

    let (title, soldier) = match args.subject {
        Subject::Model(_) => (
            format!("model {}", scene_path.as_deref().unwrap_or_default()),
            None,
        ),
        Subject::Soldier(soldier) => {
            let mut data = app.world_mut().resource_mut::<GameData>();
            let Some(chosen) = data
                .factions
                .iter()
                .find(|faction| faction.id == soldier.faction)
                .cloned()
            else {
                let known: Vec<&str> = data.factions.iter().map(|f| f.id.as_str()).collect();
                eprintln!(
                    "Unknown Faction \"{}\" (--soldier); known: {}",
                    soldier.faction,
                    known.join(", ")
                );
                return AppExit::error();
            };
            // Both Teams play the chosen Faction, so friendly and enemy only differ in the
            // team colour.
            data.factions[0] = chosen.clone();
            data.factions[1] = chosen.clone();
            (
                format!("soldier {} ({})", chosen.name, chosen.id),
                Some(soldier),
            )
        }
    };

    let centre = Vec3::Y
        * match soldier.as_ref().map(|soldier| soldier.stance) {
            Some(Stance::Crouching) => 0.65,
            Some(Stance::Prone) => 0.35,
            _ => 0.9,
        };
    let default_home = Orbit {
        yaw: 0.6,
        pitch: 0.2,
        distance: if soldier.is_some() { 3.4 } else { 3.0 },
        focus: Vec3::ZERO,
    };
    let home = match args.cam {
        Some(cam) => {
            let view = View {
                position: Vec3::new(cam[0], cam[1], cam[2]),
                yaw: cam[3].to_radians(),
                pitch: cam[4].to_radians(),
            };
            let forward = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch, 0.0) * Vec3::NEG_Z;
            // Looks along the given view, at the point as far off as the subject's centre.
            let target = view.position + forward * view.position.distance(centre);
            Orbit::from_position(view.position, target, target - centre)
        }
        None => default_home,
    };
    let pace = soldier.as_ref().map_or(Pace::Still, |soldier| soldier.pace);
    if args.shot.is_some() {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
    }
    app.insert_resource(Viewer {
        title,
        soldier,
        centre,
        home,
        pace,
        shot: args.shot,
        shot_frames: args.frames.unwrap_or(DEFAULT_SHOT_FRAMES),
        size: args.size,
        ready_frames: 0,
        shot_taken: None,
        shot_drawn: None,
        shot_attempts: 0,
        triangles: 0,
        meshes: 0,
        bounds: None,
        travelled: Vec3::ZERO,
        needs_framing: scene_path.is_some() && args.cam.is_none(),
    });
    if let Some(path) = scene_path {
        app.insert_resource(ModelToShow(path));
    } else {
        // What the Match systems read about the local player and the Maps.
        app.insert_resource(LocalLife::on_team(Team::Alpha))
            .init_resource::<MapRegistry>()
            .add_plugins(SoldierVisualsPlugin);
    }
    app.add_systems(Startup, setup).add_systems(
        Update,
        (
            walk,
            orbit_input,
            measure,
            frame_model,
            follow_camera,
            draw_guides,
            scripted_shot,
            update_hud,
        )
            .chain(),
    );
    app.run()
}

/// The texture a scripted screenshot's camera renders to.
#[derive(Resource)]
struct ShotTarget(Handle<Image>);

/// The file shown in `--model` mode.
#[derive(Resource)]
struct ModelToShow(String);

fn setup(
    mut commands: Commands,
    viewer: Res<Viewer>,
    model: Option<Res<ModelToShow>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    deaths: Option<ResMut<Messages<RemoteDeath>>>,
) {
    // The look (sky, sun, ambient, grade) is applied when a Map becomes ready, and only to the
    // cameras that exist then: the marker is spawned last (see the end of this function).

    commands.spawn((
        Name::new("Ground"),
        Ground,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.32, 0.31),
            perceptual_roughness: 0.95,
            reflectance: 0.1,
            ..default()
        })),
    ));

    let orbit = viewer.home;
    let mut camera = commands.spawn((
        Name::new("Viewer camera"),
        Camera3d::default(),
        Transform::default(),
        orbit,
    ));
    if viewer.shot.is_some() {
        // A scripted screenshot renders to its own texture: a window that is hidden, covered or
        // on a sleeping display presents nothing, and the screenshot would come out black.
        let (width, height) = viewer.size;
        let mut image =
            Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = images.add(image);
        camera.insert(RenderTarget::from(target.clone()));
        commands.insert_resource(ShotTarget(target));
    }

    commands.spawn((
        Hud,
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

    if let Some(model) = model {
        let scene: Handle<WorldAsset> =
            asset_server.load(AssetPath::from(model.0.clone()).with_label("Scene0"));
        commands.spawn((
            Name::new("Model"),
            ViewSubject,
            WorldAssetRoot(scene),
            Transform::default(),
        ));
    } else if let Some(soldier) = &viewer.soldier {
        let pose = PlayerPose {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: soldier.aim_degrees.to_radians(),
            stance: soldier.stance,
            aim_magnification: 0.0,
        };
        if soldier.corpse {
            if let Some(mut deaths) = deaths {
                deaths.write(RemoteDeath {
                    transform: Transform::default(),
                    stance: soldier.stance,
                    friendly: !soldier.enemy,
                    faction: 0,
                });
            }
            commands.spawn((Name::new("Body"), ViewSubject, Transform::default()));
        } else {
            commands.spawn((
                Name::new("Soldier"),
                ViewSubject,
                RemotePlayer,
                pose,
                Health(100.0),
                PlayerTeam(Some(if soldier.enemy {
                    Team::Bravo
                } else {
                    Team::Alpha
                })),
            ));
        }
    }

    // The Sun is its child: a parent without visibility would hide the light.
    commands.spawn((
        MapReady {
            name: "viewer".into(),
            look: Some("overcast_urban".into()),
        },
        Transform::default(),
        Visibility::default(),
    ));
}

/// A soldier's speed at `pace` in `stance`, from the movement data.
fn pace_speed(pace: Pace, stance: Stance, movement: &MovementTuning) -> f32 {
    match (pace, stance) {
        (Pace::Still, _) => 0.0,
        (Pace::Walk, Stance::Standing) => movement.walk_speed,
        (Pace::Sprint, Stance::Standing) => movement.sprint_speed,
        (_, Stance::Crouching) => movement.crouch_speed,
        (_, Stance::Prone) => movement.prone_speed,
    }
}

/// A walking soldier's pose moves along its facing.
fn walk(
    time: Res<Time>,
    data: Res<GameData>,
    mut viewer: ResMut<Viewer>,
    mut poses: Query<&mut PlayerPose, With<ViewSubject>>,
) {
    let Ok(mut pose) = poses.single_mut() else {
        return;
    };
    let speed = pace_speed(viewer.pace, pose.stance, &data.movement);
    pose.position += Vec3::NEG_Z * speed * time.delta_secs();
    viewer.travelled = pose.position;
}

fn orbit_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    viewer: Res<Viewer>,
    mut cameras: Query<&mut Orbit>,
) {
    // A scripted screenshot is of the view asked for, whatever the mouse does meanwhile.
    if viewer.shot.is_some() {
        return;
    }
    let Ok(mut orbit) = cameras.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::Home) {
        *orbit = viewer.home;
    }
    if buttons.pressed(MouseButton::Left) {
        orbit.yaw -= motion.delta.x * ORBIT_SENSITIVITY;
        orbit.pitch = (orbit.pitch + motion.delta.y * ORBIT_SENSITIVITY).clamp(-0.2, 1.5);
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if lines != 0.0 {
        orbit.distance = (orbit.distance * (1.0 - ZOOM_STEP * lines)).max(MIN_DISTANCE);
    }
}

/// Triangles drawn and the bounds of the subject's meshes in its own space (rest pose: skinned
/// meshes are measured as they are bound). A model without `--cam` is framed to its bounds the
/// first time they are known.
fn measure(
    mut viewer: ResMut<Viewer>,
    subject: Query<Entity, With<ViewSubject>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &ChildOf)>,
    drawn: Query<(&Mesh3d, &InheritedVisibility), Without<Ground>>,
    mesh_assets: Res<Assets<Mesh>>,
) {
    let mut triangles = 0;
    let mut count = 0;
    for (Mesh3d(handle), visibility) in &drawn {
        if !visibility.get() {
            continue;
        }
        let Some(mesh) = mesh_assets.get(handle) else {
            continue;
        };
        count += 1;
        triangles += mesh.indices().map_or(mesh.count_vertices(), Indices::len) / 3;
    }
    viewer.triangles = triangles;
    viewer.meshes = count;
    if count > 0 {
        viewer.ready_frames += 1;
    }

    let Ok(root) = subject.single() else {
        return;
    };
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for entity in children.iter_descendants(root) {
        let Ok((Mesh3d(handle), visibility)) = drawn.get(entity) else {
            continue;
        };
        let Some(positions) = visibility
            .get()
            .then(|| mesh_assets.get(handle))
            .flatten()
            .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
            .and_then(|positions| positions.as_float3())
        else {
            continue;
        };
        // The mesh's transform relative to the subject.
        let mut to_subject = Transform::IDENTITY;
        let mut current = entity;
        while current != root {
            let Ok((transform, parent)) = nodes.get(current) else {
                break;
            };
            to_subject = *transform * to_subject;
            current = parent.parent();
        }
        for &position in positions {
            let point = to_subject.transform_point(Vec3::from(position));
            bounds = Some(bounds.map_or((point, point), |(min, max)| {
                (min.min(point), max.max(point))
            }));
        }
    }
    if bounds.is_some() {
        viewer.bounds = bounds;
    }
}

/// A model without `--cam` is framed to its bounds the first time they are known: its centre is
/// what the camera orbits, from as far as its bounding sphere needs.
fn frame_model(mut viewer: ResMut<Viewer>, mut cameras: Query<&mut Orbit>) {
    if !viewer.needs_framing {
        return;
    }
    let (Some((min, max)), Ok(mut orbit)) = (viewer.bounds, cameras.single_mut()) else {
        return;
    };
    viewer.needs_framing = false;
    viewer.centre = (min + max) / 2.0;
    let radius = (max - min).length() / 2.0;
    // Fit the bounding sphere in the (45 degree) view.
    orbit.distance = (radius / 22.5_f32.to_radians().sin()).max(MIN_DISTANCE);
    viewer.home.distance = orbit.distance;
}

/// Keeps the camera looking at the subject (a walking soldier moves) from the orbit, and the
/// ground under it.
fn follow_camera(
    viewer: Res<Viewer>,
    mut cameras: Query<(&Orbit, &mut Transform), (With<Camera3d>, Without<Ground>)>,
    mut ground: Query<&mut Transform, (With<Ground>, Without<Camera3d>)>,
) {
    let Ok((orbit, mut transform)) = cameras.single_mut() else {
        return;
    };
    let centre = viewer.centre + viewer.travelled.with_y(0.0);
    let target = centre + orbit.focus;
    let position = target + orbit.offset();
    *transform = Transform::from_translation(position).looking_at(target, Vec3::Y);
    if let Ok(mut ground) = ground.single_mut() {
        // Under the subject, in whole metres so the grid stays put.
        ground.translation = Vec3::new(centre.x.round(), 0.0, centre.z.round());
    }
}

/// The 1 m grid and the height ruler.
fn draw_guides(viewer: Res<Viewer>, mut gizmos: Gizmos) {
    let (centre, ruler_at) = match (&viewer.soldier, viewer.bounds) {
        (None, Some((min, max))) => (
            Vec3::new((min.x + max.x) / 2.0, 0.0, (min.z + max.z) / 2.0),
            Vec3::new(max.x + 0.3, 0.0, (min.z + max.z) / 2.0),
        ),
        _ => (
            viewer.travelled.with_y(0.0),
            viewer.travelled.with_y(0.0) + Vec3::new(0.9, 0.0, 0.0),
        ),
    };
    let (cx, cz) = (centre.x.round() as i32, centre.z.round() as i32);
    for i in -GRID_HALF..=GRID_HALF {
        let major = (cx + i) % 5 == 0;
        let color = if major {
            Color::srgba(0.8, 0.8, 0.8, 0.35)
        } else {
            Color::srgba(0.8, 0.8, 0.8, 0.14)
        };
        let x = (cx + i) as f32;
        gizmos.line(
            Vec3::new(x, 0.002, (cz - GRID_HALF) as f32),
            Vec3::new(x, 0.002, (cz + GRID_HALF) as f32),
            color,
        );
        let major = (cz + i) % 5 == 0;
        let color = if major {
            Color::srgba(0.8, 0.8, 0.8, 0.35)
        } else {
            Color::srgba(0.8, 0.8, 0.8, 0.14)
        };
        let z = (cz + i) as f32;
        gizmos.line(
            Vec3::new((cx - GRID_HALF) as f32, 0.002, z),
            Vec3::new((cx + GRID_HALF) as f32, 0.002, z),
            color,
        );
    }
    let ruler = Color::srgb(1.0, 0.55, 0.1);
    gizmos.line(ruler_at, ruler_at + Vec3::Y * RULER_HEIGHT, ruler);
    for tenth in 0..=6 {
        let y = tenth as f32 * 0.3;
        gizmos.line(
            ruler_at + Vec3::new(-0.05, y, 0.0),
            ruler_at + Vec3::new(0.05, y, 0.0),
            ruler,
        );
    }
}

fn scripted_shot(
    target: Option<Res<ShotTarget>>,
    mut viewer: ResMut<Viewer>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let Some(path) = viewer.shot.clone() else {
        return;
    };
    match viewer.shot_taken {
        None if viewer.ready_frames >= viewer.shot_frames => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            info!("Screenshot {}", path.display());
            commands
                .spawn(match &target {
                    Some(target) => Screenshot::image(target.0.clone()),
                    None => Screenshot::primary_window(),
                })
                .observe(check_shot)
                .observe(save_to_disk(path));
            viewer.shot_taken = Some(0);
            viewer.shot_drawn = None;
        }
        Some(frames) => match viewer.shot_drawn {
            // The renderer's first frames can come out blank (its pipelines compile in the
            // background): wait for more frames and shoot again, the file is overwritten.
            Some(false) if viewer.shot_attempts < MAX_SHOT_ATTEMPTS => {
                viewer.shot_attempts += 1;
                viewer.shot_taken = None;
                viewer.shot_drawn = None;
                viewer.ready_frames = 0;
            }
            Some(false) => {
                error!("Every screenshot came out black");
                exit.write(AppExit::error());
            }
            Some(true) => {
                viewer.shot_taken = Some(frames + 1);
                if frames >= SHOT_LINGER_FRAMES {
                    exit.write(AppExit::Success);
                }
            }
            None => viewer.shot_taken = Some(frames + 1),
        },
        None => {}
    }
}

/// Whether the captured screenshot shows anything: a blank (all zero) frame is a renderer that
/// was not ready yet.
fn check_shot(captured: On<ScreenshotCaptured>, mut viewer: ResMut<Viewer>) {
    let drawn = captured
        .image
        .data
        .as_ref()
        .is_some_and(|data| data.iter().any(|&byte| byte != 0));
    viewer.shot_drawn = Some(drawn);
}

fn update_hud(
    viewer: Res<Viewer>,
    data: Res<GameData>,
    camera: Query<&Orbit>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let mut lines = vec![format!("Model viewer  {}", viewer.title)];
    if let Some(soldier) = &viewer.soldier {
        let pace = match soldier.pace {
            Pace::Still => "still",
            Pace::Walk => "walking",
            Pace::Sprint => "sprinting",
        };
        let speed = pace_speed(soldier.pace, soldier.stance, &data.movement);
        lines.push(format!(
            "{:?}  {pace} {speed:.1} m/s  aim {:+.0} deg  team {}{}",
            soldier.stance,
            soldier.aim_degrees,
            if soldier.enemy { "enemy" } else { "friendly" },
            if soldier.corpse { "  (body)" } else { "" },
        ));
    }
    lines.push(format!(
        "triangles {}  meshes {}",
        viewer.triangles, viewer.meshes
    ));
    if let Some((min, max)) = viewer.bounds {
        let size = max - min;
        lines.push(format!(
            "bounds x {:.2}..{:.2}  y {:.2}..{:.2}  z {:.2}..{:.2}   size {:.2} x {:.2} x {:.2} m",
            min.x, max.x, min.y, max.y, min.z, max.z, size.x, size.y, size.z
        ));
    }
    if let Ok(orbit) = camera.single() {
        lines.push(format!(
            "camera {:.1} m  yaw {:.0}  pitch {:.0}   drag: orbit  wheel: zoom  Home: reset",
            orbit.distance,
            orbit.yaw.to_degrees(),
            orbit.pitch.to_degrees()
        ));
    }
    text.0 = lines.join("\n");
}
