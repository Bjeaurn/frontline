//! The model of a TrenchBroom `.map` file (Valve 220 format) that MapEdit edits: entities with
//! key/values, brushes made of planes with their texture axes, and a writer that gives the text
//! back. No ECS in here: the editor holds a [`MapFile`] as its working copy and the viewer shows
//! the text this module writes through the game's own Map loader.
//!
//! Spaces: the file stores TrenchBroom space (inches, z up). The API of this module speaks game
//! space (metres, y up, -z forward) wherever it takes or gives positions: [`game_to_tb`] and
//! [`tb_to_game`] convert, exactly as `tools/mapgen/mapgen.py` (`to_tb`) and bevy_trenchbroom do:
//! `TrenchBroom (x, y, z) = (-z, -x, y) * UNITS_PER_METRE`. Only [`Face`] points and
//! [`Brush::vertices`] / [`Brush::polygons`] are TrenchBroom space, and say so.
//!
//! A brush plane is three points; with the file's winding the outward normal is
//! `(p3 - p1) x (p2 - p1)` (what `mapgen.py` writes and bevy_trenchbroom reads).
//!
//! Texture alignment: moving or resizing a brush keeps every face's texture offset, so textures
//! stay aligned to the world, as `tools/mapgen` writes them (offset 0, no texture lock). Seams
//! between neighbouring brushes of one surface therefore stay seamless.

use std::collections::HashMap;
use std::fmt::{self, Write as _};

use bevy::math::DVec3;

/// 1 TrenchBroom unit = 1 inch (`maps::UNITS_PER_METRE` in the game).
pub const UNITS_PER_METRE: f64 = 39.37008;

/// Smallest side of a brush, in metres.
pub const MIN_SIZE: f64 = 0.05;

/// A point from game space (metres, y up) to TrenchBroom space (inches, z up).
pub fn game_to_tb(p: DVec3) -> DVec3 {
    DVec3::new(-p.z, -p.x, p.y) * UNITS_PER_METRE
}

/// A point from TrenchBroom space to game space.
pub fn tb_to_game(p: DVec3) -> DVec3 {
    DVec3::new(-p.y, p.z, -p.x) / UNITS_PER_METRE
}

/// A direction from game space to TrenchBroom space, unscaled.
pub fn dir_to_tb(d: DVec3) -> DVec3 {
    DVec3::new(-d.z, -d.x, d.y)
}

/// A direction from TrenchBroom space to game space, unscaled.
pub fn dir_to_game(d: DVec3) -> DVec3 {
    DVec3::new(-d.y, d.z, -d.x)
}

fn round_to(value: f64, scale: f64) -> f64 {
    let rounded = (value * scale).round() / scale;
    // No negative zero in the text.
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn round_vec(v: DVec3, scale: f64) -> DVec3 {
    DVec3::new(
        round_to(v.x, scale),
        round_to(v.y, scale),
        round_to(v.z, scale),
    )
}

// ------------------------------------------------------------------------------------ surfaces

/// A surface of the game's material library (`crates/frontline/assets/textures/<name>/`): the
/// real-world size one texture repeat covers and the image size, which together fix the Valve 220
/// scale of a face. Must agree with `SurfaceKind::tile_metres` and `SURFACES` in `mapgen.py`
/// (tested).
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub name: &'static str,
    /// Metres one repeat covers, horizontal and vertical.
    pub tile_metres: (f64, f64),
    /// Colour image size in pixels.
    pub image_px: (f64, f64),
}

impl Surface {
    /// The Valve 220 face scale that keeps the real-world tile size, rounded as `mapgen.py` does.
    pub fn face_scale(&self) -> [f64; 2] {
        [
            round_to(self.tile_metres.0 * UNITS_PER_METRE / self.image_px.0, 1e4),
            round_to(self.tile_metres.1 * UNITS_PER_METRE / self.image_px.1, 1e4),
        ]
    }

    /// The texture name faces of this surface carry.
    pub fn texture(&self) -> String {
        format!("{}/color", self.name)
    }
}

pub const SURFACES: [Surface; 7] = [
    Surface {
        name: "concrete",
        tile_metres: (2.2, 1.1),
        image_px: (1024.0, 512.0),
    },
    Surface {
        name: "plaster",
        tile_metres: (2.0, 1.0),
        image_px: (1024.0, 512.0),
    },
    Surface {
        name: "metal",
        tile_metres: (2.0, 2.0),
        image_px: (1024.0, 1024.0),
    },
    Surface {
        name: "corrugated",
        tile_metres: (2.0, 2.0),
        image_px: (1024.0, 1024.0),
    },
    Surface {
        name: "wood",
        tile_metres: (1.0, 1.0),
        image_px: (1024.0, 1024.0),
    },
    Surface {
        name: "ground",
        tile_metres: (4.0, 4.0),
        image_px: (1024.0, 1024.0),
    },
    Surface {
        name: "brick",
        tile_metres: (2.4, 1.2),
        image_px: (1024.0, 512.0),
    },
];

/// The surface called `name` (`concrete`).
pub fn surface(name: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|surface| surface.name == name)
}

/// The surface a texture name belongs to: `concrete/color` is `concrete`.
pub fn surface_of_texture(texture: &str) -> Option<&'static Surface> {
    let lower = texture.to_ascii_lowercase();
    let folder = lower.split('/').next().unwrap_or("");
    surface(folder).or_else(|| SURFACES.iter().find(|s| lower.contains(s.name)))
}

// --------------------------------------------------------------------------------------- faces

/// Quake's paraxial texture axes: (face normal, U axis, V axis) in TrenchBroom space. A face
/// takes the axes of the normal it is closest to, as TrenchBroom does for new faces.
const BASE_AXES: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
];

/// One brush plane: three points (TrenchBroom space), a texture and its Valve 220 mapping.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub points: [DVec3; 3],
    pub texture: String,
    /// U axis (x y z) and offset.
    pub u_axis: [f64; 4],
    /// V axis (x y z) and offset.
    pub v_axis: [f64; 4],
    pub rotation: f64,
    pub scale: [f64; 2],
}

impl Face {
    /// The plane as (unit outward normal, distance `normal . point`), `None` when the three
    /// points are collinear.
    pub fn plane(&self) -> Option<(DVec3, f64)> {
        let [p1, p2, p3] = self.points;
        let normal = (p3 - p1).cross(p2 - p1);
        let length = normal.length();
        if length < 1e-9 || !length.is_finite() {
            return None;
        }
        let normal = normal / length;
        Some((normal, normal.dot(p1)))
    }

