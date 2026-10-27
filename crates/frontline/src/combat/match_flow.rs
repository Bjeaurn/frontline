//! The Match clock and phases (server): counts the time limit down, ends the Match at the score
//! or time limit, and after the end screen starts the next one with scores and stats reset.

use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::server::Started;
use lightyear::prelude::*;

use super::health::PendingRespawn;
use super::mode_systems::ModeRuntime;
use crate::data::{GameData, GameModeId};
use crate::maps::{ChangeMap, MapRegistry};
use crate::modes::ModeView;
use crate::rotation::ServerConfig;
use crate::shared::{MatchPhase, MatchState, PlayerStats, PlayerTeam, Team};

/// Exact seconds left in the current phase (playing or end screen); `MatchState` only carries
/// whole seconds, so it is written once per second. Search & Destroy keeps its round clock in its
/// rules instead and has no Match clock.
#[derive(Resource, Debug)]
pub(super) struct MatchClock {
    left: f32,
}

/// Whether shots and damage count: the Match is playing and no team has reached the limit.
pub(super) fn is_live(state: &MatchState) -> bool {
    state.phase == MatchPhase::Playing && state.scores.iter().all(|&s| s < state.score_limit)
}

/// The team with the higher score, `None` on a tie.
fn leader(scores: [u32; 2]) -> Option<Team> {
    match scores[Team::Alpha.index()].cmp(&scores[Team::Bravo.index()]) {
        std::cmp::Ordering::Greater => Some(Team::Alpha),
        std::cmp::Ordering::Less => Some(Team::Bravo),
        std::cmp::Ordering::Equal => None,
    }
}

fn whole_secs(left: f32) -> u32 {
    left.max(0.0).ceil() as u32
}

/// The state of a fresh Match of the Rotation's current entry.
fn fresh_match(config: &ServerConfig, data: &GameData) -> MatchState {
    let entry = config.current();
    let (score_limit, time_limit_secs) = config.limits(data, entry.mode);
    let upcoming = config.upcoming();
    MatchState {
        mode: entry.mode,
        map: entry.map.clone(),
        rules: entry.rules.clone(),
        scores: [0; 2],
        score_limit,
        seconds_left: time_limit_secs,
        phase: MatchPhase::Playing,
        next: (upcoming.map.clone(), upcoming.mode),
    }
}

/// Spawns the replicated `MatchState` entity of the first Match when a server starts.
pub(super) fn start_match(
    trigger: On<Add<Started>>,
    servers: Query<(), With<Server>>,
    data: Res<GameData>,
    mut config: ResMut<ServerConfig>,
    mut commands: Commands,
) {
    if !servers.contains(trigger.entity) {
        return;
    }
    config.restart();
    let state = fresh_match(&config, &data);
    commands.insert_resource(MatchClock {
        left: state.seconds_left as f32,
    });
    commands.insert_resource(ModeRuntime::Pending);
    info!(
        "Match started ({:?} on \"{}\", \"{}\" Match Rules; next: {} - {})",
        state.mode,
        state.map,
        state.rules,
        state.next.0,
        state.next.1.name()
    );
    commands.spawn((
        Name::new("Match"),
        state,
        ModeView::None,
        Replicate::to_clients(NetworkTarget::All),
    ));
}

/// Drops the Match when its server stops (Host leaving).
pub(super) fn end_match(
    trigger: On<Remove<Started>>,
    servers: Query<(), With<Server>>,
    matches: Query<Entity, With<MatchState>>,
    mut commands: Commands,
) {
    if !servers.contains(trigger.entity) {
        return;
    }
    for entity in &matches {
        commands.entity(entity).try_despawn();
    }
    commands.remove_resource::<MatchClock>();
    commands.remove_resource::<ModeRuntime>();
    info!("Match ended: server stopped");
}

