//! Local-player HUD: damage vignette with blood edges and direction indicators, the
//! team-coloured kill feed and the death screen. The Match Rules' `hud` block switches the
//! indicators and kill feed off and can delay the feed.

use std::collections::VecDeque;
use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::LetterSpacing;

use super::killcam::Killcam;
use super::life::{LocalLife, can_request_respawn, respawn_hold};
use crate::data::{ActiveRules, GameData};
use crate::flow::AppState;
use crate::gear::EquippedPerks;
use crate::match_hud::{FRIENDLY_COLOR, relation_color};
use crate::modes::ModeView;
use crate::net::protocol::{Damaged, DeathCause, KillFeed};
use crate::shared::{
    FromServer, Health, LocalPlayer, LocalPlayerReplica, PlayerName, PlayerPose, PlayerTeam,
};
use crate::ui::{self, AMBER, UiFonts};

const DAMAGE_RED: Color = Color::srgb(0.75, 0.0, 0.0);
/// The dark blood tint of the screen-edge vignette.
const BLOOD: Color = Color::srgb(0.32, 0.0, 0.0);
/// Vignette opacity at zero health; scales linearly with missing health.
const VIGNETTE_MAX_ALPHA: f32 = 0.6;
/// Where the vignette starts, as a percentage of the centre-to-corner distance.
const VIGNETTE_INNER_PERCENT: f32 = 45.0;
/// Side of the generated blood-edge texture, in pixels.
const BLOOD_PX: u32 = 256;
/// Blood-edge opacity from missing health (at zero health) and added by a fresh hit.
const BLOOD_HEALTH_ALPHA: f32 = 0.95;
const BLOOD_HIT_ALPHA: f32 = 0.5;
/// A hit's flash dies away at this rate (1/s); each point of damage adds `1 / FLASH_DAMAGE`.
const FLASH_DECAY_PER_SEC: f32 = 2.2;
const FLASH_DAMAGE: f32 = 60.0;
/// Below this share of health the blood edges throb like a heartbeat, this many times a second.
const HEARTBEAT_BELOW: f32 = 0.4;
const HEARTBEAT_HZ: f32 = 1.4;

/// Radius of the circle the damage direction bars sit on, in logical pixels.
const INDICATOR_RADIUS: f32 = 140.0;
const INDICATOR_SIZE: Vec2 = Vec2::new(90.0, 10.0);
const INDICATOR_SECS: f32 = 1.0;

const FEED_LINES: usize = 5;
const FEED_VISIBLE_SECS: f64 = 5.0;
const FEED_FADE_SECS: f64 = 0.5;
const FEED_FONT_SIZE: f32 = 18.0;
/// Weapon and unknown names in the kill feed.
const FEED_COLOR: Color = Color::srgb(0.80, 0.78, 0.70);
const FEED_PLATE: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);

const DEATH_KICKER_SIZE: f32 = 22.0;
const DEATH_TITLE_SIZE: f32 = 64.0;
const DEATH_DETAIL_SIZE: f32 = 20.0;
const DEATH_SUBTITLE_SIZE: f32 = 22.0;

#[derive(Component)]
pub(super) struct DamageVignette;

/// The generated blood splatter along the screen edges, tinted by `update_damage_vignette`.
#[derive(Component)]
pub(super) struct BloodEdges;

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

/// The text lines of the death screen.
#[derive(Component, PartialEq)]
pub(super) enum DeathLine {
    /// "KILLED BY".
    Kicker,
    /// The killer's name, or what happened.
    Title,
    /// The weapon that did it.
    Detail,
    /// The respawn countdown or prompt.
    Countdown,
}

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
            ColorStop::percent(BLOOD.with_alpha(0.0), VIGNETTE_INNER_PERCENT),
            ColorStop::percent(BLOOD.with_alpha(alpha), 100.0),
        ],
    ))
}

/// Cheap integer hash to `0..1`, for the blood noise.
fn hash01(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Smooth value noise at `(x, y)`.
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (ix as i32, iy as i32);
    let top = hash01(ix, iy).lerp(hash01(ix + 1, iy), sx);
    let bottom = hash01(ix, iy + 1).lerp(hash01(ix + 1, iy + 1), sx);
    top.lerp(bottom, sy)
}

/// Three octaves of value noise, `0..1`.
fn fbm(x: f32, y: f32) -> f32 {
    (value_noise(x, y) * 0.5
        + value_noise(x * 2.1, y * 2.1) * 0.3
        + value_noise(x * 4.3, y * 4.3) * 0.2)
        .clamp(0.0, 1.0)
}

