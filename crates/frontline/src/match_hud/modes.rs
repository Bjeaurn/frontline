//! The objective Game Modes' HUD: Flag icons, HQ state or Search & Destroy round info under the
//! score, a progress bar while capturing, planting or defusing, and the spectate caption.

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, clock};
use crate::data::GameData;
use crate::flow::AppState;
use crate::modes::{
    BombView, ModeView, SdPhaseView, SdView, Spectating, ZoneView, in_zone, spectated_name,
};
use crate::net::protocol::PlayerOwner;
use crate::player::LocalLife;
use crate::shared::{
    LocalPlayer, LocalPlayerReplica, MatchPhase, PlayerName, PlayerPose, RemotePlayer, Team,
};
use crate::ui::{self, UiFonts};

const BOX_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
const NEUTRAL: Color = Color::srgb(0.85, 0.85, 0.82);
const FLAG_SIZE: f32 = 34.0;
const TRACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
const BAR_WIDTH: f32 = 280.0;
/// Blinks a second.
const BLINK_HZ: f32 = 2.0;

/// Container under the score that is rebuilt when what it shows changes.
#[derive(Component)]
pub(super) struct ModeStrip;

#[derive(Component)]
pub(super) struct ActionBar;

#[derive(Component)]
pub(super) struct ActionText;

#[derive(Component)]
pub(super) struct ActionFill;

#[derive(Component)]
pub(super) struct SpectateText;

/// The track behind the action bar's fill; hidden while there is no progress to show.
#[derive(Component)]
pub(super) struct ActionTrack;

/// Blinks (alternately hidden and shown) while `true`: the alert for an enemy capture.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) struct Blinking(bool);

