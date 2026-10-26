//! Weapon HUD: centred crosshair (opening with spread, fading out when aiming), hitmarker,
//! and the ammo counter.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::{HeldWeapon, WeaponSystems};
use crate::data::GameData;
use crate::net::protocol::HitConfirm;
use crate::player::{EyeCamera, LocalLife};
use crate::shared::{FromServer, LocalPlayer};

/// Opacity of the crosshair at the hip.
const CROSSHAIR_ALPHA: f32 = 0.85;
const ARM_LENGTH: f32 = 7.0;
const ARM_THICKNESS: f32 = 2.0;
/// Smallest gap between the centre and the arms, in logical pixels.
const CROSSHAIR_MIN_GAP: f32 = 4.0;
/// The crosshair fades out over this much of the aim-down-sights blend.
const CROSSHAIR_FADE_ADS: f32 = 0.5;

const HITMARKER_SECS: f32 = 0.15;
const HITMARKER_SIZE: f32 = 26.0;
const HITMARKER_ARM: Vec2 = Vec2::new(10.0, 2.0);
const HITMARKER_COLOR: Color = Color::WHITE;
const HITMARKER_KILL_COLOR: Color = Color::srgb(1.0, 0.15, 0.1);

const AMMO_FONT_SIZE: f32 = 30.0;
const WEAPON_FONT_SIZE: f32 = 14.0;
const AMMO_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.92);
const AMMO_LOW_COLOR: Color = Color::srgb(1.0, 0.35, 0.25);
/// Rounds at or below which the count turns `AMMO_LOW_COLOR`.
const AMMO_LOW: u32 = 5;

pub(super) struct WeaponHudPlugin;

impl Plugin for WeaponHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_weapon_hud).add_systems(
            Update,
            (update_crosshair, show_hitmarker, fade_hitmarker, update_ammo)
                .after(WeaponSystems),
        );
    }
}

/// The crosshair, centred: its arms sit at the edge of the hip spread cone on screen.
#[derive(Component)]
struct Crosshair;

/// One crosshair arm, pushed out from the centre along its unit direction.
#[derive(Component)]
struct CrosshairArm(Vec2);

#[derive(Component, Default)]
struct Hitmarker {
    left: f32,
}

#[derive(Component)]
struct HitmarkerArm;

#[derive(Component)]
struct AmmoCount;

#[derive(Component)]
struct WeaponLabel;

fn crosshair_arm(direction: Vec2) -> impl Bundle {
    let size = if direction.x == 0.0 {
        Vec2::new(ARM_THICKNESS, ARM_LENGTH)
    } else {
        Vec2::new(ARM_LENGTH, ARM_THICKNESS)
    };
    (
        CrosshairArm(direction),
        Node {
            position_type: PositionType::Absolute,
            left: px(-size.x / 2.0),
            top: px(-size.y / 2.0),
            width: px(size.x),
            height: px(size.y),
            ..default()
        },
        BackgroundColor(Color::WHITE.with_alpha(CROSSHAIR_ALPHA)),
    )
}

fn hitmarker_arm(degrees: f32) -> impl Bundle {
    (
        HitmarkerArm,
        Node {
            position_type: PositionType::Absolute,
            left: px((HITMARKER_SIZE - HITMARKER_ARM.x) / 2.0),
            top: px((HITMARKER_SIZE - HITMARKER_ARM.y) / 2.0),
            width: px(HITMARKER_ARM.x),
            height: px(HITMARKER_ARM.y),
            ..default()
        },
        // Pushed out from the centre along its own axis, leaving a gap in the middle.
        UiTransform {
            translation: Val2::px(
                degrees.to_radians().cos() * HITMARKER_SIZE / 3.0,
                degrees.to_radians().sin() * HITMARKER_SIZE / 3.0,
            ),
            rotation: Rot2::degrees(degrees),
            ..UiTransform::IDENTITY
        },
        BackgroundColor(HITMARKER_COLOR),
    )
}

