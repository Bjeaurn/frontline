//! Choosing where a player (re)spawns.

use bevy::math::Vec3;

/// Picks the index of the best spawn in `spawns` given the positions of living `enemies`.
///
/// Spawns no enemy can see win over seen ones; among equals, the one whose nearest enemy is
/// furthest away wins. `visible(enemy, spawn)` reports whether `enemy` has line of sight to
/// `spawn`. Returns `None` only when `spawns` is empty.
pub fn choose_spawn(
    spawns: &[Vec3],
    enemies: &[Vec3],
    mut visible: impl FnMut(Vec3, Vec3) -> bool,
) -> Option<usize> {
    spawns
        .iter()
        .map(|&spawn| {
            let unseen = !enemies.iter().any(|&enemy| visible(enemy, spawn));
            let nearest = enemies
                .iter()
                .map(|&enemy| enemy.distance(spawn))
                .fold(f32::INFINITY, f32::min);
            (unseen, nearest)
        })
        .enumerate()
        .max_by(|(_, a), (_, b)| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nearest enemy: A 5 m, B 15 m, C 10 m.
    const A: Vec3 = Vec3::ZERO;
    const B: Vec3 = Vec3::new(10.0, 0.0, 0.0);
    const C: Vec3 = Vec3::new(50.0, 0.0, 0.0);
    const ENEMIES: [Vec3; 2] = [Vec3::new(-5.0, 0.0, 0.0), Vec3::new(60.0, 0.0, 0.0)];

    #[test]
    fn unseen_spawn_beats_a_farther_seen_one() {
        let chosen = choose_spawn(&[A, B, C], &ENEMIES, |_, spawn| spawn != C);
        assert_eq!(chosen, Some(2));
    }

    #[test]
    fn all_seen_falls_back_to_largest_distance_to_nearest_enemy() {
        assert_eq!(choose_spawn(&[A, B, C], &ENEMIES, |_, _| true), Some(1));
        assert_eq!(choose_spawn(&[], &ENEMIES, |_, _| true), None);
    }
}
