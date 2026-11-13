//! Custom Match Rules (CONTEXT.md): named rule sets a Host makes in the rules editor, saved to
//! `<config dir>/frontline/rules.ron` and selectable in a Rotation entry next to the built-in
//! presets of `data/match_rules.ron`. The built-in presets are read-only starting points.
//!
//! The server replicates the full rule *values* of the Match (`MatchState::rule_values`), so a
//! client doesn't need the file that defines a custom set.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::data::{GameData, MatchRules};
use crate::flow::{load_config, save_config};

const CUSTOM_RULES_FILE: &str = "rules.ron";

/// Longest name of a custom set, in characters.
pub const MAX_NAME_CHARS: usize = 24;

/// The custom sets by name, as saved in `rules.ron`.
pub type CustomRules = BTreeMap<String, MatchRules>;

impl MatchRules {
    /// Whether these values make a playable Match: everything finite and in a sane range.
    pub fn validate(&self) -> Result<(), String> {
        let check = |name: &str, value: f32, min: f32, max: f32| {
            if value.is_finite() && (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be {min} to {max}, not {value}"))
            }
        };
        check("max_health", self.max_health, 1.0, 1000.0)?;
        check("regen_delay_secs", self.regen_delay_secs, 0.0, 60.0)?;
        check("regen_per_sec", self.regen_per_sec, 0.0, 1000.0)?;
        check("respawn_delay_secs", self.respawn_delay_secs, 0.0, 60.0)?;
        check("min_respawn_secs", self.min_respawn_secs, 0.0, 60.0)?;
        check("killcam_secs", self.killcam_secs, 0.0, 10.0)?;
        check("damage_scale", self.damage_scale, 0.01, 10.0)?;
        check(
            "fall_damage.min_height",
            self.fall_damage.min_height,
            0.0,
            100.0,
        )?;
        check(
            "fall_damage.max_height",
            self.fall_damage.max_height,
            0.0,
            100.0,
        )?;
        if self.fall_damage.max_height <= self.fall_damage.min_height {
            return Err("fall_damage.max_height must be above min_height".to_owned());
        }
        check(
            "hud.kill_feed_delay_secs",
            self.hud.kill_feed_delay_secs,
            0.0,
            60.0,
        )?;
        check(
            "hud.name_tag_range_m",
            self.hud.name_tag_range_m,
            1.0,
            500.0,
        )
    }
}

/// Why `name` can't name a custom set, given the names already `taken`. The name is trimmed
/// first; the caller keeps the trimmed text.
pub fn check_name(name: &str, taken: impl Fn(&str) -> bool) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the set a name".to_owned());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("A name is at most {MAX_NAME_CHARS} characters"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
    {
        return Err("Names use letters, digits, spaces, - _ and .".to_owned());
    }
    if taken(name) {
        return Err(format!("\"{name}\" is already a set"));
    }
    Ok(name.to_owned())
}

/// A free name for a copy of `base`: `base copy`, then `base copy 2`, `base copy 3`... cut to
/// fit `MAX_NAME_CHARS`.
pub fn copy_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let fit = |suffix: &str| {
        let room = MAX_NAME_CHARS - suffix.chars().count();
        let stem: String = base.chars().take(room).collect();
        format!("{}{suffix}", stem.trim_end())
    };
    let first = fit(" copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| fit(&format!(" copy {n}")))
        .find(|name| !taken(name))
        .expect("an unbounded range has a free name")
}

/// The saved custom sets. A set that is invalid, or named like a built-in preset, is ignored
/// with a warning rather than stopping the game.
pub fn load_custom_rules(builtin: &GameData) -> CustomRules {
    let saved: CustomRules = load_config(CUSTOM_RULES_FILE).unwrap_or_default();
    usable(saved, builtin)
}

fn usable(saved: CustomRules, builtin: &GameData) -> CustomRules {
    saved
        .into_iter()
        .filter(|(name, rules)| {
            if builtin.match_rules.contains_key(name) && !builtin.custom_rule_names.contains(name) {
                warn!("{CUSTOM_RULES_FILE}: \"{name}\" is a built-in preset's name; ignored");
                return false;
            }
            match rules.validate() {
                Ok(()) => true,
                Err(err) => {
                    warn!("{CUSTOM_RULES_FILE}: \"{name}\" ignored: {err}");
                    false
                }
            }
        })
        .collect()
}

