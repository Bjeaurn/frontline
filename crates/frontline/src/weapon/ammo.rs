//! The rounds a Weapon carries, as pure rules (CONTEXT.md: Reserve): the magazine in the gun and the
//! Reserve of spare rounds behind it, what a reload moves between them, what a refill (an ammo
//! pack, a Flag crate, a Team Deathmatch crate) adds, and what the HUD says about the supply.
//! Ammo counts live on the client (docs/adr/0002); the server only announces refills.

use crate::data::WeaponStats;

/// Rounds a reload moves from the Reserve into the magazine: what the magazine is missing, but no
/// more than the Reserve holds. A partial magazine takes only what it lacks, the rest stays in
/// the Reserve.
pub(super) fn reload_amount(rounds: u32, reserve: u32, stats: &WeaponStats) -> u32 {
    stats.magazine.saturating_sub(rounds).min(reserve)
}

/// Whether a reload would do anything: the magazine isn't full and the Reserve isn't empty.
pub(super) fn can_reload(rounds: u32, reserve: u32, stats: &WeaponStats) -> bool {
    reload_amount(rounds, reserve, stats) > 0
}

/// Applies a refill of `share` (an `AmmoRefill::share`) to a Weapon's `rounds` and `reserve`:
/// the Reserve gains `share` of its maximum, up to the maximum. A `share` of 1 or more is the full
/// refill: the Reserve is full and so is the magazine.
pub(super) fn refill(rounds: &mut u32, reserve: &mut u32, stats: &WeaponStats, share: f32) {
    *reserve = (*reserve + stats.reserve_share(share)).min(stats.reserve_max());
    if share >= 1.0 {
        *rounds = stats.magazine;
    }
}

/// What the HUD tells about the held Weapon's supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hint {
    /// The magazine is empty and nothing reloads it: press R.
    Reload,
    /// The magazine and the Reserve are both empty.
    NoAmmo,
    /// The Reserve is down to one magazine or less.
    LowAmmo,
}

/// The Reserve is at or below one magazine: the count turns the low-ammo colour.
pub(super) fn reserve_low(reserve: u32, stats: &WeaponStats) -> bool {
    reserve <= stats.magazine
}

/// The hint to show, if any. With Auto reload a pull reloads an empty magazine by itself, so
/// no RELOAD is needed; a reload under way needs none either.
pub(super) fn hint(
    rounds: u32,
    reserve: u32,
    reloading: bool,
    auto_reload: bool,
    stats: &WeaponStats,
) -> Option<Hint> {
    if rounds == 0 && reserve == 0 {
        Some(Hint::NoAmmo)
    } else if rounds == 0 && !reloading && !auto_reload {
        Some(Hint::Reload)
    } else if reserve_low(reserve, stats) {
        Some(Hint::LowAmmo)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> WeaponStats {
        // 30 rounds a magazine, four spare: a Reserve of 120.
        WeaponStats::fixture()
    }

    #[test]
    fn a_partial_magazine_reloads_only_what_is_missing() {
        let stats = stats();
        assert_eq!(reload_amount(12, 120, &stats), 18);
        assert_eq!(reload_amount(0, 120, &stats), 30);
        assert_eq!(
            reload_amount(30, 120, &stats),
            0,
            "a full magazine takes nothing"
        );
    }

    #[test]
    fn the_last_rounds_of_the_reserve_fill_what_they_can() {
        let stats = stats();
        assert_eq!(reload_amount(0, 20, &stats), 20);
        assert_eq!(reload_amount(25, 3, &stats), 3);
        assert_eq!(
            reload_amount(25, 10, &stats),
            5,
            "the unused rounds stay in the Reserve"
        );
    }

    #[test]
    fn with_an_empty_reserve_a_reload_does_nothing() {
        let stats = stats();
        assert_eq!(reload_amount(0, 0, &stats), 0);
        assert!(!can_reload(0, 0, &stats));
        assert!(!can_reload(12, 0, &stats));
        assert!(can_reload(12, 1, &stats));
        assert!(!can_reload(30, 120, &stats));
    }

    #[test]
    fn a_share_adds_to_the_reserve_and_never_past_its_maximum() {
        let stats = stats();
        let (mut rounds, mut reserve) = (7, 10);
        refill(&mut rounds, &mut reserve, &stats, 0.25);
        assert_eq!(
            (rounds, reserve),
            (7, 40),
            "a quarter of 120 is one spare magazine"
        );
        refill(&mut rounds, &mut reserve, &stats, 0.25);
        refill(&mut rounds, &mut reserve, &stats, 0.25);
        refill(&mut rounds, &mut reserve, &stats, 0.25);
        assert_eq!(reserve, 120, "capped at the maximum");
        assert_eq!(rounds, 7, "a partial refill leaves the magazine alone");
    }

    #[test]
    fn a_small_share_still_gives_a_round() {
        let stats = WeaponStats {
            magazine: 5,
            reserve_mags: 4,
            ..WeaponStats::fixture()
        };
        let (mut rounds, mut reserve) = (0, 0);
        refill(&mut rounds, &mut reserve, &stats, 0.01);
        assert_eq!(reserve, 1);
        refill(&mut rounds, &mut reserve, &stats, 0.0);
        assert_eq!(reserve, 1, "no share, no rounds");
    }

    #[test]
    fn the_full_refill_fills_the_reserve_and_the_magazine() {
        let stats = stats();
        let (mut rounds, mut reserve) = (3, 0);
        refill(&mut rounds, &mut reserve, &stats, 1.0);
        assert_eq!((rounds, reserve), (30, 120));
    }

    #[test]
    fn the_reserve_follows_the_effective_magazine() {
        // A Perk or Attachment that changes the magazine size scales the Reserve with it.
        let bigger = WeaponStats {
            magazine: 40,
            ..WeaponStats::fixture()
        };
        assert_eq!(bigger.reserve_max(), 160);
        let (mut rounds, mut reserve) = (0, 0);
        refill(&mut rounds, &mut reserve, &bigger, 1.0);
        assert_eq!((rounds, reserve), (40, 160));
    }

    #[test]
    fn the_hint_follows_the_supply() {
        let stats = stats();
        let hint =
            |rounds, reserve, reloading, auto| hint(rounds, reserve, reloading, auto, &stats);
        assert_eq!(hint(30, 120, false, false), None);
        assert_eq!(hint(0, 120, false, false), Some(Hint::Reload));
        assert_eq!(hint(0, 120, true, false), None, "already reloading");
        assert_eq!(
            hint(0, 120, false, true),
            None,
            "Auto reload does it on a pull"
        );
        assert_eq!(
            hint(5, 30, false, false),
            Some(Hint::LowAmmo),
            "one magazine left"
        );
        assert_eq!(hint(5, 31, false, false), None);
        assert_eq!(hint(5, 0, false, false), Some(Hint::LowAmmo));
        assert_eq!(hint(0, 0, false, false), Some(Hint::NoAmmo));
        assert_eq!(
            hint(0, 0, false, true),
            Some(Hint::NoAmmo),
            "whatever Auto reload says"
        );
    }
}
