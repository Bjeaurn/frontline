//! Workaround for a lightyear 0.31.0 bug that panics a Client after a long frame.
//!
//! Lightyear maps every received Replicon replication tick to the server's simulation tick in
//! `ReplicationCheckpointMap`, which keeps 256 entries and evicts the first-*inserted* one
//! (lightyear_replication `checkpoint.rs`, `ReplicationCheckpointMap::record`). Replicon applies
//! a backlog of buffered mutate messages newest-first. So when a Client stalls for more than 256
//! ticks (4 s at 64 Hz: a frozen first frame compiling Metal pipelines, which Bevy does
//! synchronously on macOS) while other players keep changing replicated state, the whole backlog
//! lands in one frame, the newest mappings are recorded first and evicted again. Replicon's
//! last confirmed mutate tick is the newest one, so lightyear's `sync_last_confirmed_checkpoint`
//! (`client.rs:106-113`) cannot resolve it and hits
//! `debug_assert!(false, "missing authoritative checkpoint mapping for completed mutate tick")`.
//! In release builds the newest checkpoint is silently lost instead.
//!
//! Upstream fix (evict the oldest tick instead of the first-inserted): lightyear PR #1807, closed
//! unmerged; nothing on lightyear `main` after 0.31.0 changes this. The workaround: remember the
//! newest mappings ourselves and put any that lightyear evicted straight back, so the newest
//! checkpoints always resolve. Remove this module once lightyear keeps the newest ticks itself;
//! the `lightyear_evicts_newest_ticks_of_a_backlog` test fails when it does.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_replicon::client::UserdataReceived;
use bevy_replicon::prelude::RepliconTick;
use lightyear::core::tick::Tick;
use lightyear::prelude::{Client, Connected, ReplicationCheckpointMap};

/// How many of the newest checkpoints are protected from eviction: one second of ticks. Must stay
/// well below lightyear's 256 so restoring them never evicts another protected one for good.
const PROTECTED_CHECKPOINTS: usize = 64;

pub(super) struct CheckpointWorkaroundPlugin;

impl Plugin for CheckpointWorkaroundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NewestCheckpoints>()
            .add_observer(protect_newest_checkpoints)
            .add_observer(forget_checkpoints_on_disconnect);
    }
}

/// The newest Replicon checkpoints received this session and their server ticks, oldest first.
#[derive(Resource, Default, Debug)]
struct NewestCheckpoints(VecDeque<(RepliconTick, Tick)>);

impl NewestCheckpoints {
    fn insert(&mut self, replicon_tick: RepliconTick, tick: Tick) {
        let index = self.0.partition_point(|&(t, _)| t.is_older(replicon_tick));
        if let Some(entry) = self.0.get_mut(index)
            && entry.0 == replicon_tick
        {
            entry.1 = tick;
            return;
        }
        self.0.insert(index, (replicon_tick, tick));
        if self.0.len() > PROTECTED_CHECKPOINTS {
            self.0.pop_front();
        }
    }
}

/// Records each received checkpoint itself (so lightyear's own observer, in whatever order it
/// runs, finds it already present and evicts nothing for it), then restores any protected
/// checkpoint lightyear has evicted. Each restore evicts lightyear's first-inserted entry, which
/// can be another protected one, hence the loop; it ends once only unprotected entries go.
fn protect_newest_checkpoints(
    received: On<UserdataReceived>,
    mut newest: ResMut<NewestCheckpoints>,
    mut checkpoints: ResMut<ReplicationCheckpointMap>,
) {
    // Lightyear encodes the server tick as 4 little-endian bytes (`encode_authoritative_tick`)
    // and reports malformed userdata itself.
    let Ok(bytes) = <[u8; 4]>::try_from(received.bytes.as_ref()) else {
        return;
    };
    let tick = Tick(u32::from_le_bytes(bytes));
    newest.insert(received.message_tick, tick);
    checkpoints.record(received.message_tick, tick);
    while let Some(&(replicon_tick, tick)) = newest
        .0
        .iter()
        .find(|&&(replicon_tick, _)| checkpoints.get(replicon_tick).is_none())
    {
        checkpoints.record(replicon_tick, tick);
    }
}

/// Lightyear clears its checkpoint map when the client disconnects; the next session's ticks
/// must not be compared against this one's.
fn forget_checkpoints_on_disconnect(
    trigger: On<Remove<Connected>>,
    clients: Query<(), With<Client>>,
    mut newest: ResMut<NewestCheckpoints>,
) {
    if clients.contains(trigger.entity) {
        newest.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// More ticks than lightyear keeps, delivered newest-first like a stalled Client's backlog.
    const BACKLOG: u32 = 300;

    fn userdata(replicon_tick: u32) -> UserdataReceived {
        UserdataReceived {
            message_tick: RepliconTick::new(replicon_tick),
            bytes: (replicon_tick + 1000).to_le_bytes().to_vec().into(),
        }
    }

    /// Feeds the backlog the way lightyear records it, with or without the workaround.
    fn apply_backlog(workaround: bool) -> World {
        let mut world = World::new();
        world.init_resource::<ReplicationCheckpointMap>();
        world.init_resource::<NewestCheckpoints>();
        if workaround {
            world.add_observer(protect_newest_checkpoints);
        }
        for replicon_tick in (0..BACKLOG).rev() {
            world
                .resource_mut::<ReplicationCheckpointMap>()
                .record(RepliconTick::new(replicon_tick), Tick(replicon_tick + 1000));
            world.trigger(userdata(replicon_tick));
        }
        world
    }

    #[test]
    fn lightyear_evicts_newest_ticks_of_a_backlog() {
        let mut world = apply_backlog(false);
        let mut checkpoints = world.resource_mut::<ReplicationCheckpointMap>();
        assert_eq!(
            checkpoints.record_last_confirmed_checkpoint(RepliconTick::new(BACKLOG - 1)),
            None,
            "lightyear now keeps the newest checkpoints: delete net/checkpoints.rs"
        );
    }

    #[test]
    fn newest_ticks_of_a_backlog_stay_resolvable() {
        let mut world = apply_backlog(true);
        let mut checkpoints = world.resource_mut::<ReplicationCheckpointMap>();
        for replicon_tick in BACKLOG - PROTECTED_CHECKPOINTS as u32..BACKLOG {
            assert_eq!(
                checkpoints.get(RepliconTick::new(replicon_tick)),
                Some(Tick(replicon_tick + 1000))
            );
        }
        assert!(
            checkpoints
                .record_last_confirmed_checkpoint(RepliconTick::new(BACKLOG - 1))
                .is_some()
        );
    }

    #[test]
    fn keeps_only_the_newest_ticks_in_order() {
        let mut newest = NewestCheckpoints::default();
        for replicon_tick in (0..100).rev().chain([50, 99]) {
            newest.insert(RepliconTick::new(replicon_tick), Tick(replicon_tick));
        }
        let kept: Vec<u32> = newest.0.iter().map(|&(_, tick)| tick.0).collect();
        assert_eq!(
            kept,
            (100 - PROTECTED_CHECKPOINTS as u32..100).collect::<Vec<_>>()
        );
    }
}
