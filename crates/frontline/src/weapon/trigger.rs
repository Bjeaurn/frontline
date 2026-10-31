//! The trigger of a Weapon as a pure state machine: which rounds leave the barrel in a frame for
//! each fire mode (Auto, Burst, Semi, Bolt), the burst timing, and what pulling the trigger on an
//! empty magazine does (a dry-fire click, or a reload when Auto reload is on).

use crate::data::FireMode;

/// What carries over from one frame to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Trigger {
    /// Seconds until the next round may fire; it goes negative within a frame to carry the time
    /// owed to the next round (so a long frame fires several).
    pub cooldown: f32,
    /// This pull already did its one thing: fired (Semi, Bolt), clicked on an empty magazine, or
    /// was the one that emptied it. Cleared by letting go of the trigger.
    pub pull_spent: bool,
    /// Rounds still to come in the burst under way.
    pub burst_left: u32,
}

/// What the Weapon tells the trigger about this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Feed {
    /// The trigger is held.
    pub held: bool,
    /// The Weapon is up and can fire: not sprinting, switching or reloading, bolt closed.
    pub can_fire: bool,
    pub rounds: u32,
    pub mode: FireMode,
    /// Seconds between rounds of `Auto` and `Semi` (and the Bolt's nominal rate, unused).
    pub shot_secs: f32,
    /// Seconds between rounds inside a burst, and the pause after the last one.
    pub burst_shot_secs: f32,
    pub burst_delay_secs: f32,
    /// Auto reload setting: a pull on an empty magazine starts a reload.
    pub auto_reload: bool,
    /// Least seconds between two `Semi` rounds, and between two dry clicks, whatever the fire
    /// rate: mashing the button can't go faster.
    pub semi_secs: f32,
}

/// What the frame did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Outcome {
    /// Rounds that left the barrel.
    pub fired: u32,
    /// The trigger was pulled on an empty magazine: click.
    pub dry_click: bool,
    /// Start a reload now (Auto reload on, empty magazine pulled).
    pub reload: bool,
}

/// Runs the trigger for a frame of `dt` seconds. `Auto` fires as many rounds as the fire rate
/// owes, `Semi` and `Bolt` one per pull, `Burst(n)` n rounds at the burst rate per pull (and,
/// with the trigger held, a new burst after the delay); a started burst finishes even if the
/// trigger is let go, but not through a reload, switch or sprint. The carried state stays in
/// `trigger`; the caller takes `fired` rounds out of the magazine.
pub(super) fn step(trigger: &mut Trigger, feed: &Feed, dt: f32) -> Outcome {
    let mut outcome = Outcome::default();
    trigger.cooldown -= dt;
    if !feed.held {
        trigger.pull_spent = false;
    }
    if !feed.can_fire {
        trigger.burst_left = 0;
    }
    let mut rounds = feed.rounds;
    if feed.can_fire {
        while trigger.cooldown <= 0.0
            && (trigger.burst_left > 0 || (feed.held && !trigger.pull_spent))
        {
            if rounds == 0 {
                trigger.burst_left = 0;
                // Old behaviour (Auto reload on): an Auto trigger held through the reload keeps
                // firing after it; every other pull waits for a new one.
                trigger.pull_spent = !(feed.auto_reload && feed.mode == FireMode::Auto);
                outcome.dry_click = true;
                outcome.reload = feed.auto_reload;
                trigger.cooldown = trigger.cooldown.max(0.0) + feed.semi_secs;
                break;
            }
            rounds -= 1;
            outcome.fired += 1;
            match feed.mode {
                FireMode::Auto => trigger.cooldown += feed.shot_secs,
                FireMode::Semi => {
                    trigger.cooldown += feed.shot_secs.max(feed.semi_secs);
                    trigger.pull_spent = true;
                }
                FireMode::Bolt => trigger.pull_spent = true,
                FireMode::Burst(count) => {
                    if trigger.burst_left == 0 {
                        trigger.burst_left = count;
                    }
                    trigger.burst_left -= 1;
                    trigger.cooldown += feed.burst_shot_secs;
                    if trigger.burst_left == 0 {
                        trigger.cooldown += feed.burst_delay_secs;
                    }
                }
            }
            if rounds == 0 {
                // The round that emptied the magazine is part of this pull: the click needs a
                // fresh one, unless the old behaviour (reload on an empty pull) is on.
                trigger.burst_left = 0;
                trigger.pull_spent |= !feed.auto_reload;
            }
        }
    }
    trigger.cooldown = trigger.cooldown.max(0.0);
    outcome
}

