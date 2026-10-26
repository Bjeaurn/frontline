//! Local-player HUD: damage vignette and direction indicators, kill feed and the death screen.

use std::f32::consts::PI;

use bevy::prelude::*;

use super::life::LocalLife;
use crate::data::GameData;
use crate::net::protocol::{Damaged, KillFeed};
use crate::shared::{FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerPose};

const DAMAGE_RED: Color = Color::srgb(0.75, 0.0, 0.0);
/// Vignette opacity at zero health; scales linearly with missing health.
const VIGNETTE_MAX_ALPHA: f32 = 0.8;
/// Where the vignette starts, as a percentage of the centre-to-corner distance.
const VIGNETTE_INNER_PERCENT: f32 = 40.0;

/// Radius of the circle the damage direction bars sit on, in logical pixels.
const INDICATOR_RADIUS: f32 = 140.0;
const INDICATOR_SIZE: Vec2 = Vec2::new(90.0, 10.0);
const INDICATOR_SECS: f32 = 1.0;

const FEED_LINES: usize = 5;
const FEED_VISIBLE_SECS: f64 = 5.0;
const FEED_FADE_SECS: f64 = 0.5;
const FEED_FONT_SIZE: f32 = 15.0;
const FEED_COLOR: Color = Color::srgb(0.95, 0.95, 0.95);
/// Lines where the local player is the killer.
const FEED_OWN_KILL_COLOR: Color = Color::srgb(1.0, 0.85, 0.3);

const DEATH_TITLE_SIZE: f32 = 30.0;
const DEATH_SUBTITLE_SIZE: f32 = 18.0;

#[derive(Component)]
pub(super) struct DamageVignette;

/// Root that damage direction indicators are spawned under.
#[derive(Component)]
pub(super) struct DamageIndicators;

/// One damage direction indicator; rotated so its bar points toward `from`.
#[derive(Component)]
pub(super) struct DamageIndicator {
    from: Vec3,
    age: f32,
    bar: Entity,
}

#[derive(Component)]
pub(super) struct KillFeedList;

#[derive(Component)]
pub(super) struct KillFeedLine {
    born: f64,
    color: Color,
}

#[derive(Component)]
pub(super) struct DeathScreen;

#[derive(Component)]
pub(super) struct DeathTitle;

#[derive(Component)]
pub(super) struct DeathCountdown;

fn full_screen() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100.0),
        height: percent(100.0),
        ..default()
    }
}

fn vignette(alpha: f32) -> BackgroundGradient {
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::percent(DAMAGE_RED.with_alpha(0.0), VIGNETTE_INNER_PERCENT),
            ColorStop::percent(DAMAGE_RED.with_alpha(alpha), 100.0),
        ],
    ))
}

pub(super) fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Name::new("Damage vignette"),
        DamageVignette,
        full_screen(),
        vignette(0.0),
        Visibility::Hidden,
    ));
    commands.spawn((Name::new("Damage indicators"), DamageIndicators, full_screen()));
    commands.spawn((
        Name::new("Kill feed"),
        KillFeedList,
        Node {
            position_type: PositionType::Absolute,
            right: px(14.0),
            top: px(10.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(3.0),
            ..default()
        },
    ));
    commands.spawn((
        Name::new("Death screen"),
        DeathScreen,
        Node {
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(8.0),
            ..full_screen()
        },
        BackgroundColor(Color::srgba(0.15, 0.0, 0.0, 0.35)),
        Visibility::Hidden,
        children![
            (
                DeathTitle,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(DEATH_TITLE_SIZE),
                    ..default()
                },
                TextColor(Color::WHITE),
            ),
            (
                DeathCountdown,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(DEATH_SUBTITLE_SIZE),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
            ),
        ],
    ));
}

/// Red screen edges that deepen with missing health.
pub(super) fn update_damage_vignette(
    data: Res<GameData>,
    replica: Query<&Health, (With<LocalPlayerReplica>, Changed<Health>)>,
    mut vignette_node: Single<(&mut BackgroundGradient, &mut Visibility), With<DamageVignette>>,
) {
    let Ok(health) = replica.single() else {
        return;
    };
    let missing = (1.0 - health.0 / data.rules().max_health).clamp(0.0, 1.0);
    let (gradient, visibility) = &mut *vignette_node;
    **gradient = vignette(missing * VIGNETTE_MAX_ALPHA);
    visibility.set_if_neq(if missing > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

pub(super) fn spawn_damage_indicators(
    mut damaged: MessageReader<FromServer<Damaged>>,
    root: Single<Entity, With<DamageIndicators>>,
    mut commands: Commands,
) {
    for FromServer(hit) in damaged.read() {
        let bar = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(INDICATOR_RADIUS - INDICATOR_SIZE.x / 2.0),
                    top: px(0.0),
                    width: px(INDICATOR_SIZE.x),
                    height: px(INDICATOR_SIZE.y),
                    ..default()
                },
                BackgroundColor(DAMAGE_RED),
            ))
            .id();
        commands
            .spawn((
                DamageIndicator {
                    from: hit.from,
                    age: 0.0,
                    bar,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50.0),
                    top: percent(50.0),
                    width: px(2.0 * INDICATOR_RADIUS),
                    height: px(2.0 * INDICATOR_RADIUS),
                    margin: UiRect {
                        left: px(-INDICATOR_RADIUS),
                        top: px(-INDICATOR_RADIUS),
                        ..default()
                    },
                    ..default()
                },
                UiTransform::IDENTITY,
                ChildOf(*root),
            ))
            .add_child(bar);
    }
}

