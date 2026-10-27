//! Tunable game data (Weapons, Attachments, Perks, Match Rules, movement, presentation, Game
//! Modes, Factions, Loadouts), loaded once at startup.
//!
//! Each file is read from `./data/<file>` relative to the working directory, so it can be
//! edited and picked up on restart without recompiling. When the file isn't there (e.g. the
//! binary was copied on its own), the copy built into the binary is used.

use std::collections::HashMap;
use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::gear::{AmmoPackTuning, Attachment, AttachmentId, Loadout, PERK_SLOTS, Perk, PerkId};
use crate::shared::MatchState;

const WEAPONS_FILE: &str = "weapons.ron";
const MATCH_RULES_FILE: &str = "match_rules.ron";
const MOVEMENT_FILE: &str = "movement.ron";
const PRESENTATION_FILE: &str = "presentation.ron";
const GAME_MODES_FILE: &str = "game_modes.ron";
const FACTIONS_FILE: &str = "factions.ron";
const LOADOUTS_FILE: &str = "loadouts.ron";
const ATTACHMENTS_FILE: &str = "attachments.ron";
const PERKS_FILE: &str = "perks.ron";
const BUILTIN_WEAPONS: &str = include_str!("../../../data/weapons.ron");
const BUILTIN_MATCH_RULES: &str = include_str!("../../../data/match_rules.ron");
const BUILTIN_MOVEMENT: &str = include_str!("../../../data/movement.ron");
const BUILTIN_PRESENTATION: &str = include_str!("../../../data/presentation.ron");
const BUILTIN_GAME_MODES: &str = include_str!("../../../data/game_modes.ron");
const BUILTIN_FACTIONS: &str = include_str!("../../../data/factions.ron");
const BUILTIN_LOADOUTS: &str = include_str!("../../../data/loadouts.ron");
const BUILTIN_ATTACHMENTS: &str = include_str!("../../../data/attachments.ron");
const BUILTIN_PERKS: &str = include_str!("../../../data/perks.ron");

/// The Match Rules preset a server runs when `--rules` isn't given, and the one clients fall back
/// to for a preset their own data lacks.
pub const DEFAULT_MATCH_RULES: &str = "default";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum WeaponId {
    AssaultRifle,
    Smg,
    SniperRifle,
    Pistol,
}

impl WeaponId {
    /// Every Weapon, in the order menus list them.
    pub const ALL: [WeaponId; 4] = [
        WeaponId::AssaultRifle,
        WeaponId::Smg,
        WeaponId::SniperRifle,
        WeaponId::Pistol,
    ];
}

/// How the trigger works: `Auto` fires while held; `Semi` fires once per pull (bolt-action
/// rifles are `Semi` with a low `rpm` acting as the bolt cycle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum FireMode {
    Auto,
    Semi,
}

/// Which first-person gun body a Weapon is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum GunModel {
    Rifle,
    Smg,
    Sniper,
    Pistol,
}

/// The Loadout slot a Weapon goes in (CONTEXT.md: Primary, Secondary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum WeaponClass {
    Primary,
    Secondary,
}

/// The sight a Weapon is aimed through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum SightKind {
    /// Rear aperture and front post.
    #[default]
    Irons,
    RedDot,
    Acog,
    /// A scope tube; aiming looks through its mask.
    Scope,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum GameModeId {
    /// Key `tdm`. (The long names also parse, so `data/game_modes.ron` reads naturally.)
    #[serde(rename = "tdm", alias = "TeamDeathmatch")]
    TeamDeathmatch,
    #[serde(rename = "dom", alias = "Domination")]
    Domination,
    #[serde(rename = "hq", alias = "Headquarters")]
    Headquarters,
    #[serde(rename = "sd", alias = "SearchAndDestroy")]
    SearchAndDestroy,
}

impl GameModeId {
    pub const ALL: [GameModeId; 4] = [
        GameModeId::TeamDeathmatch,
        GameModeId::Domination,
        GameModeId::Headquarters,
        GameModeId::SearchAndDestroy,
    ];

