//! The Scoreboard: held open with the scoreboard key, forced open at Match end (under the result
//! banner) with the next Rotation entry, the countdown to the next Match and the local player's
//! XP summary (XP gained, Level and progress bar). Our Team's section comes first; players are
//! sorted by kills and the local player is highlighted.

use std::cmp::Reverse;

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, own_side};
use crate::data::{GameData, GameModeId};
use crate::flow::AppState;
use crate::maps::MapRegistry;
use crate::net::protocol::PlayerOwner;
use crate::player::LocalLife;
use crate::player::input::Bindings;
use crate::profile::Profile;
use crate::progression::{LevelProgress, MatchXp};
use crate::shared::{
    LocalPlayerReplica, MatchPhase, MatchState, PlayerName, PlayerStats, PlayerTeam,
};
use crate::ui::{self, UiFonts};

const WIDTH: f32 = 560.0;
const SUBTITLE_SIZE: f32 = 18.0;
const SECTION_SIZE: f32 = 24.0;
const SECTION_EMBLEM_SIZE: f32 = 30.0;
const ROW_SIZE: f32 = 18.0;
const STAT_WIDTH: f32 = 48.0;
const SELF_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
const XP_BAR_HEIGHT: f32 = 10.0;
const XP_BAR_BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

#[derive(Component)]
pub(super) struct ScoreboardRoot;

/// The panel whose children are rebuilt whenever the board's content changes.
#[derive(Component)]
pub(super) struct ScoreboardPanel;

#[derive(PartialEq, Default)]
pub(super) struct Board {
    /// The win condition, e.g. "FIRST TO 50 · 10:00 LIMIT".
    goal: String,
    result: Option<MatchResult>,
    sections: Vec<Section>,
    /// Players still choosing a Team.
    choosing: Vec<String>,
}

/// What the Scoreboard announces at Match end.
#[derive(PartialEq)]
struct MatchResult {
    next_in: u32,
    /// "Map - Game Mode" of the Rotation's next Match.
    next_up: String,
    xp: XpSummary,
}

/// What the Match paid the local player and where that leaves their Level.
#[derive(PartialEq)]
struct XpSummary {
    gained: u32,
    /// The Level at the start of the Match.
    from_level: u32,
    /// The Level now and the progress through it.
    progress: LevelProgress,
    /// The Weapons that earned Weapon XP this Match, most first.
    weapons: Vec<WeaponLine>,
}

/// What one Weapon earned this Match and where that leaves its Weapon Level.
#[derive(PartialEq)]
struct WeaponLine {
    name: String,
    gained: u32,
    /// The Weapon Level at the start of the Match.
    from_level: u32,
    progress: LevelProgress,
}

/// Most Weapons the end screen lists: it is a glance, not a report.
const MAX_WEAPON_LINES: usize = 3;

/// The Weapons that gained Weapon XP since the Match began, most gained first.
fn weapon_lines(profile: &Profile, match_xp: &MatchXp, data: &GameData) -> Vec<WeaponLine> {
    let levels = &data.progression.weapon_levels;
    let mut lines: Vec<WeaponLine> = profile
        .weapons
        .iter()
        .filter_map(|(weapon, record)| {
            let start = match_xp.start_weapon_xp.get(weapon).copied().unwrap_or(0);
            (record.xp > start).then(|| WeaponLine {
                name: data.weapon(*weapon).short_name().to_uppercase(),
                gained: record.xp - start,
                from_level: levels.level_for(start),
                progress: levels.progress(record.xp),
            })
        })
        .collect();
    lines.sort_by(|a, b| b.gained.cmp(&a.gained).then_with(|| a.name.cmp(&b.name)));
    lines.truncate(MAX_WEAPON_LINES);
    lines
}

/// `1234` as `1,234`.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[derive(PartialEq)]
struct Section {
    faction: String,
    /// Asset path of the Faction emblem.
    icon: String,
    score: u32,
    color: Color,
    rows: Vec<Row>,
}

#[derive(PartialEq)]
struct Row {
    name: String,
    kills: u32,
    deaths: u32,
    me: bool,
}

pub(super) fn spawn_scoreboard(mut commands: Commands) {
    commands.spawn((
        Name::new("Scoreboard"),
        ScoreboardRoot,
        DespawnOnExit(AppState::InGame),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        GlobalZIndex(10),
        Visibility::Hidden,
        children![(
            ScoreboardPanel,
            BackgroundColor(ui::PANEL),
            BorderColor::all(ui::PANEL_BORDER),
            Node {
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18.0)),
                row_gap: px(4.0),
                border: UiRect::all(px(1.0)),
                ..default()
            },
        )],
    ));
}