    /// A face on the plane with outward unit `normal` through `point` (both TrenchBroom space),
    /// textured with `surface` at its real-world scale: the points and axes `mapgen.py` writes.
    pub fn on_plane(normal: DVec3, point: DVec3, surface: &Surface) -> Face {
        // Any vector not parallel to the normal gives a tangent.
        let helper = if normal.z.abs() < 0.9 {
            DVec3::Z
        } else {
            DVec3::X
        };
        let u = helper.cross(normal).normalize() * 64.0;
        let v = normal.cross(u);
        let points = [point, point + v, point + u].map(|p| round_vec(p, 1e4));
        let mut best = &BASE_AXES[0];
        let mut best_dot = f64::MIN;
        for axes in &BASE_AXES {
            let dot = DVec3::from(axes.0).dot(normal);
            if dot > best_dot {
                best = axes;
                best_dot = dot;
            }
        }
        Face {
            points,
            texture: surface.texture(),
            u_axis: [best.1[0], best.1[1], best.1[2], 0.0],
            v_axis: [best.2[0], best.2[1], best.2[2], 0.0],
            rotation: 0.0,
            scale: surface.face_scale(),
        }
    }
}

// -------------------------------------------------------------------------------------- brushes

/// Rise direction of a wedge (ramp): it climbs from floor level towards this side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rise {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl Rise {
    pub const ALL: [Rise; 4] = [Rise::PosX, Rise::NegX, Rise::PosZ, Rise::NegZ];

    pub fn key(self) -> &'static str {
        match self {
            Rise::PosX => "+x",
            Rise::NegX => "-x",
            Rise::PosZ => "+z",
            Rise::NegZ => "-z",
        }
    }

    pub fn from_key(key: &str) -> Option<Rise> {
        Rise::ALL.into_iter().find(|rise| rise.key() == key)
    }

    /// The rise closest to a horizontal direction (game space).
    pub fn towards(direction: DVec3) -> Rise {
        if direction.x.abs() >= direction.z.abs() {
            if direction.x >= 0.0 {
                Rise::PosX
            } else {
                Rise::NegX
            }
        } else if direction.z >= 0.0 {
            Rise::PosZ
        } else {
            Rise::NegZ
        }
    }
}

/// A convex solid: the intersection of the half-spaces behind its faces' planes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Brush {
    pub faces: Vec<Face>,
}

/// Brush vertices closer than this (TrenchBroom units) are one.
const VERTEX_EPS: f64 = 1e-3;

impl Brush {
    /// An axis-aligned box from its `min` to `max` corner (game space, metres). Faces in
    /// `mapgen.py`'s order: +x, -x, +y, -y, +z, -z.
    pub fn cuboid(min: DVec3, max: DVec3, surface: &Surface) -> Brush {
        let centre = (min + max) / 2.0;
        let size = max - min;
        let mut faces = Vec::with_capacity(6);
        for (axis, extent) in [DVec3::X, DVec3::Y, DVec3::Z]
            .into_iter()
            .zip([size.x, size.y, size.z])
        {
            for sign in [1.0, -1.0] {
                let normal = axis * sign;
                let point = centre + normal * (extent / 2.0);
                faces.push(Face::on_plane(
                    dir_to_tb(normal).normalize(),
                    game_to_tb(point),
                    surface,
                ));
            }
        }
        Brush { faces }
    }

    /// A solid ramp filling the box `low`..`high` (game space) under a slope that climbs from
    /// floor level to the top towards `rise`: `mapgen.py`'s `wedge`.
    pub fn wedge(low: DVec3, high: DVec3, rise: Rise, surface: &Surface) -> Brush {
        let along_x = matches!(rise, Rise::PosX | Rise::NegX);
        let forward = if matches!(rise, Rise::PosX | Rise::PosZ) {
            1.0
        } else {
            -1.0
        };
        let run = if along_x {
            high.x - low.x
        } else {
            high.z - low.z
        };
        let height = high.y - low.y;
        // Slope normal: up, leaning back against the climb.
        let lean = -forward * height / run;
        let slope = if along_x {
            DVec3::new(lean, 1.0, 0.0)
        } else {
            DVec3::new(0.0, 1.0, lean)
        };
        let top_edge = if along_x {
            DVec3::new(if forward > 0.0 { high.x } else { low.x }, high.y, high.z)
        } else {
            DVec3::new(high.x, high.y, if forward > 0.0 { high.z } else { low.z })
        };
        let mut planes = vec![(DVec3::NEG_Y, low), (slope, top_edge)];
        // The tall end and the two sides; the low end is a sharp edge.
        if along_x {
            let end = DVec3::new(if forward > 0.0 { high.x } else { low.x }, low.y, low.z);
            planes.push((DVec3::new(forward, 0.0, 0.0), end));
            planes.push((DVec3::Z, DVec3::new(low.x, low.y, high.z)));
            planes.push((DVec3::NEG_Z, low));
        } else {
            let end = DVec3::new(low.x, low.y, if forward > 0.0 { high.z } else { low.z });
            planes.push((DVec3::new(0.0, 0.0, forward), end));
            planes.push((DVec3::X, DVec3::new(high.x, low.y, low.z)));
            planes.push((DVec3::NEG_X, low));
        }
        let faces = planes
            .into_iter()
            .map(|(normal, point)| {
                Face::on_plane(
                    dir_to_tb(normal.normalize()).normalize(),
                    game_to_tb(point),
                    surface,
                )
            })
            .collect();
        Brush { faces }
    }

    /// Each face's plane, `None` for a degenerate one; indices follow `faces`.
    fn planes(&self) -> Vec<Option<(DVec3, f64)>> {
        self.faces.iter().map(Face::plane).collect()
    }

    /// The faces' polygons (TrenchBroom space), in `faces` order, each ordered around its
    /// outline; empty for a face that does not touch the solid.
    pub fn polygons(&self) -> Vec<Vec<DVec3>> {
        let planes = self.planes();
        let inside = |point: DVec3| {
            planes
                .iter()
                .flatten()
                .all(|(normal, d)| normal.dot(point) - d <= VERTEX_EPS)
        };
        let mut polygons = Vec::with_capacity(planes.len());
        for (i, plane) in planes.iter().enumerate() {
            let mut points: Vec<DVec3> = Vec::new();
            if let Some((n1, d1)) = plane {
                for (j, second) in planes.iter().enumerate() {
                    for (k, third) in planes.iter().enumerate().skip(j + 1) {
                        let (Some((n2, d2)), Some((n3, d3))) = (second, third) else {
                            continue;
                        };
                        if j == i || k == i {
                            continue;
                        }
                        let cross23 = n2.cross(*n3);
                        let det = n1.dot(cross23);
                        if det.abs() < 1e-9 {
                            continue;
                        }
                        let point =
                            (cross23 * *d1 + n3.cross(*n1) * *d2 + n1.cross(*n2) * *d3) / det;
                        if inside(point) && !points.iter().any(|p| p.distance(point) < VERTEX_EPS) {
                            points.push(point);
                        }
                    }
                }
                if points.len() >= 3 {
                    let centre = points.iter().sum::<DVec3>() / points.len() as f64;
                    let axis_u = (points[0] - centre).normalize_or_zero();
                    let axis_v = n1.cross(axis_u);
                    points.sort_by(|a, b| {
                        let angle =
                            |p: &DVec3| (*p - centre).dot(axis_v).atan2((*p - centre).dot(axis_u));
                        angle(a).total_cmp(&angle(b))
                    });
                } else {
                    points.clear();
                }
            }
            polygons.push(points);
        }
        polygons
    }

