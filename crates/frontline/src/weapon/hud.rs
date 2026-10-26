//! Weapon HUD: centred crosshair (hidden when aiming), hitmarker, and the ammo counter.

use bevy::prelude::*;

use super::{HeldWeapon, WeaponSystems};
use crate::data::GameData;
use crate::net::protocol::HitConfirm;
use crate::player::LocalLife;
use crate::shared::{FromServer, LocalPlayer};

const CROSSHAIR_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.85);
/// Side of the square the four crosshair arms sit in, in logical pixels.
const CROSSHAIR_SIZE: f32 = 22.0;
const ARM_LENGTH: f32 = 7.0;
const ARM_THICKNESS: f32 = 2.0;
/// Past this aim-down-sights blend the crosshair is hidden.
const CROSSHAIR_HIDE_ADS: f32 = 0.5;

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

#[derive(Component)]
struct Crosshair;

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

fn crosshair_arm(left: f32, top: f32, width: f32, height: f32) -> impl Bundle {
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
    let centre = (CROSSHAIR_SIZE - ARM_THICKNESS) / 2.0;
    let far = CROSSHAIR_SIZE - ARM_LENGTH;
    commands.spawn((
        Name::new("Crosshair"),
        centred(),
        children![(
            Crosshair,
            Node {
                width: px(CROSSHAIR_SIZE),
                height: px(CROSSHAIR_SIZE),
                ..default()
            },
            children![
                crosshair_arm(centre, 0.0, ARM_THICKNESS, ARM_LENGTH),
                crosshair_arm(centre, far, ARM_THICKNESS, ARM_LENGTH),
                crosshair_arm(0.0, centre, ARM_LENGTH, ARM_THICKNESS),
                crosshair_arm(far, centre, ARM_LENGTH, ARM_THICKNESS),
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

fn update_crosshair(
    life: Res<LocalLife>,
    weapons: Query<&HeldWeapon, With<LocalPlayer>>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
) {
    let aiming = weapons
        .single()
        .is_ok_and(|weapon| weapon.ads > CROSSHAIR_HIDE_ADS);
    crosshair.set_if_neq(if aiming || life.is_dead() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
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
