//! Moving things about, without any ECS: turning brushes and entities about the vertical axis,
//! mirroring them, moving one face of a brush, and the operations of a [`MapFile`] that take
//! several items at once (move, paint, turn, mirror, delete, copy).
//!
//! **Turning and mirroring are exact.** A brush is planes (three points per face): the points
//! move with the motion in `f64`, nothing is rounded except to clean up float dust (a value within
//! a nanometre of a hundred-thousandth of a unit snaps to it), so twenty-four turns of 15 degrees
//! bring a brush back to where it was. Quarter turns use exact sine and cosine.
//!
//! **Textures stay on their faces.** A Valve 220 face maps a point `p` to
//! `p . axis / scale + offset`. A motion `p' = L p + t` therefore carries the axes along
//! (`axis' = L axis`) and takes the offset along too (`offset' = offset - t . axis' / scale`), so
//! every point of the face keeps the texture coordinate it had: the texture turns with the brush
//! instead of swimming. (Plain moves keep the world alignment instead, see [`Brush::translate`].)
//! A mirror also swaps two points of every face, because it reverses the winding the outward
//! normal is read from.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use bevy::math::{DMat3, DVec3};

use crate::mapfile::{
    Brush, Face, Item, MIN_SIZE, MapEntity, MapFile, PropBounds, Surface, UNITS_PER_METRE,
    dir_to_game, dir_to_tb, game_to_tb, round_to,
};

/// The turning step of the keys and the palette, degrees.
pub const TURN_STEP: f64 = 15.0;

/// The world axis a [`Motion::Mirror`] flips: the mirror plane is perpendicular to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorAxis {
    X,
    Z,
}

impl MirrorAxis {
    pub fn key(self) -> &'static str {
        match self {
            MirrorAxis::X => "x",
            MirrorAxis::Z => "z",
        }
    }

    pub fn from_key(key: &str) -> Option<MirrorAxis> {
        match key {
            "x" | "X" => Some(MirrorAxis::X),
            "z" | "Z" => Some(MirrorAxis::Z),
            _ => None,
        }
    }

    /// 0 for x, 2 for z: the index into a game-space vector.
    pub fn index(self) -> usize {
        match self {
            MirrorAxis::X => 0,
            MirrorAxis::Z => 2,
        }
    }
}

/// A motion of the Map's content (game space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Counter-clockwise about the vertical axis through `pivot` (x and z of it count), seen
    /// from above: east turns to north.
    Turn { degrees: f64, pivot: DVec3 },
    /// Reflection in the plane perpendicular to `axis` at that coordinate.
    Mirror { axis: MirrorAxis, at: f64 },
}

/// Sine and cosine in degrees, exact for quarter turns.
fn sin_cos(degrees: f64) -> (f64, f64) {
    let d = degrees.rem_euclid(360.0);
    if d == 0.0 {
        (0.0, 1.0)
    } else if d == 90.0 {
        (1.0, 0.0)
    } else if d == 180.0 {
        (0.0, -1.0)
    } else if d == 270.0 {
        (-1.0, 0.0)
    } else {
        d.to_radians().sin_cos()
    }
}

/// A value within float dust of a hundred-thousandth (a coordinate in inches) is that.
fn clean(value: f64) -> f64 {
    let rounded = (value * 1e5).round() / 1e5;
    if (value - rounded).abs() < 1e-9 {
        rounded + 0.0
    } else {
        value
    }
}

/// The same for a component of a unit vector.
fn clean_unit(value: f64) -> f64 {
    let rounded = (value * 1e9).round() / 1e9;
    if (value - rounded).abs() < 1e-12 {
        rounded + 0.0
    } else {
        value
    }
}

fn clean_vec(v: DVec3) -> DVec3 {
    DVec3::new(clean(v.x), clean(v.y), clean(v.z))
}

/// An angle in `0..360`, tidy to a millionth of a degree.
pub fn normal_angle(degrees: f64) -> f64 {
    let a = round_to(degrees.rem_euclid(360.0), 1e6);
    if a >= 360.0 { 0.0 } else { a }
}

