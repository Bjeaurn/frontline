//! The title screen over a slow fly-around of the idle Map (`--map`, the arena by default).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::flow::{AppState, FlowCommand, MenuScreen};
use crate::ui::{ACCENT, TEXT_DIM, UiFonts, button, label, title};

const ORBIT_RADIUS: f32 = 40.0;
const ORBIT_HEIGHT: f32 = 16.0;
/// Radians per second.
const ORBIT_SPEED: f32 = 0.04;
const ORBIT_TARGET: Vec3 = Vec3::new(0.0, 2.0, 0.0);

/// The main menu's camera, circling the Map.
#[derive(Component)]
pub(super) struct MenuCamera;

pub(super) fn spawn_menu_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Menu camera"),
        MenuCamera,
        DespawnOnExit(AppState::MainMenu),
        Camera3d::default(),
        orbit_transform(0.0),
    ));
}

fn orbit_transform(angle: f32) -> Transform {
    let (sin, cos) = angle.sin_cos();
    Transform::from_xyz(ORBIT_RADIUS * sin, ORBIT_HEIGHT, ORBIT_RADIUS * cos)
        .looking_at(ORBIT_TARGET, Vec3::Y)
}

pub(super) fn orbit_menu_camera(
    time: Res<Time<Real>>,
    mut cameras: Query<&mut Transform, With<MenuCamera>>,
) {
    for mut transform in &mut cameras {
        *transform = orbit_transform(time.elapsed_secs() * ORBIT_SPEED);
    }
}

pub(super) fn spawn_title(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            Name::new("Title screen"),
            DespawnOnExit(MenuScreen::Title),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                padding: UiRect::left(px(96.0)),
                row_gap: px(10.0),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::new(
                LinearGradient::TO_RIGHT,
                vec![
                    ColorStop::percent(Color::srgba(0.02, 0.03, 0.02, 0.92), 0.0),
                    ColorStop::percent(Color::srgba(0.02, 0.03, 0.02, 0.55), 45.0),
                    ColorStop::percent(Color::srgba(0.02, 0.03, 0.02, 0.0), 100.0),
                ],
            )),
        ))
        .with_children(|root| {
            root.spawn(title(&fonts, "Frontline", 112.0));
            root.spawn((
                label(&fonts, "LAN WARFARE", 24.0, ACCENT),
                Node {
                    margin: UiRect::bottom(px(36.0)),
                    ..default()
                },
            ));
            root.spawn(button(&fonts, "Host Game")).observe(
                |_: On<Activate>, mut next: ResMut<NextState<MenuScreen>>| {
                    next.set(MenuScreen::HostSetup);
                },
            );
            root.spawn(button(&fonts, "Join Game")).observe(
                |_: On<Activate>, mut next: ResMut<NextState<MenuScreen>>| {
                    next.set(MenuScreen::ServerBrowser);
                },
            );
            root.spawn(button(&fonts, "Settings")).observe(
                |_: On<Activate>, mut next: ResMut<NextState<MenuScreen>>| {
                    next.set(MenuScreen::Settings);
                },
            );
            root.spawn(button(&fonts, "Quit")).observe(
                |_: On<Activate>, mut flow: MessageWriter<FlowCommand>| {
                    flow.write(FlowCommand::Quit);
                },
            );
            root.spawn((
                label(
                    &fonts,
                    concat!("v", env!("CARGO_PKG_VERSION")),
                    16.0,
                    TEXT_DIM,
                ),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(96.0),
                    bottom: px(24.0),
                    ..default()
                },
            ));
        });
}