/// Clockwise screen angle from straight ahead to `target`, seen from `pose`.
fn screen_angle(pose: &PlayerPose, target: Vec3) -> f32 {
    let to = target - pose.position;
    let (sin, cos) = pose.yaw.sin_cos();
    let forward = Vec2::new(-sin, -cos);
    let right = Vec2::new(cos, -sin);
    let flat = Vec2::new(to.x, to.z);
    if flat.length_squared() < 1e-6 {
        return PI;
    }
    flat.dot(right).atan2(flat.dot(forward))
}

/// Keeps each indicator pointing at its shooter as the player turns, fading it out.
pub(super) fn update_damage_indicators(
    time: Res<Time>,
    player: Query<&PlayerPose, With<LocalPlayer>>,
    mut indicators: Query<(Entity, &mut DamageIndicator, &mut UiTransform)>,
    mut bars: Query<&mut BackgroundColor>,
    mut commands: Commands,
) {
    let Ok(pose) = player.single() else {
        return;
    };
    for (entity, mut indicator, mut transform) in &mut indicators {
        indicator.age += time.delta_secs();
        if indicator.age >= INDICATOR_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        transform.rotation = Rot2::radians(screen_angle(pose, indicator.from));
        if let Ok(mut color) = bars.get_mut(indicator.bar) {
            color.0 = DAMAGE_RED.with_alpha(1.0 - indicator.age / INDICATOR_SECS);
        }
    }
}

pub(super) fn add_kill_feed_lines(
    time: Res<Time>,
    data: Res<GameData>,
    mut feed: MessageReader<FromServer<KillFeed>>,
    me: Query<&PlayerName, With<LocalPlayer>>,
    list: Single<(Entity, Option<&Children>), With<KillFeedList>>,
    lines: Query<&KillFeedLine>,
    mut commands: Commands,
) {
    if feed.is_empty() {
        return;
    }
    let (list, children) = *list;
    let mut existing: Vec<Entity> = children
        .map(|children| children.iter().filter(|&c| lines.contains(c)).collect())
        .unwrap_or_default();
    for FromServer(line) in feed.read() {
        if existing.len() >= FEED_LINES {
            commands.entity(existing.remove(0)).try_despawn();
        }
        let weapon = &data.weapon(line.weapon).name;
        let headshot = if line.headshot { "  (headshot)" } else { "" };
        let color = if me.single().is_ok_and(|me| me.0 == line.killer) {
            FEED_OWN_KILL_COLOR
        } else {
            FEED_COLOR
        };
        let entity = commands
            .spawn((
                KillFeedLine {
                    born: time.elapsed_secs_f64(),
                    color,
                },
                Text::new(format!("{}  [{weapon}]  {}{headshot}", line.killer, line.victim)),
                TextFont {
                    font_size: FontSize::Px(FEED_FONT_SIZE),
                    ..default()
                },
                TextColor(color),
                ChildOf(list),
            ))
            .id();
        existing.push(entity);
    }
}

/// Fades each kill-feed line out after `FEED_VISIBLE_SECS`.
pub(super) fn fade_kill_feed(
    time: Res<Time>,
    mut lines: Query<(Entity, &KillFeedLine, &mut TextColor)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    for (entity, line, mut color) in &mut lines {
        let left = line.born + FEED_VISIBLE_SECS + FEED_FADE_SECS - now;
        if left <= 0.0 {
            commands.entity(entity).despawn();
        } else if left < FEED_FADE_SECS {
            color.0 = line.color.with_alpha((left / FEED_FADE_SECS) as f32);
        }
    }
}

pub(super) fn update_death_screen(
    time: Res<Time>,
    data: Res<GameData>,
    life: Res<LocalLife>,
    mut screen: Single<&mut Visibility, With<DeathScreen>>,
    mut title: Single<&mut Text, (With<DeathTitle>, Without<DeathCountdown>)>,
    mut countdown: Single<&mut Text, (With<DeathCountdown>, Without<DeathTitle>)>,
) {
    screen.set_if_neq(if life.is_dead() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !life.is_dead() {
        return;
    }
    let heading = match life.killer() {
        Some(killer) => format!("You were killed by {killer}"),
        None => "You were killed".to_owned(),
    };
    let left = f64::from(data.rules().respawn_delay_secs) - life.secs_dead(time.elapsed_secs_f64());
    let status = if left > 0.0 {
        format!("Respawning in {left:.1}")
    } else {
        "Respawning...".to_owned()
    };
    if title.0 != heading {
        title.0 = heading;
    }
    if countdown.0 != status {
        countdown.0 = status;
    }
}