    /// The solid's corners (TrenchBroom space), each once.
    pub fn vertices(&self) -> Vec<DVec3> {
        let mut all: Vec<DVec3> = Vec::new();
        for point in self.polygons().into_iter().flatten() {
            if !all.iter().any(|p| p.distance(point) < VERTEX_EPS) {
                all.push(point);
            }
        }
        all
    }

    /// Whether the planes close up into a solid.
    pub fn is_solid(&self) -> bool {
        let polygons = self.polygons();
        polygons.iter().filter(|p| !p.is_empty()).count() >= 4 && self.aabb().is_some()
    }

    /// Bounding box in game space (min, max); `None` when the brush encloses nothing.
    pub fn aabb(&self) -> Option<(DVec3, DVec3)> {
        let vertices = self.vertices();
        if vertices.len() < 4 {
            return None;
        }
        let mut low = DVec3::splat(f64::MAX);
        let mut high = DVec3::splat(f64::MIN);
        for vertex in vertices {
            let p = tb_to_game(vertex);
            low = low.min(p);
            high = high.max(p);
        }
        Some((low, high))
    }

    /// Where a ray (game space, `direction` need not be unit) first enters the brush: the
    /// distance in metres per unit of `direction` and the outward surface normal (game space).
    /// `None` when it misses, or starts inside.
    pub fn ray_hit(&self, origin: DVec3, direction: DVec3) -> Option<(f64, DVec3)> {
        let start = game_to_tb(origin);
        let dir = dir_to_tb(direction);
        let (mut enter, mut exit) = (f64::MIN, f64::MAX);
        let mut normal = DVec3::ZERO;
        let mut planes = 0;
        for face in &self.faces {
            let Some((n, d)) = face.plane() else {
                continue;
            };
            planes += 1;
            let denom = n.dot(dir);
            let distance = d - n.dot(start);
            if denom.abs() < 1e-12 {
                if distance < 0.0 {
                    return None;
                }
            } else {
                let t = distance / denom;
                if denom < 0.0 {
                    if t > enter {
                        enter = t;
                        normal = n;
                    }
                } else if t < exit {
                    exit = t;
                }
            }
        }
        if planes < 4 || enter > exit || enter < 0.0 {
            return None;
        }
        // `dir` is `direction` permuted, so a TrenchBroom-space `t` in units of `dir` counts
        // inches here: scale to metres per unit of `direction`.
        Some((enter / UNITS_PER_METRE, dir_to_game(normal)))
    }

    /// Rounds every coordinate to 5 places (a quarter micrometre), so moves leave clean numbers.
    fn tidy(&mut self) {
        for face in &mut self.faces {
            face.points = face.points.map(|p| round_vec(p, 1e5));
        }
    }

    /// Moves the brush by `delta` metres (game space). Textures keep their world alignment.
    pub fn translate(&mut self, delta: DVec3) {
        let shift = game_to_tb(delta);
        for face in &mut self.faces {
            face.points = face.points.map(|p| p + shift);
        }
        self.tidy();
    }

    /// Scales the brush so its bounding box becomes `min`..`max` (game space): exact for boxes
    /// and ramps, which stay boxes and ramps. A turned box would shear.
    pub fn resize(&mut self, min: DVec3, max: DVec3) -> Result<(), String> {
        let (old_min, old_max) = self.aabb().ok_or("the brush encloses nothing")?;
        let old_size = old_max - old_min;
        let size = max - min;
        if size.min_element() < MIN_SIZE {
            return Err(format!("a brush needs at least {MIN_SIZE} m on every side"));
        }
        if old_size.min_element() < 1e-6 {
            return Err("the brush has no volume".into());
        }
        let ratio = size / old_size;
        for face in &mut self.faces {
            face.points = face.points.map(|p| {
                let game = tb_to_game(p);
                game_to_tb(min + (game - old_min) * ratio)
            });
        }
        self.tidy();
        Ok(())
    }

    /// The surface of the first face, when its texture names one.
    pub fn surface(&self) -> Option<&'static Surface> {
        self.faces
            .first()
            .and_then(|face| surface_of_texture(&face.texture))
    }

    /// Paints every face with `surface` at its real-world scale.
    pub fn set_surface(&mut self, surface: &Surface) {
        for face in &mut self.faces {
            face.texture = surface.texture();
            face.scale = surface.face_scale();
        }
    }
}

// ------------------------------------------------------------------------------------ entities

/// One entity: key/values in file order (`classname` first, as written) and its brushes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MapEntity {
    pub props: Vec<(String, String)>,
    pub brushes: Vec<Brush>,
}

impl MapEntity {
    pub fn new(classname: &str) -> MapEntity {
        MapEntity {
            props: vec![("classname".into(), classname.into())],
            brushes: Vec::new(),
        }
    }

    /// A point entity standing at `position` (game space, metres) facing `angle` degrees (the
    /// game's convention: 0 faces -z), with `extra` key/values after those.
    pub fn point(
        classname: &str,
        position: DVec3,
        angle: Option<f64>,
        extra: &[(&str, &str)],
    ) -> MapEntity {
        let mut entity = MapEntity::new(classname);
        entity.set_origin(position);
        if let Some(angle) = angle {
            entity.set("angle", &format_number(angle.rem_euclid(360.0)));
        }
        for (key, value) in extra {
            entity.set(key, value);
        }
        entity
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets `key`, in place when present, else last.
    pub fn set(&mut self, key: &str, value: &str) {
        match self.props.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value.to_owned(),
            None => self.props.push((key.to_owned(), value.to_owned())),
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.props.retain(|(k, _)| k != key);
    }

    /// The `origin` in game space (metres).
    pub fn origin(&self) -> Option<DVec3> {
        let mut numbers = self
            .get("origin")?
            .split_whitespace()
            .map(str::parse::<f64>);
        let (x, y, z) = (
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
        );
        Some(tb_to_game(DVec3::new(x, y, z)))
    }

    pub fn set_origin(&mut self, position: DVec3) {
        let tb = round_vec(game_to_tb(position), 1e5);
        let text = format!(
            "{} {} {}",
            format_number(tb.x),
            format_number(tb.y),
            format_number(tb.z)
        );
        self.set("origin", &text);
    }

    /// The `angle` in degrees (0 when absent).
    pub fn angle(&self) -> f64 {
        self.get("angle")
            .and_then(|a| a.trim().parse().ok())
            .unwrap_or(0.0)
    }
}

/// A number as the file writes it: the shortest text that reads back the same.
fn format_number(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{value}")
}

// ---------------------------------------------------------------------------------------- items

/// What an edit addresses: a brush (of the worldspawn, entity 0, or of a brush entity) or a
/// whole entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Brush { entity: usize, brush: usize },
    Entity(usize),
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Item::Brush { entity, brush } => write!(f, "brush {entity}.{brush}"),
            Item::Entity(entity) => write!(f, "entity {entity}"),
        }
    }
}

