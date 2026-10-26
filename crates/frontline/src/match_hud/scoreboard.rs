//! The Scoreboard: held open with the scoreboard key, forced open at Match end with the result
//! and the countdown to the next Match. Our Team's section comes first; players are sorted by
//! kills and the local player is highlighted.

use std::cmp::Reverse;

use bevy::prelude::*;

use super::{ENEMY_COLOR, FRIENDLY_COLOR, MatchQuery, own_side};
use crate::net::protocol::PlayerOwner;
use crate::player::LocalLife;
use crate::player::input::Bindings;
use crate::shared::{LocalPlayerReplica, MatchPhase, PlayerName, PlayerStats, PlayerTeam, Team};

const WIDTH: f32 = 520.0;
const BANNER_SIZE: f32 = 40.0;
const SUBTITLE_SIZE: f32 = 18.0;
const SECTION_SIZE: f32 = 20.0;
const ROW_SIZE: f32 = 16.0;
const STAT_WIDTH: f32 = 48.0;
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
const SELF_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const DRAW_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);

#[derive(Component)]
pub(super) struct ScoreboardRoot;

/// The panel whose children are rebuilt whenever the board's content changes.
#[derive(Component)]
pub(super) struct ScoreboardPanel;

#[derive(PartialEq, Default)]
pub(super) struct Board {
    /// Result banner and seconds to the next Match, at Match end.
    result: Option<(&'static str, Color, u32)>,
    sections: Vec<Section>,
    /// Players still choosing a Team.
    choosing: Vec<String>,
}

#[derive(PartialEq)]
struct Section {
    team: Team,
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
            Node {
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16.0)),
                row_gap: px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.04, 0.82)),
        )],
    ));
}

fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// A row of a name column that takes the remaining width and two right-aligned stat columns.
fn columns(
    parent: &mut ChildSpawnerCommands,
    cells: [String; 3],
    size: f32,
    color: Color,
    background: Color,
) {
    let [name, kills, deaths] = cells;
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(6.0), px(2.0)),
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
                Text::new(name),
                font(size),
                TextColor(color),
            ));
            for stat in [kills, deaths] {
                row.spawn((
                    Node {
                        width: px(STAT_WIDTH),
                        ..default()
                    },
                    Text::new(stat),
                    font(size),
                    TextColor(color),
                    TextLayout::justify(Justify::Right),
                ));
            }
        });
}

fn build(panel: &mut ChildSpawnerCommands, board: &Board) {
    if let Some((banner, color, next_in)) = board.result {
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(px(10.0)),
                ..default()
            })
            .with_children(|header| {
                header.spawn((Text::new(banner), font(BANNER_SIZE), TextColor(color)));
                header.spawn((
                    Text::new(format!("Next match in {next_in}")),
                    font(SUBTITLE_SIZE),
                    TextColor(DIM),
                ));
            });
    }
    for section in &board.sections {
        panel
            .spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                margin: UiRect::top(px(8.0)),
                border: UiRect::bottom(px(1.0)),
                padding: UiRect::axes(px(6.0), px(2.0)),
                ..default()
            })
            .insert(BorderColor::all(section.color))
            .with_children(|title| {
                title.spawn((
                    Text::new(format!("{:?}", section.team)),
                    font(SECTION_SIZE),
                    TextColor(section.color),
                ));
                title.spawn((
                    Text::new(section.score.to_string()),
                    font(SECTION_SIZE),
                    TextColor(section.color),
                ));
            });
        columns(
            panel,
            ["Name".into(), "K".into(), "D".into()],
            ROW_SIZE,
            DIM,
            Color::NONE,
        );
        for row in &section.rows {
            columns(
                panel,
                [row.name.clone(), row.kills.to_string(), row.deaths.to_string()],
                ROW_SIZE,
                Color::WHITE,
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
            Text::new(format!("Choosing a team: {}", board.choosing.join(", "))),
            font(ROW_SIZE),
            TextColor(DIM),
        ));
    }
}

/// Shows the Scoreboard while the key is held or the Match has ended, rebuilding its rows
/// only when what they show changes.
pub(super) fn update_scoreboard(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    life: Res<LocalLife>,
    state: MatchQuery,
    players: Query<
        (&PlayerName, &PlayerTeam, &PlayerStats, Has<LocalPlayerReplica>),
        With<PlayerOwner>,
    >,
    mut root: Single<&mut Visibility, With<ScoreboardRoot>>,
    panel: Single<Entity, With<ScoreboardPanel>>,
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
        (banner, color, next_in)
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
            Section {
                team,
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
    if *shown == board {
        return;
    }
    commands
        .entity(*panel)
        .despawn_related::<Children>()
        .with_children(|panel| build(panel, &board));
    *shown = board;
}
