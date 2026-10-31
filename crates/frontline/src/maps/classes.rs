//! The entity classes a Map can place (TrenchBroom point classes) and its worldspawn settings.
//! Each becomes a component on the spawned entity; Game Modes look them up from there.

use bevy::prelude::*;
use bevy_trenchbroom::anyhow;
use bevy_trenchbroom::class::{ChoicesKey, QuakeClassPropertyType};
use bevy_trenchbroom::fgd::FgdType;
use bevy_trenchbroom::prelude::*;

/// Implements [`FgdType`] for a unit enum as a TrenchBroom dropdown of lowercase keys.
macro_rules! lowercase_choices {
    ($ty:ident { $($variant:ident => $key:literal, $title:literal;)* }) => {
        impl FgdType for $ty {
            const PROPERTY_TYPE: QuakeClassPropertyType =
                QuakeClassPropertyType::Choices(&[$((ChoicesKey::String($key), $title)),*]);

            fn fgd_parse(input: &str) -> anyhow::Result<Self> {
                match input {
                    $($key => Ok(Self::$variant),)*
                    other => Err(anyhow::anyhow!(
                        "{other} isn't a valid {}; expected one of: {}",
                        stringify!($ty),
                        [$($key),*].join(", ")
                    )),
                }
            }

            fn fgd_to_string_unquoted(&self) -> String {
                match self {
                    $(Self::$variant => $key,)*
                }
                .to_owned()
            }
        }
    };
}

/// Which Team may spawn at a [`SpawnPoint`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum SpawnTeam {
    Alpha,
    Bravo,
    #[default]
    Any,
}

lowercase_choices!(SpawnTeam {
    Alpha => "alpha", "Alpha";
    Bravo => "bravo", "Bravo";
    Any => "any", "Either Team";
});

/// Where a player may (re)spawn: the entity's origin is the feet, its `angle` the facing.
#[point_class(size(-16 -16 0, 16 16 72), color(40 220 90))]
#[derive(Clone, Debug, Default)]
pub struct SpawnPoint {
    /// Team allowed to spawn here.
    pub team: SpawnTeam,
    /// Game Mode keys this spawn serves, separated by spaces or commas (tdm dom hq sd); empty
    /// means every mode.
    pub modes: String,
}

impl SpawnPoint {
    /// The Game Mode keys of [`Self::modes`]; empty means every mode.
    pub fn mode_keys(&self) -> Vec<String> {
        self.modes
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|key| !key.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    }
}

/// Label of a Domination Flag.
#[derive(FgdType, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum FlagLabel {
    #[default]
    A,
    B,
    C,
}

impl FlagLabel {
    pub fn letter(self) -> char {
        match self {
            FlagLabel::A => 'A',
            FlagLabel::B => 'B',
            FlagLabel::C => 'C',
        }
    }
}

/// A Domination Flag; the origin is the centre of its capture area on the ground.
#[point_class(size(-32 -32 0, 32 32 96), color(250 210 40))]
#[derive(Clone, Debug, Default)]
pub struct DomFlag {
    pub label: FlagLabel,
}

/// A Headquarters site; the origin is the centre of the HQ on the ground.
#[point_class(size(-32 -32 0, 32 32 64), color(170 90 240))]
#[derive(Clone, Debug, Default)]
pub struct HqSite;

/// A fixed ammo crate (Team Deathmatch only; every other Game Mode ignores it): a player walking
/// up to it is fully resupplied, then it stays closed for a while. The origin is the centre of its
/// base on the ground.
#[point_class(size(-16 -12 0, 16 12 24), color(110 150 60))]
#[derive(Clone, Debug, Default)]
pub struct AmmoCrate;

/// Label of a Search & Destroy bomb site.
#[derive(FgdType, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum BombSiteLabel {
    #[default]
    A,
    B,
}

impl BombSiteLabel {
    pub fn letter(self) -> char {
        match self {
            BombSiteLabel::A => 'A',
            BombSiteLabel::B => 'B',
        }
    }
}

/// A Search & Destroy bomb site; the origin is where the bomb is planted.
#[point_class(size(-24 -24 0, 24 24 48), color(240 60 50))]
#[derive(Clone, Debug, Default)]
pub struct BombSite {
    pub label: BombSiteLabel,
}

/// How a [`Prop`] collides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum PropCollider {
    /// Decoration only: bullets and players pass through.
    #[default]
    None,
    /// One box around the whole model.
    Box,
    /// A convex hull per mesh of the model.
    Convex,
}

lowercase_choices!(PropCollider {
    None => "none", "None (decoration)";
    Box => "box", "Bounding box";
    Convex => "convex", "Convex hull per mesh";
});

/// A glTF model placed in the Map.
#[point_class(model({ "path": model }), size(-8 -8 0, 8 8 16), color(150 150 150))]
#[derive(Clone, Debug, Default)]
pub struct Prop {
    /// Asset path of a glTF file (its first scene is spawned), e.g. `models/barrel.glb`.
    #[class(must_set)]
    pub model: String,
    pub collider: PropCollider,
}

/// Map-wide settings.
#[solid_class(classname("worldspawn"))]
#[derive(Clone, Debug, Default)]
pub struct Worldspawn {
    /// Look preset from `data/looks.ron`, e.g. `desert_dust`. `data/maps.ron` may override it.
    pub look: String,
    /// Display name.
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_modes_split_on_spaces_and_commas() {
        let spawn = SpawnPoint {
            team: SpawnTeam::Any,
            modes: " tdm, DOM  hq,".into(),
        };
        assert_eq!(spawn.mode_keys(), ["tdm", "dom", "hq"]);
        assert!(SpawnPoint::default().mode_keys().is_empty());
    }

    #[test]
    fn choices_parse_lowercase_keys() {
        assert_eq!(SpawnTeam::fgd_parse("bravo").unwrap(), SpawnTeam::Bravo);
        assert!(SpawnTeam::fgd_parse("Bravo").is_err());
        assert_eq!(
            PropCollider::fgd_parse("convex").unwrap(),
            PropCollider::Convex
        );
        assert_eq!(SpawnTeam::Any.fgd_to_string(), "\"any\"");
        assert_eq!(FlagLabel::fgd_parse("C").unwrap(), FlagLabel::C);
    }
}