/// A ray hit on the Map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub item: Item,
    /// Metres along the ray (per unit of its direction).
    pub distance: f64,
    /// Outward surface normal, game space.
    pub normal: DVec3,
}

/// Model bounds of props by model file stem, relative to the origin (game space): what
/// `bounds.json` of a model pack lists.
pub type PropBounds = HashMap<String, (DVec3, DVec3)>;

/// The box around a point entity, relative to its origin (game space, metres): what the cursor
/// picks and the highlight draws.
pub fn entity_bounds(entity: &MapEntity, props: &PropBounds) -> (DVec3, DVec3) {
    let around = |half_x: f64, height: f64| {
        (
            DVec3::new(-half_x, 0.0, -half_x),
            DVec3::new(half_x, height, half_x),
        )
    };
    match entity.classname() {
        "spawn_point" => around(0.4, 1.8),
        "dom_flag" | "hq_site" => around(1.0, 3.0),
        "bomb_site" => around(0.8, 1.5),
        "ammo_crate" => (DVec3::new(-0.4, 0.0, -0.3), DVec3::new(0.4, 0.6, 0.3)),
        "light_point" | "light_spot" | "light_directional" => {
            (DVec3::splat(-0.25), DVec3::splat(0.25))
        }
        "prop" => {
            let stem = entity
                .get("model")
                .and_then(|model| model.rsplit('/').next())
                .map(|file| file.strip_suffix(".glb").unwrap_or(file));
            match stem.and_then(|stem| props.get(stem)) {
                Some((min, max)) => {
                    // The prop turns with `angle`; a box that holds it at any turn.
                    let half = min
                        .x
                        .abs()
                        .max(max.x.abs())
                        .max(min.z.abs())
                        .max(max.z.abs());
                    (
                        DVec3::new(-half, min.y, -half),
                        DVec3::new(half, max.y, half),
                    )
                }
                None => around(0.5, 1.0),
            }
        }
        _ => around(0.5, 1.0),
    }
}

fn ray_box(origin: DVec3, direction: DVec3, min: DVec3, max: DVec3) -> Option<(f64, DVec3)> {
    let (mut enter, mut exit) = (f64::MIN, f64::MAX);
    let mut normal = DVec3::ZERO;
    for axis in 0..3 {
        let (o, d, lo, hi) = (origin[axis], direction[axis], min[axis], max[axis]);
        if d.abs() < 1e-12 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (near, far, sign) = {
            let a = (lo - o) / d;
            let b = (hi - o) / d;
            if a < b { (a, b, -1.0) } else { (b, a, 1.0) }
        };
        if near > enter {
            enter = near;
            normal = DVec3::ZERO;
            normal[axis] = sign;
        }
        exit = exit.min(far);
    }
    (enter <= exit && enter >= 0.0).then_some((enter, normal))
}

// ----------------------------------------------------------------------------------- map file

/// A whole Map: entity 0 is the worldspawn.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MapFile {
    pub entities: Vec<MapEntity>,
}

impl MapFile {
    /// A Map with just its worldspawn (`name`, `look`).
    pub fn with_worldspawn(name: &str, look: &str) -> MapFile {
        let mut world = MapEntity::new("worldspawn");
        world.set("mapversion", "220");
        world.set("name", name);
        world.set("look", look);
        MapFile {
            entities: vec![world],
        }
    }

    pub fn worldspawn(&self) -> Option<&MapEntity> {
        self.entities
            .first()
            .filter(|entity| entity.classname() == "worldspawn")
    }

    pub fn worldspawn_mut(&mut self) -> Option<&mut MapEntity> {
        self.entities
            .first_mut()
            .filter(|entity| entity.classname() == "worldspawn")
    }

    pub fn brush(&self, entity: usize, brush: usize) -> Option<&Brush> {
        self.entities.get(entity)?.brushes.get(brush)
    }

    pub fn entity(&self, item: Item) -> Option<&MapEntity> {
        match item {
            Item::Entity(entity) => self.entities.get(entity),
            Item::Brush { entity, .. } => self.entities.get(entity),
        }
    }

    /// Whether `item` exists.
    pub fn contains(&self, item: Item) -> bool {
        match item {
            Item::Entity(entity) => self.entities.get(entity).is_some(),
            Item::Brush { entity, brush } => self.brush(entity, brush).is_some(),
        }
    }

