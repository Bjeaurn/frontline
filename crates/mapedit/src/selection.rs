//! What is selected: a list of items (the last one picked is the primary), how a click or a box
//! combines with it, and the queries that find items to select (a box, all of a kind). No ECS.

use bevy::math::{DVec3, Vec2};

use crate::mapfile::{Item, MapFile, PropBounds};

/// The items that edits act on, in the order they were picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    items: Vec<Item>,
}

/// How a pick combines with what is already selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    /// The pick becomes the selection.
    Replace,
    /// The pick is added.
    Add,
    /// Each picked item leaves the selection when it is in it, and joins when it is not.
    Toggle,
}

impl Combine {
    pub fn from_key(key: &str) -> Option<Combine> {
        match key {
            "replace" => Some(Combine::Replace),
            "add" => Some(Combine::Add),
            "toggle" => Some(Combine::Toggle),
            _ => None,
        }
    }
}

impl Selection {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, item: Item) -> bool {
        self.items.contains(&item)
    }

    /// The item when exactly one is selected.
    pub fn single(&self) -> Option<Item> {
        match self.items.as_slice() {
            [item] => Some(*item),
            _ => None,
        }
    }

    /// The item picked last.
    pub fn primary(&self) -> Option<Item> {
        self.items.last().copied()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Joins `item` (as the primary) unless it is in already.
    pub fn add(&mut self, item: Item) {
        if !self.contains(item) {
            self.items.push(item);
        }
    }

    /// Applies a pick.
    pub fn combine(&mut self, how: Combine, picked: &[Item]) {
        match how {
            Combine::Replace => {
                self.items.clear();
                picked.iter().for_each(|item| self.add(*item));
            }
            Combine::Add => picked.iter().for_each(|item| self.add(*item)),
            Combine::Toggle => {
                for item in picked {
                    if self.contains(*item) {
                        self.items.retain(|i| i != item);
                    } else {
                        self.items.push(*item);
                    }
                }
            }
        }
    }

    /// Keeps the items that `keep` accepts (after an edit that may have removed some).
    pub fn retain(&mut self, keep: impl Fn(Item) -> bool) {
        self.items.retain(|item| keep(*item));
    }
}

impl From<Vec<Item>> for Selection {
    fn from(items: Vec<Item>) -> Self {
        let mut selection = Selection::default();
        selection.combine(Combine::Add, &items);
        selection
    }
}

impl From<Option<Item>> for Selection {
    fn from(item: Option<Item>) -> Self {
        item.into_iter().collect::<Vec<_>>().into()
    }
}

/// A kind of item to select all of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Everything,
    Brushes,
    Entities,
    /// Point entities of a class (`spawn_point`).
    Class(String),
    /// Brushes painted with a surface (`concrete`).
    Surface(String),
    /// Everything of the kind of this item: the same class for an entity, the same surface
    /// for a brush.
    Like(Item),
}

/// Every item of `kind`.
pub fn of_kind(map: &MapFile, kind: &Kind) -> Vec<Item> {
    let surface_of = |item: Item| match item {
        Item::Brush { entity, brush } => map
            .brush(entity, brush)
            .and_then(|b| b.surface())
            .map(|s| s.name),
        Item::Entity(_) => None,
    };
    let class_of = |item: Item| match item {
        Item::Entity(_) => map.entity(item).map(|e| e.classname().to_owned()),
        Item::Brush { .. } => None,
    };
    map.items()
        .into_iter()
        .filter(|item| match kind {
            Kind::Everything => true,
            Kind::Brushes => matches!(item, Item::Brush { .. }),
            Kind::Entities => matches!(item, Item::Entity(_)),
            Kind::Class(class) => class_of(*item).is_some_and(|c| c == *class),
            Kind::Surface(name) => surface_of(*item).is_some_and(|s| s == name),
            Kind::Like(like) => match like {
                Item::Brush { .. } => {
                    matches!(item, Item::Brush { .. }) && surface_of(*item) == surface_of(*like)
                }
                Item::Entity(_) => class_of(*item).is_some() && class_of(*item) == class_of(*like),
            },
        })
        .collect()
}

/// The items whose bounding box `test` accepts.
pub fn where_bounds(
    map: &MapFile,
    props: &PropBounds,
    test: impl Fn(DVec3, DVec3) -> bool,
) -> Vec<Item> {
    map.items()
        .into_iter()
        .filter(|item| {
            map.bounds(*item, props)
                .is_some_and(|(min, max)| test(min, max))
        })
        .collect()
}

/// Whether the box `min`..`max` lies inside `outer_min`..`outer_max`; `flat` ignores height (a
/// rectangle on the ground, what a drag in the top view means). A tenth of a millimetre of
/// slack: brush corners carry the rounding of the file's coordinates.
pub fn box_inside(
    (min, max): (DVec3, DVec3),
    (outer_min, outer_max): (DVec3, DVec3),
    flat: bool,
) -> bool {
    const SLACK: f64 = 1e-4;
    let axes = if flat { &[0, 2][..] } else { &[0, 1, 2][..] };
    axes.iter()
        .all(|&a| min[a] >= outer_min[a] - SLACK && max[a] <= outer_max[a] + SLACK)
}

