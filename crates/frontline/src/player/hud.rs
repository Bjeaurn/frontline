//! Minimal local-player HUD: a centred crosshair.

use bevy::prelude::*;

const CROSSHAIR_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.85);
/// Side of the square the four crosshair arms sit in, in logical pixels.
const CROSSHAIR_SIZE: f32 = 22.0;
const ARM_LENGTH: f32 = 7.0;
const ARM_THICKNESS: f32 = 2.0;

fn arm(left: f32, top: f32, width: f32, height: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(width),
            height: px(height),
            ..default()
        },
        BackgroundColor(CROSSHAIR_COLOR),
    )
}

pub(super) fn spawn_crosshair(mut commands: Commands) {
    let centre = (CROSSHAIR_SIZE - ARM_THICKNESS) / 2.0;
    let far = CROSSHAIR_SIZE - ARM_LENGTH;
    commands.spawn((
        Name::new("Crosshair"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            Node {
                width: px(CROSSHAIR_SIZE),
                height: px(CROSSHAIR_SIZE),
                ..default()
            },
            children![
                arm(centre, 0.0, ARM_THICKNESS, ARM_LENGTH),
                arm(centre, far, ARM_THICKNESS, ARM_LENGTH),
                arm(0.0, centre, ARM_LENGTH, ARM_THICKNESS),
                arm(far, centre, ARM_LENGTH, ARM_THICKNESS),
            ],
        )],
    ));
}
