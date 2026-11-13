//! XP awards (server): what each combat and objective event pays and the bookkeeping behind
//! Assists and Multikills. The numbers live in `data/progression.ron`; an award is a
//! `XpAward` message to the earning client, which stores it in its Profile (docs/adr/0002).

use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::*;

use crate::net::protocol::{EventChannel, XpAward};
use crate::progression::XpReason;
use crate::shared::{PlayerName, PlayerStats};

/// Damage each enemy dealt to this player during its current life, so the Assists can be paid
/// when someone kills it. Server-only.
#[derive(Component, Debug, Default)]
pub(super) struct DamageLedger(Vec<(Entity, f32)>);

impl DamageLedger {
    /// Adds `damage` dealt by `attacker`.
    pub(super) fn record(&mut self, attacker: Entity, damage: f32) {
        match self.0.iter_mut().find(|(entity, _)| *entity == attacker) {
            Some((_, total)) => *total += damage,
            None => self.0.push((attacker, damage)),
        }
    }

    /// Everyone who dealt damage except `killer`, and empties the ledger for the next life.
    pub(super) fn take_assisters(&mut self, killer: Entity) -> Vec<(Entity, f32)> {
        let mut dealt = std::mem::take(&mut self.0);
        dealt.retain(|(entity, _)| *entity != killer);
        dealt
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

/// A player's current run of kills in quick succession. Server-only.
#[derive(Component, Debug, Default)]
pub(super) struct KillChain {
    last_kill: Option<Duration>,
    count: u32,
}

impl KillChain {
    /// Counts a kill made at `now`: continues the chain if the previous one was within
    /// `window_secs`, else starts a new one. Returns the chain length (1 for a lone kill).
    pub(super) fn record(&mut self, now: Duration, window_secs: f32) -> u32 {
        let continues = self
            .last_kill
            .is_some_and(|last| now.saturating_sub(last).as_secs_f32() <= window_secs);
        self.count = if continues { self.count + 1 } else { 1 };
        self.last_kill = Some(now);
        self.count
    }

    /// The chain is over (its player died).
    pub(super) fn reset(&mut self) {
        *self = KillChain::default();
    }
}

/// Sends XP awards to the players who earned them and adds them to their Match score.
#[derive(SystemParam)]
pub(super) struct XpOut<'w, 's> {
    links: Query<'w, 's, &'static ControlledBy>,
    senders: Query<'w, 's, &'static mut MessageSender<XpAward>>,
    names: Query<'w, 's, &'static PlayerName>,
    // Score goes through Commands: the callers already hold `&mut PlayerStats` queries, and a
    // second mutable query here would make Bevy panic on a system param conflict.
    commands: Commands<'w, 's>,
}

impl XpOut<'_, '_> {
    /// Awards `amount` XP for `reason` to the client controlling `player`. The award is always
    /// sent, even at 0, because the client counts per-Weapon kills from it.
    pub(super) fn give(&mut self, player: Entity, amount: u32, reason: XpReason) {
        if reason.counts_for_score()
            && let Ok(mut entity) = self.commands.get_entity(player)
        {
            entity.queue_silenced(move |mut entity: EntityWorldMut| {
                if let Some(mut stats) = entity.get_mut::<PlayerStats>() {
                    stats.score += amount;
                }
            });
        }
        let Ok(link) = self.links.get(player) else {
            return;
        };
        let name = self.names.get(player).map_or("?", |name| name.0.as_str());
        info!("XP: {name} +{amount} ({})", reason.label());
        if let Ok(mut sender) = self.senders.get_mut(link.owner) {
            sender.send::<EventChannel>(XpAward { amount, reason });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_continues_inside_the_window_and_restarts_after() {
        let mut chain = KillChain::default();
        let at = |secs: f32| Duration::from_secs_f32(secs);
        assert_eq!(chain.record(at(10.0), 4.0), 1);
        assert_eq!(chain.record(at(12.0), 4.0), 2);
        assert_eq!(chain.record(at(15.5), 4.0), 3);
        assert_eq!(chain.record(at(20.0), 4.0), 1);
        chain.reset();
        assert_eq!(chain.record(at(20.5), 4.0), 1);
    }

    #[test]
    fn the_ledger_sums_per_attacker_and_leaves_the_killer_out() {
        let mut world = World::new();
        let (killer, helper) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut ledger = DamageLedger::default();
        ledger.record(helper, 30.0);
        ledger.record(killer, 60.0);
        ledger.record(helper, 20.0);
        assert_eq!(ledger.take_assisters(killer), [(helper, 50.0)]);
        // Taking it empties the ledger for the next life.
        assert!(ledger.take_assisters(killer).is_empty());
    }
}