pub fn save_custom_rules(sets: &CustomRules) {
    save_config(CUSTOM_RULES_FILE, sets);
}

impl GameData {
    /// Replaces the custom sets: afterwards `match_rules` holds the built-in presets plus
    /// `sets`, and `custom_rule_names` says which are custom.
    pub fn use_custom_rules(&mut self, sets: CustomRules) {
        for name in std::mem::take(&mut self.custom_rule_names) {
            self.match_rules.remove(&name);
        }
        for (name, rules) in sets {
            self.custom_rule_names.insert(name.clone());
            self.match_rules.insert(name, rules);
        }
    }

    /// The custom sets, by name.
    pub fn custom_rules(&self) -> CustomRules {
        self.custom_rule_names
            .iter()
            .filter_map(|name| Some((name.clone(), self.match_rules.get(name)?.clone())))
            .collect()
    }

    /// Every set name, built-in presets first (in name order), then the custom ones.
    pub fn rule_set_names(&self) -> Vec<String> {
        let mut builtin: Vec<String> = self
            .match_rules
            .keys()
            .filter(|name| !self.custom_rule_names.contains(*name))
            .cloned()
            .collect();
        builtin.sort_unstable();
        builtin.extend(self.custom_rule_names.iter().cloned());
        builtin
    }

    pub fn is_custom_rules(&self, name: &str) -> bool {
        self.custom_rule_names.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load()
    }

    #[test]
    fn custom_sets_survive_a_ron_round_trip() {
        let data = data();
        let mut tough = data.rules("hardcore").clone();
        tough.max_health = 45.0;
        tough.hud.kill_feed_delay_secs = 2.5;
        let sets = CustomRules::from([("tough".to_owned(), tough)]);
        let text = ron::ser::to_string_pretty(&sets, ron::ser::PrettyConfig::default()).unwrap();
        let back: CustomRules = ron::from_str(&text).unwrap();
        assert_eq!(back, sets);
    }

    #[test]
    fn the_shipped_presets_are_valid() {
        let data = data();
        for (name, rules) in &data.match_rules {
            assert_eq!(rules.validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn broken_values_are_rejected() {
        let good = data().rules("default").clone();
        for broken in [
            MatchRules {
                max_health: 0.0,
                ..good.clone()
            },
            MatchRules {
                damage_scale: f32::NAN,
                ..good.clone()
            },
            MatchRules {
                regen_per_sec: -1.0,
                ..good.clone()
            },
            MatchRules {
                fall_damage: crate::data::FallDamageRules {
                    min_height: 8.0,
                    max_height: 8.0,
                },
                ..good.clone()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }

    #[test]
    fn names_are_checked_and_copies_get_a_free_name() {
        let taken = |name: &str| name == "default" || name == "default copy";
        assert_eq!(check_name("  Mine  ", taken), Ok("Mine".to_owned()));
        assert!(check_name("", taken).is_err());
        assert!(check_name("default", taken).is_err());
        assert!(check_name("no/slashes", taken).is_err());
        assert!(check_name(&"x".repeat(MAX_NAME_CHARS + 1), taken).is_err());
        assert_eq!(copy_name("hardcore", taken), "hardcore copy");
        assert_eq!(copy_name("default", taken), "default copy 2");
        let long = "y".repeat(MAX_NAME_CHARS);
        assert!(copy_name(&long, |_| false).chars().count() <= MAX_NAME_CHARS);
    }

    #[test]
    fn custom_sets_join_the_presets_and_can_be_replaced() {
        let mut data = data();
        let builtin = data.rule_set_names();
        assert_eq!(builtin, ["default", "hardcore"]);
        let mut mine = data.rules("default").clone();
        mine.max_health = 150.0;
        data.use_custom_rules(CustomRules::from([("mine".to_owned(), mine.clone())]));
        assert_eq!(data.rule_set_names(), ["default", "hardcore", "mine"]);
        assert!(data.is_custom_rules("mine") && !data.is_custom_rules("default"));
        assert_eq!(data.rules("mine"), &mine);
        data.use_custom_rules(CustomRules::new());
        assert_eq!(data.rule_set_names(), builtin);
        // A saved set named like a preset doesn't shadow it.
        let shadow = CustomRules::from([("default".to_owned(), mine)]);
        assert!(usable(shadow, &data).is_empty());
    }
}
