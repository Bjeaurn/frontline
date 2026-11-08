//! Copy and paste, linear arrays and prefabs, without any ECS.
//!
//! A *fragment* is a [`MapFile`] holding copies of some items: the worldspawn brushes among
//! them as the brushes of entity 0 (a bare worldspawn, no name or look), every other entity as
//! it is, a brush entity with only the brushes that were selected. The clipboard is a fragment
//! in the Map's own coordinates; a *prefab* is a fragment saved as a `.map` text file (so
//! TrenchBroom opens it too), moved so that its anchor, the middle of its footprint on its
//! lowest point, is the origin. Pasting or placing puts that anchor where it is wanted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::math::DVec3;

use crate::mapfile::{Item, MapEntity, MapFile, PropBounds, round_to};

/// The anchor of a prefab snaps to this grid, metres, so brushes keep the alignment they had.
const ANCHOR_GRID: f64 = 0.25;
/// Most copies one array makes.
pub const MAX_ARRAY: usize = 200;
const NAME_LEN: usize = 48;

/// A prefab file name: letters, digits, `_` and `-`.
pub fn valid_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "\"{name}\" is not a usable prefab name: 1 to {NAME_LEN} of letters, digits, _ and -"
        ))
    }
}

/// The middle of the footprint of `bounds` at its lowest point, on the anchor grid.
pub fn anchor((min, max): (DVec3, DVec3)) -> DVec3 {
    let snap = |v: f64| (v / ANCHOR_GRID).round() * ANCHOR_GRID;
    DVec3::new(
        snap((min.x + max.x) / 2.0),
        round_to(min.y, 1e4),
        snap((min.z + max.z) / 2.0),
    )
}

/// How far to move something anchored at `anchor` so that the anchor lands on `point` (a point
/// on a surface): across the ground in whole `grid` steps, in height exactly.
pub fn paste_offset(anchor: DVec3, point: DVec3, grid: f64) -> DVec3 {
    let step = |from: f64, to: f64| ((to - from) / grid).round() * grid;
    DVec3::new(
        step(anchor.x, point.x),
        round_to(point.y - anchor.y, 1e4),
        step(anchor.z, point.z),
    )
}

impl MapFile {
    /// Copies of `items` as a fragment (see the module docs).
    pub fn extract(&self, items: &[Item]) -> Result<MapFile, String> {
        let units = self.units(items)?;
        if units.is_empty() {
            return Err("nothing to copy".into());
        }
        let mut world = MapEntity::new("worldspawn");
        world.set("mapversion", "220");
        let mut fragment = MapFile {
            entities: vec![world],
        };
        let mut groups: BTreeMap<usize, MapEntity> = BTreeMap::new();
        for unit in units {
            match unit {
                Item::Brush { entity: 0, brush } => {
                    fragment.entities[0]
                        .brushes
                        .push(self.entities[0].brushes[brush].clone());
                }
                Item::Brush { entity, brush } => {
                    groups
                        .entry(entity)
                        .or_insert_with(|| MapEntity {
                            props: self.entities[entity].props.clone(),
                            brushes: Vec::new(),
                        })
                        .brushes
                        .push(self.entities[entity].brushes[brush].clone());
                }
                Item::Entity(entity) => fragment.entities.push(self.entities[entity].clone()),
            }
        }
        fragment.entities.extend(groups.into_values());
        Ok(fragment)
    }

    /// The box around a fragment (or any Map), game space.
    pub fn content_bounds(&self, props: &PropBounds) -> Option<(DVec3, DVec3)> {
        self.bounds_of(&self.items(), props)
    }

    /// Adds a fragment moved by `offset`; returns what it added.
    pub fn insert_fragment(&mut self, fragment: &MapFile, offset: DVec3) -> Vec<Item> {
        let mut added = Vec::new();
        for (index, entity) in fragment.entities.iter().enumerate() {
            if index == 0 {
                for brush in &entity.brushes {
                    let mut brush = brush.clone();
                    brush.translate(offset);
                    added.push(self.add_brush(brush));
                }
                continue;
            }
            let mut copy = entity.clone();
            if copy.brushes.is_empty() {
                if let Some(origin) = copy.origin() {
                    copy.set_origin(origin + offset);
                }
            } else {
                copy.brushes.iter_mut().for_each(|b| b.translate(offset));
            }
            added.push(self.add_entity(copy));
        }
        added
    }