/// A row of a name column that takes the remaining width and two right-aligned stat columns.
fn columns(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    cells: [String; 3],
    color: Color,
    background: Color,
) {
    let [name, kills, deaths] = cells;
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(6.0), px(1.0)),
                ..default()
            },
            BackgroundColor(background),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                ui::label(fonts, &name, ROW_SIZE, color),
            ));
            for stat in [kills, deaths] {
                row.spawn((
                    Node {
                        width: px(STAT_WIDTH),
                        ..default()
                    },
                    ui::label(fonts, &stat, ROW_SIZE, color),
                    TextLayout::justify(Justify::Right),
                ));
            }
        });
}

/// The win condition line at the top of the Scoreboard: the score to reach (rounds in Search &
/// Destroy, which has no Match clock) and the time limit.
fn goal_text(state: &MatchState) -> String {
    if state.mode == GameModeId::SearchAndDestroy {
        return format!("FIRST TO {} ROUNDS", state.score_limit);
    }
    format!(
        "FIRST TO {} · {} LIMIT",
        state.score_limit,
        super::clock(state.time_limit_secs)
    )
}

fn build(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, assets: &AssetServer, board: &Board) {
    panel
        .spawn(Node {
            justify_content: JustifyContent::Center,
            margin: UiRect::bottom(px(6.0)),
            ..default()
        })
        .with_child(ui::label(fonts, &board.goal, SUBTITLE_SIZE, ui::ACCENT));
    if let Some(result) = &board.result {
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(px(10.0)),
                ..default()
            })
            .with_children(|header| {
                header.spawn(ui::label(
                    fonts,
                    &format!("Next: {}", result.next_up),
                    SUBTITLE_SIZE + 2.0,
                    ui::TEXT,
                ));
                header.spawn(ui::label(
                    fonts,
                    &format!("Next match in {}", result.next_in),
                    SUBTITLE_SIZE,
                    ui::TEXT_DIM,
                ));
            });
    }
    for section in &board.sections {
        panel
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10.0),
                    margin: UiRect::top(px(10.0)),
                    border: UiRect::bottom(px(1.0)),
                    padding: UiRect::axes(px(6.0), px(3.0)),
                    ..default()
                },
                BorderColor::all(section.color),
            ))
            .with_children(|title| {
                title.spawn((
                    ImageNode::new(assets.load(section.icon.clone())),
                    Node {
                        width: px(SECTION_EMBLEM_SIZE),
                        height: px(SECTION_EMBLEM_SIZE),
                        ..default()
                    },
                ));
                title
                    .spawn((
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        ui::title(fonts, &section.faction, SECTION_SIZE),
                    ))
                    .insert(TextColor(section.color));
                title
                    .spawn(ui::title(fonts, &section.score.to_string(), SECTION_SIZE))
                    .insert(TextColor(section.color));
            });
        columns(
            panel,
            fonts,
            ["Name".into(), "K".into(), "D".into()],
            ui::TEXT_DIM,
            Color::NONE,
        );
        for row in &section.rows {
            columns(
                panel,
                fonts,
                [
                    row.name.clone(),
                    row.kills.to_string(),
                    row.deaths.to_string(),
                ],
                ui::TEXT,
                if row.me { SELF_BG } else { Color::NONE },
            );
        }
    }
    if !board.choosing.is_empty() {
        panel.spawn((
            Node {
                margin: UiRect::top(px(10.0)),
                ..default()
            },
            ui::label(
                fonts,
                &format!("Choosing a team: {}", board.choosing.join(", ")),
                ROW_SIZE,
                ui::TEXT_DIM,
            ),
        ));
    }
    if let Some(result) = &board.result {
        build_xp_summary(panel, fonts, &result.xp);
    }
}