/// A dark red, irregular splatter hugging the screen edges, with a few droplets reaching in;
/// tinted and faded through its `ImageNode` as health drops.
fn blood_edges() -> Image {
    let half = BLOOD_PX as f32 / 2.0;
    let data = (0..BLOOD_PX * BLOOD_PX)
        .flat_map(|i| {
            let u = ((i % BLOOD_PX) as f32 + 0.5 - half) / half;
            let v = ((i / BLOOD_PX) as f32 + 0.5 - half) / half;
            let r = (u * u + v * v).sqrt() / std::f32::consts::SQRT_2;
            let blotch = fbm(u * 2.6 + 7.0, v * 2.6 + 3.0);
            let edge = ((r - 0.5 + (blotch - 0.5) * 0.6) / 0.45).clamp(0.0, 1.0);
            let mut alpha = edge * edge * (3.0 - 2.0 * edge);
            let drops = fbm(u * 9.0 + 1.0, v * 9.0 + 11.0);
            if r > 0.5 && drops > 0.68 {
                alpha =
                    alpha.max(((drops - 0.68) / 0.08).min(1.0) * 0.8 * ((r - 0.5) * 3.0).min(1.0));
            }
            let shade = 1.0 - 0.45 * alpha;
            [
                (0.62 * shade * 255.0) as u8,
                (0.02 * 255.0) as u8,
                (0.03 * 255.0) as u8,
                (alpha * 255.0) as u8,
            ]
        })
        .collect();
    Image::new(
        Extent3d {
            width: BLOOD_PX,
            height: BLOOD_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A black fade at the top or bottom edge of the death screen (letterbox).
fn death_bar(to_bottom: bool) -> impl Bundle {
    let (from, to) = if to_bottom { (0.9, 0.0) } else { (0.0, 0.9) };
    (
        Node {
            width: percent(100.0),
            height: percent(18.0),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::new(
            LinearGradient::TO_BOTTOM,
            vec![
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, from), 0.0),
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, to), 100.0),
            ],
        )),
    )
}

pub(super) fn spawn_hud(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn((
        Name::new("Damage vignette"),
        DespawnOnExit(AppState::InGame),
        DamageVignette,
        full_screen(),
        vignette(0.0),
        Visibility::Hidden,
    ));
    commands.spawn((
        Name::new("Blood edges"),
        DespawnOnExit(AppState::InGame),
        BloodEdges,
        full_screen(),
        ImageNode::new(images.add(blood_edges()))
            .with_mode(NodeImageMode::Stretch)
            .with_color(Color::srgba(1.0, 1.0, 1.0, 0.0)),
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
            left: px(16.0),
            top: px(16.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
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
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..full_screen()
        },
        BackgroundColor(Color::srgba(0.12, 0.0, 0.0, 0.38)),
        Visibility::Hidden,
        children![
            death_bar(false),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(4.0),
                    ..default()
                },
                children![
                    (
                        DeathLine::Kicker,
                        ui::hud_text(&fonts.bold, "", DEATH_KICKER_SIZE, AMBER),
                        LetterSpacing::Px(6.0),
                    ),
                    (
                        DeathLine::Title,
                        ui::hud_text(&fonts.display, "", DEATH_TITLE_SIZE, Color::WHITE),
                        LetterSpacing::Px(2.0),
                    ),
                    (
                        Node {
                            width: px(280.0),
                            height: px(3.0),
                            margin: UiRect::vertical(px(8.0)),
                            ..default()
                        },
                        BackgroundColor(AMBER),
                    ),
                    (
                        DeathLine::Detail,
                        ui::hud_text(&fonts.bold, "", DEATH_DETAIL_SIZE, ui::TEXT),
                        LetterSpacing::Px(2.0),
                    ),
                    (
                        DeathLine::Countdown,
                        ui::hud_text(
                            &fonts.bold,
                            "",
                            DEATH_SUBTITLE_SIZE,
                            Color::srgba(1.0, 1.0, 1.0, 0.85)
                        ),
                        LetterSpacing::Px(2.0),
                        Node {
                            margin: UiRect::top(px(18.0)),
                            ..default()
                        },
                    ),
                ],
            ),
            death_bar(true),
        ],
    ));
}

