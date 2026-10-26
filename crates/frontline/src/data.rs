//! Tunable game data (Weapon stats, Match Rules presets), loaded once at startup.
//!
//! Each file is read from `./data/<file>` relative to the working directory, so it can be
//! edited and picked up on restart without recompiling. When the file isn't there (e.g. the
//! binary was copied on its own), the copy built into the binary is used.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const WEAPONS_FILE: &str = "weapons.ron";
const MATCH_RULES_FILE: &str = "match_rules.ron";
const BUILTIN_WEAPONS: &str = include_str!("../../../data/weapons.ron");
const BUILTIN_MATCH_RULES: &str = include_str!("../../../data/match_rules.ron");

/// The preset used until Rotations exist.
pub const DEFAULT_MATCH_RULES: &str = "default";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum WeaponId {
    AssaultRifle,
}

/// Stats of one Weapon. Distances in metres, angles in degrees, times in seconds.
#[derive(Clone, Debug, Deserialize)]
pub struct WeaponStats {
    pub name: String,
    pub rpm: f32,
    /// Body-shot damage up to `falloff_start`.
    pub damage_near: f32,
    /// Body-shot damage from `falloff_end` on; linear in between.
    pub damage_far: f32,
    pub falloff_start: f32,
    pub falloff_end: f32,
    pub headshot_multiplier: f32,
    /// Maximum hitscan distance.
    pub range: f32,
    pub magazine: u32,
    pub reload_secs: f32,
    pub hip_spread_deg: f32,
    pub ads_spread_deg: f32,
    pub ads_secs: f32,
    /// Camera FOV multiplier when fully aimed down sights.
    pub ads_fov_scale: f32,
    /// Movement speed multiplier when fully aimed down sights.
    pub ads_move_scale: f32,
    pub recoil_pitch_deg: f32,
    pub recoil_yaw_deg: f32,
    /// Exponential decay rate (1/s) at which the aim returns from recoil.
    pub recoil_recovery_per_sec: f32,
    /// Recoil kick multiplier when fully aimed down sights.
    pub ads_recoil_scale: f32,
}

impl WeaponStats {
    pub fn seconds_per_shot(&self) -> f32 {
        60.0 / self.rpm
    }
}

#[cfg(test)]
impl WeaponStats {
    /// A plain rifle for unit tests; override the fields a test is about.
    pub fn fixture() -> Self {
        Self {
            name: "Test Rifle".into(),
            rpm: 480.0,
            damage_near: 30.0,
            damage_far: 20.0,
            falloff_start: 10.0,
            falloff_end: 30.0,
            headshot_multiplier: 2.0,
            range: 200.0,
            magazine: 30,
            reload_secs: 1.0,
            hip_spread_deg: 2.0,
            ads_spread_deg: 0.5,
            ads_secs: 0.2,
            ads_fov_scale: 0.8,
            ads_move_scale: 0.7,
            recoil_pitch_deg: 1.0,
            recoil_yaw_deg: 0.5,
            recoil_recovery_per_sec: 8.0,
            ads_recoil_scale: 0.6,
        }
    }
}

/// Tunable survival settings of a Match (CONTEXT.md: Match Rules).
#[derive(Clone, Debug, Deserialize)]
pub struct MatchRules {
    pub max_health: f32,
    pub regen_delay_secs: f32,
    pub regen_per_sec: f32,
    pub respawn_delay_secs: f32,
    pub damage_scale: f32,
}

#[derive(Resource, Debug)]
pub struct GameData {
    pub weapons: HashMap<WeaponId, WeaponStats>,
    pub match_rules: HashMap<String, MatchRules>,
}

impl GameData {
    pub fn weapon(&self, id: WeaponId) -> &WeaponStats {
        &self.weapons[&id]
    }

    /// The Match Rules currently in effect.
    pub fn rules(&self) -> &MatchRules {
        &self.match_rules[DEFAULT_MATCH_RULES]
    }
}

pub struct DataPlugin;

impl Plugin for DataPlugin {
    fn build(&self, app: &mut App) {
        let data = GameData {
            weapons: load(WEAPONS_FILE, BUILTIN_WEAPONS),
            match_rules: load(MATCH_RULES_FILE, BUILTIN_MATCH_RULES),
        };
        assert!(
            data.match_rules.contains_key(DEFAULT_MATCH_RULES),
            "{MATCH_RULES_FILE} must define the \"{DEFAULT_MATCH_RULES}\" preset"
        );
        app.insert_resource(data);
    }
}

/// Parses `data/<file>` if present, else the built-in copy. A present-but-broken file is a
/// hard error: silently ignoring someone's tuning would be worse than not starting.
fn load<T: for<'de> Deserialize<'de>>(file: &str, builtin: &str) -> T {
    let path = Path::new("data").join(file);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            info!("Loaded {}", path.display());
            ron::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        }
        Err(_) => ron::from_str(builtin)
            .unwrap_or_else(|err| panic!("built-in {file}: {err}")),
    }
}
