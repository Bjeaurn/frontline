"""Generates crates/frontline/assets/maps/winkelweg.map: Winkelweg 21, an original Remix Map of a
Dutch arable farm in the polder, where potatoes and onions are harvested and stored.

The layout comes from the brief in GitHub issue #7 by @vanrijs: the annotated aerial screenshot
and its notes (furnish the buildings with plenty of objects, red warehouses, greenery next to the
farmhouse, cars and crates around, potatoes and onions everywhere). It is original work: no image
of the brief is in the repository.

The layout follows an annotated aerial screenshot of the yard (red lines: the Map's boundary,
green crosses: enterable buildings, blue marks: their entrances, a red pin: the farmhouse). The
yard is turned about 13 degrees against the screenshot; the Map is turned back so every wall is
axis-aligned. Everything below is written in "plan metres": x 0..123 east from the boundary's
north-west corner, z 0..67 south from it, and the whole layout is shifted so the Map is centred
on the origin (x east, z south, -z the Map's north, as in the other Maps).

    North strip (in front of the potato store) ............ x 0..123, z 0..5
    West lane (Alpha's side, a row of trees) .............. x 0..24, z 0..67
    Potato store (red hall, sorting line, box stacks) ..... x 24..68, z 5..31
    Alley between the stores ............................... x 12..87, z 31..35
    Onion store (red hall, bulk onion heap, tractor) ...... x 12..87, z 35..61
    Yard (harvester, cars, pallets) ......................... x 68..97, z 18..35
    Workshop and barn (the two small sheds) ................ x 82..96 and x 101..112, z 3..18
    Farmhouse and garden (Bravo's side) .................... x 97..123, z 20..67

Alpha starts in the west lane, Bravo in the farmhouse garden; the two stores and the yard are in
between. Spawn to spawn is about 110 m on foot.

    python3 tools/mapgen/winkelweg.py
    python3 tools/mapgen/winkelweg.py --check      # also flood-fill the walkable floor
    python3 tools/mapgen/lint.py crates/frontline/assets/maps/winkelweg.map

Its data/maps.ron entry is "winkelweg" (name "Winkelweg 21", look overcast_urban, baekho against
volkov). Crops are told apart by colour: potatoes are `ground` (earth brown), onions `plaster_ochre`
(orange-brown), straw bales `sand`; the boxes, heaps, clamps and lorries below are filled with them.

The halls are walled in `corrugated` (the game's red-brown sheet steel): the closest surface to
the red sheds of the screenshot. Their roofs, out of reach at 6.5-9.6 m, are `corrugated_blue`
on the south slope (where the real yard has its solar panels) and `metal_white` on the north.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

from dressing import barrels, crates, pallet_stack, tyres
from kit import MAPS_DIR, Level, door, window

OUT = MAPS_DIR / "winkelweg.map"

# Plan metres to game metres: the playfield is centred on the origin.
OX, OZ = -61.0, -33.0
X0, X1 = 0.0, 123.0  # the playfield (plan metres)
Z0, Z1 = 0.0, 67.0
EDGE = 4.5  # height of the perimeter hedges and the neighbour's shed: nobody mantles that
T = 0.4  # hall wall thickness
WALL = "corrugated"
ROOF_SOUTH, ROOF_NORTH = "corrugated_blue", "metal_white"
ONION = "plaster_ochre"  # onions: orange-brown; potatoes are `ground`, straw `sand`


def g(x: float, z: float) -> tuple[float, float]:
    """A plan point in game metres (for the checker and spawn facings, which `shifted` skips)."""
    return (x + OX, z + OZ)


# ---- helpers ------------------------------------------------------------------------------


def big_door(c: float, w: float = 4.5, h: float = 4.6):
    """A roller-door opening `w` wide centred on `c` along a wall, `h` high."""
    return door(c - w / 2, c + w / 2, h)


def hall(L: Level, x0: float, x1: float, z0: float, z1: float, h: float, rise: float, wall: str = WALL,
         roof_south: str = ROOF_SOUTH, roof_north: str = ROOF_NORTH, t: float = T, overhang: float = 0.5, **sides) -> None:
    """A shed: four walls with their door and window gaps (`north`, `south`, `west`, `east`), a gable
    on each end wall and a pitched roof (ridge along x). The gables stop 0.12 m under the roof's
    top face, so the roof plank buries them and no two faces share a plane."""
    L.room(wall, x0, x1, z0, z1, 0.0, h, t, **sides)
    zm = (z0 + z1) / 2
    for xa in (x0, x1 - t):
        L.ramp(wall, xa, xa + t, z0, zm, h, h + rise - 0.12, "+z")
        L.ramp(wall, xa, xa + t, zm, z1, h, h + rise - 0.12, "-z")
    half = (z1 - z0) / 2
    drop = rise * overhang / half  # how far the eave sinks over the overhang
    xm, width = (x0 + x1) / 2, x1 - x0 + 2 * overhang
    # The north slope is 6 cm shorter, so the two planks' end faces are never in one plane.
    L.slab_ramp(roof_north, (xm, h - drop, z0 - overhang), (xm, h + rise, zm), width - 0.06)
    L.slab_ramp(roof_south, (xm, h - drop, z1 + overhang), (xm, h + rise, zm), width)


def drum(L: Level, axis: str, c: float, a: float, w: float, top: float, outward: int) -> None:
    """The rolled-up roller door over an opening `w` wide at `a` along a wall whose outer face is
    at `c` (`outward` -1 north/west, +1 south/east): a metal box on that face, sunk 0.1 m into it and
    0.05 m above the opening's head (so its underside is not in the lintel's plane)."""
    n0, n1 = sorted((c - outward * 0.1, c + outward * 0.55))
    if axis == "z":
        L.block("metal", a - w / 2 - 0.2, a + w / 2 + 0.2, top + 0.05, top + 0.6, n0, n1)
    else:
        L.block("metal", n0, n1, top + 0.05, top + 0.6, a - w / 2 - 0.2, a + w / 2 + 0.2)


# Stack heights of the storage boxes, in boxes, read in turn so a regenerated Map is the same.
_HEIGHTS = [0, 0, -1, 0, -1, 0, 0, -2, 0, -1, 0, 0, -1, 0, -1, -1, 0, 0, -2, 0, -1]
_turn = [0]


def kisten(L: Level, x: float, z: float, nx: int, nz: int, high: int, crop: str | None = "ground", along_z: bool = False) -> None:
    """A block of wooden storage boxes ("kisten", 1.9 x 1.25 m, 1.2 m a box) stacked `high` high
    with a few columns a box or two lower, so the block has steps and ledges. Columns stand 0.05 m
    apart (no shared planes). A full box shows its crop as a layer inset in its top: `ground` for
    potatoes, `plaster_ochre` for onions, None for empty boxes."""
    bx, bz = (1.25, 1.9) if along_z else (1.9, 1.25)
    for i in range(nx):
        for j in range(nz):
            k = max(1, high + _HEIGHTS[_turn[0] % len(_HEIGHTS)])
            _turn[0] += 1
            xa, za = x + i * (bx + 0.05), z + j * (bz + 0.05)
            top = 1.2 * k
            L.block("wood", xa, xa + bx, 0.0, top, za, za + bz)
            if crop:  # the fill heaps a little over the rim
                L.block(crop, xa + 0.1, xa + bx - 0.1, top - 0.08, top + 0.1, za + 0.1, za + bz - 0.1)


def part(L: Level, x: float, z: float, yaw: float, lx: float, y: float, lz: float, sx: float, sy: float, sz: float, surface: str) -> None:
    """One block of a vehicle blockout, at (lx, lz) in the vehicle's own frame (front along +x)."""
    c, n = math.cos(yaw), math.sin(yaw)
    L.turned(surface, x + lx * c + lz * n, y, z - lx * n + lz * c, sx, sy, sz, yaw)