pub(super) fn spawn_mode_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((
        Name::new("Mode strip"),
        ModeStrip,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            top: px(46.0),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4.0),
            ..default()
        },
    ));
    commands.spawn((
        Name::new("Action bar"),
        ActionBar,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            bottom: percent(24.0),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4.0),
            ..default()
        },
        Visibility::Hidden,
        children![
            (
                ActionText,
                Blinking(false),
                ui::label(&fonts, "", 22.0, Color::WHITE)
            ),
            (
                ActionTrack,
                Node {
                    width: px(BAR_WIDTH),
                    height: px(10.0),
                    ..default()
                },
                BackgroundColor(TRACK),
                children![(
                    ActionFill,
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(FRIENDLY_COLOR),
                )],
            ),
        ],
    ));
    commands.spawn((
        Name::new("Spectate caption"),
        SpectateText,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(60.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::default(),
        TextFont {
            font: fonts.bold.clone().into(),
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::justify(Justify::Center),
    ));
}

fn side_color(team: Option<Team>, local: Option<Team>) -> Color {
    match team {
        None => NEUTRAL,
        Some(team) if Some(team) == local => FRIENDLY_COLOR,
        Some(_) => ENEMY_COLOR,
    }
}

fn line(parent: &mut ChildSpawnerCommands, fonts: &UiFonts, text: &str, size: f32, color: Color) {
    parent.spawn((
        Node {
            padding: UiRect::axes(px(10.0), px(2.0)),
            ..default()
        },
        BackgroundColor(BOX_BG),
        ui::label(fonts, text, size, color),
    ));
}

fn progress_bar(parent: &mut ChildSpawnerCommands, width: f32, percent_done: u8, color: Color) {
    parent
        .spawn((
            Node {
                width: px(width),
                height: px(5.0),
                ..default()
            },
            BackgroundColor(TRACK),
        ))
        .with_child((
            Node {
                width: percent(f32::from(percent_done)),
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(color),
        ));
}

/// What the strip shows under a Flag or the HQ: your own team's capture as a progress bar, but
/// only a blinking alert for an enemy capture or a contest (their progress isn't yours to read).
#[derive(Clone, Copy, Debug, PartialEq)]
enum ZoneBar {
    Progress(u8, Color),
    Alert(Color),
}

fn zone_bar(zone: &ZoneView, local: Option<Team>) -> ZoneBar {
    if zone.contested {
        return ZoneBar::Alert(NEUTRAL);
    }
    match zone.capturer {
        Some(team) if Some(team) != local => ZoneBar::Alert(ENEMY_COLOR),
        capturer => ZoneBar::Progress(zone.progress, side_color(capturer, local)),
    }
}

fn zone_bar_node(parent: &mut ChildSpawnerCommands, width: f32, bar: ZoneBar) {
    match bar {
        ZoneBar::Progress(percent_done, color) => progress_bar(parent, width, percent_done, color),
        ZoneBar::Alert(color) => {
            parent.spawn((
                Node {
                    width: px(width),
                    height: px(6.0),
                    ..default()
                },
                BackgroundColor(color),
                Blinking(true),
            ));
        }
    }
}

fn build_strip(
    strip: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    view: &ModeView,
    local: Option<Team>,
    me: Option<&PlayerOwner>,
) {
    match view {
        ModeView::None | ModeView::TeamDeathmatch(_) => {}
        ModeView::Domination(flags) => {
            strip
                .spawn(Node {
                    column_gap: px(8.0),
                    ..default()
                })
                .with_children(|row| {
                    for flag in flags {
                        let color = side_color(flag.zone.owner, local);
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(2.0),
                            ..default()
                        })
                        .with_children(|column| {
                            column.spawn((
                                Node {
                                    width: px(FLAG_SIZE),
                                    height: px(FLAG_SIZE),
                                    border: UiRect::all(px(2.0)),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BorderColor::all(color),
                                BackgroundColor(color.with_alpha(0.35)),
                                ui::label(fonts, &flag.label.to_string(), 24.0, Color::WHITE),
                            ));
                            zone_bar_node(column, FLAG_SIZE, zone_bar(&flag.zone, local));
                        });
                    }
                });
        }
        ModeView::Headquarters(hq) => {
            let (text, color) = match (hq.position, hq.zone.owner) {
                (None, _) => (
                    format!("HQ RELOCATING  {}", clock(hq.seconds_left)),
                    NEUTRAL,
                ),
                (Some(_), None) => (format!("HQ NEUTRAL  {}", clock(hq.seconds_left)), NEUTRAL),
                (Some(_), Some(owner)) if Some(owner) == local => (
                    format!("YOUR TEAM HOLDS THE HQ  {}", clock(hq.seconds_left)),
                    FRIENDLY_COLOR,
                ),
                (Some(_), Some(_)) => (
                    format!("ENEMY HOLDS THE HQ  {}", clock(hq.seconds_left)),
                    ENEMY_COLOR,
                ),
            };
            line(strip, fonts, &text, 20.0, color);
            if hq.position.is_some() && (hq.zone.capturer.is_some() || hq.zone.contested) {
                zone_bar_node(strip, BAR_WIDTH * 0.6, zone_bar(&hq.zone, local));
            }
        }
        ModeView::SearchAndDestroy(sd) => build_sd_strip(strip, fonts, sd, local, me),
    }
}

fn build_sd_strip(
    strip: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    sd: &SdView,
    local: Option<Team>,
    me: Option<&PlayerOwner>,
) {
    let ours = local.unwrap_or(Team::Alpha);
    let role = if sd.attackers == ours {
        "ATTACKING"
    } else {
        "DEFENDING"
    };
    line(
        strip,
        fonts,
        &format!("ROUND {}  -  {role}", sd.round),
        18.0,
        Color::WHITE,
    );
    strip
        .spawn((
            Node {
                column_gap: px(12.0),
                padding: UiRect::axes(px(10.0), px(2.0)),
                ..default()
            },
            BackgroundColor(BOX_BG),
        ))
        .with_children(|row| {
            row.spawn(ui::label(
                fonts,
                &format!("ALIVE {}", sd.alive[ours.index()]),
                18.0,
                FRIENDLY_COLOR,
            ));
            row.spawn(ui::label(
                fonts,
                &format!("ENEMY {}", sd.alive[ours.other().index()]),
                18.0,
                ENEMY_COLOR,
            ));
        });
    let attacking = sd.attackers == ours;
    let status: Option<(String, Color)> = match sd.phase {
        SdPhaseView::Waiting => Some(("WAITING FOR PLAYERS".into(), NEUTRAL)),
        SdPhaseView::Starting => Some(("ROUND STARTING".into(), NEUTRAL)),
        SdPhaseView::Over { winner, reason } => Some((
            format!(
                "{}  -  {}",
                if winner == ours {
                    "ROUND WON"
                } else {
                    "ROUND LOST"
                },
                reason.text(winner == ours).to_uppercase()
            ),
            side_color(Some(winner), local),
        )),
        SdPhaseView::Live => match sd.bomb {
            // Defenders aren't told where: they listen for the beeping.
            BombView::Planted { site, .. } if attacking => Some((
                format!("BOMB PLANTED AT {site}  {}", clock(sd.fuse_left)),
                FRIENDLY_COLOR,
            )),
            BombView::Planted { .. } => Some((
                format!("BOMB PLANTED  {}", clock(sd.fuse_left)),
                ENEMY_COLOR,
            )),
            BombView::Carried if attacking && sd.carrier == me.map(|owner| owner.0) => {
                Some(("YOU HAVE THE BOMB".into(), FRIENDLY_COLOR))
            }
            BombView::Carried if attacking => Some(("A TEAMMATE HAS THE BOMB".into(), NEUTRAL)),
            BombView::Dropped(_) if attacking => Some(("THE BOMB WAS DROPPED".into(), ENEMY_COLOR)),
            BombView::Dropped(_) => None,
            BombView::Carried | BombView::Gone => None,
        },
    };
    if let Some((text, color)) = status {
        line(strip, fonts, &text, 18.0, color);
    }
}

/// Rebuilds the strip when the Mode state (or our Team) changes.
pub(super) fn update_mode_strip(
    modes: Query<&ModeView>,
    life: Res<LocalLife>,
    fonts: Res<UiFonts>,
    me: Query<&PlayerOwner, With<LocalPlayerReplica>>,
    strip: Single<(Entity, Has<Children>), With<ModeStrip>>,
    mut shown: Local<Option<(ModeView, Option<Team>)>>,
    mut commands: Commands,
) {
    // The crates are drawn in the world, not in the strip: their opening and closing mustn't
    // rebuild it.
    let view = match modes.single() {
        Ok(ModeView::TeamDeathmatch(_)) | Err(_) => ModeView::None,
        Ok(view) => view.clone(),
    };
    let key = (view, life.team());
    let (strip, built) = *strip;
    let empty = key.0 == ModeView::None;
    if shown.as_ref() == Some(&key) && (built || empty) {
        return;
    }
    let (view, local) = &key;
    commands
        .entity(strip)
        .despawn_related::<Children>()
        .with_children(|parent| build_strip(parent, &fonts, view, *local, me.single().ok()));
    *shown = Some(key);
}

/// What the action bar says: caption, percent done (none: no bar, the caption blinks) and fill
/// colour.
type Action = (String, Option<u8>, Color);

fn zone_action(zone: &ZoneView, local: Team, name: &str, destroy: bool) -> Option<Action> {
    let verb = if destroy {
        "DESTROYING"
    } else {
        "NEUTRALISING"
    };
    if zone.contested {
        return Some((format!("{name} CONTESTED"), None, NEUTRAL));
    }
    match zone.capturer {
        Some(team) if team == local => {
            let caption = if zone.owner.is_some() {
                format!("{verb} {name}")
            } else {
                format!("CAPTURING {name}")
            };
            Some((caption, Some(zone.progress), FRIENDLY_COLOR))
        }
        Some(_) => {
            let caption = if zone.owner == Some(local) {
                format!("{name} UNDER ATTACK")
            } else {
                format!("LOSING {name}")
            };
            Some((caption, None, ENEMY_COLOR))
        }
        None => None,
    }
}

/// The bar at the bottom: capture progress while your team captures a zone you stand in,
/// a blinking caption while the enemy takes it or it is contested, plant/defuse progress and
/// hints in Search & Destroy.
pub(super) fn update_action_bar(
    modes: Query<&ModeView>,
    life: Res<LocalLife>,
    data: Res<GameData>,
    local: Query<&PlayerPose, With<LocalPlayer>>,
    me: Query<&PlayerOwner, With<LocalPlayerReplica>>,
    mut bar: Single<&mut Visibility, (With<ActionBar>, Without<ActionText>, Without<ActionTrack>)>,
    mut text: Single<(&mut Text, &mut Blinking), (With<ActionText>, Without<SpectateText>)>,
    mut track: Single<&mut Node, (With<ActionTrack>, Without<ActionFill>)>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), (With<ActionFill>, Without<ActionTrack>)>,
) {
    let action = action(&modes, &life, &data, &local, &me);
    let Some((caption, done, color)) = action else {
        bar.set_if_neq(Visibility::Hidden);
        return;
    };
    bar.set_if_neq(Visibility::Inherited);
    let (text, blinking) = &mut *text;
    if text.0 != caption {
        text.0 = caption;
    }
    blinking.set_if_neq(Blinking(done.is_none()));
    track.display = if done.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    let (node, background) = &mut *fill;
    node.width = percent(f32::from(done.unwrap_or(0)));
    background.set_if_neq(BackgroundColor(color));
}

