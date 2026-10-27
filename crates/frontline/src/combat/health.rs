//! Health, regen, death and respawn of server Players.

use std::time::Duration;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::damage::{HitZone, shot_damage};
use super::hitbox::{Hitboxes, set_hitboxes_enabled, spawn_hitboxes};
use super::match_flow::is_live;
use super::shots::PlayerHit;
use super::spawn::choose_spawn;
use crate::data::GameData;
use crate::maps::{MapSpawn, MapSpawns};
use crate::net::protocol::{Damaged, EventChannel, HitConfirm, KillFeed, PlayerOwner, Respawn};
use crate::shared::{
    GameLayer, Health, MatchState, PlayerName, PlayerPose, PlayerStats, PlayerTeam, Stance, Team,
};

/// A respawned client's poses are accepted again once one lands this close to the spawn.
const RESPAWN_ACCEPT_RADIUS: f32 = 2.0;
/// A spawn point with a living player within this horizontal distance is skipped while any
/// other is free, so teammates respawning together don't stack.
const SPAWN_CLEARANCE: f32 = 1.5;

/// When the player last took damage (elapsed time); regen waits `regen_delay_secs` after it.
#[derive(Component, Debug)]
pub(super) struct LastDamaged(Duration);

/// The player is out of play (dead, or just picked a Team) and respawns at `at` (elapsed time).
#[derive(Component, Debug)]
pub(super) struct PendingRespawn {
    at: Duration,
}

impl PendingRespawn {
    pub(super) fn at(at: Duration) -> Self {
        Self { at }
    }
}

/// The server just placed this player at `spawn` and ignores the owner's `PoseUpdate`s until
/// one lands within [`RESPAWN_ACCEPT_RADIUS`] of it, so stale poses can't drag it back.
#[derive(Component, Debug)]
pub struct PoseGate {
    spawn: Vec3,
}

impl PoseGate {
    /// Whether a reported pose at `position` lifts the gate.
    pub fn releases(&self, position: Vec3) -> bool {
        position.distance(self.spawn) <= RESPAWN_ACCEPT_RADIUS
    }
}

/// Gives each new server Player its (disabled) hitboxes. It stays out of play, dead and
/// unseen, until it picks a Team (`teams::choose_teams`).
pub(super) fn init_player(
    add: On<Add<PlayerOwner>>,
    players: Query<(), With<ControlledBy>>,
    mut commands: Commands,
) {
    if !players.contains(add.entity) {
        return;
    }
    spawn_hitboxes(&mut commands, add.entity);
    commands
        .entity(add.entity)
        .insert(LastDamaged(Duration::ZERO));
}

/// Applies each `PlayerHit` while the Match is live: damage, hit/damage feedback and, on a
/// kill, stats, team score and kill feed. Teammates are immune unless the Match Rules allow
/// friendly fire.
pub(super) fn apply_hits(
    mut hits: MessageReader<PlayerHit>,
    mut players: Query<(
        &mut Health,
        &mut LastDamaged,
        &mut PlayerStats,
        &PlayerName,
        &PlayerTeam,
        &ControlledBy,
        &Hitboxes,
    )>,
    mut state: Single<&mut MatchState>,
    mut hit_confirms: Query<&mut MessageSender<HitConfirm>>,
    mut damaged: Query<&mut MessageSender<Damaged>>,
    mut kill_feeds: Query<&mut MessageSender<KillFeed>, (With<ClientOf>, With<Connected>)>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let rules = data.rules(&state.rules);
    for hit in hits.read() {
        if !is_live(&state) {
            continue;
        }
        let Ok((shooter_name, shooter_team, shooter_link)) = players
            .get(hit.shooter)
            .map(|(_, _, _, name, team, link, _)| (name.0.clone(), team.0, link.owner))
        else {
            continue;
        };
        let Ok((
            mut health,
            mut last_damaged,
            mut stats,
            victim_name,
            victim_team,
            victim_link,
            hitboxes,
        )) = players.get_mut(hit.victim)
        else {
            continue;
        };
        if health.is_dead() {
            continue;
        }
        let enemy = hit.shooter != hit.victim && shooter_team != victim_team.0;
        if !enemy && hit.shooter != hit.victim && !rules.friendly_fire {
            debug!(
                "{shooter_name} hit teammate {}: no friendly fire",
                victim_name.0
            );
            continue;
        }
        let headshot = hit.zone == HitZone::Head;
        let amount = shot_damage(data.weapon(hit.weapon), hit.distance, hit.zone, rules);
        health.0 = (health.0 - amount).max(0.0);
        last_damaged.0 = time.elapsed();
        let kill = health.is_dead();
        info!(
            "{shooter_name} hit {} in the {} for {amount:.1} ({:?}, {:.1} m), health {:.1}",
            victim_name.0, hit.zone, hit.weapon, hit.distance, health.0
        );
        if let Ok(mut sender) = damaged.get_mut(victim_link.owner) {
            sender.send::<EventChannel>(Damaged {
                amount,
                from: hit.origin,
            });
        }
        if let Ok(mut sender) = hit_confirms.get_mut(shooter_link) {
            sender.send::<EventChannel>(HitConfirm { headshot, kill });
        }
        if !kill {
            continue;
        }

        stats.deaths += 1;
        set_hitboxes_enabled(&mut commands, hitboxes, false);
        commands.entity(hit.victim).insert(PendingRespawn::at(
            time.elapsed() + Duration::from_secs_f32(rules.respawn_delay_secs),
        ));
        let feed = KillFeed {
            killer: shooter_name,
            victim: victim_name.0.clone(),
            weapon: hit.weapon,
            headshot,
        };
        if enemy && let Ok((_, _, mut killer_stats, ..)) = players.get_mut(hit.shooter) {
            killer_stats.kills += 1;
            if let Some(team) = shooter_team {
                state.scores[team.index()] += 1;
                info!(
                    "{team:?} scores: Alpha {} - Bravo {}",
                    state.scores[Team::Alpha.index()],
                    state.scores[Team::Bravo.index()]
                );
            }
        }
        info!(
            "{} killed {}{}",
            feed.killer,
            feed.victim,
            if headshot { " (headshot)" } else { "" }
        );
        for mut sender in &mut kill_feeds {
            sender.send::<EventChannel>(feed.clone());
        }
    }
}

