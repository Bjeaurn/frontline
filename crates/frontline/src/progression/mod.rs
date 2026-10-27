//! Progression (CONTEXT.md: Level, Unlock, Camo, Challenge): the XP the server awards, the Level
//! curve, what each Level unlocks, and the per-Weapon camo Challenges. The pure rules and the
//! data of `data/progression.ron` live here and are shared by the server (what an event is
//! worth) and the client (Levels, locks, camos); `client` applies awards to the Profile and
//! `hud` shows them.

pub mod client;
mod hud;

use serde::{Deserialize, Serialize};

use crate::data::{GameData, WeaponId};
use crate::gear::{AttachmentId, Loadout, NO_CAMO, PerkId};

pub use client::{MatchXp, ProgressionPlugin};

/// Why the server awarded XP; the client shows its label and counts kills per Weapon from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum XpReason {
    /// An enemy killed with this Weapon.
    Kill(WeaponId),
    /// The kill above was a headshot (paid on top of it).
    Headshot(WeaponId),
    Assist,
    /// The `n`th kill in a row within the Multikill window.
    Multikill(u32),
    FlagCapture,
    FlagDefend,
    HqCapture,
    HqDestroy,
    BombPlant,
    BombDefuse,
    MatchWin,
    MatchPlayed,
}

impl XpReason {
    /// Text of the HUD popup and the log.
    pub fn label(self) -> String {
        match self {
            XpReason::Kill(_) => "Kill".into(),
            XpReason::Headshot(_) => "Headshot".into(),
            XpReason::Assist => "Assist".into(),
            XpReason::Multikill(2) => "Double Kill".into(),
            XpReason::Multikill(3) => "Triple Kill".into(),
            XpReason::Multikill(4) => "Quad Kill".into(),
            XpReason::Multikill(n) => format!("Multikill x{n}"),
            XpReason::FlagCapture => "Flag Captured".into(),
            XpReason::FlagDefend => "Flag Defended".into(),
            XpReason::HqCapture => "HQ Captured".into(),
            XpReason::HqDestroy => "HQ Destroyed".into(),
            XpReason::BombPlant => "Bomb Planted".into(),
            XpReason::BombDefuse => "Bomb Defused".into(),
            XpReason::MatchWin => "Match Won".into(),
            XpReason::MatchPlayed => "Match Played".into(),
        }
    }
}

/// What each kind of event is worth, in XP.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct XpTuning {
    pub kill: u32,
    pub headshot: u32,
    /// An Assist pays `damage * assist_per_damage`.
    pub assist_per_damage: f32,
    /// Damage below this earns no Assist.
    pub assist_min_damage: f32,
    pub multikill_window_secs: f32,
    /// Bonus for the 2nd, 3rd, ... kill of a chain; the last entry repeats for longer chains.
    pub multikill_bonuses: Vec<u32>,
    pub flag_capture: u32,
    pub flag_defend_kill: u32,
    pub hq_capture: u32,
    pub hq_destroy: u32,
    pub bomb_plant: u32,
    pub bomb_defuse: u32,
    pub match_win: u32,
    pub match_played: u32,
}

impl XpTuning {
    /// The Assist for `damage` dealt to a victim someone else killed, if it is worth one.
    pub fn assist(&self, damage: f32) -> Option<u32> {
        if damage < self.assist_min_damage {
            return None;
        }
        let xp = (damage * self.assist_per_damage).round() as u32;
        (xp > 0).then_some(xp)
    }

    /// The bonus for the `count`th kill of a chain (`count` of 2 is a double kill).
    pub fn multikill_bonus(&self, count: u32) -> Option<u32> {
        let step = count.checked_sub(2)? as usize;
        self.multikill_bonuses
            .get(step)
            .or(self.multikill_bonuses.last())
            .copied()
    }
}

/// How XP maps to Levels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct LevelCurve {
    /// Reaching Level n+1 takes `n * xp_per_level` XP in all.
    pub xp_per_level: u32,
    pub max_level: u32,
}

/// Where an XP total stands on the curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelProgress {
    pub level: u32,
    /// XP earned since this Level began.
    pub into_level: u32,
    /// XP between this Level and the next; 0 at the maximum Level.
    pub span: u32,
}

impl LevelProgress {
    pub fn maxed(&self) -> bool {
        self.span == 0
    }

    /// How far through the Level, 0 to 1 (1 at the maximum Level).
    pub fn fraction(&self) -> f32 {
        if self.maxed() {
            1.0
        } else {
            self.into_level as f32 / self.span as f32
        }
    }
}

