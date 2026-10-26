//! Team select ("choose your side"), shown while `Overlay::TeamSelect`: one card per Team with
//! its Faction's emblem, name and roster. Hover or 1/2 highlight a card; a click or Enter picks
//! it, AUTO-ASSIGN joins the smaller Team. Picking moves on to the Loadout screen. The flow
//! opens this overlay; CANCEL (shown once on a Team) and Esc close it, Esc going to the pause
//! menu while still without a Team.

use std::hash::{BuildHasher, RandomState};

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use super::{MatchQuery, PendingTeam, faction_color};
use crate::data::{Faction, GameData};
use crate::flow::Overlay;
use crate::net::protocol::{ChooseTeam, PlayerOwner};
use crate::shared::{LocalPlayerReplica, PlayerName, PlayerTeam, Team, ToServer};
use crate::ui::{self, UiFonts};

const HEADING_SIZE: f32 = 56.0;
const CARD_WIDTH: f32 = 360.0;
const CARD_HEIGHT: f32 = 440.0;
const EMBLEM_SIZE: f32 = 150.0;
const FACTION_NAME_SIZE: f32 = 32.0;
const HEADCOUNT_SIZE: f32 = 20.0;
const ROSTER_SIZE: f32 = 18.0;
const TAG_SIZE: f32 = 15.0;
const HINT_SIZE: f32 = 17.0;
/// Players listed per card; the rest are summed up as "+N MORE".
const LISTED_PLAYERS: usize = 8;
/// How far a highlighted card rises, in logical pixels.
const LIFT: f32 = 8.0;
/// How much of the Faction colour tints a highlighted card.
const HIGHLIGHT_TINT: f32 = 0.2;

/// Input state of the screen during one visit.
#[derive(Resource, Default)]
pub(super) struct TeamSelect {
    /// Card highlighted by hover or 1/2; Enter picks it (or auto-assigns when `None`).
    highlighted: Option<Team>,
    /// Choice clicked this frame.
    choice: Option<Choice>,
}

#[derive(Clone, Copy)]
enum Choice {
    Join(Team),
    AutoAssign,
    Cancel,
}

#[derive(Component)]
pub(super) struct TeamSelectRoot;

#[derive(Component)]
pub(super) struct TeamCard {
    team: Team,
    accent: Color,
}

/// "N PLAYERS" on a card.
#[derive(Component)]
pub(super) struct CardHeadcount(Team);

/// The first `LISTED_PLAYERS` names on a card.
#[derive(Component)]
pub(super) struct CardRoster(Team);

#[derive(Component)]
pub(super) struct RecommendedTag(Team);

/// Players per Team, optionally leaving out our own player.
fn team_counts(players: impl Iterator<Item = (Option<Team>, bool)>, skip_self: bool) -> [usize; 2] {
    let mut counts = [0; 2];
    for (team, is_self) in players {
        if let Some(team) = team
            && !(skip_self && is_self)
        {
            counts[team.index()] += 1;
        }
    }
    counts
}

/// The smaller Team; `None` on a tie.
fn smaller(counts: [usize; 2]) -> Option<Team> {
    match counts[0].cmp(&counts[1]) {
        std::cmp::Ordering::Less => Some(Team::Alpha),
        std::cmp::Ordering::Greater => Some(Team::Bravo),
        std::cmp::Ordering::Equal => None,
    }
}

/// The smaller Team, a coin flip on a tie.
fn auto_assign(counts: [usize; 2]) -> Team {
    smaller(counts).unwrap_or_else(|| {
        if RandomState::new().hash_one(()) & 1 == 0 {
            Team::Alpha
        } else {
            Team::Bravo
        }
    })
}