    /// Short key Maps use to say which Game Modes they support (`data/maps.ron`) and which a
    /// spawn point serves (its `modes` property), and that `--mode` and Rotation files use.
    pub fn key(self) -> &'static str {
        match self {
            GameModeId::TeamDeathmatch => "tdm",
            GameModeId::Domination => "dom",
            GameModeId::Headquarters => "hq",
            GameModeId::SearchAndDestroy => "sd",
        }
    }

    pub fn from_key(key: &str) -> Option<GameModeId> {
        GameModeId::ALL.into_iter().find(|mode| mode.key() == key)
    }

    /// Player-facing name.
    pub fn name(self) -> &'static str {
        match self {
            GameModeId::TeamDeathmatch => "Team Deathmatch",
            GameModeId::Domination => "Domination",
            GameModeId::Headquarters => "Headquarters",
            GameModeId::SearchAndDestroy => "Search & Destroy",
        }
    }

    /// Keys of every mode, comma-separated, for error messages.
    pub fn known_keys() -> String {
        GameModeId::ALL.map(GameModeId::key).join(", ")
    }
}

/// Stats of one Weapon. Distances in metres, angles in degrees, times in seconds.
#[derive(Clone, Debug, Deserialize)]
pub struct WeaponStats {
    pub name: String,
    pub fire_mode: FireMode,
    /// Rounds per minute from the hip.
    pub rpm: f32,
    /// Rounds per minute fully aimed down sights; equal to `rpm` for guns that don't slow.
    pub ads_rpm: f32,
    /// Damage up to `falloff_start`, before the Hit Zone multiplier.
    pub damage_near: f32,
    /// Damage from `falloff_end` on, before the Hit Zone multiplier; linear in between.
    pub damage_far: f32,
    pub falloff_start: f32,
    pub falloff_end: f32,
    pub zone_multipliers: ZoneMultipliers,
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
    /// Extra hip spread at full walking speed, scaled by current horizontal speed.
    pub move_spread_deg: f32,
    /// Extra hip spread while airborne.
    pub air_spread_deg: f32,
    /// Exponential rate (1/s) at which movement bloom follows the current speed.
    pub bloom_follow_per_sec: f32,
    /// Spread and recoil multiplier while crouched.
    pub crouch_accuracy_scale: f32,
    /// Spread and recoil multiplier while prone.
    pub prone_accuracy_scale: f32,
    /// Delay after a sprint ends before the weapon can fire or aim.
    pub sprint_out_secs: f32,
    /// Movement speed multiplier while carrying this weapon (its weight).
    pub move_scale: f32,
    /// First-person model (body and sight type).
    pub model: GunModel,
    /// Asset path of the gunshot recording, e.g. `embedded://frontline/sounds/shot_ar.ogg`.
    pub shot_sound: String,
    /// Muzzle in gun space (metres, -Z forward): where the flash and tracers start.
    pub muzzle_offset: (f32, f32, f32),
    /// Centre of the sight in gun space: the red dot's glass, the rear aperture of iron
    /// sights, or a scope's eyepiece. The sight is built around it and, fully aimed, it sits
    /// on the screen centre `sight_eye_relief` in front of the eye, its axis on the aim line.
    pub sight_point: (f32, f32, f32),
    pub sight_eye_relief: f32,
    /// Each shot is followed by working a bolt for the whole fire-rate cooldown: the gun
    /// briefly comes off the sights.
    pub bolt_action: bool,
    /// Aiming down sights looks through a scope (black mask, viewmodel hidden).
    pub scope: bool,
    /// Which Weapons a Loadout can put in which slot.
    pub class: WeaponClass,
    /// The iron sights, used when no Optic is fitted: like `sight_point` and
    /// `sight_eye_relief` (which describe where an Optic sits), plus the front post's z.
    pub iron_point: (f32, f32, f32),
    pub iron_eye_relief: f32,
    pub iron_front_z: f32,
    /// Set by the fitted Attachments (`GameData::effective_weapon`), not by the data file.
    /// The sight in use; `sight_point` and `sight_eye_relief` then describe it.
    #[serde(skip_deserializing)]
    pub sight: SightKind,
    /// Multiplies the gun motion that remains when aimed.
    #[serde(skip_deserializing, default = "one")]
    pub ads_sway_scale: f32,
    /// A Silencer is fitted: no muzzle flash.
    #[serde(skip_deserializing)]
    pub silenced: bool,
    /// Gunshot volume and playback speed multipliers (a Silencer lowers both).
    #[serde(skip_deserializing, default = "one")]
    pub shot_volume: f32,
    #[serde(skip_deserializing, default = "one")]
    pub shot_pitch: f32,
    /// How far a Silencer extends the muzzle beyond the barrel, in metres (already included
    /// in `muzzle_offset`).
    #[serde(skip_deserializing)]
    pub muzzle_length: f32,
}

