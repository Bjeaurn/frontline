//! The bolt of a bolt-action Weapon: a shot opens it, and it has to be racked before the next
//! round can fire. While aimed a click racks it slowly and keeps the sights up; letting go of the
//! sights racks it by itself, faster. Pure state, driven by `HeldWeapon`.

/// Where the bolt is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Bolt {
    /// A round is chambered: the Weapon can fire.
    #[default]
    Closed,
    /// Fired and not yet racked: waiting for a click while aimed, or for the sights to drop.
    Open,
    /// Being worked: `progress` runs from 0 to 1. `fast` once the faster unscoped rate applies:
    /// a rack the sights started by themselves, or one the sights were let go of mid-way.
    Racking { progress: f32, fast: bool },
}

/// What a step of the bolt did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoltEvent {
    Idle,
    /// Racking began, by a click while aimed (`scoped`) or by itself once the sights dropped.
    RackStarted {
        scoped: bool,
    },
    /// The rack is done: the next click fires.
    Closed,
}

impl Bolt {
    /// A shot just left the barrel.
    pub fn fire(&mut self) {
        *self = Bolt::Open;
    }

    /// A round is chambered.
    pub fn can_fire(&self) -> bool {
        *self == Bolt::Closed
    }

    /// Whether a click right now would rack it.
    pub fn waiting_for_click(&self) -> bool {
        *self == Bolt::Open
    }

    /// How far the rack has got, 0 to 1, while being worked.
    pub fn progress(&self) -> Option<f32> {
        match self {
            Bolt::Racking { progress, .. } => Some(*progress),
            _ => None,
        }
    }

    /// Advances `dt` seconds. `aiming` is the sights being held up, `click` a fresh trigger pull
    /// (the caller treats it as used when this returns `RackStarted { scoped: true }`).
    /// `scoped_secs` and `unscoped_secs` are the full rack times at either rate: progress is a
    /// fraction, so letting go of the sights mid-rack finishes the rest at the faster rate.
    pub fn step(
        &mut self,
        dt: f32,
        aiming: bool,
        click: bool,
        scoped_secs: f32,
        unscoped_secs: f32,
    ) -> BoltEvent {
        let mut event = BoltEvent::Idle;
        if *self == Bolt::Open {
            if !aiming {
                *self = Bolt::Racking {
                    progress: 0.0,
                    fast: true,
                };
                event = BoltEvent::RackStarted { scoped: false };
            } else if click {
                *self = Bolt::Racking {
                    progress: 0.0,
                    fast: false,
                };
                event = BoltEvent::RackStarted { scoped: true };
            }
        }
        if let Bolt::Racking { progress, fast } = self {
            *fast |= !aiming;
            let secs = if *fast { unscoped_secs } else { scoped_secs };
            *progress += dt / secs.max(f32::EPSILON);
            if *progress >= 1.0 {
                *self = Bolt::Closed;
                event = BoltEvent::Closed;
            }
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCOPED: f32 = 1.4;
    const UNSCOPED: f32 = 0.9;

    /// Steps in 10 ms frames until `secs` have passed; returns the last interesting event.
    fn run(bolt: &mut Bolt, secs: f32, aiming: bool) -> Vec<BoltEvent> {
        let frames = (secs / 0.01).round() as usize;
        (0..frames)
            .map(|_| bolt.step(0.01, aiming, false, SCOPED, UNSCOPED))
            .filter(|event| *event != BoltEvent::Idle)
            .collect()
    }

    #[test]
    fn an_open_bolt_cannot_fire_until_racked() {
        let mut bolt = Bolt::default();
        assert!(bolt.can_fire());
        bolt.fire();
        assert!(!bolt.can_fire() && bolt.waiting_for_click());
        // Aimed with no click: nothing happens, however long it waits.
        assert!(run(&mut bolt, 5.0, true).is_empty());
        assert!(bolt.waiting_for_click());
    }

    #[test]
    fn a_scoped_click_racks_slowly_and_the_next_click_fires() {
        let mut bolt = Bolt::default();
        bolt.fire();
        let started = bolt.step(0.01, true, true, SCOPED, UNSCOPED);
        assert_eq!(started, BoltEvent::RackStarted { scoped: true });
        assert!(!bolt.can_fire());
        // Still racking just before 1.4 s, closed just after.
        assert!(run(&mut bolt, 1.35, true).is_empty());
        assert!(bolt.progress().is_some_and(|p| p > 0.9));
        assert_eq!(run(&mut bolt, 0.1, true), vec![BoltEvent::Closed]);
        assert!(bolt.can_fire());
        // Extra clicks while racking don't restart it.
        let mut bolt = Bolt::default();
        bolt.fire();
        bolt.step(0.01, true, true, SCOPED, UNSCOPED);
        let before = bolt.progress();
        bolt.step(0.01, true, true, SCOPED, UNSCOPED);
        assert!(bolt.progress() > before);
    }

    #[test]
    fn letting_go_of_the_sights_racks_by_itself_at_the_faster_rate() {
        let mut bolt = Bolt::default();
        bolt.fire();
        let first = bolt.step(0.01, false, false, SCOPED, UNSCOPED);
        assert_eq!(first, BoltEvent::RackStarted { scoped: false });
        assert!(run(&mut bolt, 0.85, false).is_empty());
        assert_eq!(run(&mut bolt, 0.1, false), vec![BoltEvent::Closed]);
        // Aiming again mid-way doesn't slow an automatic rack.
        let mut bolt = Bolt::default();
        bolt.fire();
        bolt.step(0.01, false, false, SCOPED, UNSCOPED);
        run(&mut bolt, 0.4, false);
        assert_eq!(run(&mut bolt, 0.55, true), vec![BoltEvent::Closed]);
    }

    #[test]
    fn letting_go_mid_rack_finishes_the_rest_at_the_faster_rate() {
        let mut bolt = Bolt::default();
        bolt.fire();
        bolt.step(0.01, true, true, SCOPED, UNSCOPED);
        // Half of the 1.4 s scoped rack...
        run(&mut bolt, 0.69, true);
        let half = bolt.progress().unwrap();
        assert!((half - 0.5).abs() < 0.02, "{half}");
        // ...leaves half of the 0.9 s unscoped rack: 0.45 s.
        assert!(run(&mut bolt, 0.42, false).is_empty());
        assert_eq!(run(&mut bolt, 0.08, false), vec![BoltEvent::Closed]);
        assert!(bolt.can_fire());
    }
}