/// Builds the screen once the Match state (and so the Factions) is known.
pub(super) fn spawn_team_select(
    state: MatchQuery,
    data: Res<GameData>,
    fonts: Res<UiFonts>,
    assets: Res<AssetServer>,
    players: Query<(&PlayerTeam, Has<LocalPlayerReplica>), With<PlayerOwner>>,
    existing: Query<(), With<TeamSelectRoot>>,
    mut select: ResMut<TeamSelect>,
    mut commands: Commands,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok(state) = state.single() else {
        return;
    };
    let on_team = players
        .iter()
        .any(|(team, is_self)| is_self && team.0.is_some());
    *select = TeamSelect {
        highlighted: smaller(team_counts(players.iter().map(|(t, s)| (t.0, s)), true)),
        choice: None,
    };
    commands
        .spawn((
            Name::new("Team select"),
            TeamSelectRoot,
            ui::backdrop(),
            GlobalZIndex(20),
            DespawnOnExit(Overlay::TeamSelect),
        ))
        .with_children(|root| {
            root.spawn(ui::title(&fonts, "Choose your side", HEADING_SIZE));
            root.spawn(Node {
                column_gap: px(32.0),
                margin: UiRect::vertical(px(18.0)),
                ..default()
            })
            .with_children(|row| {
                for (slot, team) in Team::ALL.into_iter().enumerate() {
                    let faction = data.faction(state.mode, team);
                    let emblem = assets.load(faction.icon.clone());
                    spawn_card(row, &fonts, faction, emblem, slot + 1, team);
                }
            });
            root.spawn(Node {
                column_gap: px(16.0),
                ..default()
            })
            .with_children(|buttons| {
                buttons.spawn(ui::button(&fonts, "Auto-assign")).observe(
                    |_: On<Activate>, mut select: ResMut<TeamSelect>| {
                        select.choice = Some(Choice::AutoAssign);
                    },
                );
                if on_team {
                    buttons.spawn(ui::button(&fonts, "Cancel")).observe(
                        |_: On<Activate>, mut select: ResMut<TeamSelect>| {
                            select.choice = Some(Choice::Cancel);
                        },
                    );
                }
            });
            root.spawn(ui::label(
                &fonts,
                "1 / 2  SELECT      ENTER  CONFIRM",
                HINT_SIZE,
                ui::TEXT_DIM,
            ));
        });
}

fn spawn_card(
    row: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    faction: &Faction,
    emblem: Handle<Image>,
    key: usize,
    team: Team,
) {
    let accent = faction_color(faction);
    let corner = |left: bool| Node {
        position_type: PositionType::Absolute,
        top: px(10.0),
        left: if left { px(12.0) } else { Val::Auto },
        right: if left { Val::Auto } else { px(12.0) },
        padding: UiRect::axes(px(6.0), px(1.0)),
        ..default()
    };
    row.spawn((
        Name::new(format!("{team:?} card")),
        TeamCard { team, accent },
        Button,
        Node {
            width: px(CARD_WIDTH),
            height: px(CARD_HEIGHT),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::new(px(20.0), px(20.0), px(44.0), px(20.0)),
            row_gap: px(10.0),
            border: UiRect::all(px(2.0)),
            ..default()
        },
        BackgroundColor(ui::PANEL),
        BorderColor::all(accent),
        UiTransform::IDENTITY,
    ))
    .observe(move |_: On<PointerOver>, mut select: ResMut<TeamSelect>| {
        select.highlighted = Some(team);
    })
    .observe(move |_: On<Activate>, mut select: ResMut<TeamSelect>| {
        select.choice = Some(Choice::Join(team));
    })
    .with_children(|card| {
        card.spawn((
            corner(true),
            children![ui::label(
                fonts,
                &format!("[{key}]"),
                TAG_SIZE,
                ui::TEXT_DIM
            )],
        ));
        card.spawn((
            RecommendedTag(team),
            corner(false),
            BackgroundColor(accent.with_alpha(0.35)),
            Visibility::Hidden,
            children![ui::label(fonts, "RECOMMENDED", TAG_SIZE, ui::TEXT)],
        ));
        card.spawn((
            ImageNode::new(emblem),
            Node {
                width: px(EMBLEM_SIZE),
                height: px(EMBLEM_SIZE),
                ..default()
            },
        ));
        card.spawn(ui::title(fonts, &faction.name, FACTION_NAME_SIZE));
        card.spawn((
            Node {
                width: percent(100.0),
                height: px(2.0),
                ..default()
            },
            BackgroundColor(accent),
        ));
        card.spawn((
            CardHeadcount(team),
            ui::label(fonts, "", HEADCOUNT_SIZE, ui::ACCENT),
        ));
        card.spawn((
            CardRoster(team),
            ui::label(fonts, "", ROSTER_SIZE, ui::TEXT),
            TextLayout::justify(Justify::Center),
        ));
    });
}

