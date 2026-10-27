//! The Match clock and phases (server): counts the time limit down, ends the Match at the score
//! or time limit, and after the end screen starts the next one with scores and stats reset.

use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::server::Started;
use lightyear::prelude::*;

use super::health::PendingRespawn;
use crate::data::{GameData, GameModeId};
use crate::maps::{ChangeMap, MapId, MapRegistry};
use crate::shared::{MatchPhase, MatchState, PlayerStats, PlayerTeam, Team};

/// The only Game Mode until Rotations exist.
const MODE: GameModeId = GameModeId::TeamDeathmatch;

/// The Match Rules preset this process's server runs (`--rules`), validated against
/// `GameData` at startup.
#[derive(Resource, Clone, Debug)]
pub struct ServerRules(pub String);

/// The Map this process's server starts its first Match on (`--map`), validated against the
/// `MapRegistry` at startup.
#[derive(Resource, Clone, Debug)]
pub struct ServerMap(pub MapId);

/// Exact seconds left in the current phase (playing or end screen); `MatchState` only carries
/// whole seconds, so it is written once per second.
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

/// Spawns the replicated `MatchState` entity of the first Match when a server starts.
pub(super) fn start_match(
    trigger: On<Add<Started>>,
    servers: Query<(), With<Server>>,
    data: Res<GameData>,
    rules: Res<ServerRules>,
    map: Res<ServerMap>,
    mut commands: Commands,
) {
    if !servers.contains(trigger.entity) {
        return;
    }
    let settings = data.game_mode(MODE);
    commands.insert_resource(MatchClock {
        left: settings.time_limit_secs as f32,
    });
    commands.spawn((
        Name::new("Match"),
        MatchState {
            mode: MODE,
            map: map.0.clone(),
            rules: rules.0.clone(),
            scores: [0; 2],
            score_limit: settings.score_limit,
            seconds_left: settings.time_limit_secs,
            phase: MatchPhase::Playing,
        },
        Replicate::to_clients(NetworkTarget::All),
    ));
    info!(
        "Match started ({MODE:?} on \"{}\", \"{}\" Match Rules)",
        map.0, rules.0
    );
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
    info!("Match ended: server stopped");
}

/// Advances the Match: ends it at the score or time limit, and restarts it after the end screen.
pub(super) fn tick_match(
    mut state: Single<&mut MatchState>,
    mut clock: ResMut<MatchClock>,
    mut players: Query<(Entity, &PlayerTeam, &mut PlayerStats)>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let settings = data.game_mode(state.mode);
    clock.left -= time.delta_secs();
    match state.phase {
        MatchPhase::Playing => {
            let decided = !is_live(&state);
            if decided || clock.left <= 0.0 {
                let winner = leader(state.scores);
                state.seconds_left = whole_secs(clock.left);
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
            } else if state.seconds_left != whole_secs(clock.left) {
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
            clock.left = settings.time_limit_secs as f32;
            **state = MatchState {
                mode: state.mode,
                map: state.map.clone(),
                rules: state.rules.clone(),
                scores: [0; 2],
                score_limit: settings.score_limit,
                seconds_left: settings.time_limit_secs,
                phase: MatchPhase::Playing,
            };
            for (entity, team, mut stats) in &mut players {
                *stats = PlayerStats::default();
                if team.0.is_some() {
                    commands
                        .entity(entity)
                        .insert(PendingRespawn::at(Duration::ZERO));
                }
            }
            info!("New Match started");
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
