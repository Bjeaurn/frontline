"""Building blocks for generated Maps, on top of mapgen.py: a `Level` that places solid blocks,
walls with doorways and windows, ramps, vehicles and other cover, glTF props and the game's
entities, optionally point-mirrored for symmetric layouts, plus a walkability checker that finds
unreachable objectives, trapped spots and closed-off pockets without launching the game.

Coordinates are game space (metres, y up, -z is "north"); the ground's top face is y = 0.
Python 3, standard library only.
"""

from __future__ import annotations

import json
import math
import sys
from contextlib import contextmanager
from pathlib import Path

from mapgen import UNITS_PER_METRE, Brush, Entity, Map, box, oriented_box, point_entity, wedge

REPO = Path(__file__).resolve().parents[2]
MAPS_DIR = REPO / "crates" / "frontline" / "assets" / "maps"

# Where a Map's `prop` models live: the Downtown City MegaKit under crates/frontline/assets/ for
# the built-in Maps, a `private://` copy in the private content folder for Homage Maps.
DOWNTOWN = "models/downtown"
PRIVATE_DOWNTOWN = "private://models/downtown"

# Interior bounds of the MegaKit's complete buildings: (x0, x1, z0, z1, height). They face +z at
# yaw 0, the front (with its entrance stoop) on z = 0.
BUILDINGS = {
    "Building_Small_1": (-7.0, 5.0, -12.0, 0.0, 16.0),
    "Building_Medium_2_001": (-7.0, 7.0, -12.0, 0.0, 24.0),
    "Building_Large_2": (-9.0, 11.0, -16.0, 0.0, 27.0),
}

Vec3 = tuple[float, float, float]


def door(a: float, b: float, height: float = 2.4) -> tuple[float, float, float, float]:
    """A doorway gap from `a` to `b` along a wall, from the floor up `height` metres."""
    return (a, b, 0.0, height)


# The one standard window opening: a 0.9 m sill and a 2.3 m head leave a 1.4 m gap, enough to
# cross crouched (1.2 m), and every window is at least 1.0 m wide, so any of them can be
# vaulted (data/movement.ron vault_*: sills 0.6-1.2 m, walls up to 0.6 m thick).
VAULT_SILL = 0.9
VAULT_HEAD = 2.3
VAULT_MIN_WIDTH = 1.0

# Fences (`Level.fence`): heights per kind, the 1 m low sections a player can vault, and the
# narrowest gate.
FENCE_HEIGHT = {"link": 2.4, "slat": 1.9}
LOW_FENCE = 1.0
GATE_MIN = 3.0


def window(a: float, b: float) -> tuple[float, float, float, float]:
    """A vaultable window gap from `a` to `b` (at least `VAULT_MIN_WIDTH` wide) along a wall,
    the standard `VAULT_SILL` to `VAULT_HEAD` metres above the floor."""
    assert b - a >= VAULT_MIN_WIDTH - 1e-9, f"window {a}..{b} is narrower than {VAULT_MIN_WIDTH} m"
    return (a, b, VAULT_SILL, VAULT_HEAD)


def _planes(brush: Brush) -> list[tuple[Vec3, float]]:
    """A brush's half-spaces in game space: (outward normal, offset) with n . p <= offset."""
    planes = []
    for face in brush.faces:
        nx, ny, nz = face.normal
        normal = (-ny, nz, -nx)
        px, py, pz = face.point
        point = (-py / UNITS_PER_METRE, pz / UNITS_PER_METRE, -px / UNITS_PER_METRE)
        planes.append((normal, sum(n * p for n, p in zip(normal, point))))
    return planes