/// Applies keys and clicks: picking a Team sends the choice and opens the Loadout screen.
pub(super) fn drive_team_select(
    keys: Res<ButtonInput<KeyCode>>,
    mut select: ResMut<TeamSelect>,
    mut pending: ResMut<PendingTeam>,
    players: Query<(&PlayerTeam, Has<LocalPlayerReplica>), With<PlayerOwner>>,
    mut choices: MessageWriter<ToServer<ChooseTeam>>,
    mut overlay: ResMut<NextState<Overlay>>,
) {
    if keys.any_just_pressed([KeyCode::Digit1, KeyCode::Numpad1]) {
        select.highlighted = Some(Team::Alpha);
    }
    if keys.any_just_pressed([KeyCode::Digit2, KeyCode::Numpad2]) {
        select.highlighted = Some(Team::Bravo);
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        select.choice = Some(select.highlighted.map_or(Choice::AutoAssign, Choice::Join));
    }
    if keys.just_pressed(KeyCode::Escape) {
        select.choice = Some(Choice::Cancel);
    }
    let Some(choice) = select.choice.take() else {
        return;
    };
    let current = players
        .iter()
        .find_map(|(team, is_self)| is_self.then_some(team.0))
        .flatten();
    let team = match choice {
        Choice::Join(team) => team,
        Choice::AutoAssign => auto_assign(team_counts(
            players.iter().map(|(team, is_self)| (team.0, is_self)),
            true,
        )),
        Choice::Cancel => {
            // Before choosing, Esc still reaches the pause menu (leave, settings); the flow
            // reopens this screen when the pause menu closes.
            overlay.set(if current.is_some() {
                Overlay::None
            } else {
                Overlay::Pause
            });
            return;
        }
    };
    if current != Some(team) {
        info!("Choosing team {team:?}");
        choices.write(ToServer(ChooseTeam(team)));
        pending.0 = Some(team);
    }
    overlay.set(Overlay::Loadout);
}

/// Keeps headcounts, rosters, the RECOMMENDED tag and the card highlight current.
pub(super) fn update_team_select(
    select: Res<TeamSelect>,
    players: Query<(&PlayerName, &PlayerTeam, Has<LocalPlayerReplica>), With<PlayerOwner>>,
    mut cards: Query<(&TeamCard, &mut BackgroundColor, &mut UiTransform)>,
    mut headcounts: Query<(&CardHeadcount, &mut Text), Without<CardRoster>>,
    mut rosters: Query<(&CardRoster, &mut Text), Without<CardHeadcount>>,
    mut tags: Query<(&RecommendedTag, &mut Visibility)>,
) {
    let entries = || players.iter().map(|(_, team, is_self)| (team.0, is_self));
    let counts = team_counts(entries(), false);
    let recommended = smaller(team_counts(entries(), true));

    for (card, mut background, mut transform) in &mut cards {
        let lit = select.highlighted == Some(card.team);
        background.set_if_neq(BackgroundColor(if lit {
            ui::PANEL.mix(&card.accent, HIGHLIGHT_TINT)
        } else {
            ui::PANEL
        }));
        let lift = if lit {
            Val2::px(0.0, -LIFT)
        } else {
            Val2::ZERO
        };
        if transform.translation != lift {
            transform.translation = lift;
        }
    }
    for (CardHeadcount(team), mut text) in &mut headcounts {
        let count = counts[team.index()];
        let value = format!("{count} PLAYER{}", if count == 1 { "" } else { "S" });
        if text.0 != value {
            text.0 = value;
        }
    }
    for (CardRoster(team), mut text) in &mut rosters {
        let mut names: Vec<&str> = players
            .iter()
            .filter(|(_, player_team, _)| player_team.0 == Some(*team))
            .map(|(name, ..)| {
                if name.0.is_empty() {
                    "?"
                } else {
                    name.0.as_str()
                }
            })
            .collect();
        names.sort_unstable();
        let mut value = names
            .iter()
            .take(LISTED_PLAYERS)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if names.len() > LISTED_PLAYERS {
            value.push_str(&format!("\n+{} MORE", names.len() - LISTED_PLAYERS));
        }
        if text.0 != value {
            text.0 = value;
        }
    }
    for (RecommendedTag(team), mut visibility) in &mut tags {
        visibility.set_if_neq(if recommended == Some(*team) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
