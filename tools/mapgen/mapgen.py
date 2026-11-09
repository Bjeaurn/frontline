"""Tiny library that writes TrenchBroom `.map` files (Valve 220 format) for Frontline.

Layouts are described in game space, the way the Rust code thinks: metres, Bevy axes (x right,
y up, z towards the viewer; -z is "forward"). The library converts to TrenchBroom space (inches,
z up) on output, exactly as bevy_trenchbroom converts back when loading:

    TrenchBroom (x, y, z) = (-z, -x, y) * UNITS_PER_METRE

Python 3, standard library only. See README.md ("Maps") for the workflow.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field

# 1 TrenchBroom unit = 1 inch (crates/frontline/src/maps/mod.rs: UNITS_PER_METRE).
UNITS_PER_METRE = 39.37008

# Metres one texture repeat covers (horizontal, vertical) and the image size in pixels, per
# surface folder under crates/frontline/assets/textures/. Faces get the Valve 220 scale that
# keeps these real-world sizes, in-game and in TrenchBroom alike. The same table as `surfaces!`
# in crates/frontline/src/materials.rs (a test there compares the two).
SURFACES = {
    "concrete": ((2.2, 1.1), (1024, 512)),
    "plaster": ((2.0, 1.0), (1024, 512)),
    "metal": ((2.0, 2.0), (1024, 1024)),
    "corrugated": ((2.0, 2.0), (1024, 1024)),
    "wood": ((1.0, 1.0), (1024, 1024)),
    "ground": ((4.0, 4.0), (1024, 1024)),
    "brick": ((2.4, 1.2), (1024, 512)),
    "grass": ((3.0, 3.0), (1024, 1024)),
    "plaster_yellow": ((2.0, 2.0), (1024, 1024)),
    "plaster_ochre": ((2.0, 2.0), (1024, 1024)),
    "plaster_blue": ((2.0, 2.0), (1024, 1024)),
    "plaster_white": ((2.0, 2.0), (1024, 1024)),
    "plaster_green": ((2.0, 2.0), (1024, 1024)),
    "brick_old": ((1.7, 1.7), (1024, 1024)),
    "brick_hollow": ((2.0, 2.0), (1024, 1024)),
    "stone": ((2.2, 2.2), (1024, 1024)),
    "concrete_damaged": ((3.0, 3.0), (1024, 1024)),
    "wood_painted": ((1.5, 1.5), (1024, 1024)),
    "wood_siding": ((1.4, 1.4), (1024, 1024)),
    "asphalt": ((5.0, 5.0), (1024, 1024)),
    "sand": ((3.5, 3.5), (1024, 1024)),
    "gravel": ((1.6, 1.6), (1024, 1024)),
    "paving": ((2.0, 2.0), (1024, 1024)),
    "paving_brick": ((1.3, 1.3), (1024, 1024)),
    "tiles": ((2.0, 2.0), (1024, 1024)),
    "wood_floor": ((1.9, 1.9), (1024, 1024)),
    "roof_tiles": ((2.9, 2.9), (1024, 1024)),
    "roof_slate": ((2.9, 2.9), (1024, 1024)),
    "metal_rust": ((1.6, 1.6), (1024, 1024)),
    "metal_green": ((2.0, 2.0), (1024, 1024)),
    "metal_white": ((2.0, 2.0), (1024, 1024)),
    "corrugated_rust": ((2.0, 2.0), (1024, 1024)),
    "corrugated_blue": ((2.0, 2.0), (1024, 1024)),
}

Vec3 = tuple[float, float, float]


def to_tb(v: Vec3) -> Vec3:
    """A point or direction from game space (metres, y up) to TrenchBroom space (inches, z up)."""
    x, y, z = v
    return (-z * UNITS_PER_METRE, -x * UNITS_PER_METRE, y * UNITS_PER_METRE)


def dir_to_tb(v: Vec3) -> Vec3:
    """A direction from game space to TrenchBroom space, unscaled."""
    x, y, z = v
    return (-z, -x, y)


def _sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _scale(a: Vec3, s: float) -> Vec3:
    return (a[0] * s, a[1] * s, a[2] * s)


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _normalize(a: Vec3) -> Vec3:
    length = math.sqrt(_dot(a, a))
    return _scale(a, 1.0 / length)


def _num(value: float) -> str:
    """Compact decimal: at most 4 places, no trailing zeros, no negative zero."""
    text = f"{value:.4f}".rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


# Quake's paraxial texture axes: (face normal, U axis, V axis) in TrenchBroom space. A face
# takes the axes of the normal it is closest to, as TrenchBroom does for new faces.
_BASE_AXES = [
    ((0, 0, 1), (1, 0, 0), (0, -1, 0)),
    ((0, 0, -1), (1, 0, 0), (0, -1, 0)),
    ((1, 0, 0), (0, 1, 0), (0, 0, -1)),
    ((-1, 0, 0), (0, 1, 0), (0, 0, -1)),
    ((0, 1, 0), (1, 0, 0), (0, 0, -1)),
    ((0, -1, 0), (1, 0, 0), (0, 0, -1)),
]


@dataclass
class Face:
    """A brush plane in TrenchBroom space: outward unit normal and a point on it."""

    normal: Vec3
    point: Vec3
    surface: str

    def to_map(self) -> str:
        n = self.normal
        # Any vector not parallel to the normal gives a tangent.
        helper = (0.0, 0.0, 1.0) if abs(n[2]) < 0.9 else (1.0, 0.0, 0.0)
        u = _scale(_normalize(_cross(helper, n)), 64.0)
        v = _cross(n, u)
        # TrenchBroom/bevy_trenchbroom: normal = (p3 - p1) x (p2 - p1), pointing out.
        p1 = self.point
        p2 = _add(p1, v)
        p3 = _add(p1, u)
        points = " ".join(f"( {' '.join(_num(c) for c in p)} )" for p in (p1, p2, p3))

        _, u_axis, v_axis = max(_BASE_AXES, key=lambda axes: _dot(axes[0], n))
        (metres_u, metres_v), (width, height) = SURFACES[self.surface]
        scale_u = metres_u * UNITS_PER_METRE / width
        scale_v = metres_v * UNITS_PER_METRE / height
        u_text = " ".join(_num(c) for c in u_axis)
        v_text = " ".join(_num(c) for c in v_axis)
        return (
            f"{points} {self.surface}/color [ {u_text} 0 ] [ {v_text} 0 ] 0 "
            f"{_num(scale_u)} {_num(scale_v)}"
        )


@dataclass
class Brush:
    faces: list[Face]

    def to_map(self) -> str:
        return "{\n" + "".join(f"{face.to_map()}\n" for face in self.faces) + "}\n"


def oriented_box(center: Vec3, size: Vec3, surface: str, axes: tuple[Vec3, Vec3, Vec3] | None = None) -> Brush:
    """A box of full extents `size` (metres) along `axes` (orthonormal, game space; default the
    world axes), centred on `center`."""
    axes = axes or ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
    faces = []
    for axis, extent in zip(axes, size):
        for sign in (1.0, -1.0):
            normal = _scale(axis, sign)
            point = _add(center, _scale(normal, extent / 2.0))
            faces.append(Face(_normalize(dir_to_tb(normal)), to_tb(point), surface))
    return Brush(faces)


def box(center: Vec3, size: Vec3, surface: str) -> Brush:
    """An axis-aligned box of full extents `size` centred on `center` (metres)."""
    return oriented_box(center, size, surface)


def box_rotated_z(center: Vec3, size: Vec3, angle: float, surface: str) -> Brush:
    """A box turned `angle` radians about the game z axis (as `Quat::from_rotation_z`)."""
    c, s = math.cos(angle), math.sin(angle)
    return oriented_box(center, size, surface, ((c, s, 0.0), (-s, c, 0.0), (0.0, 0.0, 1.0)))


def wedge(low: Vec3, high: Vec3, rise: str, surface: str) -> Brush:
    """A solid ramp filling the box `low`..`high` (min and max corners, metres) under a slope
    that climbs from floor level to the top towards `rise`: one of "+x", "-x", "+z", "-z"."""
    (x0, y0, z0), (x1, y1, z1) = low, high
    axis = {"+x": 0, "-x": 0, "+z": 2, "-z": 2}[rise]
    forward = 1.0 if rise[0] == "+" else -1.0
    run = (x1 - x0) if axis == 0 else (z1 - z0)
    height = y1 - y0
    # Slope normal: up, leaning back against the climb.
    lean = -forward * height / run
    slope = (lean, 1.0, 0.0) if axis == 0 else (0.0, 1.0, lean)
    top_edge = (x1 if forward > 0 else x0, y1, z1) if axis == 0 else (x1, y1, z1 if forward > 0 else z0)
    planes = [
        ((0.0, -1.0, 0.0), (x0, y0, z0)),
        (slope, top_edge),
    ]
    # The tall end and the two sides; the low end is a sharp edge.
    if axis == 0:
        planes.append(((forward, 0.0, 0.0), (x1 if forward > 0 else x0, y0, z0)))
        planes += [((0.0, 0.0, 1.0), (x0, y0, z1)), ((0.0, 0.0, -1.0), (x0, y0, z0))]
    else:
        planes.append(((0.0, 0.0, forward), (x0, y0, z1 if forward > 0 else z0)))
        planes += [((1.0, 0.0, 0.0), (x1, y0, z0)), ((-1.0, 0.0, 0.0), (x0, y0, z0))]
    faces = [Face(_normalize(dir_to_tb(_normalize(n))), to_tb(p), surface) for n, p in planes]
    return Brush(faces)


@dataclass
class Entity:
    classname: str
    properties: dict[str, str] = field(default_factory=dict)
    brushes: list[Brush] = field(default_factory=list)

    def to_map(self, index: int) -> str:
        lines = [f"// entity {index}\n{{\n", f'"classname" "{self.classname}"\n']
        lines += [f'"{key}" "{value}"\n' for key, value in self.properties.items()]
        lines += [f"// brush {i}\n{brush.to_map()}" for i, brush in enumerate(self.brushes)]
        lines.append("}\n")
        return "".join(lines)


def point_entity(classname: str, feet: Vec3, yaw: float | None = None, **properties: str) -> Entity:
    """A point entity whose origin is `feet` (metres). `yaw` (radians, the game's convention:
    0 faces -z) becomes the TrenchBroom `angle`."""
    props = {"origin": " ".join(_num(c) for c in to_tb(feet))}
    if yaw is not None:
        props["angle"] = _num(math.degrees(yaw) % 360.0)
    props.update(properties)
    return Entity(classname, props)


@dataclass
class Map:
    """A whole map: worldspawn properties and brushes, then point entities."""

    worldspawn: dict[str, str] = field(default_factory=dict)
    brushes: list[Brush] = field(default_factory=list)
    entities: list[Entity] = field(default_factory=list)

    def to_map(self) -> str:
        world = Entity("worldspawn", {"mapversion": "220", **self.worldspawn}, self.brushes)
        header = "// Game: frontline\n// Format: Valve\n"
        return header + "".join(e.to_map(i) for i, e in enumerate([world, *self.entities]))

    def write(self, path: str) -> None:
        with open(path, "w", encoding="utf-8", newline="\n") as file:
            file.write(self.to_map())