class Level:
    """A Map under construction."""

    def __init__(self, name: str, look: str, models: str = DOWNTOWN):
        self.map = Map(worldspawn={"name": name, "look": look})
        self.models_dir = models
        self.models: set[str] = set()
        # (min corner, max corner, planes) per solid, for the walkability checker.
        self.solids: list[tuple[Vec3, Vec3, list[tuple[Vec3, float]]]] = []
        # What the checks read: (team, modes, x, z) per spawn; (label, x, y, z) per objective site
        # by entity class; (model, x, base y, z, model bounds) per prop.
        self.spawns: list[tuple[str, str, float, float]] = []
        self.sites: dict[str, list[tuple[str, float, float, float]]] = {}
        self.props: list[tuple[str, float, float, float, tuple[Vec3, Vec3] | None]] = []
        self._bounds = model_bounds(models)
        self._mirror = False
        # Indices into `solids` that are fence stand-ins: see-through (sight and shots pass).
        self.fenced: list[int] = []
        self.fence_bars: set[int] = set()  # the lattice's own thin bars in `solids`
        self.fence_metres = 0.0
        self._dx = self._dz = 0.0

    @contextmanager
    def shifted(self, dx: float, dz: float):
        """Everything built inside is moved by (dx, dz) metres (before any mirroring): a way to
        spread a layout's parts apart without touching their own coordinates."""
        old = (self._dx, self._dz)
        self._dx += dx
        self._dz += dz
        try:
            yield
        finally:
            self._dx, self._dz = old

    # ---- mirroring -------------------------------------------------------------------------

    @contextmanager
    def mirrored(self):
        """Everything built inside is also built turned 180 degrees about the origin."""
        self._mirror = True
        try:
            yield
        finally:
            self._mirror = False

    def _signs(self) -> tuple[float, ...]:
        return (1.0, -1.0) if self._mirror else (1.0,)

    # ---- solids ----------------------------------------------------------------------------

    def _add(self, brush: Brush, lo: Vec3, hi: Vec3) -> None:
        self.map.brushes.append(brush)
        self.solids.append((lo, hi, _planes(brush)))

    def block(self, surface: str, x0: float, x1: float, y0: float, y1: float, z0: float, z1: float) -> None:
        """An axis-aligned block between two corners."""
        assert x1 > x0 and y1 > y0 and z1 > z0, (surface, x0, x1, y0, y1, z0, z1)
        x0, x1, z0, z1 = x0 + self._dx, x1 + self._dx, z0 + self._dz, z1 + self._dz
        for s in self._signs():
            xa, xb = sorted((s * x0, s * x1))
            za, zb = sorted((s * z0, s * z1))
            brush = box(((xa + xb) / 2, (y0 + y1) / 2, (za + zb) / 2), (xb - xa, y1 - y0, zb - za), surface)
            self._add(brush, (xa, y0, za), (xb, y1, zb))

    def turned(self, surface: str, cx: float, cy: float, cz: float, sx: float, sy: float, sz: float, yaw: float) -> None:
        """A block of extents (sx, sy, sz) centred on (cx, cy, cz), turned `yaw` radians about y
        (as `Quat::from_rotation_y`): its x axis ends up at (cos, 0, -sin)."""
        cx, cz = cx + self._dx, cz + self._dz
        for s in self._signs():
            a = yaw + (math.pi if s < 0 else 0.0)
            c, n = math.cos(a), math.sin(a)
            axes = ((c, 0.0, -n), (0.0, 1.0, 0.0), (n, 0.0, c))
            centre = (s * cx, cy, s * cz)
            brush = oriented_box(centre, (sx, sy, sz), surface, axes)
            reach = abs(c) * sx / 2 + abs(n) * sz / 2, abs(n) * sx / 2 + abs(c) * sz / 2
            lo = (centre[0] - reach[0], cy - sy / 2, centre[2] - reach[1])
            hi = (centre[0] + reach[0], cy + sy / 2, centre[2] + reach[1])
            self._add(brush, lo, hi)

    def floor(self, surface: str, x0: float, x1: float, z0: float, z1: float, y: float = 0.0, depth: float = 1.0) -> None:
        """A slab whose top face is at height `y`."""
        self.block(surface, x0, x1, y - depth, y, z0, z1)

    def slab(self, surface: str, x0: float, x1: float, z0: float, z1: float, top: float, thick: float = 0.3, holes=()) -> None:
        """A floor slab whose top face is at `top`, with rectangular `holes` ((x0, x1, z0, z1)
        each, inside the slab) cut out for stairwells."""
        xs = sorted({x0, x1, *(v for h in holes for v in h[:2])})
        for xa, xb in zip(xs, xs[1:]):
            cursor = z0
            for hz0, hz1 in sorted((h[2], h[3]) for h in holes if h[0] < xb and h[1] > xa):
                if hz0 > cursor:
                    self.block(surface, xa, xb, top - thick, top, cursor, hz0)
                cursor = max(cursor, hz1)
            if cursor < z1:
                self.block(surface, xa, xb, top - thick, top, cursor, z1)

    def wall(self, surface: str, axis: str, c: float, a0: float, a1: float, y0: float, y1: float, t: float = 0.4, gaps=()) -> None:
        """A wall from y0 to y1 in the plane x = c (axis "x", running along z from a0 to a1) or
        z = c (axis "z", running along x). `gaps` are `door()`/`window()` tuples along the
        wall, heights relative to y0; they must not overlap."""
        segments = []
        cursor = a0
        for g0, g1, h0, h1 in sorted(gaps):
            assert a0 <= g0 < g1 <= a1 and g0 >= cursor, ("bad gap", axis, c, g0, g1)
            if g0 > cursor:
                segments.append((cursor, g0, y0, y1))
            if y0 + h0 > y0:
                segments.append((g0, g1, y0, y0 + h0))
            if y0 + h1 < y1:
                segments.append((g0, g1, y0 + h1, y1))
            cursor = g1
        if cursor < a1:
            segments.append((cursor, a1, y0, y1))
        for a, b, ya, yb in segments:
            if axis == "x":
                self.block(surface, c - t / 2, c + t / 2, ya, yb, a, b)
            else:
                self.block(surface, a, b, ya, yb, c - t / 2, c + t / 2)

    def room(self, surface: str, x0: float, x1: float, z0: float, z1: float, y0: float, y1: float, t: float = 0.4, north=(), south=(), west=(), east=()) -> None:
        """Four walls round the outer rectangle (x0..x1, z0..z1), each with its gaps, the corners
        belonging to the north and south walls."""
        self.wall(surface, "z", z0 + t / 2, x0, x1, y0, y1, t, north)
        self.wall(surface, "z", z1 - t / 2, x0, x1, y0, y1, t, south)
        self.wall(surface, "x", x0 + t / 2, z0 + t, z1 - t, y0, y1, t, west)
        self.wall(surface, "x", x1 - t / 2, z0 + t, z1 - t, y0, y1, t, east)

    def ramp(self, surface: str, x0: float, x1: float, z0: float, z1: float, y0: float, y1: float, rise: str) -> None:
        """A solid ramp filling the box (x0..x1, y0..y1, z0..z1), climbing from y0 to y1 towards
        `rise` ("+x", "-x", "+z" or "-z")."""
        flip = {"+x": "-x", "-x": "+x", "+z": "-z", "-z": "+z"}
        x0, x1, z0, z1 = x0 + self._dx, x1 + self._dx, z0 + self._dz, z1 + self._dz
        for s in self._signs():
            direction = rise if s > 0 else flip[rise]
            xa, xb = sorted((s * x0, s * x1))
            za, zb = sorted((s * z0, s * z1))
            self._add(wedge((xa, y0, za), (xb, y1, zb), direction, surface), (xa, y0, za), (xb, y1, zb))

    def slab_ramp(self, surface: str, start: Vec3, end: Vec3, width: float, thickness: float = 0.25) -> None:
        """A thin plank from `start` to `end` (centre line of its top face), `width` wide: a
        raised ramp or bridge with open space below."""
        start = (start[0] + self._dx, start[1], start[2] + self._dz)
        end = (end[0] + self._dx, end[1], end[2] + self._dz)
        for s in self._signs():
            a = (s * start[0], start[1], s * start[2])
            b = (s * end[0], end[1], s * end[2])
            run = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
            length = math.sqrt(sum(v * v for v in run))
            forward = tuple(v / length for v in run)
            flat = math.hypot(forward[0], forward[2])
            side = (-forward[2] / flat, 0.0, forward[0] / flat)
            up = (
                forward[1] * side[2] - forward[2] * side[1],
                forward[2] * side[0] - forward[0] * side[2],
                forward[0] * side[1] - forward[1] * side[0],
            )
            if up[1] < 0:
                up = (-up[0], -up[1], -up[2])
            centre = tuple((a[i] + b[i]) / 2 - up[i] * thickness / 2 for i in range(3))
            brush = oriented_box(centre, (length, thickness, width), surface, (forward, up, side))
            corners = [
                tuple(centre[i] + sx * forward[i] * length / 2 + sy * up[i] * thickness / 2 + sz * side[i] * width / 2 for i in range(3))
                for sx in (-1, 1)
                for sy in (-1, 1)
                for sz in (-1, 1)
            ]
            lo = tuple(min(c[i] for c in corners) for i in range(3))
            hi = tuple(max(c[i] for c in corners) for i in range(3))
            self._add(brush, lo, hi)

    # ---- props -----------------------------------------------------------------------------

    def prop(self, name: str, x: float, z: float, facing: float = 0.0, y: float = 0.0, collider: str = "none") -> None:
        """A MegaKit model (`<models dir>/<name>.glb`) standing at (x, z) with its lowest point
        (the base of the model's bounds, `bounds.json`) at height `y`, wherever the model puts
        its origin. `facing` is the direction its front (+z in the model) looks, in degrees:
        0 south (+z), 90 east (+x), 180 north (-z), 270 west (-x)."""
        self.models.add(name)
        bounds = self._bounds.get(name)
        origin_y = y - (bounds[0][1] if bounds else 0.0)
        x, z = x + self._dx, z + self._dz
        for s in self._signs():
            yaw = math.radians(facing + (180.0 if s < 0 else 0.0))
            self.props.append((name, s * x, y, s * z, bounds))
            self.map.entities.append(
                point_entity("prop", (s * x, origin_y, s * z), yaw, model=f"{self.models_dir}/{name}.glb", collider=collider)
            )

    def building(self, name: str, x: float, z: float, facing: float = 0.0, inset: float = 0.25) -> None:
        """A complete MegaKit building with a solid brush body of the same footprint behind it
        (the model itself has no collider). (x, z) is the building's origin: the middle of
        its front-face plane at yaw 0."""
        x0, x1, z0, z1, height = BUILDINGS[name]
        assert facing % 90 == 0, "buildings turn in quarter turns"
        self.prop(name, x, z, facing)
        x, z = x + self._dx, z + self._dz
        for s in self._signs():
            theta = math.radians(facing + (180.0 if s < 0 else 0.0))
            c, n = round(math.cos(theta)), round(math.sin(theta))
            corners = [
                (s * x + lx * c + lz * n, s * z - lx * n + lz * c)
                for lx, lz in ((x0 + inset, z0 + inset), (x1 - inset, z1 - inset))
            ]
            (ax, az), (bx, bz) = corners
            lo_x, hi_x = sorted((ax, bx))
            lo_z, hi_z = sorted((az, bz))
            brush = box(((lo_x + hi_x) / 2, height / 2, (lo_z + hi_z) / 2), (hi_x - lo_x, height, hi_z - lo_z), "brick")
            self._add(brush, (lo_x, 0.0, lo_z), (hi_x, height, hi_z))

    def facade(self, names, axis: str, c: float, a0: float, a1: float, gaps, y: float, outward: int) -> None:
        """MegaKit facade pieces (2 m wide, `names` taking turns) along the solid stretches of a
        wall face, skipping its `gaps` (the same `door()`/`window()` tuples the wall was built
        with). `axis` "x" is a wall in the plane x = c running along z (outward is -1 for the face
        looking west, +1 east); "z" a wall in z = c running along x (outward -1 north, +1 south).
        The pieces stand 0.22 m outside the plane, a hair off the wall brush so the faces don't
        fight."""
        index = 0
        cursor = a0
        runs = []
        for g0, g1, *_ in sorted(gaps):
            if g0 > cursor:
                runs.append((cursor, g0))
            cursor = g1
        if cursor < a1:
            runs.append((cursor, a1))
        for r0, r1 in runs:
            count = int((r1 - r0) // 2.0)
            start = r0 + ((r1 - r0) - count * 2.0) / 2.0
            for i in range(count):
                mid = start + (i + 0.5) * 2.0
                name = names[index % len(names)]
                index += 1
                if axis == "x":
                    self.prop(name, c + outward * 0.22, mid, 90.0 if outward > 0 else 270.0, y)
                else:
                    self.prop(name, mid, c + outward * 0.22, 0.0 if outward > 0 else 180.0, y)

    # ---- entities --------------------------------------------------------------------------

    def spawn(self, team: str, modes: str, x: float, z: float, towards: tuple[float, float]) -> None:
        """A spawn point at (x, z) facing the point `towards`."""
        x, z = x + self._dx, z + self._dz
        yaw = math.atan2(x - towards[0], z - towards[1])
        self.spawns.append((team, modes, x, z))
        self.map.entities.append(point_entity("spawn_point", (x, 0.0, z), yaw, team=team, modes=modes))

    def _site(self, classname: str, tag: str, x: float, y: float, z: float, **properties: str) -> None:
        x, z = x + self._dx, z + self._dz
        self.sites.setdefault(classname, []).append((tag, x, y, z))
        self.map.entities.append(point_entity(classname, (x, y, z), **properties))

    def flag(self, label: str, x: float, z: float, y: float = 0.0) -> None:
        self._site("dom_flag", label, x, y, z, label=label)

    def hq(self, x: float, z: float, y: float = 0.0) -> None:
        self._site("hq_site", "", x, y, z)

    def bomb(self, label: str, x: float, z: float, y: float = 0.0) -> None:
        self._site("bomb_site", label, x, y, z, label=label)

    def ammo_crate(self, x: float, z: float, y: float = 0.0) -> None:
        """An ammo crate (Team Deathmatch only) standing at (x, z); inside `mirrored()` its twin
        turned 180 degrees about the origin too. The game draws it, nothing is built here."""
        x, z = x + self._dx, z + self._dz
        for s in self._signs():
            self.sites.setdefault("ammo_crate", []).append(("", s * x, y, s * z))
            self.map.entities.append(point_entity("ammo_crate", (s * x, y, s * z)))

    def light(self, x: float, y: float, z: float, color: str = "1 0.92 0.8", intensity: float = 300000.0, reach: float = 12.0, practical: bool = False, shadows: bool = False) -> None:
        """A point light. `practical` ones (lamps) are switched on by the look's
        `practical_lights` (night looks) and off by day."""
        extra = {"targetname": "practical"} if practical else {}
        x, z = x + self._dx, z + self._dz
        for s in self._signs():
            self.map.entities.append(
                point_entity(
                    "light_point",
                    (s * x, y, s * z),
                    color=color,
                    intensity=f"{intensity:.0f}",
                    range=f"{reach:.1f}",
                    shadows_enabled="true" if shadows else "false",
                    **extra,
                )
            )

    # ---- cover -----------------------------------------------------------------------------

    def car(self, x: float, z: float, yaw: float = 0.0, body: str = "metal") -> None:
        """A sedan blockout, 4.5 x 1.8 m, 1.5 m tall, long axis along x at yaw 0 (radians)."""
        c, n = math.cos(yaw), math.sin(yaw)
        place = lambda dx, dz: (x + dx * c + dz * n, z - dx * n + dz * c)
        px, pz = place(0.0, 0.0)
        self.turned(body, px, 0.45, pz, 4.5, 0.9, 1.8, yaw)
        cx, cz = place(-0.2, 0.0)
        self.turned(body, cx, 1.1, cz, 2.3, 0.45, 1.6, yaw)

    def van(self, x: float, z: float, yaw: float = 0.0, body: str = "corrugated") -> None:
        """A delivery van blockout, 5.4 x 2.1 m, 2.5 m tall."""
        c, n = math.cos(yaw), math.sin(yaw)
        px, pz = x, z
        self.turned(body, px, 1.175, pz, 5.4, 2.35, 2.1, yaw)
        fx, fz = x + 2.5 * c, z - 2.5 * n
        self.turned(body, fx, 0.475, fz, 1.0, 0.95, 2.0, yaw)

    def jersey(self, x: float, z: float, yaw: float = 0.0, length: float = 3.0, surface: str = "concrete") -> None:
        """A concrete jersey barrier: knee-high cover you can crouch behind (0.9 m)."""
        self.turned(surface, x, 0.45, z, length, 0.9, 0.5, yaw)

    def lamp(self, x: float, z: float, height: float = 5.5) -> None:
        """A street lamp: a thin pole (it blocks movement but hardly any view) and an arm."""
        # The pole stops where the arm begins and the arm is a hair narrower, so no two faces
        # share a plane (they would flicker).
        self.block("metal", x - 0.1, x + 0.1, 0.0, height - 0.2, z - 0.1, z + 0.1)
        self.block("metal", x - 0.1, x + 0.9, height - 0.2, height, z - 0.06, z + 0.06)
        self.light(x + 0.8, height - 0.5, z, color="1 0.78 0.45", intensity=500000.0, reach=22.0, practical=True)

    # ---- fences ----------------------------------------------------------------------------

    def _bar(self, surface: str, axis: str, c: float, a0: float, a1: float, y0: float, y1: float, t: float) -> None:
        """A thin brush along a fence line: `axis` "x" is a fence in the plane x = c running
        along z, "z" one in the plane z = c running along x."""
        if axis == "x":
            self.block(surface, c - t / 2, c + t / 2, y0, y1, a0, a1)
        else:
            self.block(surface, a0, a1, y0, y1, c - t / 2, c + t / 2)

    def _fence_piece(self, axis: str, c: float, a0: float, a1: float, kind: str, h: float) -> None:
        """One open lattice run. Every bar thickness differs from the ones it crosses, so no two
        faces share a plane (see tools/mapgen/lint.py): posts 0.14, top rail 0.12, bottom rail
        0.10, wires 0.04, uprights 0.05, boards 0.06."""
        length = a1 - a0
        low = h <= LOW_FENCE + 1e-6
        surface = "metal" if kind == "link" else "wood"
        posts = max(1, math.ceil(length / 2.6))
        for i in range(posts + 1):
            a = a0 + 0.07 + (length - 0.14) * i / posts
            self._bar(surface, axis, c, a - 0.07, a + 0.07, 0.0, h - 0.12 if low else h + 0.1, 0.14)
        # Everything that runs between the posts stops half a post inside them, so no end face
        # shares a plane with a post's.
        r0, r1 = a0 + 0.07, a1 - 0.07
        self._bar(surface, axis, c, r0, r1, h - 0.12, h, 0.12)  # the top rail, what a vault lands on
        if kind == "link":
            self._bar(surface, axis, c, r0, r1, 0.0, 0.06, 0.10)
            for f in (0.5,) if low else (1 / 3, 2 / 3):
                self._bar(surface, axis, c, r0, r1, h * f - 0.025, h * f + 0.025, 0.04)
            count = max(1, round(length / 0.5))  # uprights every ~0.5 m: gaps of ~0.45 m
            for i in range(count):
                a = a0 + (i + 0.5) * length / count
                self._bar(surface, axis, c, a - 0.025, a + 0.025, 0.06, h - 0.12, 0.05)
        else:
            y = 0.0
            while y + 0.16 <= h - 0.12 + 1e-6:  # boards 0.16 high with 0.14 gaps
                self._bar(surface, axis, c, r0, r1, y, y + 0.16, 0.06)
                y += 0.30

    def _stand_in(self, axis: str, c: float, a0: float, a1: float, h: float) -> None:
        """The checker's stand-in for a fence run: a solid 0.4 m thick, so its 0.25 m grid always
        sees it. It is only in `solids` (and `fenced`), never in the Map."""
        if axis == "x":
            x0, x1, z0, z1 = c - 0.2, c + 0.2, a0, a1
        else:
            x0, x1, z0, z1 = a0, a1, c - 0.2, c + 0.2
        x0, x1, z0, z1 = x0 + self._dx, x1 + self._dx, z0 + self._dz, z1 + self._dz
        for s in self._signs():
            xa, xb = sorted((s * x0, s * x1))
            za, zb = sorted((s * z0, s * z1))
            planes = [
                ((1.0, 0.0, 0.0), xb), ((-1.0, 0.0, 0.0), -xa),
                ((0.0, 1.0, 0.0), h), ((0.0, -1.0, 0.0), 0.0),
                ((0.0, 0.0, 1.0), zb), ((0.0, 0.0, -1.0), -za),
            ]
            self.fenced.append(len(self.solids))
            self.solids.append(((xa, 0.0, za), (xb, h, zb), planes))

    def fence(self, axis: str, c: float, a0: float, a1: float, kind: str = "link", gaps=(), lows=()) -> None:
        """A fence line, an open lattice of thin brushes (shots and sight pass through its
        gaps, bodies do not), in the plane x = c (`axis` "x", running along z from a0 to a1) or
        z = c (`axis` "z", running along x). `kind` is "link" (2.4 m, chain link) or "slat" (1.9 m,
        boards). `gaps` are open gates (at least `GATE_MIN` metres), `lows` are 1 m vaultable
        sections (at least `VAULT_MIN_WIDTH`), both (from, to) along the line and apart."""
        cuts = sorted([(g0, g1, "gap") for g0, g1 in gaps] + [(l0, l1, "low") for l0, l1 in lows])
        pieces = []
        cursor = a0
        for g0, g1, what in cuts:
            assert cursor <= g0 < g1 <= a1, ("bad fence gap", axis, c, g0, g1)
            assert what == "low" or g1 - g0 >= GATE_MIN, ("gate under 3 m", axis, c, g0, g1)
            assert what == "gap" or g1 - g0 >= VAULT_MIN_WIDTH, ("vault section under 1 m", axis, c, g0, g1)
            if g0 > cursor:
                pieces.append((cursor, g0, False))
            if what == "low":
                pieces.append((g0, g1, True))
            cursor = g1
        if cursor < a1:
            pieces.append((cursor, a1, False))
        for p0, p1, low in pieces:
            h = LOW_FENCE if low else FENCE_HEIGHT[kind]
            start = len(self.solids)
            self._fence_piece(axis, c, p0, p1, kind, h)
            self.fence_bars.update(range(start, len(self.solids)))
            self._stand_in(axis, c, p0, p1, h)
            self.fence_metres += (p1 - p0) * len(self._signs())

    def fence_rect(self, x0: float, x1: float, z0: float, z1: float, kind: str = "link", north=(), south=(), west=(), east=(), lows=None) -> None:
        """A fenced rectangle: each side with its gates (`north` is the side on z0, `west` the
        one on x0). `lows` maps a side name to its vaultable sections. The east and west runs
        stop a post's half-width short of the corners, where the north and south runs have their
        corner posts."""
        lows = lows or {}
        self.fence("z", z0, x0, x1, kind, north, lows.get("north", ()))
        self.fence("z", z1, x0, x1, kind, south, lows.get("south", ()))
        self.fence("x", x0, z0 + 0.14, z1 - 0.14, kind, west, lows.get("west", ()))
        self.fence("x", x1, z0 + 0.14, z1 - 0.14, kind, east, lows.get("east", ()))

    def container(self, x: float, z: float, yaw: float = 0.0) -> None:
        """A shipping container blockout, 6.0 x 2.4 m, 2.6 m tall, long axis along x at yaw 0:
        solid cover that stops sight and shots, and a 2.6 m ledge nobody mantles from the ground."""
        self.turned("corrugated", x, 1.3, z, 6.0, 2.6, 2.4, yaw)

    def dumpster(self, x: float, z: float, yaw: float = 0.0) -> None:
        self.turned("metal", x, 0.65, z, 2.0, 1.3, 1.2, yaw)

    def crate(self, x: float, z: float, size: float = 1.0, y: float = 0.0, yaw: float = 0.0) -> None:
        self.turned("wood", x, y + size / 2, z, size, size, size, yaw)

    # ---- output ----------------------------------------------------------------------------

    def write(self, path: Path) -> None:
        """Writes the .map, unless an objective site breaks the minimum spacing (see
        `objective_problems`): then it prints what and exits, so a layout can't ship with HQs
        on top of each other."""
        problems = objective_problems(self)
        if problems:
            print("Objective spacing violated, nothing written:")
            for line in problems:
                print("  " + line)
            sys.exit(1)
        path.parent.mkdir(parents=True, exist_ok=True)
        self.map.write(str(path))
        counts: dict[str, int] = {}
        for entity in self.map.entities:
            counts[entity.classname] = counts.get(entity.classname, 0) + 1
        brushes = len(self.map.brushes)
        print(f"Wrote {path}: {brushes} brushes, " + ", ".join(f"{n} {k}" for k, n in sorted(counts.items())))
        print("  " + objective_summary(self))


# ---- model bounds, objective spacing, grounding ---------------------------------------------

# Minimum distance (metres) between two sites of a class: HQs and bomb sites 30, Flags 25, ammo crates 8.
MIN_SPACING = {"hq_site": 30.0, "dom_flag": 25.0, "bomb_site": 30.0, "ammo_crate": 8.0}
# Ammo crates stand at least this far (metres) from any Team Deathmatch spawn, so a spawning
# player isn't resupplied for free and nobody camps one.
CRATE_SPAWN_CLEARANCE = 12.0
WALK_SPEED = 5.5  # m/s, data/movement.ron


def model_bounds(models: str) -> dict[str, tuple[Vec3, Vec3]]:
    """{model name: (min, max)} from the `bounds.json` that tools/assets/import_props.py writes
    next to a pack's .glb files (empty when the folder has none)."""
    if models.startswith("private://"):
        folder = REPO / "content" / "private" / models.removeprefix("private://")
    else:
        folder = REPO / "crates" / "frontline" / "assets" / models
    path = folder / "bounds.json"
    if not path.exists():
        return {}
    return {name: (tuple(b["min"]), tuple(b["max"])) for name, b in json.loads(path.read_text()).items()}


def objective_problems(level: Level) -> list[str]:
    """Pairs of HQ sites, Flags, bomb sites or ammo crates closer than `MIN_SPACING`, and ammo
    crates too near a Team Deathmatch spawn or inside a solid."""
    problems = []
    for classname, minimum in MIN_SPACING.items():
        sites = level.sites.get(classname, [])
        for i, (la, xa, _, za) in enumerate(sites):
            for lb, xb, _, zb in sites[i + 1 :]:
                distance = math.hypot(xa - xb, za - zb)
                if distance < minimum:
                    problems.append(
                        f"{classname} {la or '#'+str(i)} ({xa:.1f}, {za:.1f}) and {lb or '#'} ({xb:.1f}, {zb:.1f}) are {distance:.1f} m apart, need {minimum:.0f}"
                    )
    for _, x, y, z in level.sites.get("ammo_crate", []):
        for team, modes, sx, sz in level.spawns:
            if "tdm" in modes.split() and math.hypot(x - sx, z - sz) < CRATE_SPAWN_CLEARANCE:
                problems.append(f"ammo_crate ({x:.1f}, {z:.1f}) is within {CRATE_SPAWN_CLEARANCE:.0f} m of the {team} tdm spawn ({sx:.1f}, {sz:.1f})")
        for lo, hi, _ in level.solids:
            if hi[1] > y + 0.05 and lo[1] < y + 1.0 and lo[0] - 1.0 <= x <= hi[0] + 1.0 and lo[2] - 1.0 <= z <= hi[2] + 1.0:
                problems.append(f"ammo_crate ({x:.1f}, {z:.1f}) is inside or touching a solid x {lo[0]:.1f}..{hi[0]:.1f} z {lo[2]:.1f}..{hi[2]:.1f}")
                break
    return problems


def objective_summary(level: Level) -> str:
    parts = []
    for classname, minimum in MIN_SPACING.items():
        sites = level.sites.get(classname, [])
        gaps = [math.hypot(a[1] - b[1], a[3] - b[3]) for i, a in enumerate(sites) for b in sites[i + 1 :]]
        if gaps:
            parts.append(f"{classname} min spacing {min(gaps):.1f} m (>= {minimum:.0f})")
    return "; ".join(parts)


def check_grounded(level: Level) -> list[str]:
    """Floating things: brushes that aren't attached (touching or overlapping, chained) to the
    ground, and props whose base isn't on top of a solid at their spot."""
    problems = []
    solids = level.solids
    margin = 0.05
    grounded = [lo[1] <= 0.03 for lo, _, _ in solids]
    # Index by grid cell to keep it quick.
    cells: dict[tuple[int, int], list[int]] = {}
    for index, (lo, hi, _) in enumerate(solids):
        for i in range(int((lo[0] - margin) // 4), int((hi[0] + margin) // 4) + 1):
            for j in range(int((lo[2] - margin) // 4), int((hi[2] + margin) // 4) + 1):
                cells.setdefault((i, j), []).append(index)

    def touching(a: int) -> set[int]:
        lo, hi, _ = solids[a]
        found = set()
        for i in range(int((lo[0] - margin) // 4), int((hi[0] + margin) // 4) + 1):
            for j in range(int((lo[2] - margin) // 4), int((hi[2] + margin) // 4) + 1):
                for b in cells.get((i, j), ()):
                    lo2, hi2, _ = solids[b]
                    if (
                        b != a
                        and lo2[0] <= hi[0] + margin and hi2[0] >= lo[0] - margin
                        and lo2[2] <= hi[2] + margin and hi2[2] >= lo[2] - margin
                        and lo2[1] <= hi[1] + margin and hi2[1] >= lo[1] - margin
                    ):
                        found.add(b)
        return found

    stack = [i for i, g in enumerate(grounded) if g]
    while stack:
        for other in touching(stack.pop()):
            if not grounded[other]:
                grounded[other] = True
                stack.append(other)
    for index, ok in enumerate(grounded):
        if not ok:
            lo, hi, _ = solids[index]
            problems.append(f"floating brush x {lo[0]:.1f}..{hi[0]:.1f} y {lo[1]:.2f}..{hi[1]:.2f} z {lo[2]:.1f}..{hi[2]:.1f}")

    for name, x, y, z, bounds in level.props:
        if bounds is None or bounds[1][1] - bounds[0][1] < 0.05:
            continue  # unknown, or a flat decal or tile lying on the ground
        on_top = False
        for index in cells.get((int(x // 4), int(z // 4)), ()):
            lo, hi, _ = solids[index]
            if lo[0] - 0.6 <= x <= hi[0] + 0.6 and lo[2] - 0.6 <= z <= hi[2] + 0.6 and abs(hi[1] - y) <= 0.06:
                on_top = True
                break
        if not on_top:
            problems.append(f"prop {name} at ({x:.1f}, {z:.1f}) base y {y:.2f} has nothing under it")
    return problems


# ---- walkability checker -------------------------------------------------------------------

CELL = 0.25
BODY_HEIGHT = 1.8
STEP = 0.4
MANTLE = 1.5
MAX_DROP = 6.0


def _spans(level: Level, bounds: tuple[float, float, float, float]):
    """Solid vertical spans per grid cell: {(i, j): [(y0, y1), ...]} merged and sorted."""
    x0, x1, z0, z1 = bounds
    spans: dict[tuple[int, int], list[tuple[float, float]]] = {}
    for lo, hi, planes in level.solids:
        i0, i1 = max(int((lo[0] - x0) / CELL), 0), int((hi[0] - x0) / CELL)
        j0, j1 = max(int((lo[2] - z0) / CELL), 0), int((hi[2] - z0) / CELL)
        for i in range(i0, i1 + 1):
            x = x0 + (i + 0.5) * CELL
            if not lo[0] <= x <= hi[0]:
                continue
            for j in range(j0, j1 + 1):
                z = z0 + (j + 0.5) * CELL
                if not lo[2] <= z <= hi[2]:
                    continue
                bottom, top = lo[1] - 1e-6, hi[1] + 1e-6
                for (nx, ny, nz), d in planes:
                    rest = d - nx * x - nz * z
                    if abs(ny) < 1e-9:
                        if rest < -1e-9:
                            top = bottom - 1
                            break
                    elif ny > 0:
                        top = min(top, rest / ny)
                    else:
                        bottom = max(bottom, rest / ny)
                if top > bottom + 1e-6:
                    spans.setdefault((i, j), []).append((bottom, top))
    for key, items in spans.items():
        items.sort()
        merged = [items[0]]
        for a, b in items[1:]:
            if a <= merged[-1][1] + 1e-4:
                merged[-1] = (merged[-1][0], max(merged[-1][1], b))
            else:
                merged.append((a, b))
        spans[key] = merged
    return spans


def check_walkability(level: Level, bounds, starts, goals, image: Path | None = None, loose_goals=(), tour=(), route_out: Path | None = None, walk_range: tuple[float, float] | None = None) -> bool:
    """Floods the map from `starts` ((x, z) pairs) on foot, a 1.8 m tall body that steps up 0.4 m
    and drops down freely, and reports: goals ({name: (x, z)} or (x, y, z)) it cannot reach; spots
    reachable but not escapable (a drop with no way back); and closed-off walkable areas. A second
    flood that also vaults ledges up to 1.5 m shows what is only reachable by mantling. Writes a
    top-down image (green reachable on foot, yellow by mantling, red trapped, blue other
    floor, dark solid) when `image` is given. With `tour` (a list of (x, z) or (x, y, z) points)
    and `route_out`, also writes the on-foot route through them, keeping off walls, as `x y z`
    lines (about a metre apart) for the in-game walk hook. Returns True when everything checks
    out."""
    x0, x1, z0, z1 = bounds
    width, height = int((x1 - x0) / CELL), int((z1 - z0) / CELL)
    spans = _spans(level, bounds)

    # Floors: (i, j, k) -> height, for cells whose body fits above a span top.
    floors: dict[tuple[int, int], list[float]] = {}
    for (i, j), items in spans.items():
        if not (0 <= i < width and 0 <= j < height):
            continue
        for k, (_, top) in enumerate(items):
            above = items[k + 1][0] if k + 1 < len(items) else math.inf
            if above - top >= BODY_HEIGHT:
                floors.setdefault((i, j), []).append(top)

    def shoulder_clear(i: int, j: int, h: float) -> bool:
        # No solid at body height in the 8 cells around: the body is 0.6 m wide.
        for di in (-1, 0, 1):
            for dj in (-1, 0, 1):
                for a, b in spans.get((i + di, j + dj), ()):
                    if b > h + STEP and a < h + BODY_HEIGHT:
                        return False
        return True

    def blocked(i: int, j: int, h: float) -> bool:
        """Solid at body height above a floor at `h` in this cell."""
        return any(b > h + STEP and a < h + BODY_HEIGHT for a, b in spans.get((i, j), ()))

    def falls_through(landing, h: float) -> bool:
        """A drop from height `h` that would pass through a slab above the landing node."""
        low = nodes[landing]
        return any(b > low + 1e-3 and a < h - 1e-2 for a, b in spans.get(landing[:2], ()))

    nodes = {(i, j, k): h for (i, j), hs in floors.items() for k, h in enumerate(hs) if shoulder_clear(i, j, h)}
    by_cell: dict[tuple[int, int], list[tuple[int, int, int]]] = {}
    for node in nodes:
        by_cell.setdefault(node[:2], []).append(node)

    cache: dict = {}

    def neighbours(node, rise):
        key = (node, rise)
        if key not in cache:
            cache[key] = list(_neighbours(node, rise))
        return cache[key]

    def _neighbours(node, rise):
        i, j, _ = node
        h = nodes[node]
        # Stepping off a ledge: the body's edge clears it, so land up to two cells away (the
        # cells right below a wall or ledge are not standable themselves), but never through a
        # wall or a window's sill: the cell between must be open at body height.
        for di in range(-2, 3):
            for dj in range(-2, 3):
                if max(abs(di), abs(dj)) == 2 and not blocked(i + int(di / 2), j + int(dj / 2), h) and not blocked(i + di, j + dj, h):
                    for other in by_cell.get((i + di, j + dj), ()):
                        if -MAX_DROP <= nodes[other] - h <= -0.5 and not falls_through(other, h):
                            yield other
        for di in (-1, 0, 1):
            for dj in (-1, 0, 1):
                if di == 0 and dj == 0:
                    continue
                # Diagonal moves need both side cells open, so corners are not cut.
                for other in by_cell.get((i + di, j + dj), ()):
                    dy = nodes[other] - h
                    if -MAX_DROP <= dy <= rise and not (dy < -0.5 and (falls_through(other, h) or blocked(i + di, j + dj, h))):
                        if di and dj and not (
                            any(abs(nodes[o] - h) <= rise for o in by_cell.get((i + di, j), ()))
                            and any(abs(nodes[o] - h) <= rise for o in by_cell.get((i, j + dj), ()))
                        ):
                            continue
                        yield other

    def nearest(x, z, y=None):
        cell = (int((x - x0) / CELL), int((z - z0) / CELL))
        best = None
        for radius in range(0, 6):
            for di in range(-radius, radius + 1):
                for dj in range(-radius, radius + 1):
                    for node in by_cell.get((cell[0] + di, cell[1] + dj), ()):
                        key = (abs(nodes[node] - (y if y is not None else 0.0)), radius)
                        if best is None or key < best[0]:
                            best = (key, node)
            if best:
                return best[1]
        return None

    def flood(seeds, rise):
        seen = set(seeds)
        stack = list(seeds)
        while stack:
            node = stack.pop()
            for other in neighbours(node, rise):
                if other not in seen:
                    seen.add(other)
                    stack.append(other)
        return seen

    seeds = []
    ok = True
    for x, z in starts:
        node = nearest(x, z)
        if node is None:
            print(f"  NO FLOOR at spawn ({x}, {z})")
            ok = False
        else:
            seeds.append(node)
    on_foot = flood(seeds, STEP)
    mantled = flood(seeds, MANTLE)

    for name, goal in goals.items():
        x, z = goal[0], goal[-1]
        y = goal[1] if len(goal) == 3 else 0.0
        node = nearest(x, z, y)
        if node is None or abs(nodes[node] - y) > 0.6:
            print(f"  NO FLOOR at {name} ({x}, {z})")
            ok = False
        elif node not in on_foot:
            print(f"  UNREACHABLE on foot: {name} ({x}, {z}) {'(mantle only)' if node in mantled else '(not at all)'}")
            ok = False

    # Trapped: reachable on foot but no way back to a spawn (reverse flood over directed edges).
    reverse: dict = {}
    for node in nodes:
        for other in neighbours(node, STEP):
            reverse.setdefault(other, []).append(node)
    back = set(seeds)
    stack = list(seeds)
    while stack:
        node = stack.pop()
        for other in reverse.get(node, ()):
            if other not in back:
                back.add(other)
                stack.append(other)
    trapped = [n for n in on_foot if n not in back]
    if trapped:
        ok = False
        xs = [x0 + (n[0] + 0.5) * CELL for n in trapped]
        zs = [z0 + (n[1] + 0.5) * CELL for n in trapped]
        print(f"  TRAPPED: {len(trapped)} cells ({len(trapped) * CELL * CELL:.1f} m2) x {min(xs):.1f}..{max(xs):.1f} z {min(zs):.1f}..{max(zs):.1f}")

    # Closed-off floor: connected components (by on-foot edges, either direction) apart from the
    # main one, ignoring slivers.
    rest = [n for n in nodes if n not in on_foot and n not in mantled]
    unseen = set(rest)
    pockets = []
    while unseen:
        seed = next(iter(unseen))
        comp = flood([seed], STEP) & unseen
        unseen -= comp
        if len(comp) * CELL * CELL >= 2.0:
            xs = [x0 + (n[0] + 0.5) * CELL for n in comp]
            zs = [z0 + (n[1] + 0.5) * CELL for n in comp]
            ys = [nodes[n] for n in comp]
            pockets.append((len(comp), min(xs), max(xs), min(zs), max(zs), min(ys), max(ys)))
    for count, ax, bx, az, bz, ay, by in sorted(pockets, reverse=True)[:12]:
        print(f"  closed-off floor {count * CELL * CELL:.1f} m2 x {ax:.1f}..{bx:.1f} z {az:.1f}..{bz:.1f} y {ay:.2f}..{by:.2f}")
    only_mantle = len(mantled) - len(on_foot)
    area = lambda cells: len(cells) * CELL * CELL
    print(f"  walkable {area(nodes):.0f} m2: {area(on_foot):.0f} on foot, {area(mantled - on_foot):.0f} more by mantling")
    ok = _measure_layout(level, nodes, nearest, neighbours, on_foot, x0, z0, walk_range) and ok
    for line in check_grounded(level):
        print(f"  FLOATING: {line}")
        ok = False
    for line in objective_problems(level):
        print(f"  SPACING: {line}")
        ok = False

    if tour and route_out is not None:
        _write_route(nodes, by_cell, neighbours, nearest, tour, route_out, x0, z0)

    if image is not None:
        for suffix, band in (("", (-1.0, 1.5)), (".up", (1.5, 99.0))):
            rows = []
            for j in range(height):
                row = bytearray()
                for i in range(width):
                    cell_nodes = [n for n in by_cell.get((i, j), ()) if band[0] <= nodes[n] < band[1]]
                    if any(n in on_foot for n in cell_nodes):
                        colour = (60, 200, 90) if all(n in back for n in cell_nodes if n in on_foot) else (220, 50, 50)
                    elif any(n in mantled for n in cell_nodes):
                        colour = (230, 200, 60)
                    elif cell_nodes:
                        colour = (70, 120, 220)
                    elif (i, j) in spans and spans[(i, j)][-1][1] > 0.5:
                        colour = (40, 40, 48)
                    else:
                        colour = (120, 120, 120)
                    row += bytes(colour)
                rows.append(bytes(row))
            # Row 0 is the north (-z) edge, column 0 the west edge.
            with open(image.with_suffix(suffix + ".ppm"), "wb") as handle:
                handle.write(f"P6 {width} {height} 255\n".encode() + b"".join(rows))
    return ok


def _measure_layout(level: Level, nodes, nearest, neighbours, on_foot, x0: float, z0: float, walk_range) -> bool:
    """Prints the playable extents (the on-foot cells' bounding box), the objective spacing and
    the longest walk between the Teams' spawns (shortest on-foot path of each pair, longest pair;
    also over every spawn including the free-for-all ones), with the crossing time at the walk
    speed. `walk_range` (min, max metres) is the size this Map is meant to have: outside it the
    check fails."""
    import heapq

    xs = [x0 + (n[0] + 0.5) * CELL for n in on_foot]
    zs = [z0 + (n[1] + 0.5) * CELL for n in on_foot]
    print(f"  extents: x {min(xs):.1f}..{max(xs):.1f} ({max(xs) - min(xs):.0f} m) z {min(zs):.1f}..{max(zs):.1f} ({max(zs) - min(zs):.0f} m); {objective_summary(level)}")

    def spots(*teams):
        return sorted({(x, z) for team, _, x, z in level.spawns if team in teams})

    alpha, bravo, anyone = spots("alpha"), spots("bravo"), spots("any")
    everyone = alpha + bravo + anyone
    if not alpha or not bravo:
        return True
    goals = {nearest(x, z): (x, z) for x, z in everyone}
    goals.pop(None, None)
    longest = {"teams": (0.0, None), "all": (0.0, None)}
    for sx, sz in alpha + anyone:
        source = nearest(sx, sz)
        if source is None:
            continue
        dist = {source: 0.0}
        heap = [(0.0, source)]
        pending = set(goals)
        while heap and pending:
            cost, current = heapq.heappop(heap)
            if cost > dist.get(current, math.inf):
                continue
            pending.discard(current)
            for other in neighbours(current, STEP):
                step = math.hypot(other[0] - current[0], other[1] - current[1]) * CELL + abs(nodes[other] - nodes[current])
                if cost + step < dist.get(other, math.inf):
                    dist[other] = cost + step
                    heapq.heappush(heap, (cost + step, other))
        for node, spot in goals.items():
            if node in dist and node != source:
                if dist[node] > longest["all"][0]:
                    longest["all"] = (dist[node], ((sx, sz), spot))
                if (sx, sz) in alpha and spot in bravo and dist[node] > longest["teams"][0]:
                    longest["teams"] = (dist[node], ((sx, sz), spot))
    length, pair = longest["teams"]
    print(f"  longest spawn-to-spawn walk (Alpha to Bravo): {length:.0f} m = {length / WALK_SPEED:.0f} s at {WALK_SPEED} m/s, {pair}")
    length_all, pair_all = longest["all"]
    print(f"  longest walk between any two spawns: {length_all:.0f} m = {length_all / WALK_SPEED:.0f} s, {pair_all}")
    if walk_range is not None and not walk_range[0] <= length <= walk_range[1]:
        print(f"  SIZE: longest Alpha-Bravo walk {length:.0f} m is outside the intended {walk_range[0]:.0f}..{walk_range[1]:.0f} m")
        return False
    return True



def _write_route(nodes, by_cell, neighbours, nearest, tour, route_out: Path, x0: float, z0: float) -> None:
    """Dijkstra over the on-foot graph through the tour's points, preferring cells away from
    walls, written as `x y z` lines about a metre apart."""
    import heapq

    near_cache: dict = {}

    def near_wall(node) -> bool:
        """Within 0.5 m of a wall, a ledge or a void: some cell around has no floor at about the
        node's height."""
        if node not in near_cache:
            i, j, _ = node
            h = nodes[node]
            near_cache[node] = any(
                not any(abs(nodes[o] - h) <= 0.5 for o in by_cell.get((i + di, j + dj), ()))
                for di in range(-2, 3)
                for dj in range(-2, 3)
            )
        return near_cache[node]

    def centre(node):
        return (x0 + (node[0] + 0.5) * CELL, nodes[node], z0 + (node[1] + 0.5) * CELL)

    lines = []
    previous = None
    for point in tour:
        node = nearest(point[0], point[-1], point[1] if len(point) == 3 else 0.0)
        if previous is not None:
            best = {previous: 0.0}
            parent = {}
            heap = [(0.0, previous)]
            while heap:
                cost, current = heapq.heappop(heap)
                if current == node:
                    break
                if cost > best.get(current, math.inf):
                    continue
                for other in neighbours(current, STEP):
                    step = math.hypot(other[0] - current[0], other[1] - current[1]) * CELL
                    step *= 4.0 if near_wall(other) else 1.0
                    if cost + step < best.get(other, math.inf):
                        best[other] = cost + step
                        parent[other] = current
                        heapq.heappush(heap, (cost + step, other))
            if node not in parent and node != previous:
                print(f"  NO ROUTE to {point}")
                return
            chain = [node]
            while chain[-1] in parent:
                chain.append(parent[chain[-1]])
            chain.reverse()
            lines.extend(centre(n) for n in chain[::4])
            lines.append(centre(node))
        else:
            lines.append(centre(node))
        previous = node
    route_out.write_text("".join(f"{x:.2f} {y:.2f} {z:.2f}\n" for x, y, z in lines))
    print(f"  route: {len(lines)} waypoints -> {route_out}")


# ---- placement, sight and climb checks -------------------------------------------------------

MANTLE_MAX = 1.6  # the highest ledge a body mantles (data/movement.ron)


def inside_solid(level: Level, x: float, z: float) -> bool:
    """Whether a standing body at (x, z) on the floor would overlap a brush (fences too)."""
    for lo, hi, planes in level.solids:
        if not (lo[0] - 0.3 <= x <= hi[0] + 0.3 and lo[2] - 0.3 <= z <= hi[2] + 0.3 and lo[1] < 1.7 and hi[1] > 0.05):
            continue
        for y in (0.3, 1.0, 1.6):
            if all(n[0] * x + n[1] * y + n[2] * z <= d + 1e-6 for n, d in planes):
                return True
    return False


def sees(level: Level, a: tuple[float, float], b: tuple[float, float], eye: float) -> bool:
    """Whether a ray at height `eye` from a to b misses every brush but the fences (which do not
    block sight): true when the two points can see each other."""
    fenced = set(level.fenced)
    ax, az = a
    dx, dz = b[0] - ax, b[1] - az
    for index, (lo, hi, planes) in enumerate(level.solids):
        if index in fenced or not lo[1] <= eye <= hi[1]:
            continue
        if max(ax, b[0]) < lo[0] or min(ax, b[0]) > hi[0] or max(az, b[1]) < lo[2] or min(az, b[1]) > hi[2]:
            continue
        t0, t1 = 0.0, 1.0
        for n, d in planes:
            num = d - (n[0] * ax + n[1] * eye + n[2] * az)
            den = n[0] * dx + n[2] * dz
            if abs(den) < 1e-12:
                if num < 0:
                    t0, t1 = 1.0, 0.0
                    break
            elif den > 0:
                t1 = min(t1, num / den)
            else:
                t0 = max(t0, num / den)
            if t0 >= t1:
                break
        if t0 < t1 - 1e-9:
            return False
    return True


def top_at(level: Level, x: float, z: float, below: float = 99.0) -> float:
    """Height of the highest brush top under (x, z) that is no higher than `below` (0 if none)."""
    best = 0.0
    for lo, hi, planes in level.solids:
        if hi[1] > below + 1e-6 or hi[1] <= best or not (lo[0] <= x <= hi[0] and lo[2] <= z <= hi[2]):
            continue
        y = hi[1] - 1e-3
        if all(n[0] * x + n[1] * y + n[2] * z <= d + 1e-6 for n, d in planes):
            best = hi[1]
    return best


def climb_problems(level: Level, climbs: dict[str, list[tuple[float, ...]]]) -> list[str]:
    """Trick-jump routes ({name: the (x, z) of each ledge in climbing order}; a third number
    `below` picks the highest top no higher than it, for things stacked in one spot): prints each one's
    tops and returns the steps above `MANTLE_MAX`. The flood-fill cannot test these, since a body
    that stands against a ledge is never "shoulder clear" there."""
    problems = []
    for name, points in climbs.items():
        heights = [top_at(level, *point) for point in points]
        rises = [b - a for a, b in zip(heights, heights[1:])]
        bad = [r for r in rises if r > MANTLE_MAX + 1e-6]
        print(f"  climb {name}: tops {' -> '.join(f'{h:.2f}' for h in heights)} m" + ("" if not bad else f"  TOO HIGH: step {max(bad):.2f} m"))
        if bad:
            problems.append(f"climb {name}: a step of {max(bad):.2f} m is above the {MANTLE_MAX} m mantle limit")
    return problems


def placement_problems(level: Level) -> list[str]:
    """Spawns and objective sites inside a brush, and spawns of the two Teams that see each other
    (standing or crouched; fences do not block sight). Team Deathmatch's team-less "any" spawns
    are only counted."""
    problems = []
    for team, _, x, z in level.spawns:
        if inside_solid(level, x, z):
            problems.append(f"{team} spawn ({x}, {z}) is inside a brush")
    for classname, sites in level.sites.items():
        for _, x, _, z in sites:
            if inside_solid(level, x, z):
                problems.append(f"{classname} ({x}, {z}) is inside a brush")
    alpha = [(x, z) for team, _, x, z in level.spawns if team == "alpha"]
    bravo = [(x, z) for team, _, x, z in level.spawns if team == "bravo"]
    clear = 0
    for a in alpha:
        for b in bravo:
            for eye in (1.6, 0.9):
                if sees(level, a, b, eye):
                    problems.append(f"alpha spawn {a} sees bravo spawn {b} at eye height {eye}")
                    clear += 1
    print(f"  spawn sightlines: {len(alpha)} x {len(bravo)} Alpha/Bravo pairs at 2 eye heights, {clear} clear")
    if level.fence_metres:
        print(f"  fences: {level.fence_metres:.0f} m of line")
    return problems


def fence_conflicts(level: Level) -> list[str]:
    """Things standing in a fence: any other brush that overlaps a fence run's footprint (the
    0.4 m stand-in) by more than a few centimetres. Fences meeting at a corner are fine."""
    problems = []
    fenced = set(level.fenced)
    for f in level.fenced:
        lo, hi, _ = level.solids[f]
        for i, (lo2, hi2, _) in enumerate(level.solids):
            if i in fenced or i in level.fence_bars:
                continue
            if all(min(hi[k], hi2[k]) - max(lo[k], lo2[k]) > 0.03 for k in range(3)):
                problems.append(f"fence x {lo[0]:.1f}..{hi[0]:.1f} z {lo[2]:.1f}..{hi[2]:.1f} is overlapped by a brush x {lo2[0]:.1f}..{hi2[0]:.1f} y {lo2[1]:.1f}..{hi2[1]:.1f} z {lo2[2]:.1f}..{hi2[2]:.1f}")
    return problems
