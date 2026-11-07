//! Headquarters rules (pure): one HQ is active at a time, moving between the Map's sites (a random
//! one of the far sites each time, see [`next_site`]). A team that captures it scores every
//! interval until the other destroys it (stands on it, the same as neutralising a Flag) or it
//! runs out of time and relocates.

use std::hash::{BuildHasher, RandomState};

use bevy::math::Vec3;

use super::capture::{Capture, CaptureEvent};
use crate::data::HeadquartersTuning;
use crate::shared::Team;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HqPhase {
    /// At its site, with this many seconds to live.
    Active { life_left: f32 },
    /// Gone; the next one appears in `left` seconds.
    Relocating { left: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HqState {
    sites: Vec<Vec3>,
    /// Index into `sites` of the current (or next) HQ.
    pub site: usize,
    pub phase: HqPhase,
    pub capture: Capture,
    score_timer: f32,
    rng: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HqEvent {
    Captured(Team),
    /// The enemy of the given (former) owner destroyed it.
    Destroyed(Team),
    /// Nobody owned it when its time ran out, or its owner held it to the end.
    Expired,
    /// The next HQ appeared at this site.
    Appeared(usize),
}

#[derive(Debug, Default, PartialEq)]
pub struct HqTick {
    pub points: [u32; 2],
    pub event: Option<HqEvent>,
}

impl HqState {
    /// `sites` must not be empty. The first HQ appears at `first`.
    pub fn new(sites: Vec<Vec3>, first: usize, tuning: &HeadquartersTuning) -> Self {
        Self::seeded(sites, first, tuning, RandomState::new().hash_one(0_u8))
    }

    /// [`Self::new`] with a given random seed (the relocation rolls follow from it).
    pub fn seeded(sites: Vec<Vec3>, first: usize, tuning: &HeadquartersTuning, seed: u64) -> Self {
        assert!(!sites.is_empty(), "Headquarters needs at least one site");
        Self {
            site: first % sites.len(),
            sites,
            phase: HqPhase::Active {
                life_left: tuning.life_secs,
            },
            capture: Capture::default(),
            score_timer: 0.0,
            rng: seed | 1,
        }
    }

    /// A uniform roll in `[0, 1)` (xorshift64).
    fn roll(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1_u64 << 24) as f32
    }

    /// Where the HQ is, `None` while it relocates.
    pub fn position(&self) -> Option<Vec3> {
        matches!(self.phase, HqPhase::Active { .. }).then(|| self.sites[self.site])
    }

    /// The team holding the HQ, if any.
    pub fn owner(&self) -> Option<Team> {
        self.position().and(self.capture.owner)
    }

    /// `presence[team]`: living players of each team on the HQ.
    pub fn tick(&mut self, presence: [u32; 2], dt: f32, tuning: &HeadquartersTuning) -> HqTick {
        let mut tick = HqTick::default();
        match self.phase {
            HqPhase::Relocating { left } => {
                let left = left - dt;
                if left > 0.0 {
                    self.phase = HqPhase::Relocating { left };
                } else {
                    let roll = self.roll();
                    self.site =
                        next_site(&self.sites, self.site, tuning.relocate_far_fraction, roll);
                    self.phase = HqPhase::Active {
                        life_left: tuning.life_secs,
                    };
                    tick.event = Some(HqEvent::Appeared(self.site));
                }
            }
            HqPhase::Active { life_left } => {
                match self.capture.tick(presence, dt, &tuning.capture) {
                    Some(CaptureEvent::Captured(team)) => {
                        tick.event = Some(HqEvent::Captured(team))
                    }
                    Some(CaptureEvent::Neutralized(owner)) => {
                        tick.event = Some(HqEvent::Destroyed(owner));
                    }
                    None => {}
                }
                self.score_timer += dt;
                while self.score_timer >= tuning.score_interval_secs {
                    self.score_timer -= tuning.score_interval_secs;
                    if let Some(owner) = self.capture.owner {
                        tick.points[owner.index()] += tuning.points_per_tick;
                    }
                }
                let life_left = life_left - dt;
                if matches!(tick.event, Some(HqEvent::Destroyed(_))) {
                    self.relocate(tuning);
                } else if life_left <= 0.0 {
                    tick.event = Some(HqEvent::Expired);
                    self.relocate(tuning);
                } else {
                    self.phase = HqPhase::Active { life_left };
                }
            }
        }
        tick
    }

    fn relocate(&mut self, tuning: &HeadquartersTuning) {
        self.capture = Capture::default();
        self.score_timer = 0.0;
        self.phase = HqPhase::Relocating {
            left: tuning.relocate_secs,
        };
    }
}

/// The site the HQ moves to after `current`: a random one (by `roll`, in `[0, 1)`) of the sites at
/// least `far_fraction` (0 to 1) of the farthest site's distance from the current one, measured
/// on the ground. The HQ never stays where it was; with only one site it has nowhere to go. Sites
/// in clusters make it hop between the areas and pick a different spot in the area each time.
pub fn next_site(sites: &[Vec3], current: usize, far_fraction: f32, roll: f32) -> usize {
    let distance = |i: usize| {
        let d = sites[i] - sites[current];
        d.x.hypot(d.z)
    };
    let others = || (0..sites.len()).filter(|&i| i != current);
    let Some(farthest) = others().map(distance).reduce(f32::max) else {
        return current;
    };
    let threshold = farthest * far_fraction.clamp(0.0, 1.0);
    let pool: Vec<usize> = others().filter(|&i| distance(i) >= threshold).collect();
    let pick = (roll.clamp(0.0, 0.999_999) * pool.len() as f32) as usize;
    pool[pick.min(pool.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::capture::test_tuning;

    fn tuning() -> HeadquartersTuning {
        HeadquartersTuning {
            capture: test_tuning(),
            score_interval_secs: 1.0,
            points_per_tick: 1,
            life_secs: 60.0,
            relocate_secs: 5.0,
            relocate_far_fraction: 0.6,
        }
    }

    fn run(hq: &mut HqState, presence: [u32; 2], secs: f32) -> (u32, Vec<HqEvent>) {
        let (mut points, mut events) = (0, Vec::new());
        for _ in 0..(secs * 10.0).round() as u32 {
            let tick = hq.tick(presence, 0.1, &tuning());
            points += tick.points.iter().sum::<u32>();
            events.extend(tick.event);
        }
        (points, events)
    }

    #[test]
    fn capture_scores_per_second_until_destroyed_then_relocates() {
        let mut hq = HqState::new(vec![Vec3::ZERO, Vec3::X * 50.0], 0, &tuning());
        let (_, events) = run(&mut hq, [1, 0], 10.1);
        assert_eq!(events, [HqEvent::Captured(Team::Alpha)]);
        assert_eq!(hq.owner(), Some(Team::Alpha));
        let (points, _) = run(&mut hq, [0, 0], 10.0);
        assert_eq!(points, 10);
        // Bravo neutralises (5 s for one player) = destroyed.
        let (_, events) = run(&mut hq, [0, 1], 5.2);
        assert_eq!(events, [HqEvent::Destroyed(Team::Alpha)]);
        assert_eq!(hq.position(), None);
        assert_eq!(hq.owner(), None);
        let (_, events) = run(&mut hq, [0, 0], 5.1);
        assert_eq!(events, [HqEvent::Appeared(1)]);
        assert_eq!(hq.position(), Some(Vec3::X * 50.0));
    }

    #[test]
    fn an_untouched_hq_expires_after_its_life() {
        let mut hq = HqState::new(vec![Vec3::ZERO], 0, &tuning());
        let (_, events) = run(&mut hq, [0, 0], 60.1);
        assert_eq!(events, [HqEvent::Expired]);
    }

    #[test]
    fn the_next_site_is_one_of_the_far_ones_never_the_current() {
        // Two sites in the west (0 and 1), two in the east (2 and 3).
        let sites = [Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 100.0, Vec3::X * 110.0];
        // From site 0 the farthest is 110 away; 60% of it is 66, so only the east sites qualify.
        assert_eq!(next_site(&sites, 0, 0.6, 0.0), 2);
        assert_eq!(next_site(&sites, 0, 0.6, 0.99), 3);
        // From site 3 the farthest is 110; the west sites (110 and 100 away) qualify.
        assert_eq!(next_site(&sites, 3, 0.6, 0.0), 0);
        assert_eq!(next_site(&sites, 3, 0.6, 0.99), 1);
        for roll in [0.0, 0.3, 0.6, 0.99] {
            for current in 0..sites.len() {
                assert_ne!(next_site(&sites, current, 0.6, roll), current);
            }
        }
    }

    #[test]
    fn a_zero_fraction_allows_every_other_site_and_a_lone_site_stays() {
        let sites = [Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 100.0];
        assert_eq!(next_site(&sites, 0, 0.0, 0.0), 1);
        assert_eq!(next_site(&sites, 0, 0.0, 0.99), 2);
        assert_eq!(next_site(&[Vec3::ZERO], 0, 0.6, 0.5), 0);
    }

    #[test]
    fn a_seeded_hq_walks_between_far_sites() {
        let sites = vec![Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 100.0, Vec3::X * 110.0];
        let mut hq = HqState::seeded(sites.clone(), 0, &tuning(), 7);
        let mut previous = hq.site;
        for _ in 0..20 {
            let (_, events) = run(&mut hq, [0, 0], 65.2);
            assert!(events.contains(&HqEvent::Appeared(hq.site)));
            // Always a hop of at least 66 m between the two clusters.
            assert!((sites[hq.site] - sites[previous]).length() >= 66.0);
            previous = hq.site;
        }
    }
}