    /// Every item: brushes of every entity, then point entities (entity 0 is not an item).
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for (entity, e) in self.entities.iter().enumerate() {
            for brush in 0..e.brushes.len() {
                items.push(Item::Brush { entity, brush });
            }
            if entity > 0 && e.brushes.is_empty() {
                items.push(Item::Entity(entity));
            }
        }
        items
    }

    /// Bounding box in game space of `item`.
    pub fn bounds(&self, item: Item, props: &PropBounds) -> Option<(DVec3, DVec3)> {
        match item {
            Item::Brush { entity, brush } => self.brush(entity, brush)?.aabb(),
            Item::Entity(entity) => {
                let e = self.entities.get(entity)?;
                if e.brushes.is_empty() {
                    let (min, max) = entity_bounds(e, props);
                    let origin = e.origin().unwrap_or(DVec3::ZERO);
                    Some((origin + min, origin + max))
                } else {
                    let mut low = DVec3::splat(f64::MAX);
                    let mut high = DVec3::splat(f64::MIN);
                    for (min, max) in e.brushes.iter().filter_map(Brush::aabb) {
                        low = low.min(min);
                        high = high.max(max);
                    }
                    (low.x <= high.x).then_some((low, high))
                }
            }
        }
    }

    /// The nearest brush or point entity a ray (game space) hits.
    pub fn pick(&self, origin: DVec3, direction: DVec3, props: &PropBounds) -> Option<Hit> {
        let direction = direction.normalize_or_zero();
        if direction == DVec3::ZERO {
            return None;
        }
        let mut best: Option<Hit> = None;
        let mut consider = |item: Item, hit: Option<(f64, DVec3)>| {
            if let Some((distance, normal)) = hit
                && best.is_none_or(|b| distance < b.distance)
            {
                best = Some(Hit {
                    item,
                    distance,
                    normal,
                });
            }
        };
        for (entity, e) in self.entities.iter().enumerate() {
            for (brush, b) in e.brushes.iter().enumerate() {
                consider(Item::Brush { entity, brush }, b.ray_hit(origin, direction));
            }
            if entity > 0
                && e.brushes.is_empty()
                && let Some(position) = e.origin()
            {
                let (min, max) = entity_bounds(e, props);
                consider(
                    Item::Entity(entity),
                    ray_box(origin, direction, position + min, position + max),
                );
            }
        }
        best
    }

    /// Moves `item` by `delta` metres.
    pub fn translate(&mut self, item: Item, delta: DVec3) -> Result<(), String> {
        match item {
            Item::Brush { entity, brush } => self
                .entities
                .get_mut(entity)
                .and_then(|e| e.brushes.get_mut(brush))
                .ok_or_else(|| format!("no {item}"))?
                .translate(delta),
            Item::Entity(entity) => {
                let e = self
                    .entities
                    .get_mut(entity)
                    .ok_or_else(|| format!("no {item}"))?;
                if e.brushes.is_empty() {
                    let origin = e.origin().ok_or("the entity has no origin")?;
                    e.set_origin(origin + delta);
                } else {
                    e.brushes.iter_mut().for_each(|b| b.translate(delta));
                }
            }
        }
        Ok(())
    }

    /// Resizes a brush so its bounding box becomes `min`..`max` (game space).
    pub fn resize(&mut self, item: Item, min: DVec3, max: DVec3) -> Result<(), String> {
        match item {
            Item::Brush { entity, brush } => self
                .entities
                .get_mut(entity)
                .and_then(|e| e.brushes.get_mut(brush))
                .ok_or_else(|| format!("no {item}"))?
                .resize(min, max),
            Item::Entity(_) => Err("only brushes can be resized".into()),
        }
    }

    /// Paints a brush, or all brushes of a brush entity, with `surface`.
    pub fn set_surface(&mut self, item: Item, surface: &Surface) -> Result<(), String> {
        match item {
            Item::Brush { entity, brush } => self
                .entities
                .get_mut(entity)
                .and_then(|e| e.brushes.get_mut(brush))
                .ok_or_else(|| format!("no {item}"))?
                .set_surface(surface),
            Item::Entity(entity) => {
                let e = self
                    .entities
                    .get_mut(entity)
                    .ok_or_else(|| format!("no {item}"))?;
                if e.brushes.is_empty() {
                    return Err("a point entity has no texture".into());
                }
                e.brushes.iter_mut().for_each(|b| b.set_surface(surface));
            }
        }
        Ok(())
    }

    /// Deletes `item`. The worldspawn itself stays.
    pub fn delete(&mut self, item: Item) -> Result<(), String> {
        match item {
            Item::Brush { entity, brush } => {
                let brushes = &mut self
                    .entities
                    .get_mut(entity)
                    .ok_or_else(|| format!("no {item}"))?
                    .brushes;
                if brush >= brushes.len() {
                    return Err(format!("no {item}"));
                }
                brushes.remove(brush);
            }
            Item::Entity(0) => return Err("the worldspawn cannot be deleted".into()),
            Item::Entity(entity) => {
                if entity >= self.entities.len() {
                    return Err(format!("no {item}"));
                }
                self.entities.remove(entity);
            }
        }
        Ok(())
    }

    /// Copies `item`, moved by `offset` metres; returns the copy.
    pub fn duplicate(&mut self, item: Item, offset: DVec3) -> Result<Item, String> {
        let copy = match item {
            Item::Brush { entity, brush } => {
                let list = &mut self
                    .entities
                    .get_mut(entity)
                    .ok_or_else(|| format!("no {item}"))?
                    .brushes;
                let mut copy = list.get(brush).ok_or_else(|| format!("no {item}"))?.clone();
                copy.translate(offset);
                list.push(copy);
                Item::Brush {
                    entity,
                    brush: list.len() - 1,
                }
            }
            Item::Entity(0) => return Err("the worldspawn cannot be duplicated".into()),
            Item::Entity(entity) => {
                let mut copy = self
                    .entities
                    .get(entity)
                    .ok_or_else(|| format!("no {item}"))?
                    .clone();
                if copy.brushes.is_empty() {
                    if let Some(origin) = copy.origin() {
                        copy.set_origin(origin + offset);
                    }
                } else {
                    copy.brushes.iter_mut().for_each(|b| b.translate(offset));
                }
                self.entities.push(copy);
                Item::Entity(self.entities.len() - 1)
            }
        };
        Ok(copy)
    }

    /// Adds a brush to the worldspawn.
    pub fn add_brush(&mut self, brush: Brush) -> Item {
        if self.worldspawn().is_none() {
            self.entities.insert(0, MapEntity::new("worldspawn"));
        }
        let list = &mut self.entities[0].brushes;
        list.push(brush);
        Item::Brush {
            entity: 0,
            brush: list.len() - 1,
        }
    }

    /// Adds an entity (a worldspawn is made first when the file has none).
    pub fn add_entity(&mut self, entity: MapEntity) -> Item {
        if self.worldspawn().is_none() {
            self.entities.insert(0, MapEntity::new("worldspawn"));
        }
        self.entities.push(entity);
        Item::Entity(self.entities.len() - 1)
    }
}

// --------------------------------------------------------------------------------------- text

/// A `.map` file that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Open,
    Close,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    /// A quoted string, escapes resolved.
    Quoted(String),
    Word(String),
}

fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, ParseError> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        line += 1;
                        break;
                    }
                }
            }
            '{' => tokens.push((Token::Open, line)),
            '}' => tokens.push((Token::Close, line)),
            '(' => tokens.push((Token::ParenOpen, line)),
            ')' => tokens.push((Token::ParenClose, line)),
            '[' => tokens.push((Token::BracketOpen, line)),
            ']' => tokens.push((Token::BracketClose, line)),
            '"' => {
                let start = line;
                let mut value = String::new();
                loop {
                    match chars.next() {
                        None => {
                            return Err(ParseError {
                                line: start,
                                message: "a quoted string never ends".into(),
                            });
                        }
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => value.push('\n'),
                            Some(other) => value.push(other),
                            None => {}
                        },
                        Some(other) => {
                            if other == '\n' {
                                line += 1;
                            }
                            value.push(other);
                        }
                    }
                }
                tokens.push((Token::Quoted(value), start));
            }
            first => {
                let mut word = String::from(first);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || "{}()[]\"".contains(next) {
                        break;
                    }
                    word.push(next);
                    chars.next();
                }
                tokens.push((Token::Word(word), line));
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    at: usize,
}