/// Whether all eight corners of a box land inside the screen rectangle `low`..`high`, with
/// `project` giving a pixel (`None` behind the camera).
pub fn corners_inside(
    min: DVec3,
    max: DVec3,
    project: impl Fn(DVec3) -> Option<Vec2>,
    low: Vec2,
    high: Vec2,
) -> bool {
    (0..8).all(|i| {
        let corner = DVec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        project(corner).is_some_and(|p| p.cmpge(low).all() && p.cmple(high).all())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapfile::{Brush, MapEntity, surface};

    fn brush(n: usize) -> Item {
        Item::Brush {
            entity: 0,
            brush: n,
        }
    }

    #[test]
    fn picks_replace_add_and_toggle() {
        let mut selection = Selection::default();
        selection.combine(Combine::Replace, &[brush(0)]);
        assert_eq!(selection.single(), Some(brush(0)));
        selection.combine(Combine::Add, &[brush(1), brush(0)]);
        assert_eq!(selection.items(), [brush(0), brush(1)], "no repeats");
        assert_eq!(selection.primary(), Some(brush(1)));
        assert_eq!(selection.single(), None);
        selection.combine(Combine::Toggle, &[brush(0), brush(2)]);
        assert_eq!(selection.items(), [brush(1), brush(2)]);
        selection.combine(Combine::Replace, &[]);
        assert!(selection.is_empty(), "a plain click on nothing deselects");
        assert_eq!(Selection::from(vec![brush(3), brush(3)]).len(), 1);
    }

    fn map() -> MapFile {
        let mut map = MapFile::with_worldspawn("T", "x");
        let concrete = surface("concrete").unwrap();
        let brick = surface("brick").unwrap();
        map.add_brush(Brush::cuboid(DVec3::ZERO, DVec3::splat(1.0), concrete));
        map.add_brush(Brush::cuboid(
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::new(6.0, 1.0, 1.0),
            brick,
        ));
        map.add_brush(Brush::cuboid(
            DVec3::new(-9.0, 0.0, 0.0),
            DVec3::new(9.0, 0.5, 9.0),
            concrete,
        ));
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(2.0, 0.0, 2.0),
            Some(0.0),
            &[],
        ));
        map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(3.0, 0.0, 2.0),
            Some(0.0),
            &[],
        ));
        map.add_entity(MapEntity::point(
            "dom_flag",
            DVec3::new(4.0, 0.0, 2.0),
            None,
            &[],
        ));
        map
    }

    #[test]
    fn all_of_a_kind() {
        let map = map();
        assert_eq!(of_kind(&map, &Kind::Everything).len(), 6);
        assert_eq!(of_kind(&map, &Kind::Brushes).len(), 3);
        assert_eq!(of_kind(&map, &Kind::Entities).len(), 3);
        assert_eq!(of_kind(&map, &Kind::Class("spawn_point".into())).len(), 2);
        assert_eq!(
            of_kind(&map, &Kind::Surface("concrete".into())),
            [brush(0), brush(2)]
        );
        assert_eq!(of_kind(&map, &Kind::Like(brush(1))), [brush(1)]);
        assert_eq!(of_kind(&map, &Kind::Like(Item::Entity(2))).len(), 2);
        assert_eq!(
            of_kind(&map, &Kind::Like(Item::Entity(3))),
            [Item::Entity(3)]
        );
    }

    #[test]
    fn a_box_takes_what_it_wholly_encloses() {
        let map = map();
        let props = PropBounds::new();
        let around = (DVec3::new(-1.0, -1.0, -1.0), DVec3::new(7.0, 5.0, 3.0));
        let picked = where_bounds(&map, &props, |min, max| {
            box_inside((min, max), around, false)
        });
        // The big floor sticks out of the box and is not taken.
        assert_eq!(
            picked,
            [
                brush(0),
                brush(1),
                Item::Entity(1),
                Item::Entity(2),
                Item::Entity(3)
            ]
        );
        // A flat box ignores height: a rectangle dragged on the ground.
        let ground = (DVec3::new(-1.0, 0.0, -1.0), DVec3::new(1.5, 0.0, 1.5));
        let picked = where_bounds(&map, &props, |min, max| {
            box_inside((min, max), ground, true)
        });
        assert_eq!(picked, [brush(0)]);
    }

    #[test]
    fn a_screen_box_needs_every_corner_in_front_of_the_camera_and_inside() {
        // A flat "camera": x and z map to pixels, anything with y > 10 is behind.
        let project =
            |p: DVec3| (p.y <= 10.0).then(|| Vec2::new(p.x as f32 * 10.0, p.z as f32 * 10.0));
        let (low, high) = (Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0));
        assert!(corners_inside(
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(5.0, 2.0, 5.0),
            project,
            low,
            high
        ));
        assert!(!corners_inside(
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(11.0, 2.0, 5.0),
            project,
            low,
            high
        ));
        assert!(!corners_inside(
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(5.0, 12.0, 5.0),
            project,
            low,
            high
        ));
    }
}
