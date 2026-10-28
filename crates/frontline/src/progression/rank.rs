//! Rank insignia (CONTEXT.md: Rank insignia): the worn cloth patch shown before a player's name
//! on the Scoreboard. Each Level has one, drawn by `tools/ranks/ranks.py` into
//! `assets/icons/ranks/<band>_NN.png`: chevrons for Levels 1-10, bars for 11-20, stars for 21-30,
//! one more of them per Level within a band.

/// Levels per insignia band; also how many chevrons, bars or stars the band's last Level shows.
pub const BAND_SIZE: u32 = 10;

/// The highest Level that has an insignia of its own (higher ones wear the last).
pub const LAST_INSIGNIA_LEVEL: u32 = 3 * BAND_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Chevrons,
    Bars,
    Stars,
}

/// What a Level's patch shows: `count` symbols of a `band`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insignia {
    pub band: Band,
    pub count: u32,
}

/// The insignia of `level`; Levels below 1 wear Level 1's, above the last band the last.
pub fn insignia(level: u32) -> Insignia {
    let index = level.clamp(1, LAST_INSIGNIA_LEVEL) - 1;
    Insignia {
        band: match index / BAND_SIZE {
            0 => Band::Chevrons,
            1 => Band::Bars,
            _ => Band::Stars,
        },
        count: index % BAND_SIZE + 1,
    }
}

impl Band {
    fn file_prefix(self) -> &'static str {
        match self {
            Band::Chevrons => "chevrons",
            Band::Bars => "bars",
            Band::Stars => "stars",
        }
    }
}

/// Asset path (under `assets/`) of `level`'s patch.
pub fn asset_path(level: u32) -> String {
    let Insignia { band, count } = insignia(level);
    format!("icons/ranks/{}_{count:02}.png", band.file_prefix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_and_counts_follow_the_level() {
        let ins = |band, count| Insignia { band, count };
        assert_eq!(insignia(1), ins(Band::Chevrons, 1));
        assert_eq!(insignia(10), ins(Band::Chevrons, 10));
        assert_eq!(insignia(11), ins(Band::Bars, 1));
        assert_eq!(insignia(20), ins(Band::Bars, 10));
        assert_eq!(insignia(21), ins(Band::Stars, 1));
        assert_eq!(insignia(30), ins(Band::Stars, 10));
    }

    #[test]
    fn out_of_range_levels_wear_the_nearest_patch() {
        assert_eq!(insignia(0), insignia(1));
        assert_eq!(insignia(99), insignia(30));
        assert_eq!(asset_path(0), "icons/ranks/chevrons_01.png");
        assert_eq!(asset_path(7), "icons/ranks/chevrons_07.png");
        assert_eq!(asset_path(12), "icons/ranks/bars_02.png");
        assert_eq!(asset_path(99), "icons/ranks/stars_10.png");
    }

    #[test]
    fn every_level_has_a_distinct_patch_file() {
        let paths: std::collections::HashSet<_> =
            (1..=LAST_INSIGNIA_LEVEL).map(asset_path).collect();
        assert_eq!(paths.len(), LAST_INSIGNIA_LEVEL as usize);
    }
}