/// Dark-red screen edges and blood splatter that deepen with missing health, flash on every
/// hit and throb at low health; none while still choosing a Team or watching the Kill-cam.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_damage_vignette(
    time: Res<Time>,
    rules: ActiveRules,
    killcam: Res<Killcam>,
    replica: Query<(&Health, &PlayerTeam, Option<&EquippedPerks>), With<LocalPlayerReplica>>,
    data: Res<GameData>,
    mut damaged: MessageReader<FromServer<Damaged>>,
    mut flash: Local<f32>,
    mut applied: Local<f32>,
    mut vignette_node: Single<(&mut BackgroundGradient, &mut Visibility), With<DamageVignette>>,
    mut blood: Single<
        (&mut ImageNode, &mut Visibility),
        (With<BloodEdges>, Without<DamageVignette>),
    >,
) {
    let hit_flash: f32 = damaged.read().map(|FromServer(hit)| hit.amount).sum();
    *flash = (*flash + hit_flash / FLASH_DAMAGE).min(1.0);
    *flash = (*flash - FLASH_DECAY_PER_SEC * time.delta_secs()).max(0.0);
    let missing = match replica.single() {
        // The Kill-cam shows the killer's view: none of our own wounds on it.
        _ if killcam.is_playing() => {
            *flash = 0.0;
            0.0
        }
        Ok((health, team, perks)) if team.0.is_some() => {
            // Juggernaut raises the maximum; the server replicates the Perks.
            let max_health = perks
                .map(|perks| data.perk_effects(perks.0.iter()))
                .unwrap_or_default()
                .max_health(rules.get().max_health);
            (1.0 - health.0 / max_health).clamp(0.0, 1.0)
        }
        _ => {
            *flash = 0.0;
            0.0
        }
    };
    let (gradient, visibility) = &mut *vignette_node;
    if (missing - *applied).abs() > 0.002 {
        *applied = missing;
        **gradient = vignette(missing * VIGNETTE_MAX_ALPHA);
    }
    visibility.set_if_neq(if missing > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });

    let low = ((missing - (1.0 - HEARTBEAT_BELOW)) / HEARTBEAT_BELOW).clamp(0.0, 1.0);
    let beat = (time.elapsed_secs() * HEARTBEAT_HZ * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let alpha =
        (missing * BLOOD_HEALTH_ALPHA + *flash * BLOOD_HIT_ALPHA + beat.powi(3) * 0.2 * low)
            .clamp(0.0, 1.0);
    let (image, visibility) = &mut *blood;
    image.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    visibility.set_if_neq(if alpha > 0.01 {
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
        font: fonts.bold.clone().into(),
        font_size: FontSize::Px(FEED_FONT_SIZE),
        ..default()
    };
    let is_me = |name: &str| me.single().is_ok_and(|me| me.0 == name);
    while let Some((_, line)) = pending.0.pop_front_if(|(at, _)| *at <= now) {
        debug!("Kill feed shows: {} died ({:?})", line.victim, line.cause);
        if existing.len() >= FEED_LINES {
            commands.entity(existing.remove(0)).try_despawn();
        }
        let victim = name_color(&line.victim);
        let span = |text: String, color: Color| {
            (
                TextSpan::new(text),
                font.clone(),
                TextColor(color),
                FeedColor(color),
            )
        };
        // Every line sits on a dark plate with a coloured edge: amber when the local player
        // got the kill, else the killer's (or the victim's) side colour.
        let plate = |edge: Color| {
            (
                Node {
                    padding: UiRect::axes(px(10.0), px(3.0)),
                    border: UiRect::left(px(3.0)),
                    ..default()
                },
                BackgroundColor(FEED_PLATE),
                BorderColor::all(edge),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.85),
                },
            )
        };
        let entity = match &line.cause {
            DeathCause::Shot { .. } | DeathCause::Explosive { .. } => {
                let (killer, weapon, headshot) = match &line.cause {
                    DeathCause::Shot {
                        killer,
                        weapon,
                        headshot,
                        ..
                    } => (
                        killer,
                        data.weapon(*weapon).name.clone(),
                        if *headshot { "   HEADSHOT" } else { "" },
                    ),
                    DeathCause::Explosive { killer, kind, .. } => {
                        (killer, data.grenade(*kind).name.clone(), "")
                    }
                    _ => unreachable!("matched above"),
                };
                let killer_color = name_color(killer);
                let edge = if is_me(killer) { AMBER } else { killer_color };
                commands
                    .spawn((
                        KillFeedLine { born: now },
                        Text::new(killer.clone()),
                        font.clone(),
                        TextColor(killer_color),
                        FeedColor(killer_color),
                        plate(edge),
                        ChildOf(list),
                        children![
                            span(format!("   {}   ", weapon.to_uppercase()), FEED_COLOR),
                            span(line.victim.clone(), victim),
                            span(headshot.to_owned(), AMBER),
                        ],
                    ))
                    .id()
            }
            DeathCause::Fall | DeathCause::Respawn => {
                let what = match line.cause {
                    DeathCause::Fall => "   fell to their death",
                    _ => "   respawned",
                };
                commands
                    .spawn((
                        KillFeedLine { born: now },
                        Text::new(line.victim.clone()),
                        font.clone(),
                        TextColor(victim),
                        FeedColor(victim),
                        plate(victim),
                        ChildOf(list),
                        children![span(what.to_owned(), FEED_COLOR)],
                    ))
                    .id()
            }
        };
        existing.push(entity);
    }
}