/// Advances the Match: ends it at the score or time limit, and after the end screen starts the
/// Rotation's next one (changing Map and Game Mode when its entry says so).
pub(super) fn tick_match(
    mut state: Single<&mut MatchState>,
    mut clock: ResMut<MatchClock>,
    mut config: ResMut<ServerConfig>,
    mut players: Query<(Entity, &PlayerTeam, &mut PlayerStats)>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let settings = data.game_mode(state.mode);
    // Search & Destroy runs on rounds: it has no Match clock.
    let timed = state.mode != GameModeId::SearchAndDestroy;
    if timed || state.phase != MatchPhase::Playing {
        clock.left -= time.delta_secs();
    }
    match state.phase {
        MatchPhase::Playing => {
            let decided = !is_live(&state);
            if decided || (timed && clock.left <= 0.0) {
                let winner = leader(state.scores);
                if timed {
                    state.seconds_left = whole_secs(clock.left);
                }
                state.phase = MatchPhase::Ended {
                    winner,
                    next_in_secs: settings.end_screen_secs,
                };
                clock.left = settings.end_screen_secs as f32;
                info!(
                    "Match over ({}): {}, Alpha {} - Bravo {}",
                    if decided { "score limit" } else { "time limit" },
                    winner.map_or_else(|| "draw".to_owned(), |team| format!("{team:?} wins")),
                    state.scores[Team::Alpha.index()],
                    state.scores[Team::Bravo.index()],
                );
            } else if timed && state.seconds_left != whole_secs(clock.left) {
                state.seconds_left = whole_secs(clock.left);
            }
        }
        MatchPhase::Ended { winner, .. } if clock.left > 0.0 => {
            let next_in_secs = whole_secs(clock.left);
            if state.phase
                != (MatchPhase::Ended {
                    winner,
                    next_in_secs,
                })
            {
                state.phase = MatchPhase::Ended {
                    winner,
                    next_in_secs,
                };
            }
        }
        MatchPhase::Ended { .. } => {
            config.advance();
            let next = fresh_match(&config, &data);
            clock.left = next.seconds_left as f32;
            if next.map != state.map {
                info!(
                    "Rotation: changing Map from \"{}\" to \"{}\"",
                    state.map, next.map
                );
            }
            info!(
                "New Match started ({:?} on \"{}\", \"{}\" Match Rules; next: {} - {})",
                next.mode,
                next.map,
                next.rules,
                next.next.0,
                next.next.1.name()
            );
            **state = next;
            // The new Match's objective is built once its Map is loaded.
            commands.insert_resource(ModeRuntime::Pending);
            for (entity, team, mut stats) in &mut players {
                *stats = PlayerStats::default();
                if team.0.is_some() {
                    commands
                        .entity(entity)
                        .insert(PendingRespawn::at(Duration::ZERO));
                }
            }
        }
    }
}

/// Carries out [`ChangeMap`]: names the new Map in `MatchState` (every process follows it) and
/// sends everyone on a Team to respawn, which waits until the new Map is ready.
pub(super) fn change_map(
    mut requests: MessageReader<ChangeMap>,
    mut state: Single<&mut MatchState>,
    registry: Res<MapRegistry>,
    players: Query<(Entity, &PlayerTeam)>,
    mut commands: Commands,
) {
    let Some(ChangeMap(map)) = requests.read().last() else {
        return;
    };
    let Some(info) = registry.get(map) else {
        error!(
            "Can't change to unknown Map \"{map}\"; known: {}",
            registry.known_ids()
        );
        return;
    };
    if !info.supports(state.mode) {
        error!(
            "Can't change to Map \"{map}\": it has no {:?} support",
            state.mode
        );
        return;
    }
    if state.map == *map {
        return;
    }
    info!("Changing Map from \"{}\" to \"{map}\"", state.map);
    state.map = map.clone();
    for (entity, team) in &players {
        if team.0.is_some() {
            commands
                .entity(entity)
                .insert(PendingRespawn::at(Duration::ZERO));
        }
    }
}
