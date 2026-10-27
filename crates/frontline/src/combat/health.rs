//! Health, regen, death and respawn of server Players.

use std::time::Duration;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::damage::{HitZone, shot_damage};
use super::hitbox::{Hitboxes, set_hitboxes_enabled, spawn_hitboxes};
use super::match_flow::is_live;
use super::mode_systems::{ModeRuntime, UseHeld};
use super::packs::drop_ammo_pack;
use super::shots::PlayerHit;
use super::spawn::{bias_toward, choose_spawn};
use super::xp::{DamageLedger, KillChain, XpOut};
use crate::data::{GameData, GameModeId, MatchRules};
use crate::gear::EquippedPerks;
use crate::maps::{MapSpawn, MapSpawns};
use crate::net::protocol::{
    Damaged, DeathCause, EventChannel, HitConfirm, KillFeed, PlayerOwner, Respawn,
};
use crate::progression::XpReason;
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
pub(super) struct LastDamaged(pub(super) Duration);

/// The player is out of play (dead, or just picked a Team) and respawns at `at` (elapsed time),
/// or sooner if they ask once `earliest` has passed.
#[derive(Component, Debug)]
pub(super) struct PendingRespawn {
    at: Duration,
    earliest: Duration,
}

impl PendingRespawn {
    /// Respawns at `at`, no sooner: a first spawn, a new round or a new Match.
    pub(super) fn at(at: Duration) -> Self {
        Self { at, earliest: at }
    }

    /// The wait after a death at `now`: the Match Rules' respawn delay, which the player can cut
    /// short after `min_respawn_secs`.
    pub(super) fn after_death(now: Duration, rules: &MatchRules) -> Self {
        Self {
            at: now + Duration::from_secs_f32(rules.respawn_delay_secs),
            earliest: now + Duration::from_secs_f32(rules.early_respawn_secs()),
        }
    }

    /// The player asked to respawn at `now`: honoured once the minimum wait has passed.
    pub(super) fn request(&mut self, now: Duration) -> bool {
        let honoured = now >= self.earliest;
        if honoured {
            self.at = self.at.min(now);
        }
        honoured
    }
}