def tractor(L: Level, x: float, z: float, yaw: float = 0.0, body: str = "metal_green") -> None:
    """A farm tractor, 4.6 x 2.5 m, 3.3 m to the cab roof, front along +x at yaw 0 (radians): big
    rear wheels you can crouch behind, a cab you can see through only by its height."""
    p = lambda *a: part(L, x, z, yaw, *a)
    for s in (-1.0, 1.0):
        p(-0.9, 0.85, s * 1.0, 1.7, 1.7, 0.5, "asphalt")  # rear wheels
        p(1.6, 0.5, s * 0.85, 1.0, 1.0, 0.36, "asphalt")  # front wheels
    p(-0.9, 0.85, 0.0, 0.22, 0.2, 1.6, "metal")  # rear axle
    p(1.6, 0.5, 0.0, 0.2, 0.18, 1.4, "metal")  # front axle
    p(0.2, 0.75, 0.0, 4.0, 0.5, 0.9, "metal")  # chassis
    p(1.0, 1.55, 0.0, 2.6, 1.1, 1.1, body)  # bonnet
    p(-0.7, 1.45, 0.0, 1.7, 0.9, 1.5, body)  # cab base
    p(-0.7, 2.6, 0.0, 1.6, 1.4, 1.46, "metal_white")  # cab
    p(-0.7, 3.36, 0.0, 1.8, 0.12, 1.7, body)  # cab roof


def trailer(L: Level, x: float, z: float, yaw: float = 0.0, load: str | None = "ground") -> None:
    """A tipping trailer, 6 x 2.4 m with 1.1 m sides, front (drawbar) along +x: a long cover wall
    a body can crouch behind or mantle onto (2.2 m: a jump and a mantle)."""
    p = lambda *a: part(L, x, z, yaw, *a)
    for s in (-1.0, 1.0):
        for wx in (-0.6, 0.6):
            p(wx, 0.5, s * 1.05, 1.0, 1.0, 0.36, "asphalt")
    p(0.0, 0.95, 0.0, 6.0, 0.2, 2.4, "corrugated_rust")  # bed
    p(0.0, 1.6, 1.14, 6.0, 1.1, 0.12, "corrugated_rust")  # sides
    p(0.0, 1.6, -1.14, 6.0, 1.1, 0.12, "corrugated_rust")
    p(2.94, 1.6, 0.0, 0.12, 1.1, 2.16, "corrugated_rust")
    p(-2.94, 1.6, 0.0, 0.12, 1.1, 2.16, "corrugated_rust")
    p(3.5, 0.8, 0.0, 1.2, 0.15, 0.2, "metal")  # drawbar, under the bed's front
    if load:
        p(0.0, 1.65, 0.0, 5.76, 1.2, 2.16, load)  # heaped 0.1 m over the sides


def harvester(L: Level, x: float, z: float, yaw: float = 0.0) -> None:
    """A trailed two-row potato harvester, 9 x 3 m: a low body, a 4.2 m bunker full of potatoes
    and the slanted elevator up to it. Its bulk hides a body, the elevator is a step up."""
    p = lambda *a: part(L, x, z, yaw, *a)
    for s in (-1.0, 1.0):
        for wx in (-2.2, -0.6):
            p(wx, 0.75, s * 1.55, 1.5, 1.5, 0.55, "asphalt")
    p(-1.4, 0.75, 0.0, 2.0, 0.22, 2.6, "metal")  # axles
    p(0.0, 1.7, 0.0, 8.5, 1.6, 2.6, "metal_white")  # body
    p(-1.8, 3.4, 0.0, 4.0, 1.8, 3.0, "corrugated_rust")  # bunker
    p(-1.8, 4.33, 0.0, 3.8, 0.06, 2.8, "ground")  # potatoes in it
    p(4.6, 0.6, 0.0, 1.6, 0.6, 2.9, "metal")  # the digging head at the front
    p(5.8, 0.9, 0.0, 1.0, 0.15, 0.2, "metal")  # drawbar


def forklift(L: Level, x: float, z: float, yaw: float = 0.0) -> None:
    """A warehouse forklift, 2.2 x 1.2 m, forks along +x."""
    p = lambda *a: part(L, x, z, yaw, *a)
    p(0.0, 0.85, 0.0, 2.2, 1.0, 1.2, "plaster_yellow")
    p(-0.7, 1.75, 0.0, 0.9, 0.8, 1.1, "metal")  # seat and counterweight
    p(1.2, 1.4, 0.0, 0.14, 2.8, 0.9, "metal")  # mast
    p(1.85, 0.1, 0.0, 1.2, 0.08, 0.7, "metal")  # forks
    p(-0.2, 2.8, 0.0, 1.3, 0.08, 1.2, "metal")  # overhead guard
    for s in (-1.0, 1.0):
        p(-0.6, 1.45, s * 0.55, 0.08, 2.7, 0.08, "metal")
        p(0.7, 0.35, s * 0.62, 0.7, 0.7, 0.2, "asphalt")
        p(-0.8, 0.35, s * 0.62, 0.7, 0.7, 0.2, "asphalt")