    /// `count` copies of `items`, the k-th moved by `k * step`; returns all copies.
    pub fn linear_array(
        &mut self,
        items: &[Item],
        count: usize,
        step: DVec3,
    ) -> Result<Vec<Item>, String> {
        if count == 0 || count > MAX_ARRAY {
            return Err(format!("an array makes 1 to {MAX_ARRAY} copies"));
        }
        if step.length() < 1e-6 {
            return Err("the offset of an array cannot be zero".into());
        }
        let units = self.units(items)?;
        let mut copies = Vec::new();
        for k in 1..=count {
            copies.extend(self.duplicate_many(&units, step * k as f64)?);
        }
        Ok(copies)
    }
}

/// The file of prefab `name` in `dir`.
pub fn file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.map"))
}

/// The prefab names in `dir` (the stems of its `.map` files), sorted; none when it is missing.
pub fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "map"))
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
        .filter(|name| valid_name(name).is_ok())
        .collect();
    names.sort();
    names
}

/// Writes `fragment` as prefab `name` in `dir`, moved so its anchor is the origin. Refuses to
/// replace a prefab unless `overwrite`.
pub fn save(
    dir: &Path,
    name: &str,
    fragment: &MapFile,
    props: &PropBounds,
    overwrite: bool,
) -> Result<PathBuf, String> {
    valid_name(name)?;
    let path = file(dir, name);
    if path.exists() && !overwrite {
        return Err(format!(
            "the prefab \"{name}\" exists already (pass overwrite to replace it)"
        ));
    }
    let bounds = fragment
        .content_bounds(props)
        .ok_or("the selection has no size")?;
    let mut world = MapEntity::new("worldspawn");
    world.set("mapversion", "220");
    let mut moved = MapFile {
        entities: vec![world],
    };
    moved.insert_fragment(fragment, -anchor(bounds));
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    std::fs::write(&path, moved.to_text()).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}