/// XP gained, the Level reached and a bar of the progress through it.
fn build_xp_summary(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, xp: &XpSummary) {
    let progress = xp.progress;
    let level = if progress.level > xp.from_level {
        format!("LEVEL {} > {}  LEVEL UP", xp.from_level, progress.level)
    } else {
        format!("LEVEL {}", progress.level)
    };
    let remaining = if progress.maxed() {
        "MAX LEVEL".to_owned()
    } else {
        format!(
            "{} / {} XP",
            thousands(progress.into_level),
            thousands(progress.span)
        )
    };
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4.0),
            margin: UiRect::top(px(14.0)),
            padding: UiRect::top(px(8.0)),
            border: UiRect::top(px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(ui::PANEL_BORDER))
        .with_children(|summary| {
            summary
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|line| {
                    line.spawn(ui::label(
                        fonts,
                        &format!("XP EARNED  +{}", thousands(xp.gained)),
                        SECTION_SIZE - 4.0,
                        ui::ACCENT,
                    ));
                    line.spawn(ui::label(fonts, &level, SECTION_SIZE - 4.0, ui::TEXT));
                });
            summary
                .spawn((
                    Node {
                        height: px(XP_BAR_HEIGHT),
                        ..default()
                    },
                    BackgroundColor(XP_BAR_BACK),
                ))
                .with_child((
                    Node {
                        width: percent(progress.fraction() * 100.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(ui::ACCENT),
                ));
            summary.spawn(ui::label(fonts, &remaining, ROW_SIZE, ui::TEXT_DIM));
            for line in &xp.weapons {
                let level = if line.progress.level > line.from_level {
                    format!("LEVEL {} > {}", line.from_level, line.progress.level)
                } else {
                    format!("LEVEL {}", line.progress.level)
                };
                let next = if line.progress.maxed() {
                    "MAX".to_owned()
                } else {
                    format!("{} / {} XP", line.progress.into_level, line.progress.span)
                };
                summary
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(ui::label(
                            fonts,
                            &format!("{}  +{} XP", line.name, line.gained),
                            ROW_SIZE,
                            ui::TEXT,
                        ));
                        row.spawn(ui::label(
                            fonts,
                            &format!("{level}  {next}"),
                            ROW_SIZE,
                            ui::TEXT_DIM,
                        ));
                    });
            }
        });
}

/// Shows the Scoreboard while the key is held or the Match has ended, rebuilding its rows
/// only when what they show changes.
pub(super) fn update_scoreboard(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    life: Res<LocalLife>,
    data: Res<GameData>,
    maps: Res<MapRegistry>,
    fonts: Res<UiFonts>,
    assets: Res<AssetServer>,
    state: MatchQuery,
    profile: Res<Profile>,
    match_xp: Res<MatchXp>,
    players: Query<
        (
            &PlayerName,
            &PlayerTeam,
            &PlayerStats,
            Has<LocalPlayerReplica>,
        ),
        With<PlayerOwner>,
    >,
    mut root: Single<&mut Visibility, With<ScoreboardRoot>>,
    panel: Single<(Entity, Has<Children>), With<ScoreboardPanel>>,
    mut shown: Local<Board>,
    mut commands: Commands,
) {
    let Ok(state) = state.single() else {
        root.set_if_neq(Visibility::Hidden);
        return;
    };
    let ended = match state.phase {
        MatchPhase::Ended {
            winner,
            next_in_secs,
        } => Some((winner, next_in_secs)),
        MatchPhase::Playing => None,
    };
    let open = ended.is_some() || bindings.scoreboard.pressed(&keys);
    root.set_if_neq(if open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !open {
        return;
    }

    let ours = own_side(life.team());
    let result = ended.map(|(_, next_in)| MatchResult {
        xp: XpSummary {
            gained: match_xp.gained,
            from_level: data.progression.levels.level_for(match_xp.start_xp),
            progress: profile.level_progress(&data),
            weapons: weapon_lines(&profile, &match_xp, &data),
        },
        next_in,
        next_up: format!(
            "{} - {}",
            maps.get(&state.next.0)
                .map_or_else(|| state.next.0.to_string(), |info| info.name.clone()),
            state.next.1.name()
        ),
    });
    let display_name = |name: &PlayerName| {
        if name.0.is_empty() {
            "?".to_owned()
        } else {
            name.0.clone()
        }
    };
    let sections = [(ours, FRIENDLY_COLOR), (ours.other(), ENEMY_COLOR)]
        .into_iter()
        .map(|(team, color)| {
            let mut rows: Vec<Row> = players
                .iter()
                .filter(|(_, player_team, ..)| player_team.0 == Some(team))
                .map(|(name, _, stats, me)| Row {
                    name: display_name(name),
                    kills: stats.kills,
                    deaths: stats.deaths,
                    me,
                })
                .collect();
            rows.sort_by(|a, b| {
                Reverse(a.kills)
                    .cmp(&Reverse(b.kills))
                    .then(a.deaths.cmp(&b.deaths))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let faction = data.faction(&maps, &state.map, team);
            Section {
                faction: faction.name.clone(),
                icon: faction.icon.clone(),
                score: state.scores[team.index()],
                color,
                rows,
            }
        })
        .collect();
    let mut choosing: Vec<String> = players
        .iter()
        .filter(|(_, team, ..)| team.0.is_none())
        .map(|(name, ..)| display_name(name))
        .collect();
    choosing.sort();
    let board = Board {
        goal: goal_text(state),
        result,
        sections,
        choosing,
    };
    let (panel, built) = *panel;
    // A fresh panel (new session) is empty even when the board matches the last one shown.
    if built && *shown == board {
        return;
    }
    commands
        .entity(panel)
        .despawn_related::<Children>()
        .with_children(|panel| build(panel, &fonts, &assets, &board));
    *shown = board;
}