impl LevelCurve {
    /// Total XP at which `level` is reached.
    pub fn xp_to_reach(&self, level: u32) -> u32 {
        level.saturating_sub(1).saturating_mul(self.xp_per_level)
    }

    pub fn level_for(&self, xp: u32) -> u32 {
        (xp / self.xp_per_level + 1).min(self.max_level)
    }

    pub fn progress(&self, xp: u32) -> LevelProgress {
        let level = self.level_for(xp);
        if level >= self.max_level {
            return LevelProgress {
                level,
                into_level: 0,
                span: 0,
            };
        }
        LevelProgress {
            level,
            into_level: xp - self.xp_to_reach(level),
            span: self.xp_per_level,
        }
    }
}

/// Something a Level unlocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum UnlockItem {
    Weapon(WeaponId),
    Attachment(AttachmentId),
    Perk(PerkId),
    /// Editing and choosing the Profile's custom Loadouts.
    CustomLoadouts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Unlock {
    pub level: u32,
    pub item: UnlockItem,
}

/// What a Weapon must do to unlock a camo, counted on that Weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Challenge {
    Kills(u32),
    /// Kills where the killing shot hit the head.
    Headshots(u32),
    /// Every camo that has its own Challenge.
    AllOthers,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Camo {
    /// Stable key stored in a Loadout; never `none`.
    pub id: String,
    pub name: String,
    /// Under `assets/camos/`.
    pub texture: String,
    /// Colour the editor shows for it.
    pub swatch: (f32, f32, f32),
    #[serde(default)]
    pub metallic: f32,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    pub challenge: Challenge,
}

fn default_roughness() -> f32 {
    0.7
}

/// What the Profile counts per Weapon for camo Challenges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponRecord {
    #[serde(default)]
    pub kills: u32,
    /// Kills whose killing shot hit the head.
    #[serde(default)]
    pub headshots: u32,
}

/// How far a Weapon is on a camo's Challenge: unlocked when `current >= target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CamoProgress {
    pub current: u32,
    pub target: u32,
    /// What is counted: "kills", "headshots" or "camos".
    pub unit: &'static str,
}

impl CamoProgress {
    pub fn done(&self) -> bool {
        self.current >= self.target
    }

    /// `34/50 kills`, never past the target.
    pub fn text(&self) -> String {
        format!(
            "{}/{} {}",
            self.current.min(self.target),
            self.target,
            self.unit
        )
    }
}

/// `data/progression.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Progression {
    pub xp: XpTuning,
    pub levels: LevelCurve,
    pub unlocks: Vec<Unlock>,
    pub camos: Vec<Camo>,
}

impl Progression {
    /// Panics (stopping the game at startup) on a file that contradicts itself.
    pub fn check(&self, file: &str) {
        assert!(
            self.levels.xp_per_level > 0,
            "{file}: xp_per_level must be above 0"
        );
        assert!(
            self.levels.max_level >= 1,
            "{file}: max_level must be at least 1"
        );
        assert!(
            !self.xp.multikill_bonuses.is_empty(),
            "{file}: multikill_bonuses needs at least one entry"
        );
        for unlock in &self.unlocks {
            assert!(
                (1..=self.levels.max_level).contains(&unlock.level),
                "{file}: {:?} unlocks at Level {}, outside 1 to {}",
                unlock.item,
                unlock.level,
                self.levels.max_level
            );
        }
        for (index, camo) in self.camos.iter().enumerate() {
            assert!(
                camo.id != NO_CAMO && !camo.id.is_empty(),
                "{file}: a camo id can't be \"{NO_CAMO}\" or empty"
            );
            assert!(
                self.camos[..index].iter().all(|other| other.id != camo.id),
                "{file}: camo \"{}\" is listed twice",
                camo.id
            );
        }
    }

    /// The Level `item` unlocks at; 1 for an item the table doesn't list.
    pub fn unlock_level(&self, item: UnlockItem) -> u32 {
        self.unlocks
            .iter()
            .find(|unlock| unlock.item == item)
            .map_or(1, |unlock| unlock.level)
    }

    /// Whether a player at `level` may use `item`.
    pub fn is_unlocked(&self, item: UnlockItem, level: u32) -> bool {
        level >= self.unlock_level(item)
    }