def bales(L: Level, x: float, z: float, count: int = 3, along_z: bool = False, layers: int = 1) -> None:
    """Square straw bales (2.4 x 1.2 m, 0.9 m high) side by side: waist-high cover, each layer a
    mantle up. Laid with 0.05 m gaps and the upper layer set back half a bale."""
    for layer in range(layers):
        for i in range(count - layer):
            a = i * 1.25 + layer * 0.6
            y0, y1 = 0.9 * layer, 0.9 * (layer + 1)
            if along_z:
                L.block("sand", x, x + 2.4, y0, y1, z + a, z + a + 1.2)
            else:
                L.block("sand", x + a, x + a + 1.2, y0, y1, z, z + 2.4)


def tree(L: Level, x: float, z: float, big: bool = False) -> None:
    """A farmyard tree: a trunk and a two-tier canopy (grass surface) above head height."""
    r = 2.2 if big else 1.8
    L.block("wood", x - 0.25, x + 0.25, 0.0, 3.2, z - 0.25, z + 0.25)
    L.block("grass", x - r, x + r, 3.0, 4.7, z - r, z + r)
    L.block("grass", x - r * 0.66, x + r * 0.66, 4.7, 6.0, z - r * 0.66, z + r * 0.66)


def bush(L: Level, x: float, z: float, r: float = 0.9, h: float = 1.2) -> None:
    L.block("grass", x - r, x + r, 0.0, h, z - r, z + r)


def shelf(L: Level, x0: float, x1: float, z0: float, z1: float, levels=(0.45, 1.15, 1.85), stock: bool = True) -> None:
    """A storage rack (its long side along x or z): boards at `levels`, uprights at both ends and
    every 2 m, cardboard boxes standing on the boards with gaps. Solid cover; the boxes are decoration."""
    along_x = (x1 - x0) >= (z1 - z0)
    a0, a1 = (x0, x1) if along_x else (z0, z1)
    n = max(1, round((a1 - a0) / 2.0))
    for i in range(n + 1):
        a = a0 + 0.1 + (a1 - a0 - 0.2) * i / n
        if along_x:
            L.block("metal", a - 0.04, a + 0.04, 0.0, levels[-1] + 0.5, z0 + 0.03, z1 - 0.03)
        else:
            L.block("metal", x0 + 0.03, x1 - 0.03, 0.0, levels[-1] + 0.5, a - 0.04, a + 0.04)
    for y in levels:
        L.block("wood", x0, x1, y, y + 0.05, z0, z1)
    if not stock:
        return
    for k, y in enumerate(levels):
        for j in range(int((a1 - a0 - 0.4) / 0.62)):
            if (j + k) % 4 == 3:
                continue
            a = a0 + 0.4 + j * 0.62
            cx, cz = (a, (z0 + z1) / 2) if along_x else ((x0 + x1) / 2, a)
            L.model("props/cardboard_box", cx, cz, 0.0 if along_x else 90.0, y + 0.05, solid=False, mounted=True)


def scatter(L: Level, *items: tuple) -> None:
    """Small props (model, x, z, facing in degrees), as the lists of a room's clutter."""
    for name, x, z, facing in items:
        L.model("props/" + name, x, z, facing)


def truck(L: Level, x: float, z: float, yaw: float = 0.0, load: str | None = ONION) -> None:
    """A farm lorry with a tipper body, 9 x 2.5 m, 2.6 m to the top of its load, cab and front along +x
    at yaw 0: a long wall with high sides (no mantle), full of crop heaped over the sides."""
    p = lambda *a: part(L, x, z, yaw, *a)
    for s in (-1.0, 1.0):
        for wx in (3.2, -1.6, -3.0):
            p(wx, 0.55, s * 1.12, 1.1, 1.1, 0.42, "asphalt")
    p(-0.2, 0.95, 0.0, 8.8, 0.3, 1.7, "metal")  # chassis
    p(3.2, 2.05, 0.0, 2.0, 1.9, 2.3, "plaster_blue")  # cab
    p(4.21, 2.45, 0.0, 0.04, 0.8, 1.9, "metal_white")  # windscreen
    p(-1.3, 1.2, 0.0, 6.4, 0.2, 2.5, "corrugated_rust")  # bed
    for s in (-1.0, 1.0):
        p(-1.3, 1.9, s * 1.19, 6.4, 1.2, 0.12, "corrugated_blue")
    p(1.9, 2.1, 0.0, 0.12, 1.6, 2.38, "corrugated_blue")
    p(-4.42, 1.85, 0.0, 0.12, 1.1, 2.38, "corrugated_blue")
    if load:
        p(-1.3, 1.95, 0.0, 6.1, 1.3, 2.2, load)


def clamp(L: Level, x0: float, x1: float, z0: float, z1: float) -> None:
    """A potato clamp ("aardappelkuil"): a long heap of potatoes in two tiers (each a mantle) under a
    black sheet along its crest."""
    L.block("ground", x0, x1, 0.0, 0.9, z0, z1)
    L.block("ground", x0 + 1.2, x1 - 1.2, 0.9, 1.6, z0 + 0.8, z1 - 0.8)
    L.block("asphalt", x0 + 1.6, x1 - 1.6, 1.6, 1.66, z0 + 1.1, z1 - 1.1)


def potato_rows(L: Level, x0: float, x1: float, z0: float, rows: int, pitch: float = 1.0) -> None:
    """Rows of potato plants along x: a 0.25 m ridge of earth with a strip of green on its crest
    (all of it low enough to step over)."""
    for i in range(rows):
        z = z0 + i * pitch
        L.block("ground", x0, x1, 0.0, 0.25, z, z + 0.55)
        L.block("grass", x0 + 0.2, x1 - 0.2, 0.25, 0.55, z + 0.12, z + 0.43)


# ---- ground, perimeter ----------------------------------------------------------------------


def ground(L: Level) -> None:
    """Concrete yard, a lawn round the farmhouse, a grass strip under the west trees and along the
    south edge. Non-overlapping floor tiles (no two top faces in one place)."""
    L.floor("concrete", X0, 97.0, Z0, 62.0)
    L.floor("concrete", 97.0, X1, Z0, 20.0)
    L.floor("grass", 97.0, X1, 20.0, Z1)
    L.floor("grass", X0, 97.0, 62.0, Z1)


