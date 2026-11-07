//! The free-fly camera: hold the right mouse button to look, WASD to move, Q and E for down and
//! up, Shift for fast and Ctrl for slow, the wheel for the base speed. `T` is the top view, `Home`
//! the default aerial view.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::menu::InputCapture;

const LOOK_SENSITIVITY: f32 = 0.0025;
const DEFAULT_SPEED: f32 = 14.0;
/// Just short of straight down or up, where yaw and pitch stop being independent.
const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.0001;

/// Where the camera stands and looks (radians; yaw 0 looks along -z, pitch below 0 looks down).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl View {
    /// An aerial view over the middle of a Map, looking north.
    pub const AERIAL: View = View {
        position: Vec3::new(0.0, 55.0, 110.0),
        yaw: 0.0,
        pitch: -0.45,
    };
    /// Straight down, north (-z) up the screen.
    pub const TOP: View = View {
        position: Vec3::new(0.0, 175.0, 0.0),
        yaw: 0.0,
        pitch: -PITCH_LIMIT,
    };
}

#[derive(Component, Debug)]
pub struct FlyCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

impl FlyCamera {
    fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

/// The view the camera starts in (`--cam`, `--top`).
#[derive(Resource)]
pub struct StartView(pub View);

/// Asks the camera to jump to a view.
#[derive(Message, Clone, Copy, Debug)]
pub struct GoTo(pub View);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GoTo>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (presets, fly, apply_goto).chain());
    }
}

fn spawn_camera(start: Res<StartView>, mut commands: Commands) {
    let view = start.0;
    let fly = FlyCamera {
        yaw: view.yaw,
        pitch: view.pitch,
        speed: DEFAULT_SPEED,
    };
    commands.spawn((
        Name::new("MapEdit camera"),
        Camera3d::default(),
        Transform::from_translation(view.position).with_rotation(fly.rotation()),
        fly,
    ));
}

fn presets(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    mut goto: MessageWriter<GoTo>,
) {
    if capture.blocked() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        goto.write(GoTo(View::TOP));
    }
    if keys.just_pressed(KeyCode::Home) {
        goto.write(GoTo(View::AERIAL));
    }
}

fn apply_goto(mut goto: MessageReader<GoTo>, mut cameras: Query<(&mut FlyCamera, &mut Transform)>) {
    let Some(GoTo(view)) = goto.read().last().copied() else {
        return;
    };
    for (mut fly, mut transform) in &mut cameras {
        fly.yaw = view.yaw;
        fly.pitch = view.pitch;
        transform.translation = view.position;
        transform.rotation = fly.rotation();
    }
}

fn fly(
    time: Res<Time>,
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cameras: Query<(&mut FlyCamera, &mut Transform)>,
) {
    let looking = buttons.pressed(MouseButton::Right) && !capture.menu;
    // Ctrl (or Cmd) with a letter is an editing shortcut (Ctrl+D, Ctrl+S), not movement; while
    // looking, Ctrl still slows the camera.
    let shortcut = !looking
        && (keys.pressed(KeyCode::ControlLeft)
            || keys.pressed(KeyCode::ControlRight)
            || keys.pressed(KeyCode::SuperLeft)
            || keys.pressed(KeyCode::SuperRight));
    let moving = !capture.blocked() && !shortcut;
    if let Ok(mut cursor) = cursor.single_mut()
        && (buttons.just_pressed(MouseButton::Right) || buttons.just_released(MouseButton::Right))
    {
        cursor.grab_mode = if looking {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        cursor.visible = !looking;
    }
    for (mut fly, mut transform) in &mut cameras {
        if scroll.delta.y != 0.0 {
            fly.speed = (fly.speed * 1.15_f32.powf(scroll.delta.y)).clamp(1.0, 200.0);
        }
        if looking {
            fly.yaw -= motion.delta.x * LOOK_SENSITIVITY;
            fly.pitch =
                (fly.pitch - motion.delta.y * LOOK_SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }
        transform.rotation = fly.rotation();
        let axis = |plus: KeyCode, minus: KeyCode| {
            f32::from(keys.pressed(plus)) - f32::from(keys.pressed(minus))
        };
        let wish = if moving {
            transform.forward() * axis(KeyCode::KeyW, KeyCode::KeyS)
                + transform.right() * axis(KeyCode::KeyD, KeyCode::KeyA)
                + Vec3::Y * axis(KeyCode::KeyE, KeyCode::KeyQ)
        } else {
            Vec3::ZERO
        };
        let factor = if keys.pressed(KeyCode::ShiftLeft) {
            4.0
        } else if keys.pressed(KeyCode::ControlLeft) {
            0.25
        } else {
            1.0
        };
        transform.translation += wish.normalize_or_zero() * fly.speed * factor * time.delta_secs();
    }
}