/// Blinks everything marked [`Blinking`]: on a bit over half of each period.
pub(super) fn blink_alerts(time: Res<Time>, mut blinking: Query<(&mut Visibility, &Blinking)>) {
    let on = (time.elapsed_secs() * BLINK_HZ).fract() < 0.55;
    for (mut visibility, blinking) in &mut blinking {
        let shown = if !blinking.0 || on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(shown);
    }
}

fn action(
    modes: &Query<&ModeView>,
    life: &LocalLife,
    data: &GameData,
    local: &Query<&PlayerPose, With<LocalPlayer>>,
    me: &Query<&PlayerOwner, With<LocalPlayerReplica>>,
) -> Option<Action> {
    let (view, team, pose) = (
        modes.single().ok()?,
        life.team().filter(|_| !life.is_dead())?,
        local.single().ok()?,
    );
    match view {
        ModeView::None | ModeView::TeamDeathmatch(_) => None,
        ModeView::Domination(flags) => {
            let tuning = &data.domination().capture;
            let flag = flags
                .iter()
                .find(|flag| in_zone(pose.position, flag.position, tuning))?;
            zone_action(&flag.zone, team, &format!("FLAG {}", flag.label), false)
        }
        ModeView::Headquarters(hq) => {
            let position = hq.position?;
            in_zone(pose.position, position, &data.headquarters().capture)
                .then(|| zone_action(&hq.zone, team, "HQ", true))?
        }
        ModeView::SearchAndDestroy(sd) => {
            if let Some(action) = sd.action
                && me.single().is_ok_and(|owner| owner.0 == action.peer)
            {
                let caption = if action.planting {
                    "PLANTING THE BOMB"
                } else {
                    "DEFUSING"
                };
                return Some((caption.into(), Some(action.progress), FRIENDLY_COLOR));
            }
            if sd.phase != SdPhaseView::Live {
                return None;
            }
            let radius = data.search_and_destroy().site_radius;
            let at_site = |at: Vec3| {
                pose.position.xz().distance(at.xz()) <= radius
                    && (pose.position.y - at.y).abs() <= 2.5
            };
            let carrying = sd.carrier == me.single().ok().map(|owner| owner.0);
            match sd.bomb {
                BombView::Carried
                    if sd.attackers == team
                        && carrying
                        && sd.sites.iter().any(|&(_, at)| at_site(at)) =>
                {
                    Some(("HOLD F TO PLANT".into(), Some(0), FRIENDLY_COLOR))
                }
                BombView::Planted { position, .. } if sd.attackers != team && at_site(position) => {
                    Some(("HOLD F TO DEFUSE".into(), Some(0), FRIENDLY_COLOR))
                }
                _ => None,
            }
        }
    }
}