def perimeter(L: Level) -> None:
    """The boundary: the neighbour's long shed along the north (a red wall with a few shut doors),
    tall hedges round the other three sides. All `EDGE` high, out of mantle reach."""
    r = 2.0
    L.block("grass", X0 - r, 22.0, 0.0, EDGE, Z0 - r, Z0)  # north hedge, west part
    L.block(WALL, 22.0, 92.0, 0.0, 7.0, Z0 - 8.0, Z0)  # the neighbour's shed
    L.block("metal_white", 22.0, 92.0, 7.0, 7.3, Z0 - 8.0, Z0)
    L.block("grass", 92.0, X1 + r, 0.0, EDGE, Z0 - r, Z0)  # north hedge, east part
    L.block("grass", X0 - r, X1 + r, 0.0, EDGE, Z1, Z1 + r)  # south
    L.block("grass", X0 - r, X0, 0.0, EDGE, Z0, Z1)  # west
    L.block("grass", X1, X1 + r, 0.0, EDGE, Z0, Z1)  # east, along the Winkelweg
    # Shut doors and a drain pipe on the neighbour's wall (decoration flat on it, 0.1 m proud).
    for c in (34.0, 58.0, 80.0):
        L.block("metal_white", c - 2.0, c + 2.0, 0.0, 4.2, Z0, Z0 + 0.1)


# ---- the potato store ------------------------------------------------------------------------


def potato_store(L: Level) -> None:
    """The north hall, x 24..68, z 5..31: storage boxes in blocks of different heights (lanes and
    ledges), the sorting line along the south wall, a forklift, bagged potatoes by the west doors,
    the ventilation duct up under the roof. Five ways in: two roller doors on the north strip,
    two on the west lane, one on the yard, and a door to the alley."""
    x0, x1, z0, z1, h = 24.0, 68.0, 5.0, 31.0, 6.5
    hall(
        L, x0, x1, z0, z1, h, 2.4,
        north=[big_door(35.0), big_door(52.0)],
        south=[door(44.6, 45.8, 2.3)],
        west=[big_door(12.0), big_door(25.0, 4.0, 4.4)],
        east=[big_door(16.0)],
    )
    for c in (35.0, 52.0):
        drum(L, "z", z0, c, 4.5, 4.6, -1)
    drum(L, "x", x0, 12.0, 4.5, 4.6, -1)
    drum(L, "x", x1, 16.0, 4.5, 4.6, 1)
    # Storage boxes: blocks of 3-4 high with lanes between them.
    kisten(L, 27.0, 16.0, 3, 5, 4)
    kisten(L, 37.0, 8.0, 4, 3, 3)
    kisten(L, 37.0, 19.0, 4, 5, 4)
    kisten(L, 49.0, 13.0, 3, 3, 2)
    kisten(L, 56.0, 21.0, 4, 4, 4)
    kisten(L, 58.0, 7.0, 3, 3, 3)
    # Sorting line: a hopper, a 15 m belt on legs, an inspection table at the end.
    L.block("plaster_yellow", 44.7, 46.3, 0.0, 1.1, 26.3, 27.9)  # hopper base
    L.block("metal", 44.2, 46.8, 1.1, 2.6, 25.8, 28.4)  # hopper
    for x in range(48, 62, 3):
        L.block("metal", x - 0.06, x + 0.06, 0.0, 0.9, 26.55, 26.67)
        L.block("metal", x - 0.06, x + 0.06, 0.0, 0.9, 27.53, 27.65)
    L.block("asphalt", 46.8, 61.5, 0.9, 1.05, 26.5, 27.7)  # the belt
    L.block("plaster_yellow", 46.8, 61.5, 1.05, 1.3, 26.42, 26.5)  # its side boards
    L.block("plaster_yellow", 46.8, 61.5, 1.05, 1.3, 27.7, 27.78)
    L.block("metal_white", 59.3, 61.7, 0.0, 1.1, 28.2, 29.6)  # inspection table
    L.block("ground", 59.45, 61.55, 1.1, 1.16, 28.35, 29.45)
    forklift(L, 50.0, 9.5, 0.4)
    # Bagged potatoes on pallets by the west doors.
    pallet_stack(L, 27.5, 28.6, 0, 4)
    pallet_stack(L, 29.0, 28.6, 0, 3)
    pallet_stack(L, 27.5, 29.6, 0, 2)
    # The foreman's office in the north-west corner: a plasterboard cabin with a desk and a file cabinet.
    L.room("plaster_white", 24.6, 31.4, 5.5, 9.6, 0.0, 2.5, 0.15, south=[door(26.4, 27.6, 2.1)], east=[window(6.4, 7.9)])
    L.block("metal", 24.55, 31.45, 2.5, 2.65, 5.45, 9.65)
    L.block("wood", 27.4, 29.8, 0.0, 0.75, 5.75, 6.55)
    L.block("metal_white", 30.4, 31.15, 0.0, 1.35, 5.75, 6.4)
    L.model("props/plastic_chair", 28.6, 7.2, 180.0)
    shelf(L, 24.8, 25.6, 6.0, 9.2, levels=(0.5, 1.2, 1.9))
    L.light(28.0, 2.3, 7.5, intensity=90000.0, reach=5.0)
    # A rack of boxes on the south wall and one on the west wall, a bulk potato heap behind boards in the
    # south-east corner (a mantle up from the boards, as the onion heap).
    shelf(L, 31.0, 41.0, 29.55, 30.55)
    shelf(L, 24.45, 25.35, 15.0, 21.5)
    L.block("wood", 64.2, 64.5, 0.0, 1.0, 21.2, 30.55)
    L.block("wood", 64.5, 67.55, 0.0, 1.0, 21.2, 21.5)
    L.block("ground", 64.5, 67.55, 0.0, 1.0, 21.5, 30.55)
    L.block("ground", 65.5, 67.55, 1.0, 1.9, 23.5, 30.55)
    kisten(L, 38.0, 5.8, 4, 1, 1, "ground")  # a row of single boxes: the potatoes in them show at eye level
    scatter(
        L,
        ("generator", 49.5, 29.9, 0.0), ("jerrycan", 50.6, 29.9, 20.0), ("trash_can", 65.8, 12.2, 0.0),
        ("cardboard_box", 63.2, 29.6, 10.0), ("cardboard_box", 63.7, 28.9, -15.0), ("trash_can", 42.4, 29.8, 0.0),
        ("jerrycan", 66.8, 10.8, 40.0), ("barrel_blue", 66.7, 9.0, 0.0), ("barrel_red", 66.6, 7.8, 0.0),
    )
    L.on_wall("props/power_box", "z", 30.6, 56.0, -1, y=1.2)
    # Ventilation duct along the north wall, under the eaves; the fans on the west wall.
    L.block("metal", x0 + T, x1 - T, 5.2, 5.9, z0 + T + 0.1, z0 + T + 0.9)
    for z in (7.4, 19.0):
        L.block("metal", x0 + T, x0 + T + 0.4, 2.6, 4.6, z - 1.0, z + 1.0)
    for x, z in ((32.0, 12.0), (46.0, 18.0), (60.0, 12.0), (32.0, 26.0), (52.0, 25.0)):
        L.light(x, 5.6, z, intensity=260000.0, reach=12.0)