/// Heals living players `regen_per_sec` once `regen_delay_secs` have passed without damage.
pub(super) fn regenerate(
    mut players: Query<(&mut Health, &LastDamaged)>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    time: Res<Time>,
) {
    let rules = data.rules(&state.rules);
    let delay = Duration::from_secs_f32(rules.regen_delay_secs);
    for (mut health, last_damaged) in &mut players {
        if health.is_dead()
            || health.0 >= rules.max_health
            || time.elapsed().saturating_sub(last_damaged.0) < delay
        {
            continue;
        }
        health.0 = (health.0 + rules.regen_per_sec * time.delta_secs()).min(rules.max_health);
    }
}

/// Brings due players on a Team back: picks a spawn away from the other team and not on top of
/// anyone, restores health, places them and tells the owner. Players placed earlier in the same
/// tick count as living, so a whole team respawning at once spreads out.
pub(super) fn respawn_players(
    mut players: Query<(
        Entity,
        &mut Health,
        &mut PlayerPose,
        &PlayerTeam,
        Has<PendingRespawn>,
    )>,
    respawning: Query<(
        Entity,
        &PendingRespawn,
        &PlayerName,
        &PlayerTeam,
        &PlayerOwner,
        &ControlledBy,
        &Hitboxes,
    )>,
    mut senders: Query<&mut MessageSender<Respawn>>,
    spawns: Res<MapSpawns>,
    spatial: SpatialQuery,
    state: Single<&MatchState>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let now = time.elapsed();
    let world_only = SpatialQueryFilter::from_mask(GameLayer::World);
    let standing_eye = Vec3::Y * Stance::Standing.eye_height();
    // Eye position and team of every living player.
    let mut living: Vec<(Vec3, Option<Team>)> = players
        .iter()
        .filter(|(_, health, _, _, pending)| !health.is_dead() && !pending)
        .map(|(_, _, pose, team, _)| (pose.position + Vec3::Y * pose.stance.eye_height(), team.0))
        .collect();

    for (entity, pending, name, team, owner, link, hitboxes) in &respawning {
        let Some(team) = team.0 else {
            continue;
        };
        if pending.at > now {
            continue;
        }
        let enemy_eyes: Vec<Vec3> = living
            .iter()
            .filter(|(_, other)| *other == Some(team.other()))
            .map(|(eye, _)| *eye)
            .collect();
        let usable = spawns.usable(Some(team), state.mode);
        let free: Vec<&MapSpawn> = usable
            .iter()
            .copied()
            .filter(|spawn| {
                living
                    .iter()
                    .all(|(eye, _)| eye.xz().distance(spawn.position.xz()) > SPAWN_CLEARANCE)
            })
            .collect();
        let candidates = if free.is_empty() { &usable } else { &free };
        let candidate_eyes: Vec<Vec3> = candidates
            .iter()
            .map(|spawn| spawn.position + standing_eye)
            .collect();
        let line_of_sight = |from: Vec3, to: Vec3| {
            let (direction, distance) = Dir3::new_and_length(to - from).unwrap_or((Dir3::Y, 0.0));
            spatial
                .cast_ray(from, direction, distance, true, &world_only)
                .is_none()
        };
        let Some(index) = choose_spawn(&candidate_eyes, &enemy_eyes, line_of_sight) else {
            warn!("No spawn points; cannot respawn {}", name.0);
            continue;
        };
        let MapSpawn { position, yaw, .. } = *candidates[index];
        living.push((position + standing_eye, Some(team)));

        let Ok((_, mut health, mut pose, ..)) = players.get_mut(entity) else {
            continue;
        };
        health.0 = data.rules(&state.rules).max_health;
        *pose = PlayerPose {
            position,
            yaw,
            pitch: 0.0,
            stance: Stance::Standing,
        };
        if let Ok(mut sender) = senders.get_mut(link.owner) {
            sender.send::<EventChannel>(Respawn { position, yaw });
        }
        set_hitboxes_enabled(&mut commands, hitboxes, true);
        commands
            .entity(entity)
            .remove::<PendingRespawn>()
            .insert(PoseGate { spawn: position });
        info!("Client {} \"{}\" spawned at {position}", owner.0, name.0);
    }
}
