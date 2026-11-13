//! The Scoreboard: held open with the scoreboard key, forced open at Match end (under the result
//! banner) with the next Rotation entry, the countdown to the next Match and the local player's
//! XP summary (XP gained, Level and progress bar). Our Team's section comes first; players are
//! sorted by score (all match points, as in CoD4) and the local player is highlighted.

use std::cmp::Reverse;

use bevy::prelude::*;
use bevy::text::LetterSpacing;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, own_side};
use crate::assets::EMBEDDED_ROOT;
use crate::data::{GameData, GameModeId};
use crate::flow::AppState;
use crate::maps::MapRegistry;
use crate::net::protocol::PlayerOwner;
use crate::player::LocalLife;
use crate::player::input::Bindings;
use crate::profile::Profile;
use crate::progression::{LevelProgress, MatchXp, rank};
use crate::shared::{
    LocalPlayerReplica, MatchPhase, MatchState, PlayerLevel, PlayerName, PlayerStats, PlayerTeam,
};
use crate::ui::{self, UiFonts};

const WIDTH: f32 = 640.0;
const TITLE_SIZE: f32 = 30.0;
const SUBTITLE_SIZE: f32 = 18.0;
const SECTION_SIZE: f32 = 26.0;
const SECTION_EMBLEM_SIZE: f32 = 32.0;
const ROW_SIZE: f32 = 19.0;
const HEADER_SIZE: f32 = 15.0;
const STAT_WIDTH: f32 = 72.0;
const INSIGNIA_SIZE: f32 = 24.0;
const LEVEL_WIDTH: f32 = 30.0;
/// The local player's row: an amber wash with an amber edge.
const SELF_BG: Color = Color::srgba(0.96, 0.68, 0.18, 0.2);
/// Every second row, so a long list is easy to follow across.
const STRIPE_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.045);
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
    level: u32,
    score: u32,
    kills: u32,
    deaths: u32,
    me: bool,
}

/// Best player first: most points, then most kills, then by name so the order is stable.
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        Reverse(a.score)
            .cmp(&Reverse(b.score))
            .then(Reverse(a.kills).cmp(&Reverse(b.kills)))
            .then_with(|| a.name.cmp(&b.name))
    });
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
            BorderColor {
                top: ui::AMBER,
                ..BorderColor::all(ui::PANEL_BORDER)
            },
            Node {
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20.0)),
                row_gap: px(3.0),
                border: UiRect {
                    top: px(3.0),
                    ..UiRect::all(px(1.0))
                },
                ..default()
            },
        )],
    ));
}

/// A row of a name column that takes the remaining width and three right-aligned stat columns. A
/// player's row (`level`) puts their rank insignia and Level before the name; the header row
/// leaves that space empty.
fn columns(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    assets: &AssetServer,
    level: Option<u32>,
    cells: [String; 4],
    color: Color,
    background: Color,
    edge: Color,
) {
    let [name, score, kills, deaths] = cells;
    let size = if level.is_some() {
        ROW_SIZE
    } else {
        HEADER_SIZE
    };
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(6.0), px(2.0)),
                border: UiRect::left(px(3.0)),
                ..default()
            },
            BackgroundColor(background),
            BorderColor::all(edge),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: px(INSIGNIA_SIZE + LEVEL_WIDTH + 12.0),
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                ..default()
            })
            .with_children(|lead| {
                let Some(level) = level else {
                    return;
                };
                lead.spawn((
                    ImageNode::new(
                        assets.load(format!("{EMBEDDED_ROOT}{}", rank::asset_path(level))),
                    ),
                    Node {
                        width: px(INSIGNIA_SIZE),
                        height: px(INSIGNIA_SIZE),
                        ..default()
                    },
                ));
                lead.spawn((
                    Node {
                        width: px(LEVEL_WIDTH),
                        ..default()
                    },
                    ui::label(fonts, &level.to_string(), ROW_SIZE, ui::ACCENT),
                    TextLayout::justify(Justify::Right),
                ));
            });
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                ui::label(fonts, &name, size, color),
                LetterSpacing::Px(if level.is_some() { 0.0 } else { 2.0 }),
            ));
            for stat in [score, kills, deaths] {
                row.spawn((
                    Node {
                        width: px(STAT_WIDTH),
                        ..default()
                    },
                    ui::label(fonts, &stat, size, color),
                    LetterSpacing::Px(if level.is_some() { 0.0 } else { 2.0 }),
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
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexEnd,
            ..default()
        })
        .with_children(|head| {
            head.spawn(ui::title(fonts, "Scoreboard", TITLE_SIZE));
            head.spawn((
                ui::label(fonts, &board.goal, SUBTITLE_SIZE, ui::ACCENT),
                LetterSpacing::Px(1.5),
            ));
        });
    panel.spawn(ui::rule(ui::AMBER));
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
                    margin: UiRect::top(px(12.0)),
                    border: UiRect::bottom(px(2.0)),
                    padding: UiRect::axes(px(8.0), px(4.0)),
                    ..default()
                },
                BackgroundColor(section.color.with_alpha(0.14)),
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
            assets,
            None,
            [
                "PLAYER".into(),
                "SCORE".into(),
                "KILLS".into(),
                "DEATHS".into(),
            ],
            ui::TEXT_DIM,
            Color::NONE,
            Color::NONE,
        );
        for (index, row) in section.rows.iter().enumerate() {
            let (background, edge) = if row.me {
                (SELF_BG, ui::AMBER)
            } else if index % 2 == 1 {
                (STRIPE_BG, Color::NONE)
            } else {
                (Color::NONE, Color::NONE)
            };
            columns(
                panel,
                fonts,
                assets,
                Some(row.level),
                [
                    row.name.clone(),
                    row.score.to_string(),
                    row.kills.to_string(),
                    row.deaths.to_string(),
                ],
                ui::TEXT,
                background,
                edge,
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
            Option<&PlayerLevel>,
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
                .map(|(name, _, stats, level, me)| Row {
                    name: display_name(name),
                    level: level.map_or(1, |level| level.0.max(1)),
                    score: stats.score,
                    kills: stats.kills,
                    deaths: stats.deaths,
                    me,
                })
                .collect();
            sort_rows(&mut rows);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, score: u32, kills: u32) -> Row {
        Row {
            name: name.into(),
            level: 1,
            score,
            kills,
            deaths: 0,
            me: false,
        }
    }

    #[test]
    fn rows_sort_by_score_then_kills_then_name() {
        let mut rows = vec![
            row("delta", 100, 1),
            row("bravo", 300, 2),
            row("alpha", 300, 2),
            row("charlie", 300, 5),
            row("echo", 0, 0),
        ];
        sort_rows(&mut rows);
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["charlie", "alpha", "bravo", "delta", "echo"]);
    }
}