impl Motion {
    /// The motion in TrenchBroom space: `p' = m p + t`.
    fn linear(&self) -> (DMat3, DVec3) {
        match *self {
            Motion::Turn { degrees, pivot } => {
                let (s, c) = sin_cos(degrees);
                // Game yaw about +y is a turn about z in TrenchBroom space, same direction.
                let m = DMat3::from_cols(DVec3::new(c, s, 0.0), DVec3::new(-s, c, 0.0), DVec3::Z);
                let centre = game_to_tb(DVec3::new(pivot.x, 0.0, pivot.z));
                (m, centre - m * centre)
            }
            Motion::Mirror { axis, at } => {
                // Game x is TrenchBroom -y, game z is -x.
                let (m, plane) = match axis {
                    MirrorAxis::X => (
                        DMat3::from_diagonal(DVec3::new(1.0, -1.0, 1.0)),
                        DVec3::new(2.0 * at, 0.0, 0.0),
                    ),
                    MirrorAxis::Z => (
                        DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)),
                        DVec3::new(0.0, 0.0, 2.0 * at),
                    ),
                };
                (m, game_to_tb(plane))
            }
        }
    }

    /// Where a point (game space) goes.
    pub fn point(&self, p: DVec3) -> DVec3 {
        match *self {
            Motion::Turn { degrees, pivot } => {
                let (s, c) = sin_cos(degrees);
                let (dx, dz) = (p.x - pivot.x, p.z - pivot.z);
                DVec3::new(pivot.x + dx * c + dz * s, p.y, pivot.z - dx * s + dz * c)
            }
            Motion::Mirror { axis, at } => {
                let mut q = p;
                q[axis.index()] = 2.0 * at - p[axis.index()];
                q
            }
        }
    }

    /// Where an entity facing `angle` (the game's convention: 0 faces -z, counter-clockwise)
    /// faces afterwards.
    pub fn angle(&self, angle: f64) -> f64 {
        normal_angle(match *self {
            Motion::Turn { degrees, .. } => angle + degrees,
            Motion::Mirror {
                axis: MirrorAxis::X,
                ..
            } => -angle,
            Motion::Mirror {
                axis: MirrorAxis::Z,
                ..
            } => 180.0 - angle,
        })
    }
}

impl Face {
    fn transform(&mut self, m: &DMat3, t: DVec3, flip: bool) {
        self.points = self.points.map(|p| clean_vec(*m * p + t));
        if flip {
            // The outward normal is read from the winding, which a reflection reverses.
            self.points.swap(1, 2);
        }
        for (axis, scale) in [
            (&mut self.u_axis, self.scale[0]),
            (&mut self.v_axis, self.scale[1]),
        ] {
            let moved = *m * DVec3::new(axis[0], axis[1], axis[2]);
            let scale = if scale.abs() < 1e-12 { 1.0 } else { scale };
            axis[3] = clean(axis[3] - t.dot(moved) / scale);
            axis[0] = clean_unit(moved.x);
            axis[1] = clean_unit(moved.y);
            axis[2] = clean_unit(moved.z);
        }
    }
}

impl Brush {
    /// Turns or mirrors the brush; textures go along (see the module docs).
    pub fn transform(&mut self, transform: &Motion) {
        let (m, t) = transform.linear();
        let flip = m.determinant() < 0.0;
        for face in &mut self.faces {
            face.transform(&m, t, flip);
        }
    }

    /// The outward normal of a face, game space.
    pub fn face_normal(&self, face: usize) -> Option<DVec3> {
        let (normal, _) = self.faces.get(face)?.plane()?;
        Some(dir_to_game(normal))
    }

    /// The middle of a face's outline, game space; `None` when it does not touch the solid.
    pub fn face_centre(&self, face: usize) -> Option<DVec3> {
        let polygon = self.polygons().into_iter().nth(face)?;
        if polygon.len() < 3 {
            return None;
        }
        let sum: DVec3 = polygon.iter().map(|p| crate::mapfile::tb_to_game(*p)).sum();
        Some(sum / polygon.len() as f64)
    }