impl Parser {
    fn line(&self) -> usize {
        self.tokens
            .get(self.at)
            .or_else(|| self.tokens.last())
            .map_or(1, |(_, line)| *line)
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line(),
            message: message.into(),
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(token, _)| token)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).map(|(token, _)| token.clone());
        self.at += 1;
        token
    }

    fn expect(&mut self, wanted: Token, what: &str) -> Result<(), ParseError> {
        let line = self.line();
        match self.next() {
            Some(token) if token == wanted => Ok(()),
            other => Err(ParseError {
                line,
                message: format!("expected {what}, found {}", describe(other.as_ref())),
            }),
        }
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        let line = self.line();
        match self.next() {
            Some(Token::Word(word)) => match word.parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(n),
                _ => Err(ParseError {
                    line,
                    message: format!("\"{word}\" is not a number"),
                }),
            },
            other => Err(ParseError {
                line,
                message: format!("expected a number, found {}", describe(other.as_ref())),
            }),
        }
    }

    fn point(&mut self) -> Result<DVec3, ParseError> {
        self.expect(Token::ParenOpen, "`(`")?;
        let point = DVec3::new(self.number()?, self.number()?, self.number()?);
        self.expect(Token::ParenClose, "`)`")?;
        Ok(point)
    }

    fn axis(&mut self) -> Result<[f64; 4], ParseError> {
        self.expect(Token::BracketOpen, "`[` (Valve 220 texture axis)")?;
        let axis = [
            self.number()?,
            self.number()?,
            self.number()?,
            self.number()?,
        ];
        self.expect(Token::BracketClose, "`]`")?;
        Ok(axis)
    }

    fn face(&mut self) -> Result<Face, ParseError> {
        let points = [self.point()?, self.point()?, self.point()?];
        let line = self.line();
        let texture = match self.next() {
            Some(Token::Word(word) | Token::Quoted(word)) => word,
            other => {
                return Err(ParseError {
                    line,
                    message: format!(
                        "expected a texture name, found {}",
                        describe(other.as_ref())
                    ),
                });
            }
        };
        let u_axis = self.axis()?;
        let v_axis = self.axis()?;
        let rotation = self.number()?;
        let scale = [self.number()?, self.number()?];
        Ok(Face {
            points,
            texture,
            u_axis,
            v_axis,
            rotation,
            scale,
        })
    }

    fn brush(&mut self) -> Result<Brush, ParseError> {
        let mut faces = Vec::new();
        loop {
            match self.peek() {
                Some(Token::Close) => {
                    self.at += 1;
                    return Ok(Brush { faces });
                }
                Some(Token::ParenOpen) => faces.push(self.face()?),
                _ => return self.error("expected a brush face `( x y z ) ...` or `}`"),
            }
        }
    }

    fn entity(&mut self) -> Result<MapEntity, ParseError> {
        let mut entity = MapEntity::default();
        loop {
            match self.peek() {
                Some(Token::Close) => {
                    self.at += 1;
                    return Ok(entity);
                }
                Some(Token::Open) => {
                    self.at += 1;
                    entity.brushes.push(self.brush()?);
                }
                Some(Token::Quoted(_)) => {
                    let Some(Token::Quoted(key)) = self.next() else {
                        unreachable!()
                    };
                    let line = self.line();
                    match self.next() {
                        Some(Token::Quoted(value)) => entity.props.push((key, value)),
                        other => {
                            return Err(ParseError {
                                line,
                                message: format!(
                                    "expected a value for \"{key}\", found {}",
                                    describe(other.as_ref())
                                ),
                            });
                        }
                    }
                }
                _ => return self.error("expected a \"key\" \"value\" pair, a brush `{` or `}`"),
            }
        }
    }
}

fn describe(token: Option<&Token>) -> String {
    match token {
        None => "the end of the file".into(),
        Some(Token::Open) => "`{`".into(),
        Some(Token::Close) => "`}`".into(),
        Some(Token::ParenOpen) => "`(`".into(),
        Some(Token::ParenClose) => "`)`".into(),
        Some(Token::BracketOpen) => "`[`".into(),
        Some(Token::BracketClose) => "`]`".into(),
        Some(Token::Quoted(s)) => format!("\"{s}\""),
        Some(Token::Word(w)) => format!("`{w}`"),
    }
}

impl MapFile {
    /// Parses Valve 220 `.map` text. Comments (`// entity 3`) are dropped; [`MapFile::to_text`]
    /// writes them again.
    pub fn parse(text: &str) -> Result<MapFile, ParseError> {
        let mut parser = Parser {
            tokens: tokenize(text)?,
            at: 0,
        };
        let mut entities = Vec::new();
        while parser.peek().is_some() {
            parser.expect(Token::Open, "`{` starting an entity")?;
            entities.push(parser.entity()?);
        }
        if entities.is_empty() {
            return Err(ParseError {
                line: 1,
                message: "the file has no entities".into(),
            });
        }
        Ok(MapFile { entities })
    }