/// Reads prefab `name` from `dir`.
pub fn load(dir: &Path, name: &str) -> Result<MapFile, String> {
    valid_name(name)?;
    let path = file(dir, name);
    let text =
        std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    MapFile::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapfile::{Brush, surface};

    fn near(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-4
    }

    fn map() -> MapFile {
        let mut map = MapFile::with_worldspawn("T", "x");
        let concrete = surface("concrete").unwrap();
        map.add_brush(Brush::cuboid(
            DVec3::new(10.0, 1.0, 10.0),
            DVec3::new(12.0, 3.0, 14.0),
            concrete,
        ));
        map.add_brush(Brush::cuboid(
            DVec3::new(30.0, 0.0, 0.0),
            DVec3::new(31.0, 1.0, 1.0),
            concrete,
        ));
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(11.0, 1.0, 12.0),
            Some(90.0),
            &[("team", "alpha")],
        ));
        let mut group = MapEntity::new("func_group");
        group.brushes.push(Brush::cuboid(
            DVec3::new(20.0, 0.0, 0.0),
            DVec3::new(21.0, 1.0, 1.0),
            concrete,
        ));
        group.brushes.push(Brush::cuboid(
            DVec3::new(22.0, 0.0, 0.0),
            DVec3::new(23.0, 1.0, 1.0),
            concrete,
        ));
        map.entities.push(group);
        map
    }

    #[test]
    fn a_fragment_holds_copies_of_what_was_picked() {
        let map = map();
        let fragment = map
            .extract(&[
                Item::Entity(1),
                Item::Brush {
                    entity: 0,
                    brush: 0,
                },
                Item::Brush {
                    entity: 2,
                    brush: 1,
                },
            ])
            .unwrap();
        // The worldspawn brush first, then the point entity, then the group with the one brush.
        assert_eq!(fragment.entities.len(), 3);
        assert_eq!(fragment.entities[0].classname(), "worldspawn");
        assert_eq!(fragment.entities[0].get("name"), None, "a bare worldspawn");
        assert_eq!(fragment.entities[0].brushes.len(), 1);
        assert_eq!(fragment.entities[1].get("team"), Some("alpha"));
        assert_eq!(fragment.entities[2].classname(), "func_group");
        assert_eq!(fragment.entities[2].brushes.len(), 1);
        assert_eq!(fragment.entities[2].brushes[0], map.entities[2].brushes[1]);
        assert!(map.extract(&[]).is_err());
        assert!(map.extract(&[Item::Entity(0)]).is_err());
    }

    #[test]
    fn pasting_copies_the_fragment_moved_and_returns_what_it_added() {
        let mut map = map();
        let props = PropBounds::new();
        let fragment = map
            .extract(&[
                Item::Brush {
                    entity: 0,
                    brush: 0,
                },
                Item::Entity(1),
            ])
            .unwrap();
        let added = map.insert_fragment(&fragment, DVec3::new(100.0, 0.0, -50.0));
        assert_eq!(added.len(), 2);
        assert!(near(
            map.bounds(added[0], &props).unwrap().0,
            DVec3::new(110.0, 1.0, -40.0)
        ));
        let spawn = map.entity(added[1]).unwrap();
        assert!(near(spawn.origin().unwrap(), DVec3::new(111.0, 1.0, -38.0)));
        assert_eq!(spawn.angle(), 90.0);
        assert_eq!(
            map.entities[0].brushes.len(),
            3,
            "the original is still there"
        );
    }

    #[test]
    fn the_anchor_is_the_middle_of_the_footprint_at_the_lowest_point() {
        let a = anchor((DVec3::new(10.0, 1.0, 10.0), DVec3::new(12.0, 3.0, 14.0)));
        assert!(near(a, DVec3::new(11.0, 1.0, 12.0)));
        let a = anchor((DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.7, 1.0, 0.3)));
        assert!(near(a, DVec3::new(0.25, 0.0, 0.25)), "on the grid: {a}");
        // Landing the anchor: whole grid steps across the ground, exact in height.
        let off = paste_offset(DVec3::new(11.0, 1.0, 12.0), DVec3::new(3.1, 2.0, -4.9), 0.5);
        assert!(near(off, DVec3::new(-8.0, 1.0, -17.0)), "{off}");
    }

    #[test]
    fn an_array_makes_count_copies_at_multiples_of_the_step() {
        let mut map = map();
        let props = PropBounds::new();
        let item = Item::Brush {
            entity: 0,
            brush: 0,
        };
        let copies = map
            .linear_array(&[item], 3, DVec3::new(4.0, 0.0, 0.0))
            .unwrap();
        assert_eq!(copies.len(), 3);
        for (k, copy) in copies.iter().enumerate() {
            let min = map.bounds(*copy, &props).unwrap().0;
            assert!(
                near(min, DVec3::new(10.0 + 4.0 * (k + 1) as f64, 1.0, 10.0)),
                "{k}: {min}"
            );
        }
        assert!(map.linear_array(&[item], 0, DVec3::X).is_err());
        assert!(map.linear_array(&[item], MAX_ARRAY + 1, DVec3::X).is_err());
        assert!(map.linear_array(&[item], 2, DVec3::ZERO).is_err());
        // A mixed array: brushes and entities both repeat.
        let more = map
            .linear_array(&[item, Item::Entity(1)], 2, DVec3::new(0.0, 0.0, 3.0))
            .unwrap();
        assert_eq!(more.len(), 4);
    }

    #[test]
    fn a_prefab_round_trips_through_its_file_and_lands_where_asked() {
        let dir = std::env::temp_dir().join(format!("mapedit-prefab-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let map = map();
        let props = PropBounds::new();
        let items = [
            Item::Brush {
                entity: 0,
                brush: 0,
            },
            Item::Entity(1),
        ];
        let fragment = map.extract(&items).unwrap();
        assert!(save(&dir, "bad name", &fragment, &props, false).is_err());
        let path = save(&dir, "hut", &fragment, &props, false).unwrap();
        assert!(path.ends_with("hut.map"));
        assert!(
            save(&dir, "hut", &fragment, &props, false).is_err(),
            "no silent replace"
        );
        save(&dir, "hut", &fragment, &props, true).unwrap();
        assert_eq!(list(&dir), ["hut"]);

        // The file is a .map: it parses, its anchor is the origin.
        let loaded = load(&dir, "hut").unwrap();
        let (min, max) = loaded.content_bounds(&props).unwrap();
        assert!(near(min, DVec3::new(-1.0, 0.0, -2.0)), "{min}");
        assert!(near(max, DVec3::new(1.0, 2.0, 2.0)), "{max}");
        // Placed back where it was, it is what was copied.
        let mut again = MapFile::with_worldspawn("T", "x");
        let placed = again.insert_fragment(&loaded, DVec3::new(11.0, 1.0, 12.0));
        assert_eq!(placed.len(), 2);
        assert!(near(
            again.bounds(placed[0], &props).unwrap().0,
            DVec3::new(10.0, 1.0, 10.0)
        ));
        assert!(near(
            again.entities[1].origin().unwrap(),
            DVec3::new(11.0, 1.0, 12.0)
        ));
        assert!(load(&dir, "nothing").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn prefab_names_are_file_safe() {
        assert!(valid_name("sandbag-wall_2").is_ok());
        for bad in ["", "a b", "../x", "a.b", "é", &"x".repeat(49)] {
            assert!(valid_name(bad).is_err(), "{bad}");
        }
    }
}