    /// The face whose outward normal looks most nearly along `direction` (game space), when it
    /// looks that way at all.
    pub fn face_towards(&self, direction: DVec3) -> Option<usize> {
        let direction = direction.normalize_or_zero();
        (0..self.faces.len())
            .filter_map(|i| Some((i, self.face_normal(i)?.dot(direction))))
            .filter(|(_, dot)| *dot > 0.2)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Moves a face by sliding its plane `amount` metres along `direction` (game space, unit):
    /// along the face's own normal to push it out, along an axis to keep a slope's end on the
    /// grid. Every other plane stays where it is, so the rest of the brush keeps its shape.
    /// Refused (and nothing changes) when the brush would collapse or get thinner than
    /// [`MIN_SIZE`].
    pub fn move_face(&mut self, face: usize, direction: DVec3, amount: f64) -> Result<(), String> {
        let normal = self
            .face_normal(face)
            .ok_or_else(|| format!("the brush has no face {face}"))?;
        let direction = direction.normalize_or_zero();
        if direction.dot(normal).abs() < 1e-6 {
            return Err("that face cannot move that way".into());
        }
        let alive = |brush: &Brush| brush.polygons().iter().filter(|p| p.len() >= 3).count();
        let before = alive(self);
        let backup = self.clone();
        let shift = dir_to_tb(direction) * (amount * UNITS_PER_METRE);
        for p in &mut self.faces[face].points {
            *p += shift;
        }
        self.tidy();
        let thin = self
            .aabb()
            .is_none_or(|(min, max)| (max - min).min_element() < MIN_SIZE);
        if thin || alive(self) < before {
            *self = backup;
            return Err(format!(
                "moving that face by {} m would collapse the brush (a brush needs at least {MIN_SIZE} m on every side)",
                round_to(amount, 1e4)
            ));
        }
        Ok(())
    }
}

impl MapEntity {
    /// Moves a point entity with `transform`, and turns the facing of one that has one
    /// (`faces`: the class faces somewhere, or `angle` is set).
    fn transform(&mut self, transform: &Motion, faces: bool) -> Result<(), String> {
        let origin = self.origin().ok_or("the entity has no origin")?;
        self.set_origin(transform.point(origin));
        if faces || self.get("angle").is_some() {
            let angle = normal_angle(transform.angle(self.angle()));
            self.set("angle", &crate::mapfile::format_number(angle));
        }
        Ok(())
    }
}

impl MapFile {
    /// What a list of items stands for: brushes, and point entities; a brush entity becomes its
    /// brushes, repeats are dropped. Errors on an item that does not exist or on the
    /// worldspawn.
    pub fn units(&self, items: &[Item]) -> Result<Vec<Item>, String> {
        let mut units = Vec::with_capacity(items.len());
        let mut seen = HashSet::new();
        for &item in items {
            let mut add = |unit: Item| {
                if seen.insert(unit) {
                    units.push(unit);
                }
            };
            match item {
                Item::Brush { .. } => {
                    if !self.contains(item) {
                        return Err(format!("there is no {item}"));
                    }
                    add(item);
                }
                Item::Entity(0) => return Err("the worldspawn is not an item".into()),
                Item::Entity(entity) => {
                    let e = self
                        .entities
                        .get(entity)
                        .ok_or_else(|| format!("there is no {item}"))?;
                    if e.brushes.is_empty() {
                        add(item);
                    } else {
                        for brush in 0..e.brushes.len() {
                            add(Item::Brush { entity, brush });
                        }
                    }
                }
            }
        }
        Ok(units)
    }

    /// The box around all of `items` (game space).
    pub fn bounds_of(&self, items: &[Item], props: &PropBounds) -> Option<(DVec3, DVec3)> {
        let mut low = DVec3::splat(f64::MAX);
        let mut high = DVec3::splat(f64::MIN);
        let mut any = false;
        for (min, max) in items.iter().filter_map(|i| self.bounds(*i, props)) {
            low = low.min(min);
            high = high.max(max);
            any = true;
        }
        any.then_some((low, high))
    }

