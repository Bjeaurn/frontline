//! The Rotation (CONTEXT.md): the ordered list of Map + Game Mode + Match Rules entries a server
//! cycles through, one per Match, plus the Host's remembered setup.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::{DEFAULT_MATCH_RULES, GameData, GameModeId};
use crate::maps::{DEFAULT_MAP, MapId, MapRegistry};

/// Most players a server accepts.
pub const MAX_PLAYERS: usize = 16;

/// One entry of a Rotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotationEntry {
    pub map: MapId,
    pub mode: GameModeId,
    /// Match Rules preset (`data/match_rules.ron`).
    #[serde(default = "default_rules")]
    pub rules: String,
}

fn default_rules() -> String {
    DEFAULT_MATCH_RULES.to_owned()
}

impl RotationEntry {
    pub fn new(map: &str, mode: GameModeId, rules: &str) -> Self {
        Self {
            map: MapId::new(map),
            mode,
            rules: rules.to_owned(),
        }
    }
}

/// A Host's replacement for a Game Mode's `score_limit` and `time_limit_secs`
/// (`data/game_modes.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitOverride {
    pub score_limit: u32,
    pub time_limit_secs: u32,
}

/// What the Host setup screen edits and Settings remembers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostSetup {
    /// Empty: the player's own name.
    pub server_name: String,
    pub max_players: usize,
    pub rotation: Vec<RotationEntry>,
    pub limits: HashMap<GameModeId, LimitOverride>,
}

impl Default for HostSetup {
    fn default() -> Self {
        Self {
            server_name: String::new(),
            max_players: MAX_PLAYERS,
            rotation: vec![RotationEntry::new(
                DEFAULT_MAP,
                GameModeId::TeamDeathmatch,
                DEFAULT_MATCH_RULES,
            )],
            limits: HashMap::new(),
        }
    }
}

impl HostSetup {
    pub fn to_config(&self) -> ServerConfig {
        ServerConfig::new(self.rotation.clone(), self.limits.clone(), self.max_players)
    }
}

/// What this process's server plays: the Rotation and where it stands, per-mode limit overrides
/// and the player cap. Inserted at startup (from the command line) and replaced by the Host
/// setup screen; the server reads it when it starts and at every Match end.
#[derive(Resource, Clone, Debug)]
pub struct ServerConfig {
    pub rotation: Vec<RotationEntry>,
    pub limits: HashMap<GameModeId, LimitOverride>,
    pub max_players: usize,
    index: usize,
}

impl ServerConfig {
    /// `rotation` must not be empty.
    pub fn new(
        rotation: Vec<RotationEntry>,
        limits: HashMap<GameModeId, LimitOverride>,
        max_players: usize,
    ) -> Self {
        assert!(!rotation.is_empty(), "a Rotation needs at least one entry");
        Self {
            rotation,
            limits,
            max_players,
            index: 0,
        }
    }

    /// A one-entry Rotation: a plain one-map game.
    pub fn single(entry: RotationEntry) -> Self {
        Self::new(vec![entry], HashMap::new(), MAX_PLAYERS)
    }

    /// Back to the first entry (a server starting).
    pub fn restart(&mut self) {
        self.index = 0;
    }

    pub fn current(&self) -> &RotationEntry {
        &self.rotation[self.index]
    }

    /// The entry after the current one; a single entry repeats.
    pub fn upcoming(&self) -> &RotationEntry {
        &self.rotation[(self.index + 1) % self.rotation.len()]
    }

    pub fn advance(&mut self) {
        self.index = (self.index + 1) % self.rotation.len();
    }

    /// `(score_limit, time_limit_secs)` of `mode`: the Host's override, else the data's.
    pub fn limits(&self, data: &GameData, mode: GameModeId) -> (u32, u32) {
        match self.limits.get(&mode) {
            Some(limit) => (limit.score_limit, limit.time_limit_secs),
            None => {
                let settings = data.game_mode(mode);
                (settings.score_limit, settings.time_limit_secs)
            }
        }
    }

    /// Checks every entry against the Maps and presets this process has.
    pub fn validate(&self, data: &GameData, maps: &MapRegistry) -> Result<(), String> {
        for entry in &self.rotation {
            let Some(info) = maps.get(&entry.map) else {
                return Err(format!(
                    "Unknown Map \"{}\" in the Rotation; known: {}",
                    entry.map,
                    maps.known_ids()
                ));
            };
            if !info.supports(entry.mode) {
                return Err(format!(
                    "Map \"{}\" doesn't support {} ({}); it has: {}",
                    entry.map,
                    entry.mode.name(),
                    entry.mode.key(),
                    info.modes.join(", ")
                ));
            }
            if !data.match_rules.contains_key(&entry.rules) {
                let mut known: Vec<&str> = data.match_rules.keys().map(String::as_str).collect();
                known.sort_unstable();
                return Err(format!(
                    "Unknown Match Rules preset \"{}\" in the Rotation; known: {}",
                    entry.rules,
                    known.join(", ")
                ));
            }
        }
        Ok(())
    }
}

/// Reads a Rotation file: a RON list such as
/// `[(map: "arena", mode: tdm, rules: "default"), (map: "crossroads", mode: dom)]`.
pub fn load_rotation_file(path: &Path) -> Result<Vec<RotationEntry>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let entries: Vec<RotationEntry> =
        ron::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
    if entries.is_empty() {
        return Err(format!("{}: the Rotation has no entries", path.display()));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_advances_and_wraps_and_a_single_entry_repeats() {
        let entry = |map: &str| RotationEntry::new(map, GameModeId::TeamDeathmatch, "default");
        let mut config =
            ServerConfig::new(vec![entry("arena"), entry("crossroads")], HashMap::new(), 8);
        assert_eq!(config.upcoming().map, MapId::new("crossroads"));
        config.advance();
        assert_eq!(config.current().map, MapId::new("crossroads"));
        config.advance();
        assert_eq!(config.current().map, MapId::new("arena"));
        let mut single = ServerConfig::single(entry("arena"));
        single.advance();
        assert_eq!(single.current().map, MapId::new("arena"));
    }

    #[test]
    fn rotation_files_use_mode_keys_and_default_the_rules() {
        let entries: Vec<RotationEntry> = ron::from_str(
            r#"[(map: "arena", mode: tdm), (map: "crossroads", mode: dom, rules: "hardcore")]"#,
        )
        .unwrap();
        assert_eq!(entries[0].rules, "default");
        assert_eq!(entries[1].mode, GameModeId::Domination);
    }
}