fn centred() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100.0),
        height: percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn spawn_weapon_hud(mut commands: Commands) {
    commands.spawn((
        Name::new("Crosshair"),
        centred(),
        children![(
            Crosshair,
            Node::default(),
            children![
                crosshair_arm(Vec2::NEG_Y),
                crosshair_arm(Vec2::Y),
                crosshair_arm(Vec2::NEG_X),
                crosshair_arm(Vec2::X),
            ],
        )],
    ));
    commands.spawn((
        Name::new("Hitmarker"),
        centred(),
        children![(
            Hitmarker::default(),
            Node {
                width: px(HITMARKER_SIZE),
                height: px(HITMARKER_SIZE),
                ..default()
            },
            Visibility::Hidden,
            children![
                hitmarker_arm(45.0),
                hitmarker_arm(135.0),
                hitmarker_arm(225.0),
                hitmarker_arm(315.0),
            ],
        )],
    ));
    commands.spawn((
        Name::new("Ammo"),
        Node {
            position_type: PositionType::Absolute,
            right: px(24.0),
            bottom: px(18.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        children![
            (
                WeaponLabel,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(WEAPON_FONT_SIZE),
                    ..default()
                },
                TextColor(AMMO_COLOR),
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    (
                        AmmoCount,
                        Text::default(),
                        TextFont {
                            font_size: FontSize::Px(AMMO_FONT_SIZE),
                            ..default()
                        },
                        TextColor(AMMO_COLOR),
                    ),
                    (
                        Text::new(" / "),
                        TextFont {
                            font_size: FontSize::Px(AMMO_FONT_SIZE),
                            ..default()
                        },
                        TextColor(AMMO_COLOR),
                    ),
                    // Infinite reserve: the built-in font has no "∞", so a sideways "8".
                    (
                        Text::new("8"),
                        TextFont {
                            font_size: FontSize::Px(AMMO_FONT_SIZE),
                            ..default()
                        },
                        TextColor(AMMO_COLOR),
                        UiTransform::from_rotation(Rot2::degrees(90.0)),
                    ),
                ],
            ),
        ],
    ));
}

/// Opens the arms to the hip spread cone's edge on screen and fades them out with the sights.
fn update_crosshair(
    data: Res<GameData>,
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    eye: Query<&Projection, With<EyeCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
    mut arms: Query<(&CrosshairArm, &mut UiTransform, &mut BackgroundColor)>,
) {
    let (Ok(weapon), Ok(Projection::Perspective(projection)), Ok(window)) =
        (weapons.single(), eye.single(), window.single())
    else {
        crosshair.set_if_neq(Visibility::Hidden);
        return;
    };
    let alpha = CROSSHAIR_ALPHA * (1.0 - weapon.ads / CROSSHAIR_FADE_ADS).clamp(0.0, 1.0);
    crosshair.set_if_neq(if alpha <= 0.0 || life.is_dead() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
    let spread = weapon.hip_spread_deg(data.weapon(weapon.id)).to_radians();
    let gap = (spread.tan() / (projection.fov / 2.0).tan() * window.height() / 2.0)
        .max(CROSSHAIR_MIN_GAP);
    let offset = gap + ARM_LENGTH / 2.0;
    for (arm, mut transform, mut color) in &mut arms {
        let translation = Val2::px(arm.0.x * offset, arm.0.y * offset);
        if transform.translation != translation {
            transform.translation = translation;
        }
        color.set_if_neq(BackgroundColor(Color::WHITE.with_alpha(alpha)));
    }
}

fn show_hitmarker(
    mut hits: MessageReader<FromServer<HitConfirm>>,
    mut marker: Single<(&mut Hitmarker, &mut Visibility)>,
    mut arms: Query<&mut BackgroundColor, With<HitmarkerArm>>,
) {
    let Some(kill) = hits
        .read()
        .map(|FromServer(hit)| hit.kill)
        .reduce(|a, b| a || b)
    else {
        return;
    };
    let (hitmarker, visibility) = &mut *marker;
    hitmarker.left = HITMARKER_SECS;
    **visibility = Visibility::Inherited;
    let color = if kill {
        HITMARKER_KILL_COLOR
    } else {
        HITMARKER_COLOR
    };
    for mut arm in &mut arms {
        arm.0 = color;
    }
}

fn fade_hitmarker(time: Res<Time>, mut marker: Single<(&mut Hitmarker, &mut Visibility)>) {
    let (hitmarker, visibility) = &mut *marker;
    if hitmarker.left > 0.0 {
        hitmarker.left -= time.delta_secs();
        if hitmarker.left <= 0.0 {
            **visibility = Visibility::Hidden;
        }
    }
}

/// Rewrites the counter only when the rounds or reload state shown actually change.
fn update_ammo(
    data: Res<GameData>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut shown: Local<Option<(u32, bool)>>,
    mut count: Single<(&mut Text, &mut TextColor), (With<AmmoCount>, Without<WeaponLabel>)>,
    mut label: Single<&mut Text, (With<WeaponLabel>, Without<AmmoCount>)>,
) {
    let Ok(weapon) = weapons.single() else {
        return;
    };
    let state = (weapon.rounds, weapon.reload_left.is_some());
    if *shown == Some(state) {
        return;
    }
    *shown = Some(state);
    let (text, color) = &mut *count;
    text.0 = weapon.rounds.to_string();
    color.0 = if weapon.rounds <= AMMO_LOW {
        AMMO_LOW_COLOR
    } else {
        AMMO_COLOR
    };
    let name = &data.weapon(weapon.id).name;
    label.0 = if state.1 {
        format!("{name}  reloading")
    } else {
        name.clone()
    };
}
