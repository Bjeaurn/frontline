//! Team choice (server): applies each client's `ChooseTeam`.

use bevy::prelude::*;
use lightyear::prelude::*;

use super::health::PendingRespawn;
use super::hitbox::{Hitboxes, set_hitboxes_enabled};
use crate::data::GameData;
use crate::maps::MapRegistry;
use crate::net::protocol::ChooseTeam;
use crate::net::{LinkPlayer, SystemChat};
use crate::shared::{Health, MatchState, PlayerName, PlayerTeam};

/// Puts players on the team they ask for. A first choice spawns them right away; switching
/// while alive kills them without anyone getting credit, and they respawn after the usual delay.
pub(super) fn choose_teams(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ChooseTeam>)>,
    mut players: Query<(
        &mut PlayerTeam,
        &mut Health,
        &PlayerName,
        &Hitboxes,
        Has<PendingRespawn>,
    )>,
    state: Single<&MatchState>,
    data: Res<GameData>,
    maps: Res<MapRegistry>,
    time: Res<Time>,
    mut chat: SystemChat,
    mut commands: Commands,
) {
    for (player, mut receiver) in &mut links {
        for ChooseTeam(team) in receiver.receive() {
            let Ok((mut current, mut health, name, hitboxes, pending)) = players.get_mut(player.0)
            else {
                continue;
            };
            if current.0 == Some(team) {
                continue;
            }
            let previous = current.0.replace(team);
            let faction = &data.faction(&maps, &state.map, team).name;
            if previous.is_none() {
                info!("\"{}\" joined {team:?}", name.0);
                chat.say(format!("{} joined {faction}", name.0));
                commands
                    .entity(player.0)
                    .insert(PendingRespawn::at(time.elapsed()));
                continue;
            }
            info!("\"{}\" switched to {team:?}", name.0);
            chat.say(format!("{} switched to {faction}", name.0));
            if pending || health.is_dead() {
                continue;
            }
            health.0 = 0.0;
            set_hitboxes_enabled(&mut commands, hitboxes, false);
            commands
                .entity(player.0)
                .insert(PendingRespawn::after_death(
                    time.elapsed(),
                    &state.rule_values,
                ));
        }
    }
}