/// Whether the underslung grenade launcher is selected in place of the rifle's fire mode:
/// V switches it on and off, a click fires a round only while it is on, and the rifle cannot
/// fire while it is on. Dying, switching Weapon or opening a menu switches it off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LauncherMode {
    pub on: bool,
}

impl LauncherMode {
    /// The rifle's trigger works: not while the launcher is selected.
    pub fn rifle_may_fire(&self) -> bool {
        !self.on
    }
}

/// What the player and the Weapon tell the launcher this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct LauncherFeed {
    /// The launcher is fitted to the Weapon in hand and the player can use it: alive, cursor
    /// grabbed (no menu), the Primary in hand and not switching, not mantling.
    pub available: bool,
    /// The Weapon is up and can fire: not sprinting or still raising it.
    pub up: bool,
    /// V went down this frame.
    pub toggle: bool,
    /// The fire button went down this frame (a fresh click: holding it does not repeat).
    pub click: bool,
    /// Rounds left, and seconds until the next may leave (the gap between rounds).
    pub rounds: u32,
    pub cooldown: f32,
}

/// What the frame did to the launcher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LauncherOutcome {
    /// V changed the mode: the switch click.
    pub switched: bool,
    /// A round leaves the launcher; the caller takes it from the rounds.
    pub fire: bool,
    /// A click with no rounds left: the dry click.
    pub dry_click: bool,
}

