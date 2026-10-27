//! Lives ended or cut short by something other than a shot (server): fall damage, early
//! respawn requests and the pause menu's RESPAWN.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::health::{LastDamaged, PendingRespawn, end_life};
use super::hitbox::Hitboxes;
use super::match_flow::is_live;
use crate::data::{GameData, GameModeId};
use crate::net::LinkPlayer;
use crate::net::protocol::{
    DeathCause, EventChannel, FallDamage, ForceRespawn, KillFeed, RequestRespawn,
};
use crate::shared::{Health, MatchState, PlayerName, PlayerPose, PlayerStats, PlayerTeam};

type KillFeeds<'w, 's> =
    Query<'w, 's, &'static mut MessageSender<KillFeed>, (With<ClientOf>, With<Connected>)>;

fn announce(feeds: &mut KillFeeds, victim: &PlayerName, cause: DeathCause) {
    let line = KillFeed {
        victim: victim.0.clone(),
        cause,
    };
    for mut sender in feeds {
        sender.send::<EventChannel>(line.clone());
    }
}

/// Applies each client's reported landing: the Match Rules' fall damage, with no attacker. A
/// lethal fall ends the life like any other death and the kill feed says so. Clients are
/// trusted (docs/adr/0002), so the reported height is taken as is.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_falls(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<FallDamage>)>,
    mut players: Query<(
        &mut Health,
        &mut LastDamaged,
        &mut PlayerStats,
        &PlayerName,
        &PlayerTeam,
        &PlayerPose,
        &Hitboxes,
    )>,
    mut feeds: KillFeeds,
    state: Single<&MatchState>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let rules = data.rules(&state.rules);
    for (player, mut receiver) in &mut links {
        for FallDamage { height } in receiver.receive() {
            let Ok((mut health, mut last_damaged, mut stats, name, team, pose, hitboxes)) =
                players.get_mut(player.0)
            else {
                continue;
            };
            if !is_live(&state) || health.is_dead() || team.0.is_none() {
                continue;
            }
            let damage = rules.fall_damage.damage(height, rules.max_health);
            if damage <= 0.0 {
                continue;
            }
            health.0 = (health.0 - damage).max(0.0);
            last_damaged.0 = time.elapsed();
            info!(
                "\"{}\" fell {height:.1} m for {damage:.1} damage, health {:.1}",
                name.0, health.0
            );
            if !health.is_dead() {
                continue;
            }
            end_life(
                &mut commands,
                &data,
                &time,
                &state,
                player.0,
                hitboxes,
                &mut stats,
                Some(pose.position),
            );
            info!("\"{}\" fell to their death", name.0);
            announce(&mut feeds, name, DeathCause::Fall);
        }
    }
}

/// Honours each dead player's request to respawn now once the Match Rules' minimum wait since
/// the death has passed; `respawn_players` still waits for the Game Mode to let them back.
pub(super) fn request_respawns(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<RequestRespawn>)>,
    mut players: Query<(&Health, &PlayerName, &mut PendingRespawn)>,
    time: Res<Time>,
) {
    for (player, mut receiver) in &mut links {
        for RequestRespawn in receiver.receive() {
            let Ok((health, name, mut pending)) = players.get_mut(player.0) else {
                continue;
            };
            if health.is_dead() && pending.request(time.elapsed()) {
                info!("\"{}\" asked to respawn early", name.0);
            }
        }
    }
}

/// Carries out the pause menu's RESPAWN for a living player: a death with no credit that counts
/// in their stats, then the usual respawn. Not in Search & Destroy, where the dead wait for the
/// next round.
#[allow(clippy::too_many_arguments)]
pub(super) fn force_respawns(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ForceRespawn>)>,
    mut players: Query<(
        &mut Health,
        &mut PlayerStats,
        &PlayerName,
        &PlayerTeam,
        &PlayerPose,
        &Hitboxes,
    )>,
    mut feeds: KillFeeds,
    state: Single<&MatchState>,
    data: Res<GameData>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (player, mut receiver) in &mut links {
        for ForceRespawn in receiver.receive() {
            let Ok((mut health, mut stats, name, team, pose, hitboxes)) = players.get_mut(player.0)
            else {
                continue;
            };
            if state.mode == GameModeId::SearchAndDestroy
                || !is_live(&state)
                || health.is_dead()
                || team.0.is_none()
            {
                continue;
            }
            health.0 = 0.0;
            end_life(
                &mut commands,
                &data,
                &time,
                &state,
                player.0,
                hitboxes,
                &mut stats,
                Some(pose.position),
            );
            info!("\"{}\" respawned from the pause menu", name.0);
            announce(&mut feeds, name, DeathCause::Respawn);
        }
    }
}