    /// What reaching exactly `level` unlocks (nothing for Level 1: that is the starting set).
    pub fn unlocked_at(&self, level: u32) -> impl Iterator<Item = UnlockItem> + '_ {
        self.unlocks
            .iter()
            .filter(move |unlock| unlock.level == level && level > 1)
            .map(|unlock| unlock.item)
    }

    pub fn camo(&self, id: &str) -> Option<&Camo> {
        self.camos.iter().find(|camo| camo.id == id)
    }

    /// How far `record` is on `camo`'s Challenge.
    pub fn camo_progress(&self, camo: &Camo, record: &WeaponRecord) -> CamoProgress {
        match camo.challenge {
            Challenge::Kills(target) => CamoProgress {
                current: record.kills,
                target,
                unit: "kills",
            },
            Challenge::Headshots(target) => CamoProgress {
                current: record.headshots,
                target,
                unit: "headshots",
            },
            Challenge::AllOthers => {
                let others = self
                    .camos
                    .iter()
                    .filter(|other| other.challenge != Challenge::AllOthers);
                CamoProgress {
                    current: others
                        .clone()
                        .filter(|other| self.camo_progress(other, record).done())
                        .count() as u32,
                    target: others.count() as u32,
                    unit: "camos",
                }
            }
        }
    }

    /// Whether a Weapon with `record` has unlocked the camo `id` (`none` always is; an
    /// unknown id never is).
    pub fn camo_unlocked(&self, id: &str, record: &WeaponRecord) -> bool {
        id == NO_CAMO
            || self
                .camo(id)
                .is_some_and(|camo| self.camo_progress(camo, record).done())
    }

    /// The camos `after` has unlocked that `before` hadn't.
    pub fn newly_unlocked(&self, before: &WeaponRecord, after: &WeaponRecord) -> Vec<&Camo> {
        self.camos
            .iter()
            .filter(|camo| {
                self.camo_progress(camo, after).done() && !self.camo_progress(camo, before).done()
            })
            .collect()
    }
}

impl GameData {
    /// Player-facing name of an unlockable.
    pub fn unlock_name(&self, item: UnlockItem) -> String {
        match item {
            UnlockItem::Weapon(id) => self.weapon(id).name.clone(),
            UnlockItem::Attachment(id) => self.attachment(id).name.clone(),
            UnlockItem::Perk(id) => self.perk(id).name.clone(),
            UnlockItem::CustomLoadouts => "Custom classes".into(),
        }
    }

    /// Everything in `loadout` that a player at `level` with `primary_record` (the Primary's
    /// camo counters) hasn't unlocked, one text each. Empty when it is all theirs.
    pub fn loadout_locks(
        &self,
        loadout: &Loadout,
        level: u32,
        primary_record: &WeaponRecord,
    ) -> Vec<String> {
        let progression = &self.progression;
        let items = [
            UnlockItem::Weapon(loadout.primary),
            UnlockItem::Weapon(loadout.secondary),
        ]
        .into_iter()
        .chain(loadout.attachments.iter().map(UnlockItem::Attachment))
        .chain(loadout.perks.iter().copied().map(UnlockItem::Perk));
        let mut locks: Vec<String> = items
            .filter(|item| !progression.is_unlocked(*item, level))
            .map(|item| {
                format!(
                    "{} unlocks at Level {}",
                    self.unlock_name(item),
                    progression.unlock_level(item)
                )
            })
            .collect();
        if !progression.camo_unlocked(&loadout.camo, primary_record)
            && let Some(camo) = progression.camo(&loadout.camo)
        {
            locks.push(format!(
                "{} camo locked ({})",
                camo.name,
                progression.camo_progress(camo, primary_record).text()
            ));
        }
        locks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load()
    }

    #[test]
    fn levels_follow_the_curve_and_stop_at_the_maximum() {
        let curve = LevelCurve {
            xp_per_level: 1500,
            max_level: 30,
        };
        assert_eq!(curve.level_for(0), 1);
        assert_eq!(curve.level_for(1499), 1);
        assert_eq!(curve.level_for(1500), 2);
        assert_eq!(curve.level_for(4500), 4);
        assert_eq!(curve.level_for(u32::MAX), 30);
        assert_eq!(curve.xp_to_reach(1), 0);
        assert_eq!(curve.xp_to_reach(3), 3000);

        let mid = curve.progress(2250);
        assert_eq!((mid.level, mid.into_level, mid.span), (2, 750, 1500));
        assert!((mid.fraction() - 0.5).abs() < 1e-6);
        let top = curve.progress(curve.xp_to_reach(30) + 5000);
        assert!(top.maxed() && top.level == 30 && top.fraction() == 1.0);
    }

