//! Search & Destroy rules (pure): rounds without respawns, one bomb, attackers against
//! defenders. The Bevy side feeds [`SdInput`] each tick and carries out the [`SdEvent`]s.

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::data::SearchAndDestroyTuning;
use crate::shared::Team;

/// Seconds a new round waits for everyone to be placed before it goes live.
const START_GRACE_SECS: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundEnd {
    Elimination,
    Detonation,
    Defused,
    TimeExpired,
}

impl RoundEnd {
    /// What decided the round, for the winner (`won`) or the loser.
    pub fn text(self, won: bool) -> &'static str {
        match self {
            RoundEnd::Elimination if won => "Enemies eliminated",
            RoundEnd::Elimination => "Your team was eliminated",
            RoundEnd::Detonation => "Target destroyed",
            RoundEnd::Defused => "Bomb defused",
            RoundEnd::TimeExpired => "Time expired",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SdPhase {
    /// A team has no players: nobody can win a round yet. Respawns are open.
    Waiting,
    /// Everyone is being placed for the new round.
    Starting {
        left: f32,
    },
    Live {
        clock: f32,
    },
    /// The round is decided; the next starts in `left`.
    Over {
        winner: Team,
        reason: RoundEnd,
        left: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bomb {
    /// A living attacker has it.
    Carried,
    Dropped(Vec3),
    Planted {
        site: usize,
        fuse_left: f32,
    },
    /// Defused or exploded.
    Gone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SdState {
    /// 1-based round being played.
    pub round: u32,
    pub attackers: Team,
    pub phase: SdPhase,
    pub bomb: Bomb,
    /// Seconds the carrier has spent planting, and a defender defusing.
    pub plant: f32,
    pub defuse: f32,
    pub sites: Vec<Vec3>,
}

/// What the world looks like to the rules this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct SdInput {
    /// Players on each team.
    pub totals: [u32; 2],
    /// Living players on each team.
    pub alive: [u32; 2],
    /// Nobody is still waiting to be placed for the round start.
    pub spawned: bool,
    /// The living carrier is holding use inside this bomb site.
    pub planting_at: Option<usize>,
    /// A living defender is holding use at the planted bomb.
    pub defusing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SdEvent {
    /// Everyone should respawn for round `round`.
    RoundStarting {
        round: u32,
        attackers_swapped: bool,
    },
    /// Everyone is placed; a carrier should be chosen.
    RoundLive,
    Planted {
        site: usize,
    },
    Defused,
    Exploded {
        position: Vec3,
    },
    /// The team scores a round.
    RoundOver {
        winner: Team,
        reason: RoundEnd,
    },
}

impl SdState {
    /// A fresh Match: waiting for players, `attackers` attacking first. `sites` may be empty
    /// (then the bomb can never be planted and rounds end on time or elimination).
    pub fn new(sites: Vec<Vec3>, attackers: Team) -> Self {
        Self {
            round: 1,
            attackers,
            phase: SdPhase::Waiting,
            bomb: Bomb::Gone,
            plant: 0.0,
            defuse: 0.0,
            sites,
        }
    }

    pub fn defenders(&self) -> Team {
        self.attackers.other()
    }

    /// Whether dead players may respawn: between rounds' setup, never mid-round.
    pub fn respawn_open(&self) -> bool {
        matches!(self.phase, SdPhase::Waiting | SdPhase::Starting { .. })
    }

    /// Where the planted bomb is, if it is.
    pub fn planted_at(&self) -> Option<Vec3> {
        match self.bomb {
            Bomb::Planted { site, .. } => self.sites.get(site).copied(),
            _ => None,
        }
    }

    /// The carrier died (or left) at `position`.
    pub fn drop_bomb(&mut self, position: Vec3) {
        if self.bomb == Bomb::Carried {
            self.bomb = Bomb::Dropped(position);
            self.plant = 0.0;
        }
    }

    /// An attacker walked over the dropped bomb.
    pub fn pick_up(&mut self) {
        if matches!(self.bomb, Bomb::Dropped(_)) {
            self.bomb = Bomb::Carried;
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.phase, SdPhase::Live { .. })
    }

    /// Whole seconds left on the round clock (the full clock outside a live round).
    pub fn clock_secs(&self, tuning_round_secs: f32) -> u32 {
        match self.phase {
            SdPhase::Live { clock } => clock.max(0.0).ceil() as u32,
            _ => tuning_round_secs as u32,
        }
    }

    /// Advances by `dt`. `round_secs` is the round clock (`time_limit_secs`).
    pub fn tick(
        &mut self,
        input: &SdInput,
        dt: f32,
        round_secs: f32,
        tuning: &SearchAndDestroyTuning,
    ) -> Vec<SdEvent> {
        let mut events = Vec::new();
        let both_teams = input.totals.iter().all(|&total| total > 0);
        match self.phase {
            SdPhase::Waiting => {
                if both_teams {
                    self.start_round(&mut events, false);
                }
            }
            SdPhase::Starting { left } => {
                let left = left - dt;
                if !both_teams {
                    self.phase = SdPhase::Waiting;
                } else if left <= 0.0 && input.spawned {
                    self.phase = SdPhase::Live { clock: round_secs };
                    self.bomb = Bomb::Carried;
                    events.push(SdEvent::RoundLive);
                } else {
                    self.phase = SdPhase::Starting {
                        left: left.max(0.0),
                    };
                }
            }
            SdPhase::Live { clock } => self.tick_live(input, dt, clock, tuning, &mut events),
            SdPhase::Over {
                winner,
                reason,
                left,
            } => {
                let left = left - dt;
                if left > 0.0 {
                    self.phase = SdPhase::Over {
                        winner,
                        reason,
                        left,
                    };
                } else {
                    self.round += 1;
                    let swap = self.round - 1 == tuning.swap_after_round;
                    if swap {
                        self.attackers = self.attackers.other();
                    }
                    if both_teams {
                        self.start_round(&mut events, swap);
                    } else {
                        self.phase = SdPhase::Waiting;
                        self.bomb = Bomb::Gone;
                    }
                }
            }
        }
        events
    }

    fn start_round(&mut self, events: &mut Vec<SdEvent>, attackers_swapped: bool) {
        self.phase = SdPhase::Starting {
            left: START_GRACE_SECS,
        };
        self.bomb = Bomb::Gone;
        self.plant = 0.0;
        self.defuse = 0.0;
        events.push(SdEvent::RoundStarting {
            round: self.round,
            attackers_swapped,
        });
    }

    fn tick_live(
        &mut self,
        input: &SdInput,
        dt: f32,
        clock: f32,
        tuning: &SearchAndDestroyTuning,
        events: &mut Vec<SdEvent>,
    ) {
        let (attackers, defenders) = (self.attackers, self.defenders());
        let clock = clock - dt;
        let outcome = match self.bomb {
            Bomb::Planted { site, fuse_left } => {
                let fuse_left = fuse_left - dt;
                self.defuse = if input.defusing {
                    self.defuse + dt
                } else {
                    0.0
                };
                if self.defuse >= tuning.defuse_secs {
                    events.push(SdEvent::Defused);
                    self.bomb = Bomb::Gone;
                    Some((defenders, RoundEnd::Defused))
                } else if fuse_left <= 0.0 {
                    let position = self.sites.get(site).copied().unwrap_or_default();
                    events.push(SdEvent::Exploded { position });
                    self.bomb = Bomb::Gone;
                    Some((attackers, RoundEnd::Detonation))
                } else {
                    self.bomb = Bomb::Planted { site, fuse_left };
                    // With a bomb ticking, attackers dying doesn't end the round; defenders
                    // all dying does.
                    (input.alive[defenders.index()] == 0)
                        .then_some((attackers, RoundEnd::Elimination))
                }
            }
            _ => {
                match (self.bomb, input.planting_at) {
                    (Bomb::Carried, Some(site)) => {
                        self.plant += dt;
                        if self.plant >= tuning.plant_secs {
                            self.plant = 0.0;
                            self.bomb = Bomb::Planted {
                                site,
                                fuse_left: tuning.fuse_secs,
                            };
                            events.push(SdEvent::Planted { site });
                        }
                    }
                    _ => self.plant = 0.0,
                }
                if matches!(self.bomb, Bomb::Planted { .. }) {
                    None
                } else if input.alive[attackers.index()] == 0 {
                    Some((defenders, RoundEnd::Elimination))
                } else if input.alive[defenders.index()] == 0 {
                    Some((attackers, RoundEnd::Elimination))
                } else if clock <= 0.0 {
                    Some((defenders, RoundEnd::TimeExpired))
                } else {
                    None
                }
            }
        };
        match outcome {
            Some((winner, reason)) => {
                self.plant = 0.0;
                self.defuse = 0.0;
                self.phase = SdPhase::Over {
                    winner,
                    reason,
                    left: tuning.round_end_secs,
                };
                events.push(SdEvent::RoundOver { winner, reason });
            }
            None => self.phase = SdPhase::Live { clock },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> SearchAndDestroyTuning {
        SearchAndDestroyTuning {
            swap_after_round: 3,
            round_end_secs: 5.0,
            plant_secs: 5.0,
            defuse_secs: 7.0,
            fuse_secs: 45.0,
            site_radius: 2.5,
            pickup_radius: 1.5,
            blast_radius: 20.0,
            beep_secs: 1.0,
            beep_fast_secs: 0.15,
            beep_ramp_secs: 10.0,
        }
    }

    fn live() -> SdState {
        let mut sd = SdState::new(vec![Vec3::ZERO, Vec3::X * 30.0], Team::Alpha);
        let input = SdInput {
            totals: [2, 2],
            alive: [2, 2],
            spawned: true,
            ..default_input()
        };
        let mut events = Vec::new();
        for _ in 0..20 {
            events.extend(sd.tick(&input, 0.1, 150.0, &tuning()));
        }
        assert!(events.contains(&SdEvent::RoundLive), "{events:?}");
        sd
    }

    fn default_input() -> SdInput {
        SdInput {
            totals: [2, 2],
            alive: [2, 2],
            spawned: true,
            planting_at: None,
            defusing: false,
        }
    }

    fn run(sd: &mut SdState, input: SdInput, secs: f32) -> Vec<SdEvent> {
        (0..(secs * 10.0).round() as u32)
            .flat_map(|_| sd.tick(&input, 0.1, 150.0, &tuning()))
            .collect()
    }

    #[test]
    fn elimination_wins_the_round_and_the_next_round_starts() {
        let mut sd = live();
        let events = run(
            &mut sd,
            SdInput {
                alive: [2, 0],
                ..default_input()
            },
            0.2,
        );
        assert_eq!(
            events,
            [SdEvent::RoundOver {
                winner: Team::Alpha,
                reason: RoundEnd::Elimination
            }]
        );
        let events = run(&mut sd, default_input(), 5.2);
        assert_eq!(
            events,
            [SdEvent::RoundStarting {
                round: 2,
                attackers_swapped: false
            }]
        );
    }

    #[test]
    fn plant_then_defuse_beats_the_fuse_and_defenders_win() {
        let mut sd = live();
        let planting = SdInput {
            planting_at: Some(1),
            ..default_input()
        };
        assert!(run(&mut sd, planting, 4.5).is_empty());
        assert_eq!(run(&mut sd, planting, 1.0), [SdEvent::Planted { site: 1 }]);
        // Attackers dying after the plant doesn't end the round.
        let dead_attackers = SdInput {
            alive: [0, 2],
            ..default_input()
        };
        assert!(run(&mut sd, dead_attackers, 3.0).is_empty());
        let defusing = SdInput {
            defusing: true,
            alive: [0, 2],
            ..default_input()
        };
        assert_eq!(
            run(&mut sd, defusing, 7.2),
            [
                SdEvent::Defused,
                SdEvent::RoundOver {
                    winner: Team::Bravo,
                    reason: RoundEnd::Defused
                }
            ]
        );
    }

    #[test]
    fn an_unattended_planted_bomb_detonates_and_attackers_win() {
        let mut sd = live();
        run(
            &mut sd,
            SdInput {
                planting_at: Some(0),
                ..default_input()
            },
            5.2,
        );
        let events = run(&mut sd, default_input(), 46.0);
        assert_eq!(
            events,
            [
                SdEvent::Exploded {
                    position: Vec3::ZERO
                },
                SdEvent::RoundOver {
                    winner: Team::Alpha,
                    reason: RoundEnd::Detonation
                }
            ]
        );
    }

    #[test]
    fn defenders_win_on_time_and_sides_swap_at_half() {
        let mut sd = live();
        let events = run(&mut sd, default_input(), 150.2);
        assert_eq!(
            events,
            [SdEvent::RoundOver {
                winner: Team::Bravo,
                reason: RoundEnd::TimeExpired
            }]
        );
        // Rounds 1 and 2 pass without a swap; the one after round 3 swaps.
        sd.round = 3;
        sd.phase = SdPhase::Over {
            winner: Team::Bravo,
            reason: RoundEnd::TimeExpired,
            left: 0.0,
        };
        let events = run(&mut sd, default_input(), 0.1);
        assert_eq!(
            events,
            [SdEvent::RoundStarting {
                round: 4,
                attackers_swapped: true
            }]
        );
        assert_eq!(sd.attackers, Team::Bravo);
    }
}