# ---- the onion store -------------------------------------------------------------------------


def onion_store(L: Level) -> None:
    """The south hall, x 12..87, z 35..61: a bulk onion heap in its west end behind board walls
    (the east board wall is mantle high: the heap is a climb), blocks of onion boxes, a tractor
    with a loaded trailer by the east door, pallets of onion bags."""
    x0, x1, z0, z1, h = 12.0, 87.0, 35.0, 61.0, 7.0
    hall(
        L, x0, x1, z0, z1, h, 2.6,
        north=[big_door(30.0, 3.5, 4.2), door(50.0, 51.2, 2.3), big_door(79.0, 5.0, 5.0)],
        west=[big_door(40.0, 4.5, 4.8)],
        east=[big_door(48.0, 5.0, 5.0)],
    )
    drum(L, "z", z0, 30.0, 3.5, 4.2, -1)
    drum(L, "z", z0, 79.0, 5.0, 5.0, -1)
    drum(L, "x", x0, 40.0, 4.5, 4.8, -1)
    drum(L, "x", x1, 48.0, 5.0, 5.0, 1)
    # The bulk heap in three tiers (each a mantle), behind boards: 2.4 m on the north (no way
    # over), 1.0 m on the east, level with the first tier (a mantle onto the heap).
    inner_w, inner_s = x0 + T, z1 - T
    L.block("wood", inner_w, 33.0, 0.0, 2.4, 43.2, 43.5)
    L.block("wood", 32.75, 33.0, 0.0, 1.0, 43.5, inner_s)
    for x in (16.0, 20.0, 24.0, 28.0):
        L.block("metal", x - 0.1, x + 0.1, 0.0, 2.6, 42.95, 43.2)  # posts in front of the boards
    L.block(ONION, inner_w, 32.75, 0.0, 1.0, 43.5, inner_s)
    L.block(ONION, 15.0, 30.5, 1.0, 2.0, 46.0, 58.5)
    L.block(ONION, 18.0, 27.0, 2.0, 2.9, 49.0, 55.5)
    # The stacker that fills the heap: a belt on a frame, slanting up from the board wall.
    L.slab_ramp("asphalt", (37.0, 1.2, 44.5), (27.0, 4.2, 50.0), 0.9, 0.2)
    L.block("plaster_yellow", 36.4, 37.8, 0.0, 1.2, 44.0, 45.4)
    # Onion boxes.
    kisten(L, 36.0, 38.0, 4, 4, 3, ONION)
    kisten(L, 36.0, 50.0, 4, 6, 4, ONION)
    kisten(L, 48.0, 41.5, 3, 6, 3, ONION)
    kisten(L, 58.0, 37.0, 5, 3, 4, ONION)
    kisten(L, 58.0, 52.0, 5, 5, 3, ONION)
    kisten(L, 72.0, 54.0, 3, 3, 2, ONION)
    # Tractor and a trailer of onions, nose to the east door.
    tractor(L, 77.0, 44.8, 0.0)
    trailer(L, 69.0, 44.8, 0.0, ONION)
    # Onion bags on pallets inside the west door.
    for x in (22.0, 23.5, 25.0):
        pallet_stack(L, x, 37.5, 0, 3)
    # Racks on the south wall, a second forklift, a row of single onion boxes, clutter.
    shelf(L, 46.0, 56.0, 59.45, 60.5)
    shelf(L, 69.0, 80.0, 59.45, 60.5)
    forklift(L, 54.5, 39.5, math.pi)
    kisten(L, 82.0, 41.0, 2, 1, 1, ONION)
    scatter(
        L,
        ("generator", 84.2, 37.3, 0.0), ("jerrycan", 85.4, 37.3, 10.0), ("trash_can", 85.6, 60.0, 0.0),
        ("cardboard_box", 69.5, 37.5, 20.0), ("cardboard_box", 70.1, 38.2, -10.0), ("trash_can", 45.2, 37.4, 0.0),
        ("barrel_rusty", 13.2, 36.4, 0.0), ("barrel_blue", 13.9, 36.5, 0.0),
    )
    L.on_wall("props/power_box", "x", x1 - T, 56.0, -1, y=1.2)
    # Fans on the east wall.
    for z in (40.0, 56.0):
        L.block("metal", x1 - T - 0.4, x1 - T, 2.6, 4.6, z - 1.0, z + 1.0)
    for x, z in ((24.0, 40.0), (44.0, 47.0), (56.0, 40.0), (66.0, 50.0), (80.0, 40.0), (80.0, 56.0)):
        L.light(x, 6.0, z, intensity=260000.0, reach=12.0)


# ---- workshop, barn, farmhouse ----------------------------------------------------------------