/// "Spectating <name>" while a dead Search & Destroy player follows a teammate.
pub(super) fn update_spectate_text(
    spectating: Res<Spectating>,
    names: Query<&PlayerName, With<RemotePlayer>>,
    mut caption: Single<&mut Text, With<SpectateText>>,
) {
    let shown = spectated_name(&spectating, &names)
        .map(|name| format!("SPECTATING {name}  -  CLICK FOR NEXT TEAMMATE"))
        .unwrap_or_default();
    if caption.0 != shown {
        caption.0 = shown;
    }
}

/// ROUND WON / ROUND LOST over the screen when a Search & Destroy round is decided (the Match
/// result banner takes over when that round ended the Match).
pub(super) fn show_round_banner(
    modes: Query<&ModeView>,
    matches: super::MatchQuery,
    life: Res<LocalLife>,
    fonts: Res<UiFonts>,
    mut seen: Local<Option<u32>>,
    mut commands: Commands,
) {
    let (Ok(ModeView::SearchAndDestroy(sd)), Ok(state)) = (modes.single(), matches.single()) else {
        *seen = None;
        return;
    };
    let SdPhaseView::Over { winner, reason } = sd.phase else {
        *seen = None;
        return;
    };
    if *seen == Some(sd.round) {
        return;
    }
    *seen = Some(sd.round);
    if state.scores[winner.index()] >= state.score_limit {
        return;
    }
    let won = Some(winner) == life.team();
    let banner = ui::spawn_result_banner(
        &mut commands,
        &fonts,
        &ui::BannerSpec {
            headline: if won { "Round won" } else { "Round lost" },
            color: if won { FRIENDLY_COLOR } else { ENEMY_COLOR },
            subline: reason.text(won),
            top_percent: 38.0,
            scale: 1.0,
            hold_secs: 3.6,
            z_index: 15,
        },
    );
    commands
        .entity(banner)
        .insert(DespawnOnExit(AppState::InGame));
}