    /// The `.map` text, in the layout `tools/mapgen` writes (so a generated Map comes back
    /// byte for byte).
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.entities.len() * 200 + 64);
        out.push_str("// Game: frontline\n// Format: Valve\n");
        for (index, entity) in self.entities.iter().enumerate() {
            let _ = writeln!(out, "// entity {index}\n{{");
            for (key, value) in &entity.props {
                let _ = writeln!(out, "\"{}\" \"{}\"", escape(key), escape(value));
            }
            for (index, brush) in entity.brushes.iter().enumerate() {
                let _ = writeln!(out, "// brush {index}\n{{");
                for face in &brush.faces {
                    write_face(&mut out, face);
                }
                out.push_str("}\n");
            }
            out.push_str("}\n");
        }
        out
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn write_face(out: &mut String, face: &Face) {
    for p in &face.points {
        let _ = write!(
            out,
            "( {} {} {} ) ",
            format_number(p.x),
            format_number(p.y),
            format_number(p.z)
        );
    }
    let axis = |a: &[f64; 4]| {
        format!(
            "[ {} {} {} {} ]",
            format_number(a[0]),
            format_number(a[1]),
            format_number(a[2]),
            format_number(a[3])
        )
    };
    let _ = writeln!(
        out,
        "{} {} {} {} {} {}",
        face.texture,
        axis(&face.u_axis),
        axis(&face.v_axis),
        format_number(face.rotation),
        format_number(face.scale[0]),
        format_number(face.scale[1]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn close(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-3
    }

    fn committed_maps() -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontline/assets/maps");
        let mut maps: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "map"))
            .filter(|p| {
                !p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".mapedit-")
            })
            .collect();
        maps.sort();
        maps
    }

    fn unit_box() -> Brush {
        Brush::cuboid(
            DVec3::new(-1.0, 0.0, -2.0),
            DVec3::new(1.0, 3.0, 2.0),
            surface("concrete").unwrap(),
        )
    }

    #[test]
    fn units_convert_both_ways() {
        let p = DVec3::new(3.0, 1.5, -7.0);
        assert!(close(tb_to_game(game_to_tb(p)), p));
        // mapgen.py: to_tb((x, y, z)) = (-z, -x, y) * 39.37008.
        let tb = game_to_tb(DVec3::new(1.0, 2.0, 3.0));
        assert!(close(tb, DVec3::new(-3.0, -1.0, 2.0) * UNITS_PER_METRE));
        assert!(close(
            dir_to_game(dir_to_tb(DVec3::new(1.0, 2.0, 3.0))),
            DVec3::new(1.0, 2.0, 3.0)
        ));
    }

    #[test]
    fn surfaces_match_the_game_and_mapgen() {
        use frontline::materials::SurfaceKind;
        for kind in SurfaceKind::ALL {
            let surface = surface(kind.name()).unwrap_or_else(|| panic!("{}", kind.name()));
            let tile = kind.tile_metres();
            assert_eq!(surface.tile_metres.0 as f32, tile.x, "{}", kind.name());
            assert_eq!(surface.tile_metres.1 as f32, tile.y, "{}", kind.name());
        }
        assert_eq!(SURFACES.len(), SurfaceKind::ALL.len());
        // The scale of a concrete face in the committed maps.
        assert_eq!(surface("concrete").unwrap().face_scale(), [0.0846, 0.0846]);
        assert_eq!(surface("ground").unwrap().face_scale(), [0.1538, 0.1538]);
        assert_eq!(surface_of_texture("brick/color").unwrap().name, "brick");
        assert!(surface_of_texture("unknown/thing").is_none());
    }

    #[test]
    fn a_box_is_a_closed_solid_with_outward_normals() {
        let brush = unit_box();
        assert!(brush.is_solid());
        assert_eq!(brush.faces.len(), 6);
        assert_eq!(brush.vertices().len(), 8);
        let (min, max) = brush.aabb().unwrap();
        assert!(close(min, DVec3::new(-1.0, 0.0, -2.0)));
        assert!(close(max, DVec3::new(1.0, 3.0, 2.0)));
        let centre = game_to_tb(DVec3::new(0.0, 1.5, 0.0));
        for face in &brush.faces {
            let (normal, d) = face.plane().unwrap();
            assert!(normal.dot(centre) < d, "the centre is behind every plane");
        }
        for polygon in brush.polygons() {
            assert_eq!(polygon.len(), 4);
        }
    }

    #[test]
    fn rays_hit_the_nearest_face() {
        let brush = unit_box();
        let (t, normal) = brush
            .ray_hit(DVec3::new(0.0, 1.0, 10.0), DVec3::new(0.0, 0.0, -1.0))
            .unwrap();
        assert!((t - 8.0).abs() < 1e-4, "{t}");
        assert!(close(normal, DVec3::Z));
        let (t, normal) = brush
            .ray_hit(DVec3::new(0.0, 9.0, 0.0), DVec3::new(0.0, -2.0, 0.0))
            .unwrap();
        assert!(
            (t - 3.0).abs() < 1e-4,
            "{t} per unit of a length-2 direction"
        );
        assert!(close(normal, DVec3::Y));
        assert!(
            brush
                .ray_hit(DVec3::new(5.0, 1.0, 10.0), DVec3::NEG_Z)
                .is_none()
        );
        assert!(
            brush
                .ray_hit(DVec3::new(0.0, 1.0, -10.0), DVec3::NEG_Z)
                .is_none(),
            "pointing away"
        );
        assert!(
            brush
                .ray_hit(DVec3::new(0.0, 1.0, 0.0), DVec3::NEG_Z)
                .is_none(),
            "starts inside"
        );
    }

    #[test]
    fn translate_moves_and_keeps_texture_alignment() {
        let mut brush = unit_box();
        let before = brush.faces.clone();
        brush.translate(DVec3::new(0.25, 1.0, -0.5));
        let (min, max) = brush.aabb().unwrap();
        assert!(close(min, DVec3::new(-0.75, 1.0, -2.5)));
        assert!(close(max, DVec3::new(1.25, 4.0, 1.5)));
        for (now, was) in brush.faces.iter().zip(&before) {
            assert_eq!(now.u_axis, was.u_axis);
            assert_eq!(now.v_axis, was.v_axis);
            assert_eq!(now.scale, was.scale);
        }
        brush.translate(DVec3::new(-0.25, -1.0, 0.5));
        let (min, max) = brush.aabb().unwrap();
        assert!(close(min, DVec3::new(-1.0, 0.0, -2.0)) && close(max, DVec3::new(1.0, 3.0, 2.0)));
    }

    #[test]
    fn resize_scales_a_box_and_a_wedge_to_the_new_bounds() {
        let mut brush = unit_box();
        brush
            .resize(DVec3::new(-2.0, 0.0, -2.0), DVec3::new(2.0, 1.0, 6.0))
            .unwrap();
        let (min, max) = brush.aabb().unwrap();
        assert!(close(min, DVec3::new(-2.0, 0.0, -2.0)) && close(max, DVec3::new(2.0, 1.0, 6.0)));
        assert!(
            brush
                .resize(DVec3::ZERO, DVec3::new(0.01, 1.0, 1.0))
                .is_err()
        );

        let mut ramp = Brush::wedge(
            DVec3::ZERO,
            DVec3::new(4.0, 2.0, 2.0),
            Rise::PosX,
            surface("wood").unwrap(),
        );
        assert_eq!(ramp.faces.len(), 5);
        assert_eq!(ramp.vertices().len(), 6);
        ramp.resize(DVec3::new(1.0, 0.0, 0.0), DVec3::new(9.0, 1.0, 2.0))
            .unwrap();
        let (min, max) = ramp.aabb().unwrap();
        assert!(close(min, DVec3::new(1.0, 0.0, 0.0)) && close(max, DVec3::new(9.0, 1.0, 2.0)));
        assert_eq!(ramp.vertices().len(), 6);
        // The slope still climbs towards +x: a ray down at the low end hits lower than at the high end.
        let low = ramp
            .ray_hit(DVec3::new(2.0, 5.0, 1.0), DVec3::NEG_Y)
            .unwrap()
            .0;
        let high = ramp
            .ray_hit(DVec3::new(8.0, 5.0, 1.0), DVec3::NEG_Y)
            .unwrap()
            .0;
        assert!(5.0 - low < 5.0 - high);
    }

    #[test]
    fn set_surface_repaints_with_the_real_world_scale() {
        let mut brush = unit_box();
        brush.set_surface(surface("brick").unwrap());
        assert_eq!(brush.surface().unwrap().name, "brick");
        for face in &brush.faces {
            assert_eq!(face.texture, "brick/color");
            assert_eq!(face.scale, surface("brick").unwrap().face_scale());
        }
    }

    #[test]
    fn point_entities_keep_their_origin_in_game_space() {
        let mut e = MapEntity::point(
            "spawn_point",
            DVec3::new(-38.0, 0.0, -38.0),
            Some(225.0),
            &[("team", "any"), ("modes", "tdm")],
        );
        assert!(e.get("origin").unwrap().starts_with("1496.063"));
        assert!(close(e.origin().unwrap(), DVec3::new(-38.0, 0.0, -38.0)));
        assert_eq!(e.angle(), 225.0);
        assert_eq!(e.classname(), "spawn_point");
        e.set("team", "alpha");
        assert_eq!(e.get("team"), Some("alpha"));
        assert_eq!(e.props.len(), 5, "set replaces in place");
        e.set_origin(DVec3::new(1.0, 2.0, 3.0));
        assert!(close(e.origin().unwrap(), DVec3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn items_pick_move_duplicate_delete() {
        let mut map = MapFile::with_worldspawn("T", "overcast_urban");
        let wall = map.add_brush(unit_box());
        let spawn = map.add_entity(MapEntity::point(
            "spawn_point",
            DVec3::new(10.0, 0.0, 0.0),
            Some(0.0),
            &[],
        ));
        assert_eq!(map.items(), vec![wall, spawn]);
        let props = PropBounds::new();
        let hit = map
            .pick(DVec3::new(0.0, 1.0, 10.0), DVec3::NEG_Z, &props)
            .unwrap();
        assert_eq!(hit.item, wall);
        let hit = map
            .pick(DVec3::new(10.0, 1.0, 10.0), DVec3::NEG_Z, &props)
            .unwrap();
        assert_eq!(hit.item, spawn);
        map.translate(spawn, DVec3::new(0.0, 0.0, 5.0)).unwrap();
        assert!(close(
            map.entities[1].origin().unwrap(),
            DVec3::new(10.0, 0.0, 5.0)
        ));
        let copy = map.duplicate(wall, DVec3::new(4.0, 0.0, 0.0)).unwrap();
        assert_eq!(
            copy,
            Item::Brush {
                entity: 0,
                brush: 1
            }
        );
        assert!(close(
            map.bounds(copy, &props).unwrap().0,
            DVec3::new(3.0, 0.0, -2.0)
        ));
        map.delete(wall).unwrap();
        assert_eq!(map.entities[0].brushes.len(), 1);
        assert!(map.delete(Item::Entity(0)).is_err());
        assert!(map.delete(Item::Entity(9)).is_err());
        map.delete(spawn).unwrap();
        assert_eq!(map.entities.len(), 1);
    }

    #[test]
    fn text_round_trips_and_reports_errors_with_lines() {
        let mut map = MapFile::with_worldspawn("A \"quoted\" name", "x");
        map.add_brush(unit_box());
        map.add_entity(MapEntity::point(
            "prop",
            DVec3::new(1.0, 0.0, 1.0),
            None,
            &[("model", "models/a b.glb")],
        ));
        let text = map.to_text();
        assert_eq!(MapFile::parse(&text).unwrap(), map);
        assert_eq!(MapFile::parse(&text).unwrap().to_text(), text);

        let err = MapFile::parse("{\n\"a\" \"b\"\n{\n( 0 0 0 ) ( 1 0 0 ) x\n").unwrap_err();
        assert_eq!(err.line, 4, "{err}");
        assert!(MapFile::parse("").is_err());
        assert!(MapFile::parse("{ \"a\" }").is_err());
        assert!(MapFile::parse("{ \"a\" \"unterminated }").is_err());
        let bad_number = MapFile::parse(
            "{\n{\n( 0 0 zero ) ( 1 0 0 ) ( 0 1 0 ) t [ 1 0 0 0 ] [ 0 1 0 0 ] 0 1 1\n}\n}",
        )
        .unwrap_err();
        assert_eq!(bad_number.line, 3);
        // A Quake-format face (no brackets) is not Valve 220.
        assert!(MapFile::parse("{\n{\n( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) t 0 0 0 1 1\n}\n}").is_err());
    }

    #[test]
    fn comments_and_layout_do_not_matter() {
        let text = "// Game: frontline\n{ \"classname\" \"worldspawn\" // trailing\n{ ( 0 0 0 ) ( 0 64 0 ) ( 64 0 0 ) t/color [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 0.5 0.5 } }";
        let map = MapFile::parse(text).unwrap();
        assert_eq!(map.entities[0].brushes[0].faces[0].scale, [0.5, 0.5]);
    }

    #[test]
    fn every_committed_map_round_trips() {
        let maps = committed_maps();
        assert!(!maps.is_empty());
        for path in maps {
            let text = std::fs::read_to_string(&path).unwrap();
            let parsed =
                MapFile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let written = parsed.to_text();
            let again = MapFile::parse(&written)
                .unwrap_or_else(|e| panic!("{} (written): {e}", path.display()));
            assert_eq!(parsed, again, "{}", path.display());
            // Generated maps come back as they were written.
            assert_eq!(
                written,
                text,
                "{} is not byte for byte what mapgen writes",
                path.display()
            );
            assert!(parsed.worldspawn().is_some(), "{}", path.display());
            for entity in &parsed.entities {
                for brush in &entity.brushes {
                    assert!(brush.is_solid(), "{}: an open brush", path.display());
                }
            }
        }
    }

    #[test]
    fn our_primitives_match_what_mapgen_wrote() {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontline/assets/maps/arena.map"),
        )
        .unwrap();
        let map = MapFile::parse(&text).unwrap();
        // Every 6-face brush of the Arena is a mapgen `box`: rebuilding it from its bounds with the
        // same surface gives the same planes, points, axes and scale.
        let mut checked = 0;
        for brush in &map.entities[0].brushes {
            let axis_aligned = brush.faces.iter().all(|f| {
                f.plane()
                    .is_some_and(|(n, _)| n.abs().max_element() > 0.9999)
            });
            if brush.faces.len() != 6 || !axis_aligned {
                continue;
            }
            let (min, max) = brush.aabb().unwrap();
            let rebuilt = Brush::cuboid(min, max, brush.surface().unwrap());
            for a in &rebuilt.faces {
                let (na, da) = a.plane().unwrap();
                // The same plane may come in another order (a box turned a quarter circle).
                let b = brush
                    .faces
                    .iter()
                    .find(|b| {
                        let (nb, db) = b.plane().unwrap();
                        na.dot(nb) > 0.9999 && (da - db).abs() < 0.01
                    })
                    .expect("a face on the same plane");
                assert_eq!(a.texture, b.texture);
                assert_eq!(a.u_axis, b.u_axis);
                assert_eq!(a.v_axis, b.v_axis);
                assert_eq!(a.scale, b.scale);
            }
            checked += 1;
        }
        assert!(checked > 10, "{checked} boxes");
    }

    #[test]
    fn our_wedges_match_what_mapgen_wrote() {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../frontline/assets/maps/crossroads.map"),
        )
        .unwrap();
        let map = MapFile::parse(&text).unwrap();
        let mut checked = 0;
        for brush in map.entities[0]
            .brushes
            .iter()
            .filter(|b| b.faces.len() == 5)
        {
            let (min, max) = brush.aabb().unwrap();
            let matches = Rise::ALL.iter().any(|&rise| {
                let rebuilt = Brush::wedge(min, max, rise, brush.surface().unwrap());
                rebuilt.faces.iter().zip(&brush.faces).all(|(a, b)| {
                    a.texture == b.texture
                        && a.scale == b.scale
                        && a.u_axis == b.u_axis
                        && a.points.iter().zip(&b.points).all(|(p, q)| close(*p, *q))
                })
            });
            assert!(
                matches,
                "a wedge of crossroads.map differs from Brush::wedge"
            );
            checked += 1;
        }
        assert!(checked > 0, "crossroads has ramps");
    }
}