def workshop(L: Level) -> None:
    """The smaller shed on the yard, x 82..96, z 3..18: a workbench, tool cabinet, barrels, tyres
    and an old tractor. A roller door each side."""
    x0, x1, z0, z1 = 82.0, 96.0, 3.0, 18.0
    hall(L, x0, x1, z0, z1, 4.8, 1.6, roof_south=ROOF_NORTH, west=[big_door(11.0, 3.6, 3.6)], east=[big_door(10.0, 3.2, 3.4)])
    drum(L, "x", x0, 11.0, 3.6, 3.6, -1)
    drum(L, "x", x1, 10.0, 3.2, 3.4, 1)
    L.block("wood", 84.0, 90.0, 0.0, 0.9, z0 + T, z0 + T + 0.8)  # workbench
    L.block("metal", 84.0, 90.0, 0.9, 0.96, z0 + T, z0 + T + 0.85)
    L.block("metal_rust", 91.8, 93.2, 0.0, 1.8, z0 + T, z0 + T + 0.6)  # tool cabinet
    barrels(L, 94.2, 5.0, 3, 1)
    tyres(L, 84.2, 16.6, 0, 3)
    tractor(L, 89.5, 12.5, math.pi, "corrugated_rust")
    L.block("metal_white", 94.0, 95.2, 0.0, 1.2, 16.0, 17.2)  # a water tank (IBC)
    # A second bench with a vice on the west wall, a rack of parts, a coffee corner, the compressor.
    L.block("wood", 82.5, 83.3, 0.0, 0.9, 4.6, 8.0)
    L.block("metal", 82.5, 83.3, 0.9, 0.96, 4.6, 8.0)
    L.block("metal_rust", 82.55, 82.85, 0.96, 1.1, 5.4, 5.7)
    shelf(L, 85.5, 91.5, 16.6, 17.55)
    L.block("wood", 84.0, 85.0, 0.0, 0.75, 13.2, 14.2)  # the coffee table
    L.model("props/plastic_chair", 83.4, 13.7, -90.0)
    L.model("props/plastic_chair", 85.7, 13.7, 90.0)
    kisten(L, 86.0, 6.2, 2, 1, 2, "ground")  # seed potatoes waiting for the planter
    scatter(
        L,
        ("generator", 92.6, 6.4, 0.0), ("jerrycan", 93.6, 6.5, 30.0), ("jerrycan", 93.8, 7.0, -20.0),
        ("trash_can", 83.2, 16.9, 0.0), ("ladder", 92.9, 16.9, 0.0), ("propane_tank", 94.8, 12.6, 0.0),
        ("cardboard_box", 90.5, 4.9, 15.0),
    )
    L.light(89.0, 4.4, 10.0, intensity=220000.0, reach=10.0)


def barn(L: Level) -> None:
    """The small barn east of the workshop, x 101..112, z 4..18: a covered car, shelves of crates,
    empty boxes. Doors to the yard (west) and to the road side (east)."""
    x0, x1, z0, z1 = 101.0, 112.0, 4.0, 18.0
    hall(L, x0, x1, z0, z1, 4.2, 1.3, roof_south=ROOF_NORTH, west=[big_door(12.5, 3.4, 3.2)], east=[big_door(15.0, 3.0, 3.0)])
    drum(L, "x", x0, 12.5, 3.4, 3.2, -1)
    L.car(106.0, 8.5, 0.05, "covered")
    for y in (0.5, 1.2, 1.9):  # shelves along the east wall
        L.block("metal", x1 - T - 0.6, x1 - T, y, y + 0.05, 10.0, 13.6)
    L.block("metal", x1 - T - 0.58, x1 - T - 0.48, 0.0, 2.0, 10.02, 10.12)
    L.block("metal", x1 - T - 0.58, x1 - T - 0.48, 0.0, 2.0, 13.48, 13.58)
    kisten(L, 102.0, 14.4, 2, 1, 3, None)
    bales(L, 102.0, 4.6, 4, False, 2)  # straw along the north wall
    kisten(L, 108.6, 5.0, 1, 3, 3, "ground", along_z=True)  # a row of potato boxes
    L.block("wood", 101.5, 103.5, 0.0, 0.9, 16.6, 17.4)  # a workbench by the south wall
    L.block("metal", 101.5, 103.5, 0.9, 0.96, 16.6, 17.4)
    scatter(
        L,
        ("cardboard_box", 104.5, 17.0, 10.0), ("cardboard_box", 105.1, 17.0, -10.0), ("jerrycan", 109.8, 16.8, 20.0),
        ("barrel_rusty", 110.5, 11.5, 0.0), ("trash_can", 110.7, 17.0, 0.0), ("ladder", 102.2, 10.0, 90.0),
    )
    L.light(106.5, 3.8, 11.0, intensity=200000.0, reach=10.0)