/// Ends the life of `entity`, whoever or whatever caused it: its ammo pack drops, the death is
/// counted, its hitboxes go off and it waits to respawn (Search & Destroy has no respawns: the
/// dead wait for the next round, which sends everyone back). The caller zeroes `Health` and
/// announces the death.
#[allow(clippy::too_many_arguments)]
pub(super) fn end_life(
    commands: &mut Commands,
    data: &GameData,
    time: &Time,
    state: &MatchState,
    entity: Entity,
    hitboxes: &Hitboxes,
    stats: &mut PlayerStats,
    position: Option<Vec3>,
) {
    if let Some(position) = position {
        drop_ammo_pack(commands, data, time, position);
    }
    stats.deaths += 1;
    set_hitboxes_enabled(commands, hitboxes, false);
    if state.mode != GameModeId::SearchAndDestroy {
        commands.entity(entity).insert(PendingRespawn::after_death(
            time.elapsed(),
            &state.rule_values,
        ));
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
    commands.entity(add.entity).insert((
        LastDamaged(Duration::ZERO),
        UseHeld::default(),
        DamageLedger::default(),
        KillChain::default(),
    ));
}

/// Applies each `PlayerHit` while the Match is live: damage, hit/damage feedback and, on a
/// kill, stats, team score, kill feed and XP (the kill, headshot, Multikill, Flag-defend and
/// Assist awards, `xp`). Teammates are immune unless the Match Rules allow friendly fire.
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
    perks: Query<&EquippedPerks>,
    poses: Query<&PlayerPose>,
    mut ledgers: Query<&mut DamageLedger>,
    mut chains: Query<&mut KillChain>,
    mut xp: XpOut,
    runtime: Option<Res<ModeRuntime>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let rules = state.rule_values.clone();
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
        let effects = perks
            .get(hit.shooter)
            .map(|perks| data.perk_effects(&perks.0))
            .unwrap_or_default();
        let amount = shot_damage(
            data.weapon(hit.weapon),
            &data.attachment_mods(hit.attachments),
            &effects,
            hit.distance,
            hit.zone,
            &rules,
        );
        let applied = amount.min(health.0);
        health.0 = (health.0 - amount).max(0.0);
        last_damaged.0 = time.elapsed();
        if enemy && let Ok(mut ledger) = ledgers.get_mut(hit.victim) {
            ledger.record(hit.shooter, applied);
        }
        let kill = health.is_dead();
        info!(
            "{shooter_name} hit {} in the {} for {amount:.1} ({:?}, {:.1} m, Perks x{:.2} damage), health {:.1}",
            victim_name.0, hit.zone, hit.weapon, hit.distance, effects.damage_mul, health.0
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

        end_life(
            &mut commands,
            &data,
            &time,
            &state,
            hit.victim,
            hitboxes,
            &mut stats,
            poses.get(hit.victim).ok().map(|pose| pose.position),
        );
        let feed = KillFeed {
            victim: victim_name.0.clone(),
            cause: DeathCause::Shot {
                killer: shooter_name.clone(),
                weapon: hit.weapon,
                headshot,
            },
        };
        // The life is over: its damage ledger pays out (to enemies only) and the victim's own
        // Multikill chain ends.
        let assisters = ledgers
            .get_mut(hit.victim)
            .map(|mut ledger| ledger.take_assisters(hit.shooter))
            .unwrap_or_default();
        if let Ok(mut chain) = chains.get_mut(hit.victim) {
            chain.reset();
        }
        if enemy {
            let tuning = &data.progression.xp;
            xp.give(hit.shooter, tuning.kill, XpReason::Kill(hit.weapon));
            if headshot {
                xp.give(hit.shooter, tuning.headshot, XpReason::Headshot(hit.weapon));
            }
            if let Ok(mut chain) = chains.get_mut(hit.shooter) {
                let count = chain.record(time.elapsed(), tuning.multikill_window_secs);
                if let Some(bonus) = tuning.multikill_bonus(count) {
                    xp.give(hit.shooter, bonus, XpReason::Multikill(count));
                }
            }
            if let (Some(team), Ok(pose), Some(runtime)) =
                (shooter_team, poses.get(hit.victim), runtime.as_deref())
                && runtime.flag_held_at(team, pose.position, &data.domination().capture)
            {
                xp.give(hit.shooter, tuning.flag_defend_kill, XpReason::FlagDefend);
            }
            for (assister, damage) in assisters {
                if let Some(amount) = tuning.assist(damage) {
                    xp.give(assister, amount, XpReason::Assist);
                }
            }
        }
        if enemy && let Ok((_, _, mut killer_stats, ..)) = players.get_mut(hit.shooter) {
            killer_stats.kills += 1;
            // Only Team Deathmatch scores for kills; the other modes score their objective.
            if state.mode == GameModeId::TeamDeathmatch
                && let Some(team) = shooter_team
            {
                state.scores[team.index()] += 1;
                info!(
                    "{team:?} scores: Alpha {} - Bravo {}",
                    state.scores[Team::Alpha.index()],
                    state.scores[Team::Bravo.index()]
                );
            }
        }
        info!(
            "{shooter_name} killed {}{}",
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
    time: Res<Time>,
) {
    let rules = &state.rule_values;
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
    mut ledgers: Query<&mut DamageLedger>,
    spawns: Res<MapSpawns>,
    runtime: Res<ModeRuntime>,
    spatial: SpatialQuery,
    state: Single<&MatchState>,
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
        if pending.at > now || !runtime.respawn_open(team) {
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
        let pool = if free.is_empty() { &usable } else { &free };
        // Domination: spawn near the Flags the team holds.
        let positions: Vec<Vec3> = pool.iter().map(|spawn| spawn.position).collect();
        let candidates: Vec<&MapSpawn> = bias_toward(&positions, &runtime.spawn_anchors(team))
            .into_iter()
            .map(|index| pool[index])
            .collect();
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
        health.0 = state.rule_values.max_health;
        // A new life: damage dealt to the old one no longer pays Assists.
        if let Ok(mut ledger) = ledgers.get_mut(entity) {
            ledger.clear();
        }
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