    /// Moves every item by `delta` metres.
    pub fn translate_many(&mut self, items: &[Item], delta: DVec3) -> Result<(), String> {
        for unit in self.units(items)? {
            self.translate(unit, delta)?;
        }
        Ok(())
    }

    /// Turns or mirrors every item. `faces` says whether an entity class faces somewhere.
    pub fn transform_many(
        &mut self,
        items: &[Item],
        transform: &Motion,
        faces: &dyn Fn(&str) -> bool,
    ) -> Result<(), String> {
        for unit in self.units(items)? {
            match unit {
                Item::Brush { entity, brush } => {
                    self.entities[entity].brushes[brush].transform(transform)
                }
                Item::Entity(entity) => {
                    let e = &mut self.entities[entity];
                    let facing = faces(e.classname());
                    e.transform(transform, facing)?;
                }
            }
        }
        Ok(())
    }

    /// Paints the brushes among `items`; an error when there are none.
    pub fn set_surface_many(&mut self, items: &[Item], surface: &Surface) -> Result<(), String> {
        let brushes: Vec<Item> = self
            .units(items)?
            .into_iter()
            .filter(|unit| matches!(unit, Item::Brush { .. }))
            .collect();
        if brushes.is_empty() {
            return Err("a point entity has no texture".into());
        }
        for unit in brushes {
            self.set_surface(unit, surface)?;
        }
        Ok(())
    }

    /// Deletes every item, whatever order they come in. A brush entity left without brushes
    /// goes too.
    pub fn delete_many(&mut self, items: &[Item]) -> Result<(), String> {
        let mut brushes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut gone: BTreeSet<usize> = BTreeSet::new();
        for unit in self.units(items)? {
            match unit {
                Item::Brush { entity, brush } => brushes.entry(entity).or_default().push(brush),
                Item::Entity(entity) => {
                    gone.insert(entity);
                }
            }
        }
        for (entity, mut list) in brushes {
            list.sort_unstable_by(|a, b| b.cmp(a));
            for brush in list {
                self.entities[entity].brushes.remove(brush);
            }
            if entity > 0 && self.entities[entity].brushes.is_empty() {
                gone.insert(entity);
            }
        }
        for entity in gone.into_iter().rev() {
            self.entities.remove(entity);
        }
        Ok(())
    }

    /// Copies every item, moved by `offset` metres; returns the copies.
    pub fn duplicate_many(&mut self, items: &[Item], offset: DVec3) -> Result<Vec<Item>, String> {
        // Copies are appended, so the indices of the originals stay valid while copying.
        let units = self.units(items)?;
        units
            .into_iter()
            .map(|unit| self.duplicate(unit, offset))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapfile::{Rise, surface, tb_to_game};

    fn near(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-4
    }

    fn concrete() -> &'static Surface {
        surface("concrete").unwrap()
    }

    /// Texture coordinate of a point (TrenchBroom space) on a face.
    fn uv(face: &Face, p: DVec3) -> (f64, f64) {
        let u = DVec3::new(face.u_axis[0], face.u_axis[1], face.u_axis[2]);
        let v = DVec3::new(face.v_axis[0], face.v_axis[1], face.v_axis[2]);
        (
            p.dot(u) / face.scale[0] + face.u_axis[3],
            p.dot(v) / face.scale[1] + face.v_axis[3],
        )
    }

    fn corners(brush: &Brush) -> Vec<DVec3> {
        brush.vertices()
    }

    #[test]
    fn a_quarter_turn_about_the_centre_swaps_the_footprint_exactly() {
        let mut brush = Brush::cuboid(
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 2.0, 1.0),
            concrete(),
        );
        // Centre (2, 1, 0.5): a 4 x 1 footprint becomes 1 x 4, still about the centre.
        brush.transform(&Motion::Turn {
            degrees: 90.0,
            pivot: DVec3::new(2.0, 0.0, 0.5),
        });
        let (min, max) = brush.aabb().unwrap();
        assert!(near(min, DVec3::new(1.5, 0.0, -1.5)), "{min}");
        assert!(near(max, DVec3::new(2.5, 2.0, 2.5)), "{max}");
        // Exact: the text has no float dust.
        for face in &brush.faces {
            for p in face.points {
                for v in [p.x, p.y, p.z] {
                    assert_eq!(v, (v * 1e5).round() / 1e5, "{v}");
                }
            }
        }
        assert!(brush.is_solid());
    }