/// VICTORY / DEFEAT / DRAW when the Match ends.
pub(super) fn show_match_banner(
    matches: super::MatchQuery,
    life: Res<LocalLife>,
    data: Res<GameData>,
    maps: Res<crate::maps::MapRegistry>,
    fonts: Res<UiFonts>,
    mut was_ended: Local<bool>,
    mut commands: Commands,
) {
    let Ok(state) = matches.single() else {
        *was_ended = false;
        return;
    };
    let MatchPhase::Ended { winner, .. } = state.phase else {
        *was_ended = false;
        return;
    };
    if std::mem::replace(&mut *was_ended, true) {
        return;
    }
    let (headline, color, subline) = match winner {
        None => ("Draw", Color::srgb(0.9, 0.9, 0.9), "Match drawn".to_owned()),
        Some(team) => {
            let faction = data.faction(&maps, &state.map, team).name.clone();
            if Some(team) == life.team() {
                ("Victory", FRIENDLY_COLOR, format!("{faction} wins"))
            } else {
                ("Defeat", ENEMY_COLOR, format!("{faction} wins"))
            }
        }
    };
    let banner = ui::spawn_result_banner(
        &mut commands,
        &fonts,
        &ui::BannerSpec {
            headline,
            color,
            subline: &subline,
            top_percent: 5.0,
            scale: 1.0,
            hold_secs: 7.5,
            z_index: 15,
        },
    );
    commands
        .entity(banner)
        .insert(DespawnOnExit(AppState::InGame));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(capturer: Option<Team>, progress: u8, contested: bool) -> ZoneView {
        ZoneView {
            owner: None,
            capturer,
            progress,
            contested,
        }
    }

    #[test]
    fn only_your_own_teams_capture_shows_progress() {
        let me = Some(Team::Alpha);
        assert_eq!(
            zone_bar(&zone(Some(Team::Alpha), 40, false), me),
            ZoneBar::Progress(40, FRIENDLY_COLOR)
        );
        // An enemy capture, or a contest, is a blinking alert: no progress to read.
        assert_eq!(
            zone_bar(&zone(Some(Team::Bravo), 70, false), me),
            ZoneBar::Alert(ENEMY_COLOR)
        );
        assert_eq!(
            zone_bar(&zone(Some(Team::Alpha), 40, true), me),
            ZoneBar::Alert(NEUTRAL)
        );
        // Nobody capturing: the empty bar as before.
        assert_eq!(
            zone_bar(&zone(None, 0, false), me),
            ZoneBar::Progress(0, NEUTRAL)
        );
    }

    #[test]
    fn standing_in_a_zone_the_enemy_takes_blinks_without_a_percentage() {
        let enemy_taking = ZoneView {
            owner: Some(Team::Alpha),
            ..zone(Some(Team::Bravo), 55, false)
        };
        let (caption, progress, color) =
            zone_action(&enemy_taking, Team::Alpha, "FLAG A", false).unwrap();
        assert_eq!(caption, "FLAG A UNDER ATTACK");
        assert_eq!(progress, None);
        assert_eq!(color, ENEMY_COLOR);
        let contested =
            zone_action(&zone(Some(Team::Alpha), 55, true), Team::Alpha, "HQ", true).unwrap();
        assert_eq!(contested.1, None);
        // Your own capture keeps its bar.
        let own = zone_action(
            &zone(Some(Team::Alpha), 55, false),
            Team::Alpha,
            "FLAG A",
            false,
        )
        .unwrap();
        assert_eq!(own.1, Some(55));
    }
}