impl WeaponStats {
    /// Time between rounds at an aim-down-sights blend `ads` (0 hip, 1 fully aimed).
    pub fn seconds_per_shot(&self, ads: f32) -> f32 {
        60.0 / self.rpm.lerp(self.ads_rpm, ads)
    }
}

/// Damage multiplier per Hit Zone (CONTEXT.md: Hit Zone).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct ZoneMultipliers {
    pub head: f32,
    pub upper_torso: f32,
    pub lower_torso: f32,
    /// Arms and legs.
    pub limbs: f32,
}

#[cfg(test)]
impl WeaponStats {
    /// A plain rifle for unit tests; override the fields a test is about.
    pub fn fixture() -> Self {
        Self {
            name: "Test Rifle".into(),
            fire_mode: FireMode::Auto,
            rpm: 480.0,
            ads_rpm: 480.0,
            damage_near: 30.0,
            damage_far: 20.0,
            falloff_start: 10.0,
            falloff_end: 30.0,
            zone_multipliers: ZoneMultipliers {
                head: 2.0,
                upper_torso: 1.0,
                lower_torso: 0.9,
                limbs: 0.5,
            },
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
            move_spread_deg: 0.0,
            air_spread_deg: 0.0,
            bloom_follow_per_sec: 8.0,
            crouch_accuracy_scale: 1.0,
            prone_accuracy_scale: 1.0,
            sprint_out_secs: 0.0,
            move_scale: 1.0,
            model: GunModel::Rifle,
            shot_sound: String::new(),
            bolt_action: false,
            muzzle_offset: (0.0, 0.0, -0.5),
            sight_point: (0.0, 0.08, 0.0),
            sight_eye_relief: 0.15,
            scope: false,
            class: WeaponClass::Primary,
            iron_point: (0.0, 0.06, 0.05),
            iron_eye_relief: 0.1,
            iron_front_z: -0.3,
            sight: SightKind::Irons,
            ads_sway_scale: 1.0,
            silenced: false,
            shot_volume: 1.0,
            shot_pitch: 1.0,
            muzzle_length: 0.0,
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
    /// Whether bullets hurt teammates (and Killstreaks follow the same rule).
    pub friendly_fire: bool,
    pub hud: HudRules,
}

/// Which HUD elements a Match shows (IDEA.md: HUD in Match Rules).
#[derive(Clone, Debug, Deserialize)]
pub struct HudRules {
    pub crosshair: bool,
    /// A white marker on every hit.
    pub hitmarkers: bool,
    /// A red marker on a kill; shown even when `hitmarkers` is off.
    pub kill_hitmarker: bool,
    pub kill_feed: bool,
    /// Kill-feed lines appear this long after the kill (Rising Storm style); the death screen
    /// still names the killer at once.
    pub kill_feed_delay_secs: f32,
    /// Bars pointing toward whoever damaged you.
    pub damage_direction: bool,
    /// Names over friendly players' heads.
    pub name_tags: bool,
}

/// Client-only presentation tuning (seconds).
#[derive(Clone, Debug, Deserialize)]
pub struct PresentationTuning {
    /// How long a dead player's body lies on the ground before it sinks away.
    pub corpse_secs: f32,
    /// Most bodies on the ground at once; the oldest goes first.
    pub max_corpses: usize,
}

/// Movement tuning of the local player (metres, seconds).
#[derive(Clone, Debug, Deserialize)]
pub struct MovementTuning {
    pub walk_speed: f32,
    pub sprint_speed: f32,
    pub crouch_speed: f32,
    pub prone_speed: f32,
    /// Seconds of continuous sprint on a full tank.
    pub sprint_stamina_secs: f32,
    /// Stamina regained per second while not sprinting.
    pub stamina_regen_per_sec: f32,
    /// After running dry, sprint restarts only once this much stamina is back.
    pub sprint_restart_stamina: f32,
    pub ground_accel: f32,
    pub air_accel: f32,
    pub gravity: f32,
    pub terminal_fall_speed: f32,
    pub jump_height: f32,
    /// Speed multiplier right after landing, easing back to 1 over `landing_slow_secs`.
    pub landing_slow_scale: f32,
    pub landing_slow_secs: f32,
    /// Highest ledge that can be mantled, above the feet (or, mid-jump, above where the jump
    /// started).
    pub mantle_max_height: f32,
    /// How far ahead of the body a ledge is looked for.
    pub mantle_reach: f32,
    pub mantle_secs: f32,
    /// Prone pressed mid-sprint dives: a lunge at `dive_speed` for `dive_secs`, with a hop of
    /// `dive_hop_height`.
    pub dive_speed: f32,
    pub dive_secs: f32,
    pub dive_hop_height: f32,
    /// After the lunge: seconds of no movement, firing or standing, looking still allowed.
    pub dive_recover_secs: f32,
    /// After the recovery: seconds before the prone player may change stance (and so sprint).
    pub dive_getup_secs: f32,
}

#[cfg(test)]
impl MovementTuning {
    /// Round numbers for unit tests; override the fields a test is about.
    pub fn fixture() -> Self {
        Self {
            walk_speed: 5.0,
            sprint_speed: 8.0,
            crouch_speed: 3.0,
            prone_speed: 1.0,
            sprint_stamina_secs: 4.0,
            stamina_regen_per_sec: 1.0,
            sprint_restart_stamina: 1.0,
            ground_accel: 60.0,
            air_accel: 8.0,
            gravity: 20.0,
            terminal_fall_speed: 40.0,
            jump_height: 1.0,
            landing_slow_scale: 0.5,
            landing_slow_secs: 0.5,
            mantle_max_height: 1.5,
            mantle_reach: 0.5,
            mantle_secs: 0.5,
            dive_speed: 4.0,
            dive_secs: 0.4,
            dive_hop_height: 0.1,
            dive_recover_secs: 0.5,
            dive_getup_secs: 0.3,
        }
    }
}

/// Win conditions of a Game Mode, plus the tuning of its objective. In Search & Destroy
/// `score_limit` is the rounds a team needs to win and `time_limit_secs` is one round's clock.
#[derive(Clone, Debug, Deserialize)]
pub struct GameModeSettings {
    pub score_limit: u32,
    pub time_limit_secs: u32,
    /// How long the end-of-Match scoreboard shows before the next Match starts.
    pub end_screen_secs: u32,
    #[serde(default)]
    pub domination: Option<DominationTuning>,
    #[serde(default)]
    pub headquarters: Option<HeadquartersTuning>,
    #[serde(default)]
    pub search_and_destroy: Option<SearchAndDestroyTuning>,
}

/// A capturable zone: a cylinder around its entity (Domination Flags, the Headquarters).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct CaptureTuning {
    /// Seconds one player needs to capture a neutral zone.
    pub capture_secs: f32,
    /// Seconds one player needs to neutralise a zone held by the enemy.
    pub neutralize_secs: f32,
    /// Capture speed multiplier cap: `n` players on a zone capture `min(n, max_speedup)` times
    /// as fast as one.
    pub max_speedup: f32,
    /// Horizontal radius of the zone, metres.
    pub zone_radius: f32,
    /// Height of the zone above its origin, metres.
    pub zone_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct DominationTuning {
    pub capture: CaptureTuning,
    /// Every this many seconds each team scores for the Flags it holds.
    pub score_interval_secs: f32,
    pub points_per_flag: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct HeadquartersTuning {
    /// Capturing a neutral HQ and destroying an enemy one.
    pub capture: CaptureTuning,
    pub score_interval_secs: f32,
    pub points_per_tick: u32,
    /// An HQ stays at its site this long, then relocates.
    pub life_secs: f32,
    /// Seconds between an HQ falling (or expiring) and the next one appearing.
    pub relocate_secs: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct SearchAndDestroyTuning {
    /// Attackers and defenders swap after this many rounds.
    pub swap_after_round: u32,
    /// How long the round result shows before the next round.
    pub round_end_secs: f32,
    pub plant_secs: f32,
    pub defuse_secs: f32,
    pub fuse_secs: f32,
    /// A carrier within this horizontal distance of a bomb site can plant, metres.
    pub site_radius: f32,
    /// Walking within this distance of the dropped bomb picks it up.
    pub pickup_radius: f32,
    /// The explosion kills everyone within this distance.
    pub blast_radius: f32,
    /// The planted bomb beeps (positional sound) this often, speeding up to `beep_fast_secs`
    /// over the last `beep_ramp_secs` of the fuse.
    pub beep_secs: f32,
    pub beep_fast_secs: f32,
    pub beep_ramp_secs: f32,
}

/// The identity a Team plays as: name, emblem and accent colour. Purely presentational.
#[derive(Clone, Debug, Deserialize)]
pub struct Faction {
    /// Stable key referenced by Maps (`maps.ron`: each Map's theater picks its two Factions).
    pub id: String,
    pub name: String,
    /// Short form for tight HUD spots (kill feed, score).
    pub short_name: String,
    /// Asset path of the emblem image; built-ins live under `embedded://frontline/icons/`.
    pub icon: String,
    /// Accent colour (linear-ish sRGB 0..1) used on the Faction's card and banners.
    pub color: (f32, f32, f32),
}

/// Layout of `loadouts.ron`.
#[derive(Deserialize)]
struct LoadoutsFile {
    swap_window_secs: f32,
    weapon_switch_secs: f32,
    defaults: Vec<Loadout>,
}

/// Layout of `perks.ron`.
#[derive(Deserialize)]
struct PerksFile {
    perks: HashMap<PerkId, Perk>,
    ammo_pack: AmmoPackTuning,
}

#[derive(Resource, Debug)]
pub struct GameData {
    pub weapons: HashMap<WeaponId, WeaponStats>,
    pub attachments: HashMap<AttachmentId, Attachment>,
    pub perks: HashMap<PerkId, Perk>,
    pub ammo_pack: AmmoPackTuning,
    pub match_rules: HashMap<String, MatchRules>,
    pub movement: MovementTuning,
    pub presentation: PresentationTuning,
    pub game_modes: HashMap<GameModeId, GameModeSettings>,
    /// In file order, so menus list them predictably.
    pub factions: Vec<Faction>,
    /// The Default Loadouts, in file order.
    pub default_loadouts: Vec<Loadout>,
    /// A Loadout chosen within this many seconds of spawning applies at once; later, on
    /// the next spawn.
    pub loadout_swap_secs: f32,
    /// Seconds to lower one Weapon and raise the other.
    pub weapon_switch_secs: f32,
}

impl GameData {
    pub fn weapon(&self, id: WeaponId) -> &WeaponStats {
        &self.weapons[&id]
    }

    pub fn game_mode(&self, id: GameModeId) -> &GameModeSettings {
        &self.game_modes[&id]
    }

    pub fn domination(&self) -> &DominationTuning {
        self.game_mode(GameModeId::Domination)
            .domination
            .as_ref()
            .expect("checked at startup")
    }

    pub fn headquarters(&self) -> &HeadquartersTuning {
        self.game_mode(GameModeId::Headquarters)
            .headquarters
            .as_ref()
            .expect("checked at startup")
    }

    pub fn search_and_destroy(&self) -> &SearchAndDestroyTuning {
        self.game_mode(GameModeId::SearchAndDestroy)
            .search_and_destroy
            .as_ref()
            .expect("checked at startup")
    }

    /// The Faction a Team plays as on `map`: Factions belong to the Map's theater
    /// (`maps.ron`). A Map this process doesn't know falls back to the Factions in file order.
    pub fn faction(
        &self,
        maps: &crate::maps::MapRegistry,
        map: &crate::maps::MapId,
        team: crate::shared::Team,
    ) -> &Faction {
        maps.get(map)
            .and_then(|info| self.faction_by_id(&info.factions[team.index()]))
            .unwrap_or(&self.factions[team.index()])
    }

    pub fn faction_by_id(&self, id: &str) -> Option<&Faction> {
        self.factions.iter().find(|faction| faction.id == id)
    }

    /// The Match Rules preset named `preset`, or the default one when this process's data
    /// lacks it (a client with older files than its server). Servers validate their preset at
    /// startup, so for them it never falls back.
    pub fn rules(&self, preset: &str) -> &MatchRules {
        self.match_rules
            .get(preset)
            .unwrap_or_else(|| &self.match_rules[DEFAULT_MATCH_RULES])
    }
}

/// The Match Rules of the current Match, as named in the replicated `MatchState`; the default
/// preset before the Match has arrived.
#[derive(SystemParam)]
pub struct ActiveRules<'w, 's> {
    data: Res<'w, GameData>,
    matches: Query<'w, 's, &'static MatchState>,
}

impl ActiveRules<'_, '_> {
    pub fn get(&self) -> &MatchRules {
        let preset = self
            .matches
            .iter()
            .next()
            .map_or(DEFAULT_MATCH_RULES, |state| state.rules.as_str());
        self.data.rules(preset)
    }
}

pub struct DataPlugin;

impl Plugin for DataPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GameData::load());
    }
}

impl GameData {
    /// Reads every data file (see the module docs) and checks they agree with each other. A
    /// file that is broken or inconsistent stops the game at startup.
    pub fn load() -> GameData {
        let loadouts: LoadoutsFile = load(LOADOUTS_FILE, BUILTIN_LOADOUTS);
        let perks: PerksFile = load(PERKS_FILE, BUILTIN_PERKS);
        let data = GameData {
            weapons: load(WEAPONS_FILE, BUILTIN_WEAPONS),
            attachments: load(ATTACHMENTS_FILE, BUILTIN_ATTACHMENTS),
            perks: perks.perks,
            ammo_pack: perks.ammo_pack,
            match_rules: load(MATCH_RULES_FILE, BUILTIN_MATCH_RULES),
            movement: load(MOVEMENT_FILE, BUILTIN_MOVEMENT),
            presentation: load(PRESENTATION_FILE, BUILTIN_PRESENTATION),
            game_modes: load(GAME_MODES_FILE, BUILTIN_GAME_MODES),
            factions: load(FACTIONS_FILE, BUILTIN_FACTIONS),
            default_loadouts: loadouts.defaults,
            loadout_swap_secs: loadouts.swap_window_secs,
            weapon_switch_secs: loadouts.weapon_switch_secs,
        };
        assert!(
            data.match_rules.contains_key(DEFAULT_MATCH_RULES),
            "{MATCH_RULES_FILE} must define the \"{DEFAULT_MATCH_RULES}\" preset"
        );
        // Two Teams, and the fallback for Maps that name no Factions (`maps::registry`).
        assert!(
            data.factions.len() >= 2,
            "{FACTIONS_FILE} must define at least two Factions"
        );
        for mode in GameModeId::ALL {
            let settings = data
                .game_modes
                .get(&mode)
                .unwrap_or_else(|| panic!("{GAME_MODES_FILE} must define {mode:?}"));
            let tuned = match mode {
                GameModeId::TeamDeathmatch => true,
                GameModeId::Domination => settings.domination.is_some(),
                GameModeId::Headquarters => settings.headquarters.is_some(),
                GameModeId::SearchAndDestroy => settings.search_and_destroy.is_some(),
            };
            assert!(
                tuned,
                "{GAME_MODES_FILE}: {mode:?} is missing its tuning block"
            );
        }
        for id in WeaponId::ALL {
            assert!(
                data.weapons.contains_key(&id),
                "{WEAPONS_FILE} must define {id:?}"
            );
        }
        for class in [WeaponClass::Primary, WeaponClass::Secondary] {
            assert!(
                WeaponId::ALL
                    .into_iter()
                    .any(|id| data.weapon(id).class == class),
                "{WEAPONS_FILE} must define a {class:?} Weapon"
            );
        }
        for id in AttachmentId::ALL {
            assert!(
                data.attachments.contains_key(&id),
                "{ATTACHMENTS_FILE} must define {id:?}"
            );
        }
        for id in PerkId::ALL {
            assert!(
                data.perks.contains_key(&id),
                "{PERKS_FILE} must define {id:?}"
            );
        }
        for slot in 1..=PERK_SLOTS {
            assert!(
                data.perks_in_slot(slot).next().is_some(),
                "{PERKS_FILE} must define a Perk for Perk Slot {slot}"
            );
        }
        assert!(
            data.perks
                .values()
                .all(|perk| (1..=PERK_SLOTS).contains(&perk.slot)),
            "{PERKS_FILE}: a Perk's slot is 1 to {PERK_SLOTS}"
        );
        assert!(
            !data.default_loadouts.is_empty(),
            "{LOADOUTS_FILE} must list at least one Loadout"
        );
        for loadout in &data.default_loadouts {
            assert!(
                *loadout == loadout.clone().validated(&data),
                "{LOADOUTS_FILE}: \"{}\" has a Weapon, Attachment or Perk that doesn't fit its place",
                loadout.name
            );
        }
        data
    }
}

/// Parses `data/<file>` if present, else the built-in copy. A present-but-broken file is a
/// hard error: silently ignoring someone's tuning would be worse than not starting.
pub(crate) fn load<T: for<'de> Deserialize<'de>>(file: &str, builtin: &str) -> T {
    let path = Path::new("data").join(file);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            info!("Loaded {}", path.display());
            ron::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        }
        Err(_) => ron::from_str(builtin).unwrap_or_else(|err| panic!("built-in {file}: {err}")),
    }
}