    #[test]
    fn turning_is_counter_clockwise_seen_from_above_like_an_entity_angle() {
        // East of the pivot turns to north (-z) by 90 degrees, and a spawn facing north
        // (angle 0) then faces west (angle 90), as `angle` counts.
        let turn = Motion::Turn {
            degrees: 90.0,
            pivot: DVec3::ZERO,
        };
        assert!(near(
            turn.point(DVec3::new(1.0, 5.0, 0.0)),
            DVec3::new(0.0, 5.0, -1.0)
        ));
        assert_eq!(turn.angle(0.0), 90.0);
        assert_eq!(turn.angle(300.0), 30.0);
        // A brush agrees with the entity maths.
        let mut brush = Brush::cuboid(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 1.0),
            concrete(),
        );
        brush.transform(&turn);
        let (min, max) = brush.aabb().unwrap();
        assert!(near(min, DVec3::new(0.0, 0.0, -2.0)), "{min}");
        assert!(near(max, DVec3::new(1.0, 1.0, -1.0)), "{max}");
        // The facing of an entity: angle a looks along (-sin a, -cos a).
        let looks = |a: f64| DVec3::new(-a.to_radians().sin(), 0.0, -a.to_radians().cos());
        let turned = turn.point(DVec3::new(0.0, 0.0, 0.0) + looks(0.0));
        assert!(near(turned, looks(turn.angle(0.0))));
    }

    #[test]
    fn twenty_four_turns_of_fifteen_degrees_come_home() {
        let mut brush = Brush::wedge(
            DVec3::new(1.0, 0.0, 1.0),
            DVec3::new(5.0, 1.5, 3.0),
            Rise::PosX,
            concrete(),
        );
        let start = brush.clone();
        let turn = Motion::Turn {
            degrees: TURN_STEP,
            pivot: DVec3::new(3.0, 0.0, 2.0),
        };
        for _ in 0..24 {
            brush.transform(&turn);
        }
        for (a, b) in brush.faces.iter().zip(&start.faces) {
            for (p, q) in a.points.iter().zip(&b.points) {
                assert!(p.distance(*q) < 1e-6, "{p} vs {q}");
            }
            for i in 0..4 {
                assert!((a.u_axis[i] - b.u_axis[i]).abs() < 1e-6);
                assert!(
                    (a.v_axis[i] - b.v_axis[i]).abs() < 1e-5,
                    "offsets come home too"
                );
            }
        }
        assert!(brush.is_solid());
    }

    #[test]
    fn a_turned_brush_keeps_its_planes_and_its_texture_on_its_faces() {
        let original = Brush::wedge(
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 2.0, 2.0),
            Rise::NegZ,
            surface("brick").unwrap(),
        );
        for degrees in [15.0, 37.0, 90.0, -45.0, 200.0] {
            let turn = Motion::Turn {
                degrees,
                pivot: DVec3::new(1.3, 0.0, -0.7),
            };
            let mut brush = original.clone();
            brush.transform(&turn);
            assert!(brush.is_solid(), "{degrees}");
            assert_eq!(brush.faces.len(), original.faces.len());
            // Same solid, moved: every corner went where the turn sends it.
            let mut expected: Vec<DVec3> = corners(&original)
                .into_iter()
                .map(|v| game_to_tb(turn.point(tb_to_game(v))))
                .collect();
            for v in corners(&brush) {
                let at = expected
                    .iter()
                    .position(|e| e.distance(v) < 1e-3)
                    .unwrap_or_else(|| panic!("{degrees}: stray corner {v}"));
                expected.swap_remove(at);
            }
            assert!(expected.is_empty());
            // Texture lock: a point of a face has the same coordinate before and after.
            for (face, turned) in original.faces.iter().zip(&brush.faces) {
                let (motion, shift) = turn.linear();
                for p in face.points {
                    let (u0, v0) = uv(face, p);
                    let (u1, v1) = uv(turned, motion * p + shift);
                    assert!(
                        (u0 - u1).abs() < 1e-6 && (v0 - v1).abs() < 1e-6,
                        "{degrees}"
                    );
                }
                // The axes stay unit vectors in the plane family of Valve 220.
                let axis = DVec3::new(turned.u_axis[0], turned.u_axis[1], turned.u_axis[2]);
                assert!((axis.length() - 1.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_mirrored_brush_is_the_reflection_with_outward_normals_and_locked_texture() {
        let original = Brush::wedge(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(5.0, 2.0, 2.0),
            Rise::PosX,
            surface("metal").unwrap(),
        );
        for (axis, at) in [
            (MirrorAxis::X, 0.0),
            (MirrorAxis::X, 3.5),
            (MirrorAxis::Z, -1.0),
        ] {
            let mirror = Motion::Mirror { axis, at };
            let mut brush = original.clone();
            brush.transform(&mirror);
            assert!(brush.is_solid());
            let centre =
                brush.vertices().iter().copied().sum::<DVec3>() / brush.vertices().len() as f64;
            for face in &brush.faces {
                let (normal, d) = face.plane().unwrap();
                assert!(normal.dot(centre) < d, "normals still point outwards");
            }
            let mut expected: Vec<DVec3> = corners(&original)
                .into_iter()
                .map(|v| game_to_tb(mirror.point(tb_to_game(v))))
                .collect();
            for v in corners(&brush) {
                let at = expected
                    .iter()
                    .position(|e| e.distance(v) < 1e-3)
                    .expect("a corner");
                expected.swap_remove(at);
            }
            assert!(expected.is_empty());
            let (m, t) = mirror.linear();
            for (face, flipped) in original.faces.iter().zip(&brush.faces) {
                for p in face.points {
                    let (u0, v0) = uv(face, p);
                    let (u1, v1) = uv(flipped, m * p + t);
                    assert!((u0 - u1).abs() < 1e-6 && (v0 - v1).abs() < 1e-6);
                }
            }
        }
        // Mirroring twice is the identity.
        let mut twice = original.clone();
        let mirror = Motion::Mirror {
            axis: MirrorAxis::Z,
            at: 0.7,
        };
        twice.transform(&mirror);
        twice.transform(&mirror);
        assert!(near(twice.aabb().unwrap().0, original.aabb().unwrap().0));
        assert!(near(twice.aabb().unwrap().1, original.aabb().unwrap().1));
    }

    #[test]
    fn entities_turn_and_mirror_with_their_facing() {
        let faces = |class: &str| class == "spawn_point";
        let mut map = MapFile::with_worldspawn("T", "x");
        let spawn = map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(2.0, 0.0, 0.0),
            None,
            &[],
        ));
        let flag = map.add_entity(MapEntity::point(
            "dom_flag",
            DVec3::new(0.0, 0.0, 2.0),
            None,
            &[],
        ));
        let turn = Motion::Turn {
            degrees: 90.0,
            pivot: DVec3::ZERO,
        };
        map.transform_many(&[spawn, flag], &turn, &faces).unwrap();
        assert!(near(
            map.entities[1].origin().unwrap(),
            DVec3::new(0.0, 0.0, -2.0)
        ));
        assert_eq!(
            map.entities[1].angle(),
            90.0,
            "a spawn with no angle set gets one"
        );
        assert!(
            map.entities[2].get("angle").is_none(),
            "a flag does not face anywhere"
        );
        assert!(near(
            map.entities[2].origin().unwrap(),
            DVec3::new(2.0, 0.0, 0.0)
        ));
        // Mirror about x = 0: x flips, and 90 (facing -x) becomes 270 (facing +x).
        let mirror = Motion::Mirror {
            axis: MirrorAxis::X,
            at: 0.0,
        };
        map.transform_many(&[spawn], &mirror, &faces).unwrap();
        assert_eq!(map.entities[1].angle(), 270.0);
        assert_eq!(mirror.angle(0.0), 0.0, "facing -z stays");
        let across = Motion::Mirror {
            axis: MirrorAxis::Z,
            at: 0.0,
        };
        assert_eq!(across.angle(0.0), 180.0, "facing -z turns to +z");
        assert_eq!(across.angle(90.0), 90.0, "facing -x stays");
    }

    #[test]
    fn moving_a_face_keeps_every_other_plane() {
        let mut brush = Brush::cuboid(DVec3::ZERO, DVec3::new(2.0, 2.0, 2.0), concrete());
        let east = brush.face_towards(DVec3::X).unwrap();
        let others: Vec<Face> = brush
            .faces
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != east)
            .map(|(_, f)| f.clone())
            .collect();
        brush.move_face(east, DVec3::X, 0.75).unwrap();
        let (min, max) = brush.aabb().unwrap();
        assert!(near(min, DVec3::ZERO) && near(max, DVec3::new(2.75, 2.0, 2.0)));
        let after: Vec<Face> = brush
            .faces
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != east)
            .map(|(_, f)| f.clone())
            .collect();
        assert_eq!(after, others, "only that face changed");
        // In.
        brush.move_face(east, DVec3::X, -1.5).unwrap();
        assert!(near(brush.aabb().unwrap().1, DVec3::new(1.25, 2.0, 2.0)));
        // Through the other side: refused, unchanged.
        let before = brush.clone();
        assert!(brush.move_face(east, DVec3::X, -1.24).is_err());
        assert!(brush.move_face(east, DVec3::X, -5.0).is_err());
        assert_eq!(brush, before);
        // Sideways does nothing to a face: it cannot slide along itself.
        assert!(brush.move_face(east, DVec3::Y, 1.0).is_err());
    }

    #[test]
    fn a_ramp_end_moves_without_reshaping_the_slope() {
        let mut ramp = Brush::wedge(
            DVec3::ZERO,
            DVec3::new(4.0, 1.5, 2.0),
            Rise::PosX,
            concrete(),
        );
        // The tall end is the +x face.
        let end = ramp.face_towards(DVec3::X).unwrap();
        let slope_before = ramp.faces[ramp.face_towards(DVec3::Y).unwrap()].clone();
        ramp.move_face(end, DVec3::X, 1.0).unwrap();
        let (_, max) = ramp.aabb().unwrap();
        assert!(near(max, DVec3::new(5.0, 1.5 + 0.375, 2.0)), "{max}");
        let slope = ramp.faces[ramp.face_towards(DVec3::Y).unwrap()].clone();
        assert_eq!(
            slope, slope_before,
            "the slope plane is where it was; it just reaches further"
        );
        // The slope moves straight up for the ramp's height, whatever its lean.
        let up = ramp.face_towards(DVec3::Y).unwrap();
        let (_, top) = ramp.aabb().unwrap();
        ramp.move_face(up, DVec3::Y, 0.5).unwrap();
        assert!((ramp.aabb().unwrap().1.y - (top.y + 0.5)).abs() < 1e-6);
        assert!(ramp.is_solid());
    }

    #[test]
    fn a_selection_moves_paints_copies_and_deletes_as_one() {
        let mut map = MapFile::with_worldspawn("T", "x");
        let a = map.add_brush(Brush::cuboid(DVec3::ZERO, DVec3::splat(1.0), concrete()));
        let b = map.add_brush(Brush::cuboid(
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(3.0, 1.0, 1.0),
            concrete(),
        ));
        let c = map.add_brush(Brush::cuboid(
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(5.0, 1.0, 1.0),
            concrete(),
        ));
        let spawn = map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(9.0, 0.0, 0.0),
            Some(0.0),
            &[],
        ));
        let props = PropBounds::new();
        map.translate_many(&[a, c, spawn], DVec3::new(0.0, 1.0, 0.0))
            .unwrap();
        assert!(near(
            map.bounds(a, &props).unwrap().0,
            DVec3::new(0.0, 1.0, 0.0)
        ));
        assert!(near(
            map.bounds(b, &props).unwrap().0,
            DVec3::new(2.0, 0.0, 0.0)
        ));
        assert!(near(
            map.entities[1].origin().unwrap(),
            DVec3::new(9.0, 1.0, 0.0)
        ));
        let (min, max) = map.bounds_of(&[a, b, c, spawn], &props).unwrap();
        assert!(near(min, DVec3::new(0.0, 0.0, -0.4)), "{min}");
        assert!(near(max, DVec3::new(9.4, 2.8, 1.0)), "{max}");
        // Paint skips the entity; entities alone cannot be painted.
        map.set_surface_many(&[a, spawn], surface("brick").unwrap())
            .unwrap();
        assert_eq!(map.brush(0, 0).unwrap().surface().unwrap().name, "brick");
        assert_eq!(map.brush(0, 1).unwrap().surface().unwrap().name, "concrete");
        assert!(
            map.set_surface_many(&[spawn], surface("brick").unwrap())
                .is_err()
        );
        // Copies are appended and selected in order.
        let copies = map
            .duplicate_many(&[b, spawn, a], DVec3::new(0.0, 0.0, 5.0))
            .unwrap();
        assert_eq!(copies.len(), 3);
        assert_eq!(map.entities[0].brushes.len(), 5);
        assert_eq!(map.entities.len(), 3);
        assert!(near(
            map.bounds(copies[0], &props).unwrap().0,
            DVec3::new(2.0, 0.0, 5.0)
        ));
        // Deleting a mixed list in any order leaves exactly the rest.
        map.delete_many(&[
            Item::Entity(1),
            Item::Brush {
                entity: 0,
                brush: 0,
            },
            Item::Brush {
                entity: 0,
                brush: 3,
            },
        ])
        .unwrap();
        assert_eq!(map.entities[0].brushes.len(), 3);
        assert_eq!(map.entities.len(), 2);
        assert!(near(
            map.bounds(
                Item::Brush {
                    entity: 0,
                    brush: 0
                },
                &props
            )
            .unwrap()
            .0,
            DVec3::new(2.0, 0.0, 0.0)
        ));
        assert!(map.delete_many(&[Item::Entity(0)]).is_err());
        assert!(map.delete_many(&[Item::Entity(7)]).is_err());
    }

    #[test]
    fn a_brush_entity_is_its_brushes_and_goes_when_empty() {
        let mut map = MapFile::with_worldspawn("T", "x");
        map.add_brush(Brush::cuboid(DVec3::ZERO, DVec3::splat(1.0), concrete()));
        let mut group = MapEntity::new("func_group");
        group.brushes.push(Brush::cuboid(
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::new(6.0, 1.0, 1.0),
            concrete(),
        ));
        group.brushes.push(Brush::cuboid(
            DVec3::new(7.0, 0.0, 0.0),
            DVec3::new(8.0, 1.0, 1.0),
            concrete(),
        ));
        map.entities.push(group);
        // Naming the entity and one of its brushes does not move that brush twice.
        let units = map
            .units(&[
                Item::Entity(1),
                Item::Brush {
                    entity: 1,
                    brush: 0,
                },
            ])
            .unwrap();
        assert_eq!(units.len(), 2);
        map.translate_many(
            &[
                Item::Entity(1),
                Item::Brush {
                    entity: 1,
                    brush: 0,
                },
            ],
            DVec3::X,
        )
        .unwrap();
        assert!(near(
            map.brush(1, 0).unwrap().aabb().unwrap().0,
            DVec3::new(6.0, 0.0, 0.0)
        ));
        map.delete_many(&[
            Item::Brush {
                entity: 1,
                brush: 0,
            },
            Item::Brush {
                entity: 1,
                brush: 1,
            },
        ])
        .unwrap();
        assert_eq!(
            map.entities.len(),
            1,
            "an entity left without brushes is removed"
        );
    }
}