    #[test]
    fn assists_scale_with_damage_and_multikills_with_the_chain() {
        let xp = data().progression.xp;
        assert_eq!(xp.assist(99.0), Some(99));
        assert_eq!(xp.assist(xp.assist_min_damage - 1.0), None);
        assert_eq!(xp.multikill_bonus(1), None);
        assert_eq!(xp.multikill_bonus(2), Some(50));
        assert_eq!(xp.multikill_bonus(3), Some(100));
        // Beyond the table the last bonus repeats.
        assert_eq!(xp.multikill_bonus(40), xp.multikill_bonuses.last().copied());
    }

    #[test]
    fn the_shipped_table_unlocks_the_starting_kit_then_the_rest_in_order() {
        let data = data();
        let progression = &data.progression;
        for id in WeaponId::ALL {
            assert!(progression.is_unlocked(UnlockItem::Weapon(id), 1), "{id:?}");
        }
        assert!(progression.is_unlocked(UnlockItem::Attachment(AttachmentId::RedDot), 1));
        for slot in 1..=crate::gear::PERK_SLOTS {
            assert!(
                data.perks_in_slot(slot)
                    .any(|perk| progression.is_unlocked(UnlockItem::Perk(perk), 1)),
                "Perk Slot {slot} has a Perk at Level 1"
            );
        }
        assert!(!progression.is_unlocked(UnlockItem::CustomLoadouts, 1));
        assert!(progression.is_unlocked(UnlockItem::CustomLoadouts, 2));
        let acog = progression.unlock_level(UnlockItem::Attachment(AttachmentId::Acog));
        let silencer = progression.unlock_level(UnlockItem::Attachment(AttachmentId::Silencer));
        assert!((3..=8).contains(&acog) && (3..=8).contains(&silencer));
        assert_eq!(
            progression.unlocked_at(2).collect::<Vec<_>>(),
            [UnlockItem::CustomLoadouts]
        );
        // Level 1 is the starting set, not an unlock.
        assert_eq!(progression.unlocked_at(1).count(), 0);
    }

    #[test]
    fn a_camo_unlocks_when_its_challenge_is_met_and_gold_needs_the_rest() {
        let data = data();
        let progression = &data.progression;
        let record = |kills, headshots| WeaponRecord { kills, headshots };
        let woodland = progression.camo("woodland").unwrap();
        assert_eq!(woodland.challenge, Challenge::Kills(25));
        assert!(!progression.camo_unlocked("woodland", &record(24, 0)));
        assert!(progression.camo_unlocked("woodland", &record(25, 0)));
        assert_eq!(
            progression.camo_progress(woodland, &record(10, 0)).text(),
            "10/25 kills"
        );
        assert!(progression.camo_unlocked(NO_CAMO, &WeaponRecord::default()));
        assert!(!progression.camo_unlocked("no-such-camo", &record(9999, 9999)));

        assert!(!progression.camo_unlocked("gold", &record(100, 0)));
        let all = record(100, 50);
        assert!(progression.camo_unlocked("gold", &all));
        let names: Vec<_> = progression
            .newly_unlocked(&record(24, 0), &record(25, 0))
            .iter()
            .map(|camo| camo.id.as_str())
            .collect();
        assert_eq!(names, ["woodland"]);
    }

    #[test]
    fn a_loadout_reports_what_the_player_has_not_unlocked() {
        let data = data();
        let acog = Loadout {
            name: "Scoped".into(),
            primary: WeaponId::AssaultRifle,
            attachments: crate::gear::Attachments {
                optic: Some(AttachmentId::Acog),
                muzzle: None,
            },
            secondary: WeaponId::Pistol,
            perks: [
                PerkId::SleightOfHand,
                PerkId::StoppingPower,
                PerkId::Marathon,
            ],
            camo: "tiger".into(),
        };
        let none = WeaponRecord::default();
        let locks = data.loadout_locks(&acog, 1, &none);
        assert_eq!(locks.len(), 2, "{locks:?}");
        assert!(locks[0].contains("ACOG") && locks[0].contains("Level 3"));
        assert!(locks[1].contains("Tiger") && locks[1].contains("0/100 kills"));
        let tiger = WeaponRecord {
            kills: 100,
            headshots: 0,
        };
        assert!(data.loadout_locks(&acog, 3, &tiger).is_empty());
    }
}