/// Fades each kill-feed line (text and plate) out after `FEED_VISIBLE_SECS`.
pub(super) fn fade_kill_feed(
    time: Res<Time>,
    lines: Query<(Entity, &KillFeedLine, &Children)>,
    mut sections: Query<(&FeedColor, &mut TextColor)>,
    mut plates: Query<(&mut BackgroundColor, &mut BorderColor), With<KillFeedLine>>,
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
            if let Ok((mut background, mut border)) = plates.get_mut(entity) {
                background.0 = FEED_PLATE.with_alpha(FEED_PLATE.alpha() * alpha);
                let edge = border.left.with_alpha(alpha);
                *border = BorderColor::all(edge);
            }
        }
    }
}

/// Shown while dead; not while still choosing a Team, nor while the Kill-cam plays (it has a
/// screen of its own).
#[allow(clippy::too_many_arguments)]
pub(super) fn update_death_screen(
    time: Res<Time>,
    rules: ActiveRules,
    data: Res<GameData>,
    life: Res<LocalLife>,
    killcam: Res<Killcam>,
    modes: Query<&ModeView>,
    mut screen: Single<&mut Visibility, With<DeathScreen>>,
    mut lines: Query<(&DeathLine, &mut Text)>,
) {
    let shown = life.is_dead() && life.team().is_some() && !killcam.is_playing();
    screen.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !shown {
        return;
    }
    // "KILLED BY" over the killer's name, then the weapon; other deaths just say what happened.
    let (kicker, heading, detail) = match life.cause() {
        Some(DeathCause::Shot {
            killer,
            weapon,
            headshot,
            ..
        }) => (
            "KILLED BY",
            killer.clone(),
            format!(
                "{}{}",
                data.weapon(*weapon).name.to_uppercase(),
                if *headshot { "   HEADSHOT" } else { "" }
            ),
        ),
        Some(DeathCause::Explosive { killer, kind, .. }) => (
            "KILLED BY",
            killer.clone(),
            data.grenade(*kind).name.to_uppercase(),
        ),
        Some(DeathCause::Fall) => ("", "YOU FELL TO YOUR DEATH".to_owned(), String::new()),
        Some(DeathCause::Respawn) => ("", "YOU RESPAWNED".to_owned(), String::new()),
        None if life.switched_team() => (
            "",
            "YOU SWITCHED TO THE OTHER TEAM".to_owned(),
            String::new(),
        ),
        None => ("", "YOU WERE KILLED".to_owned(), String::new()),
    };
    let now = time.elapsed_secs_f64();
    let rules = rules.get();
    let left = f64::from(rules.respawn_wait_secs(life.killer().is_some())) - life.secs_dead(now);
    let status = if let Some(hold) = respawn_hold(modes.single().ok(), life.team()) {
        hold.to_uppercase()
    } else if left <= 0.0 {
        "RESPAWNING...".to_owned()
    } else if can_request_respawn(&life, modes.single().ok(), rules, now) {
        "PRESS SPACE OR CLICK TO RESPAWN".to_owned()
    } else {
        format!("RESPAWNING IN {left:.1}")
    };
    for (line, mut text) in &mut lines {
        let wanted = match line {
            DeathLine::Kicker => kicker,
            DeathLine::Title => heading.as_str(),
            DeathLine::Detail => detail.as_str(),
            DeathLine::Countdown => status.as_str(),
        };
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    }
}