def farmhouse(L: Level) -> None:
    """The farmhouse (the red pin), x 98.5..109, z 29..38.5: brick walls, a tiled roof, windows
    on every side (all vaultable), a front door on the yard and a back door to the garden. The
    west half is the living room, the east half kitchen and bedroom."""
    x0, x1, z0, z1, h, t = 98.5, 109.0, 29.0, 38.5, 3.0, 0.3
    hall(
        L, x0, x1, z0, z1, h, 3.0, wall="brick", roof_south="roof_tiles", roof_north="roof_tiles", t=t, overhang=0.4,
        west=[window(30.1, 31.5), door(33.05, 34.15, 2.2), window(35.9, 37.3)],
        south=[window(100.7, 102.3), window(103.9, 105.1), door(106.85, 107.95, 2.2)],
        north=[window(101.8, 103.2), window(106.3, 107.3)],
        east=[window(30.9, 32.1), window(35.1, 36.5)],
    )
    L.wall("plaster_white", "x", 104.0, z0 + t, z1 - t, 0.0, h, 0.15, [door(33.3, 34.3, 2.1)])
    L.block("brick", 106.15, 106.85, h + 1.8, h + 3.4, 31.15, 31.85)  # chimney, standing on the roof
    # Living room: sofa, coffee table, dining table and chairs, a cabinet.
    L.block("wood_painted", 99.5, 101.7, 0.0, 0.45, 36.4, 37.3)
    L.block("wood_painted", 99.5, 101.7, 0.45, 0.95, 37.3, 37.5)
    L.block("wood", 100.25, 101.35, 0.0, 0.4, 35.0, 35.6)
    L.block("wood", 100.7, 102.3, 0.72, 0.77, 31.05, 31.95)
    for lx in (100.75, 102.19):
        for lz in (31.1, 31.84):
            L.block("wood", lx, lx + 0.06, 0.0, 0.72, lz, lz + 0.06)
    for cx, cz in ((100.9, 30.5), (102.1, 30.5), (100.9, 32.5), (102.1, 32.5)):
        L.block("wood_painted", cx - 0.22, cx + 0.22, 0.0, 0.45, cz - 0.22, cz + 0.22)
    L.block("wood", 103.2, 103.8, 0.0, 1.9, 35.7, 37.5)
    # More living room: a bookshelf, an armchair, a rug, a sideboard.
    L.block("wood", 98.8, 99.3, 0.0, 1.9, 31.6, 32.95)
    L.block("wood_painted", 98.9, 99.8, 0.0, 0.45, 34.6, 35.5)
    L.block("wood_painted", 98.9, 99.15, 0.45, 0.95, 34.6, 35.5)
    L.block("plaster_green", 99.9, 102.5, 0.0, 0.03, 34.3, 36.2)
    L.block("wood", 99.2, 100.8, 0.0, 0.9, 29.35, 29.9)
    # Kitchen (north-east): counter with hob and sink, wall cabinets, an island, the fridge.
    L.block("tiles", 104.5, 108.7, 0.0, 0.9, 29.3, 29.95)
    L.block("metal", 105.3, 106.1, 0.9, 0.96, 29.45, 29.85)
    L.block("metal_white", 107.3, 108.1, 0.9, 0.96, 29.45, 29.85)
    L.block("wood", 104.5, 106.2, 1.5, 2.1, 29.3, 29.65)
    L.block("wood", 107.4, 108.7, 1.5, 2.1, 29.3, 29.65)
    L.block("wood", 105.6, 107.6, 0.0, 0.9, 31.5, 32.5)
    L.block("tiles", 105.5, 107.7, 0.9, 0.95, 31.4, 32.6)
    L.block("metal_white", 107.85, 108.65, 0.0, 1.85, 32.7, 33.6)
    L.model("props/plastic_chair", 104.8, 32.0, -90.0)
    L.model("props/trash_can", 104.8, 30.7, 0.0)
    # Bedroom (south-east): a bed with headboard and pillows, a nightstand, a wardrobe on the east wall
    # (clear of the windows and the back door).
    L.block("wood", 104.4, 106.4, 0.0, 0.3, 35.3, 37.3)
    L.block("wood", 104.42, 104.55, 0.3, 1.0, 35.3, 37.3)
    L.block("plaster_white", 104.6, 106.3, 0.3, 0.55, 35.4, 37.2)
    L.block("plaster_blue", 104.65, 105.15, 0.55, 0.65, 35.5, 36.0)
    L.block("plaster_blue", 104.65, 105.15, 0.55, 0.65, 36.6, 37.1)
    L.block("wood", 104.4, 104.9, 0.0, 0.55, 34.6, 35.15)
    L.block("wood", 108.05, 108.65, 0.0, 2.0, 36.6, 38.1)
    L.light(101.5, 2.7, 33.5, intensity=150000.0, reach=7.0)
    L.light(106.5, 2.7, 33.5, intensity=150000.0, reach=7.0)


def small_shed(L: Level) -> None:
    """The little brick outbuilding on the west lane, x 7..11, z 10.5..14.5 (not enterable)."""
    L.block("brick", 7.0, 11.0, 0.0, 2.8, 10.5, 14.5)
    L.block("roof_slate", 6.8, 11.2, 2.8, 3.05, 10.3, 14.7)


# ---- garden, trees -----------------------------------------------------------------------------


def garden(L: Level) -> None:
    """The lawn round the farmhouse: trees and bushes (cover for Bravo's spawn), a picnic table,
    a vaultable 1 m hedge along the yard with a gap by the house, a garden bench."""
    for x, z in ((113.0, 26.0), (118.5, 33.0), (119.5, 42.0), (101.0, 50.0), (112.0, 60.0), (119.0, 60.0), (103.0, 59.0), (118.5, 21.5)):
        tree(L, x, z, big=x > 110.0)
    for x, z, r in ((114.0, 38.5, 1.1), (108.5, 41.5, 0.9), (99.0, 41.5, 0.8), (110.0, 49.5, 1.0), (118.5, 51.5, 0.9), (106.5, 63.5, 1.0)):
        bush(L, x, z, r)
    # The low hedge between yard and garden (0.6 m thick, 1.0 m: a vault), open by the house.
    L.block("grass", 96.7, 97.3, 0.0, 1.0, 20.0, 27.5)
    L.block("grass", 96.7, 97.3, 0.0, 1.0, 41.0, 59.0)
    L.model("props/picnic_table", 112.0, 45.5, 0.0)
    L.model("props/bench", 101.5, 39.6, 0.0)
    # Greenery round the house: more trees and bushes, planters at the door, rows of potatoes on the east side.
    for x, z in ((104.0, 23.5), (121.0, 27.0), (121.0, 48.0), (96.5, 63.0), (98.5, 25.0)):
        tree(L, x, z, big=x > 110.0)
    for x, z, r in ((98.2, 28.2, 0.7), (110.1, 31.0, 0.7), (110.1, 37.5, 0.8), (97.8, 39.8, 0.7), (101.5, 40.8, 0.6), (121.0, 38.0, 0.9)):
        bush(L, x, z, r)
    L.block("grass", 109.6, 123.0, 0.0, 1.0, 39.8, 40.5)  # a low hedge, a vault, in line with the east side
    scatter(L, ("planter", 106.4, 39.7, 0.0), ("planter", 108.4, 39.7, 0.0), ("planter", 99.6, 28.1, 0.0))
    potato_rows(L, 111.5, 121.0, 28.6, 3, 1.2)


def west_trees(L: Level) -> None:
    """A row of trees along the west hedge (the screenshot's tree belt), trunks 1.5 m in so the
    lane stays open."""
    for z in (22.0, 28.5, 35.0, 41.5, 48.0, 54.5, 61.0):
        tree(L, 1.5, z)
    tree(L, 3.0, 4.0)


# ---- cover in the open --------------------------------------------------------------------------