/// Runs the launcher mode for a frame. V toggles the mode (even with no rounds left, and with
/// the Weapon lowered); a click fires one round when the mode is on and the Weapon is up, or
/// clicks dry with no rounds left, and does nothing inside the gap between rounds. When the
/// launcher isn't `available` the mode is switched off and nothing else happens.
pub(super) fn launcher_step(mode: &mut LauncherMode, feed: &LauncherFeed) -> LauncherOutcome {
    let mut outcome = LauncherOutcome::default();
    if !feed.available {
        mode.on = false;
        return outcome;
    }
    if feed.toggle {
        mode.on = !mode.on;
        outcome.switched = true;
    } else if mode.on && feed.click && feed.up {
        if feed.rounds == 0 {
            outcome.dry_click = true;
        } else if feed.cooldown <= 0.0 {
            outcome.fire = true;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(mode: FireMode) -> Feed {
        Feed {
            held: false,
            can_fire: true,
            rounds: 30,
            mode,
            shot_secs: 0.1,
            burst_shot_secs: 0.05,
            burst_delay_secs: 0.2,
            auto_reload: false,
            semi_secs: 0.0,
        }
    }

    /// Runs `frames` frames of `dt`, spending the rounds fired from the feed's magazine;
    /// returns the time of every round fired.
    fn run(trigger: &mut Trigger, feed: &mut Feed, frames: u32, dt: f32) -> Vec<f32> {
        let mut times = Vec::new();
        for frame in 0..frames {
            let outcome = step(trigger, feed, dt);
            feed.rounds -= outcome.fired;
            for _ in 0..outcome.fired {
                times.push(frame as f32 * dt);
            }
        }
        times
    }

    #[test]
    fn auto_fires_while_held_at_its_rate() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Auto);
        feed.held = true;
        let times = run(&mut trigger, &mut feed, 100, 0.01);
        assert!(
            (10..=11).contains(&times.len()),
            "600 rpm for a second: {times:?}"
        );
        for pair in times.windows(2) {
            assert!((pair[1] - pair[0] - 0.1).abs() < 0.011, "{times:?}");
        }
    }

    #[test]
    fn burst_fires_its_rounds_at_the_burst_rate_then_waits_out_the_delay() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Burst(3));
        feed.held = true;
        // Held 1 s in 10 ms frames: bursts of three, 0.05 s apart; the next burst starts 0.35 s
        // after the first (two spacings, the last round's spacing and the 0.2 s delay).
        let times = run(&mut trigger, &mut feed, 100, 0.01);
        assert_eq!(times.len(), 9, "three bursts in a second: {times:?}");
        for burst in times.chunks(3) {
            assert!((burst[1] - burst[0] - 0.05).abs() < 0.011, "{burst:?}");
            assert!((burst[2] - burst[1] - 0.05).abs() < 0.011, "{burst:?}");
        }
        assert!((times[3] - times[0] - 0.35).abs() < 0.02, "{times:?}");
    }

    #[test]
    fn a_tap_still_fires_the_whole_burst() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Burst(3));
        feed.held = true;
        assert_eq!(step(&mut trigger, &feed, 0.01).fired, 1);
        feed.held = false;
        feed.rounds -= 1;
        let rest = run(&mut trigger, &mut feed, 30, 0.01);
        assert_eq!(rest.len(), 2, "the other two come without the trigger");
        assert_eq!(trigger.burst_left, 0);
        assert_eq!(
            run(&mut trigger, &mut feed, 50, 0.01).len(),
            0,
            "and no more"
        );
    }

    #[test]
    fn letting_go_during_the_delay_still_waits_it_out() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Burst(2));
        feed.held = true;
        let first = run(&mut trigger, &mut feed, 10, 0.01);
        assert_eq!(first.len(), 2);
        // Tap again right after the burst: the delay (0.2 s) has to pass first.
        feed.held = false;
        step(&mut trigger, &feed, 0.01);
        feed.held = true;
        let soon: usize = (0..10)
            .map(|_| step(&mut trigger, &feed, 0.01).fired as usize)
            .sum();
        assert_eq!(soon, 0, "the burst delay isn't skipped by re-pulling");
        let later: usize = (0..30)
            .map(|_| step(&mut trigger, &feed, 0.01).fired as usize)
            .sum();
        assert!(later >= 1, "and then the next burst starts");
    }

    #[test]
    fn an_interrupted_burst_is_dropped() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Burst(3));
        feed.held = true;
        assert_eq!(step(&mut trigger, &feed, 0.01).fired, 1);
        assert_eq!(trigger.burst_left, 2);
        feed.can_fire = false;
        step(&mut trigger, &feed, 0.01);
        assert_eq!(trigger.burst_left, 0, "a reload, switch or sprint ends it");
    }

    #[test]
    fn a_burst_stops_where_the_magazine_runs_out() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Burst(3));
        feed.held = true;
        feed.rounds = 2;
        let times = run(&mut trigger, &mut feed, 30, 0.01);
        assert_eq!(times.len(), 2);
        assert_eq!(feed.rounds, 0);
        assert_eq!(trigger.burst_left, 0);
    }

    #[test]
    fn semi_and_bolt_fire_once_per_pull() {
        for mode in [FireMode::Semi, FireMode::Bolt] {
            let mut trigger = Trigger::default();
            let mut feed = feed(mode);
            feed.held = true;
            assert_eq!(run(&mut trigger, &mut feed, 30, 0.1).len(), 1, "{mode:?}");
            feed.held = false;
            step(&mut trigger, &feed, 0.1);
            feed.held = true;
            assert_eq!(
                step(&mut trigger, &feed, 0.1).fired,
                1,
                "{mode:?}: new pull"
            );
        }
    }

    #[test]
    fn semi_respects_the_cooldown_between_pulls() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Semi);
        feed.shot_secs = 1.0;
        feed.held = true;
        assert_eq!(step(&mut trigger, &feed, 0.1).fired, 1);
        feed.held = false;
        step(&mut trigger, &feed, 0.1);
        feed.held = true;
        let early: u32 = (0..5).map(|_| step(&mut trigger, &feed, 0.1).fired).sum();
        assert_eq!(early, 0, "the next pull waits for the cooldown");
        let late: u32 = (0..10).map(|_| step(&mut trigger, &feed, 0.1).fired).sum();
        assert_eq!(late, 1, "and fires once it is over");
    }

    #[test]
    fn an_empty_pull_clicks_once_per_pull_and_never_repeats() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Auto);
        feed.rounds = 0;
        feed.held = true;
        let first = step(&mut trigger, &feed, 0.1);
        assert_eq!(
            first,
            Outcome {
                dry_click: true,
                ..Outcome::default()
            }
        );
        let clicks: usize = (0..20)
            .map(|_| step(&mut trigger, &feed, 0.1).dry_click as usize)
            .sum();
        assert_eq!(clicks, 0, "holding doesn't click again");
        feed.held = false;
        step(&mut trigger, &feed, 0.1);
        feed.held = true;
        assert!(step(&mut trigger, &feed, 0.1).dry_click, "a new pull does");
    }

    #[test]
    fn the_pull_that_empties_the_magazine_does_not_also_click() {
        for mode in [FireMode::Auto, FireMode::Burst(3)] {
            let mut trigger = Trigger::default();
            let mut feed = feed(mode);
            feed.rounds = 3;
            feed.held = true;
            let mut clicks = 0;
            for _ in 0..100 {
                let outcome = step(&mut trigger, &feed, 0.05);
                feed.rounds -= outcome.fired;
                clicks += outcome.dry_click as u32;
            }
            assert_eq!(clicks, 0, "{mode:?}: held through the last round");
            feed.held = false;
            step(&mut trigger, &feed, 0.5);
            feed.held = true;
            assert!(step(&mut trigger, &feed, 0.05).dry_click, "{mode:?}");
        }
    }

    #[test]
    fn auto_reload_turns_the_empty_pull_into_a_reload() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Auto);
        feed.auto_reload = true;
        feed.rounds = 2;
        feed.held = true;
        let mut reloads = 0;
        for _ in 0..10 {
            let outcome = step(&mut trigger, &feed, 0.1);
            feed.rounds -= outcome.fired;
            reloads += outcome.reload as u32;
            if outcome.reload {
                break;
            }
        }
        assert_eq!(
            reloads, 1,
            "the held trigger reloads once the rounds are gone"
        );
        assert_eq!(feed.rounds, 0);
    }

    #[test]
    fn nothing_fires_or_clicks_while_the_weapon_is_down() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Auto);
        feed.held = true;
        feed.can_fire = false;
        assert_eq!(step(&mut trigger, &feed, 0.1), Outcome::default());
        feed.rounds = 0;
        assert_eq!(
            step(&mut trigger, &feed, 0.1),
            Outcome::default(),
            "no click while reloading"
        );
    }

    /// Mashes the button (a pull and a release on alternate frames) for `secs`.
    fn mash(feed: &mut Feed, dt: f32, secs: f32) -> Vec<f32> {
        let mut trigger = Trigger::default();
        let mut times = Vec::new();
        let frames = (secs / dt).round() as u32;
        for frame in 0..frames {
            feed.held = frame % 2 == 0;
            let outcome = step(&mut trigger, feed, dt);
            feed.rounds -= outcome.fired;
            for _ in 0..outcome.fired {
                times.push(frame as f32 * dt);
            }
        }
        times
    }

    #[test]
    fn mashing_semi_never_beats_its_interval_at_any_frame_rate() {
        for dt in [0.004, 0.01, 0.0167, 0.05] {
            let mut feed = feed(FireMode::Semi);
            feed.shot_secs = 0.05;
            feed.semi_secs = 0.15;
            let times = mash(&mut feed, dt, 2.0);
            assert!(
                times.len() >= 10,
                "dt {dt}: a click after the interval fires: {times:?}"
            );
            assert!(times.len() <= 14, "dt {dt}: 2 s at 0.15 s: {times:?}");
            for pair in times.windows(2) {
                assert!(pair[1] - pair[0] >= 0.15 - dt, "dt {dt}: {times:?}");
            }
        }
    }

    #[test]
    fn a_click_inside_the_semi_interval_does_nothing_and_one_after_it_fires() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Semi);
        feed.shot_secs = 0.05;
        feed.semi_secs = 0.2;
        feed.held = true;
        assert_eq!(step(&mut trigger, &feed, 0.01).fired, 1);
        for _ in 0..8 {
            feed.held = false;
            step(&mut trigger, &feed, 0.01);
            feed.held = true;
            assert_eq!(
                step(&mut trigger, &feed, 0.01).fired,
                0,
                "inside the interval"
            );
        }
        feed.held = false;
        step(&mut trigger, &feed, 0.15);
        feed.held = true;
        assert_eq!(step(&mut trigger, &feed, 0.01).fired, 1, "after it");
    }

    #[test]
    fn the_interval_never_slows_a_weapon_below_its_own_rate() {
        let mut feed = feed(FireMode::Semi);
        feed.shot_secs = 0.3;
        feed.semi_secs = 0.1;
        let times = mash(&mut feed, 0.01, 2.0);
        for pair in times.windows(2) {
            assert!(pair[1] - pair[0] >= 0.3 - 0.01, "{times:?}");
        }
    }

    #[test]
    fn dry_clicks_keep_to_the_interval_too() {
        let mut trigger = Trigger::default();
        let mut feed = feed(FireMode::Semi);
        feed.rounds = 0;
        feed.semi_secs = 0.2;
        let mut clicks = 0;
        for frame in 0..200 {
            feed.held = frame % 2 == 0;
            clicks += step(&mut trigger, &feed, 0.01).dry_click as u32;
        }
        assert!(
            (9..=10).contains(&clicks),
            "2 s of mashing at 0.2 s: {clicks}"
        );
    }

    fn launcher_feed() -> LauncherFeed {
        LauncherFeed {
            available: true,
            up: true,
            toggle: false,
            click: false,
            rounds: 3,
            cooldown: 0.0,
        }
    }

    fn toggled(mode: &mut LauncherMode) -> LauncherOutcome {
        launcher_step(
            mode,
            &LauncherFeed {
                toggle: true,
                ..launcher_feed()
            },
        )
    }

    fn clicked(mode: &mut LauncherMode, feed: LauncherFeed) -> LauncherOutcome {
        launcher_step(
            mode,
            &LauncherFeed {
                click: true,
                ..feed
            },
        )
    }

    #[test]
    fn v_switches_into_the_launcher_and_back() {
        let mut mode = LauncherMode::default();
        let outcome = toggled(&mut mode);
        assert!(mode.on && outcome.switched && !outcome.fire);
        let outcome = toggled(&mut mode);
        assert!(!mode.on && outcome.switched && !outcome.fire);
        // Nothing pressed: the mode stays as it is.
        mode.on = true;
        assert_eq!(
            launcher_step(&mut mode, &launcher_feed()),
            LauncherOutcome::default()
        );
        assert!(mode.on);
    }

    #[test]
    fn a_click_fires_one_round_only_in_launcher_mode() {
        let mut mode = LauncherMode::default();
        let outcome = clicked(&mut mode, launcher_feed());
        assert_eq!(outcome, LauncherOutcome::default(), "rifle mode: no round");
        mode.on = true;
        let outcome = clicked(&mut mode, launcher_feed());
        assert!(outcome.fire && !outcome.dry_click && !outcome.switched);
        assert!(mode.on, "firing leaves the mode on");
        // A click and V in one frame switch only: the click isn't a shot out of the new mode.
        let mut mode = LauncherMode::default();
        let outcome = clicked(
            &mut mode,
            LauncherFeed {
                toggle: true,
                ..launcher_feed()
            },
        );
        assert!(mode.on && outcome.switched && !outcome.fire);
    }

    #[test]
    fn the_rifle_cannot_fire_while_the_launcher_is_selected() {
        let mut mode = LauncherMode::default();
        let mut trigger = Trigger::default();
        let mut rifle = feed(FireMode::Auto);
        rifle.held = true;
        rifle.can_fire = mode.rifle_may_fire();
        assert!(step(&mut trigger, &rifle, 0.1).fired > 0);
        toggled(&mut mode);
        rifle.can_fire = mode.rifle_may_fire();
        assert_eq!(step(&mut trigger, &rifle, 1.0).fired, 0);
        toggled(&mut mode);
        rifle.can_fire = mode.rifle_may_fire();
        assert!(step(&mut trigger, &rifle, 0.1).fired > 0, "and back again");
    }

    #[test]
    fn with_no_rounds_left_the_click_is_dry_and_v_still_switches_back() {
        let mut mode = LauncherMode { on: true };
        let empty = LauncherFeed {
            rounds: 0,
            ..launcher_feed()
        };
        let outcome = clicked(&mut mode, empty);
        assert!(outcome.dry_click && !outcome.fire);
        assert!(mode.on);
        let outcome = launcher_step(
            &mut mode,
            &LauncherFeed {
                toggle: true,
                ..empty
            },
        );
        assert!(!mode.on && outcome.switched);
    }

    #[test]
    fn rounds_wait_out_the_reload_gap_in_silence() {
        let mut mode = LauncherMode { on: true };
        let outcome = clicked(
            &mut mode,
            LauncherFeed {
                cooldown: 1.2,
                ..launcher_feed()
            },
        );
        assert_eq!(outcome, LauncherOutcome::default());
    }

    #[test]
    fn the_weapon_being_down_blocks_the_shot_but_not_the_switch() {
        // Sprinting or still raising: no round leaves, V still works.
        let mut mode = LauncherMode { on: true };
        let down = LauncherFeed {
            up: false,
            ..launcher_feed()
        };
        assert_eq!(clicked(&mut mode, down), LauncherOutcome::default());
        let outcome = launcher_step(
            &mut mode,
            &LauncherFeed {
                toggle: true,
                ..down
            },
        );
        assert!(!mode.on && outcome.switched);
    }

    #[test]
    fn dying_a_switch_a_menu_or_losing_the_launcher_returns_to_the_rifle() {
        // `available` is false when dead, in a menu, switching Weapon, holding the Secondary or
        // without the launcher fitted; V and clicks do nothing then.
        let gone = LauncherFeed {
            available: false,
            ..launcher_feed()
        };
        let mut mode = LauncherMode { on: true };
        assert_eq!(launcher_step(&mut mode, &gone), LauncherOutcome::default());
        assert!(!mode.on);
        let outcome = launcher_step(
            &mut mode,
            &LauncherFeed {
                toggle: true,
                click: true,
                ..gone
            },
        );
        assert_eq!(outcome, LauncherOutcome::default());
        assert!(!mode.on, "V cannot switch it on when unavailable");
    }
}
