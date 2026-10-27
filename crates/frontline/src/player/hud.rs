//! Local-player HUD: damage vignette and direction indicators, team-coloured kill feed and the
//! death screen. The Match Rules' `hud` block switches the indicators and kill feed off and
//! can delay the feed.

use std::collections::VecDeque;
use std::f32::consts::PI;

use bevy::prelude::*;

use super::life::LocalLife;
use crate::data::{ActiveRules, GameData};
use crate::flow::AppState;
use crate::match_hud::{FRIENDLY_COLOR, relation_color};
use crate::net::protocol::{Damaged, KillFeed};
use crate::shared::{
    FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerPose, PlayerTeam,
};
use crate::ui::UiFonts;

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
const FEED_FONT_SIZE: f32 = 17.0;
/// Weapon and unknown names in the kill feed.
const FEED_COLOR: Color = Color::srgb(0.95, 0.95, 0.95);

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
}

/// Kill-feed lines received but not shown yet, each with the time (`elapsed_secs_f64`) it
/// is due; the Match Rules' `kill_feed_delay_secs` holds them back.
#[derive(Resource, Default, Debug)]
pub(super) struct PendingKillFeed(VecDeque<(f64, KillFeed)>);

/// Drops held-back kill-feed lines when leaving the game.
pub(super) fn reset_kill_feed(mut pending: ResMut<PendingKillFeed>) {
    pending.0.clear();
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
        DespawnOnExit(AppState::InGame),
        DamageVignette,
        full_screen(),
        vignette(0.0),
        Visibility::Hidden,
    ));
    commands.spawn((
        Name::new("Damage indicators"),
        DespawnOnExit(AppState::InGame),
        DamageIndicators,
        full_screen(),
    ));
    commands.spawn((
        Name::new("Kill feed"),
        DespawnOnExit(AppState::InGame),
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
        DespawnOnExit(AppState::InGame),
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

/// Red screen edges that deepen with missing health; none while still choosing a Team.
pub(super) fn update_damage_vignette(
    rules: ActiveRules,
    replica: Query<
        (&Health, &PlayerTeam),
        (
            With<LocalPlayerReplica>,
            Or<(Changed<Health>, Changed<PlayerTeam>)>,
        ),
    >,
    mut vignette_node: Single<(&mut BackgroundGradient, &mut Visibility), With<DamageVignette>>,
) {
    let Ok((health, team)) = replica.single() else {
        return;
    };
    let missing = if team.0.is_some() {
        (1.0 - health.0 / rules.get().max_health).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (gradient, visibility) = &mut *vignette_node;
    **gradient = vignette(missing * VIGNETTE_MAX_ALPHA);
    visibility.set_if_neq(if missing > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// Spawns a direction indicator per `Damaged`, unless the Match Rules turn them off.
pub(super) fn spawn_damage_indicators(
    rules: ActiveRules,
    mut damaged: MessageReader<FromServer<Damaged>>,
    root: Single<Entity, With<DamageIndicators>>,
    mut commands: Commands,
) {
    if !rules.get().hud.damage_direction {
        damaged.clear();
        return;
    }
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

/// Base colour of a kill-feed text section, faded by `fade_kill_feed`.
#[derive(Component)]
pub(super) struct FeedColor(Color);

/// Queues each kill-feed message for `kill_feed_delay_secs`, then shows it: friendly names
/// blue, enemy names red. Nothing shows when the Match Rules turn the kill feed off. (The
/// death screen reads the messages itself, so it names the killer at once.)
pub(super) fn add_kill_feed_lines(
    time: Res<Time>,
    data: Res<GameData>,
    rules: ActiveRules,
    fonts: Res<UiFonts>,
    life: Res<LocalLife>,
    mut feed: MessageReader<FromServer<KillFeed>>,
    mut pending: ResMut<PendingKillFeed>,
    me: Query<&PlayerName, With<LocalPlayer>>,
    players: Query<(&PlayerName, &PlayerTeam)>,
    list: Single<(Entity, Option<&Children>), With<KillFeedList>>,
    lines: Query<&KillFeedLine>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    let hud = &rules.get().hud;
    if !hud.kill_feed {
        feed.clear();
        pending.0.clear();
        return;
    }
    let due = now + f64::from(hud.kill_feed_delay_secs);
    pending
        .0
        .extend(feed.read().map(|FromServer(line)| (due, line.clone())));
    if pending.0.front().is_none_or(|(at, _)| *at > now) {
        return;
    }
    let (list, children) = *list;
    let mut existing: Vec<Entity> = children
        .map(|children| children.iter().filter(|&c| lines.contains(c)).collect())
        .unwrap_or_default();
    let name_color = |name: &str| {
        if me.single().is_ok_and(|me| me.0 == name) {
            return FRIENDLY_COLOR;
        }
        players
            .iter()
            .find(|(player, _)| player.0 == name)
            .map_or(FEED_COLOR, |(_, team)| relation_color(life.team(), team.0))
    };
    let font = TextFont {
        font: fonts.regular.clone().into(),
        font_size: FontSize::Px(FEED_FONT_SIZE),
        ..default()
    };
    while let Some((_, line)) = pending.0.pop_front_if(|(at, _)| *at <= now) {
        debug!("Kill feed shows: {} killed {}", line.killer, line.victim);
        if existing.len() >= FEED_LINES {
            commands.entity(existing.remove(0)).try_despawn();
        }
        let weapon = &data.weapon(line.weapon).name;
        let headshot = if line.headshot { "  (headshot)" } else { "" };
        let killer = name_color(&line.killer);
        let victim = name_color(&line.victim);
        let entity = commands
            .spawn((
                KillFeedLine { born: now },
                Text::new(line.killer.clone()),
                font.clone(),
                TextColor(killer),
                FeedColor(killer),
                ChildOf(list),
                children![
                    (
                        TextSpan::new(format!("  [{weapon}]  ")),
                        font.clone(),
                        TextColor(FEED_COLOR),
                        FeedColor(FEED_COLOR),
                    ),
                    (
                        TextSpan::new(line.victim.clone()),
                        font.clone(),
                        TextColor(victim),
                        FeedColor(victim),
                    ),
                    (
                        TextSpan::new(headshot),
                        font.clone(),
                        TextColor(FEED_COLOR),
                        FeedColor(FEED_COLOR),
                    ),
                ],
            ))
            .id();
        existing.push(entity);
    }
}

/// Fades each kill-feed line out after `FEED_VISIBLE_SECS`.
pub(super) fn fade_kill_feed(
    time: Res<Time>,
    lines: Query<(Entity, &KillFeedLine, &Children)>,
    mut sections: Query<(&FeedColor, &mut TextColor)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    for (entity, line, children) in &lines {
        let left = line.born + FEED_VISIBLE_SECS + FEED_FADE_SECS - now;
        if left <= 0.0 {
            commands.entity(entity).despawn();
        } else if left < FEED_FADE_SECS {
            let alpha = (left / FEED_FADE_SECS) as f32;
            for section in std::iter::once(entity).chain(children.iter()) {
                if let Ok((base, mut color)) = sections.get_mut(section) {
                    color.0 = base.0.with_alpha(alpha);
                }
            }
        }
    }
}

/// Shown while dead; not while still choosing a Team.
pub(super) fn update_death_screen(
    time: Res<Time>,
    rules: ActiveRules,
    life: Res<LocalLife>,
    mut screen: Single<&mut Visibility, With<DeathScreen>>,
    mut title: Single<&mut Text, (With<DeathTitle>, Without<DeathCountdown>)>,
    mut countdown: Single<&mut Text, (With<DeathCountdown>, Without<DeathTitle>)>,
) {
    let shown = life.is_dead() && life.team().is_some();
    screen.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !shown {
        return;
    }
    let heading = match life.killer() {
        Some(killer) => format!("You were killed by {killer}"),
        None if life.switched_team() => "You switched to the other team".to_owned(),
        None => "You were killed".to_owned(),
    };
    let left = f64::from(rules.get().respawn_delay_secs) - life.secs_dead(time.elapsed_secs_f64());
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
