//! The Scoreboard: held open with the scoreboard key, forced open at Match end with the result
//! and the countdown to the next Match. Our Team's section comes first; players are sorted by
//! kills and the local player is highlighted.

use std::cmp::Reverse;

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, own_side};
use crate::data::GameData;
use crate::flow::AppState;
use crate::maps::MapRegistry;
use crate::net::protocol::PlayerOwner;
use crate::player::LocalLife;
use crate::player::input::Bindings;
use crate::shared::{LocalPlayerReplica, MatchPhase, PlayerName, PlayerStats, PlayerTeam};
use crate::ui::{self, UiFonts};

const WIDTH: f32 = 560.0;
const BANNER_SIZE: f32 = 56.0;
const WINNER_SIZE: f32 = 24.0;
const SUBTITLE_SIZE: f32 = 18.0;
const SECTION_SIZE: f32 = 24.0;
const SECTION_EMBLEM_SIZE: f32 = 30.0;
const ROW_SIZE: f32 = 18.0;
const STAT_WIDTH: f32 = 48.0;
const SELF_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
const DRAW_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);

#[derive(Component)]
pub(super) struct ScoreboardRoot;

/// The panel whose children are rebuilt whenever the board's content changes.
#[derive(Component)]
pub(super) struct ScoreboardPanel;

#[derive(PartialEq, Default)]
pub(super) struct Board {
    result: Option<MatchResult>,
    sections: Vec<Section>,
    /// Players still choosing a Team.
    choosing: Vec<String>,
}

/// What the Scoreboard announces at Match end.
#[derive(PartialEq)]
struct MatchResult {
    banner: &'static str,
    color: Color,
    /// Name of the winning Faction; `None` on a draw.
    winner: Option<String>,
    next_in: u32,
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

fn build(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, assets: &AssetServer, board: &Board) {
    if let Some(result) = &board.result {
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(px(10.0)),
                ..default()
            })
            .with_children(|header| {
                header
                    .spawn(ui::title(fonts, result.banner, BANNER_SIZE))
                    .insert(TextColor(result.color));
                if let Some(winner) = &result.winner {
                    header.spawn(ui::title(fonts, &format!("{winner} wins"), WINNER_SIZE));
                }
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
    let result = ended.map(|(winner, next_in)| {
        let (banner, color) = match winner {
            None => ("DRAW", DRAW_COLOR),
            Some(team) if team == ours => ("VICTORY", FRIENDLY_COLOR),
            Some(_) => ("DEFEAT", ENEMY_COLOR),
        };
        MatchResult {
            banner,
            color,
            winner: winner.map(|team| data.faction(&maps, &state.map, team).name.clone()),
            next_in,
        }
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