def yard(L: Level) -> None:
    """Machinery, cars, boxes, bales and crates in the yard, lanes and alley."""
    # The yard between the stores and the farmhouse.
    harvester(L, 76.0, 21.5, 0.08)
    L.car(92.0, 25.0, -0.5)
    L.van(93.0, 41.5, math.pi / 2 - 0.1, "metal_white")
    kisten(L, 83.0, 29.5, 1, 2, 3, None, along_z=True)
    kisten(L, 70.0, 31.4, 2, 1, 2)
    bales(L, 86.0, 19.5, 3, layers=2)
    pallet_stack(L, 80.0, 25.5, 1, 2)
    crates(L, 88.0, 33.0, 1)
    # The road side of the barn: the second tractor with a trailer of potatoes.
    tractor(L, 116.5, 8.0, -math.pi / 2)
    trailer(L, 116.5, 15.5, -math.pi / 2, "ground")
    # The north strip in front of the potato store.
    L.car(60.0, 2.5, math.pi, "metal")
    bales(L, 42.0, 0.6, 3)
    crates(L, 70.0, 1.2, 0)
    barrels(L, 97.0, 1.0, 3, 0)
    crates(L, 99.0, 6.2, 0, ("props/wooden_crate",), 3)
    # The west lane: empty boxes, a car, an old trailer.
    kisten(L, 13.5, 1.5, 2, 2, 3, None)
    kisten(L, 13.5, 7.0, 2, 2, 2, None)
    L.car(19.0, 30.0, 0.3, "covered")
    kisten(L, 14.5, 32.0, 2, 1, 2, None)
    trailer(L, 6.5, 50.0, math.pi / 2, None)
    bales(L, 4.0, 40.0, 2, along_z=True)
    # The alley between the stores: pallets and crates at intervals.
    for x in (30.0, 45.0, 58.0):
        L.crate(x, 33.0, 1.2)
        pallet_stack(L, x + 1.6, 33.2, 0, 2)
    crates(L, 66.0, 33.0, 0)
    tyres(L, 40.0, 32.0, 0, 2)
    # Potato clamps on the north strip, onion lorry and more potato boxes in the yard.
    clamp(L, 72.0, 79.5, 0.8, 4.4)
    clamp(L, 85.0, 92.5, 0.8, 4.4)
    truck(L, 92.0, 56.0, -math.pi / 2, ONION)
    kisten(L, 118.0, 3.5, 2, 1, 3, "ground")
    scatter(L, ("barrel_rusty", 70.0, 19.8, 0.0), ("jerrycan", 71.2, 19.8, 0.0), ("trash_can", 95.5, 35.2, 0.0))
    # The potato field along the south edge.
    potato_rows(L, 4.0, 96.0, 62.4, 5, 0.95)


# ---- entities --------------------------------------------------------------------------------

# All in plan metres.
CENTRE = (61.0, 33.0)
ALPHA = [(4.5, 21.0), (8.0, 23.5), (4.5, 27.0), (8.0, 30.0), (13.0, 21.0), (13.0, 26.0)]
BRAVO = [(106.0, 44.0), (110.0, 47.0), (114.0, 44.0), (106.0, 53.0), (112.0, 55.0), (117.0, 47.5)]
FFA = [(30.0, 2.5), (82.2, 2.5), (18.0, 12.0), (78.0, 28.0), (52.0, 33.0), (90.0, 50.0), (60.0, 64.0), (110.0, 22.0)]
FLAGS = {"A": (46.0, 14.5), "B": (82.0, 27.0), "C": (45.0, 46.0)}
HQS = {"potato store west": (31.0, 26.5), "onion store east": (82.0, 56.0), "workshop": (86.0, 8.0)}
BOMBS = {"A": (52.0, 23.5), "B": (70.0, 50.0)}
AMMO = [(76.0, 29.0), (33.0, 10.0), (45.0, 56.5), (70.0, 3.0), (52.0, 33.0), (93.0, 30.0)]


def entities(L: Level) -> None:
    towards = g(*CENTRE)
    for x, z in ALPHA:
        L.spawn("alpha", "tdm dom hq sd", x, z, towards)
    for x, z in BRAVO:
        L.spawn("bravo", "tdm dom hq sd", x, z, towards)
    for x, z in FFA:
        L.spawn("any", "ffa", x, z, towards)
    for label, (x, z) in FLAGS.items():
        L.flag(label, x, z)
    for x, z in HQS.values():
        L.hq(x, z)
    for label, (x, z) in BOMBS.items():
        L.bomb(label, x, z)
    for x, z in AMMO:
        L.ammo_crate(x, z)


def build() -> Level:
    L = Level("Winkelweg 21", "overcast_urban")
    with L.shifted(OX, OZ):
        ground(L)
        perimeter(L)
        potato_store(L)
        onion_store(L)
        workshop(L)
        barn(L)
        farmhouse(L)
        small_shed(L)
        garden(L)
        west_trees(L)
        yard(L)
        entities(L)
    return L


# The trick-jump routes (game metres, like the checker): each step at most the mantle height.
# The onion heap is only reached by mantling, so it is checked here, not as a walking goal.
CLIMBS = {
    "onion heap: board wall, tiers": [g(34.5, 52.0), g(32.85, 52.0), g(31.5, 52.0), g(29.0, 52.0), g(26.0, 52.0)],
}


if __name__ == "__main__":
    level = build()
    level.write(OUT)
    if "--check" in sys.argv:
        from kit import check_walkability, climb_problems, fence_conflicts, placement_problems

        starts = [g(*ALPHA[3]), g(*BRAVO[0])]
        goals = {
            **{f"flag {k}": g(*v) for k, v in FLAGS.items()},
            **{f"HQ {k}": g(*v) for k, v in HQS.items()},
            **{f"bomb {k}": g(*v) for k, v in BOMBS.items()},
            "farmhouse living room": g(101.5, 34.5),
            "farmhouse bedroom": g(107.0, 33.0),
            "barn": g(103.0, 12.0),
            "north strip": g(82.2, 2.5),
            "potato office": g(29.5, 8.2),
            "workshop coffee corner": g(84.6, 15.5),
            "farmhouse kitchen": g(106.5, 33.5),
            "farmhouse bedroom door": g(107.4, 37.2),
            "potato field": g(50.0, 64.8),
        }
        walks = check_walkability(
            level, (X0 + OX - 1, X1 + OX + 1, Z0 + OZ - 1, Z1 + OZ + 1), starts, goals,
            image=Path("/tmp/winkelweg_nav.ppm"), walk_range=(80.0, 140.0),
        )
        problems = placement_problems(level) + climb_problems(level, CLIMBS) + fence_conflicts(level)
        for line in problems:
            print("  PROBLEM: " + line)
        print("check: " + ("ok" if walks and not problems else "FAILED"))
        sys.exit(0 if walks and not problems else 1)
