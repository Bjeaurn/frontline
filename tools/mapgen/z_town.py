"""Generates crates/frontline/assets/maps/z_town.map: Z-Town, an original Remix Map in
the centre of a Dutch polder town (docs/z-town-spec.md, docs/z-town/).

The layout follows the user's drawing on a Google Maps screenshot (docs/z-town/), at the
scale they measured (12 px per metre in the 1568-px view). The town's grid is turned about 15.6
degrees against the screenshot; the Map is turned back so every street and wall is axis-aligned
(x east, z south, -z "north" of the Map; true north is about 16 degrees off). The origin is the
church's west front.

    Kerkplein (the market square, with stalls and a van) .... x -31..-5, z -33..-3
    Kerkstraat (shopping street, west) .................... x -37..-31, z -33..54
    FlevoMeer library (west of Kerkstraat) ................ x -66..-42, z 3..26
    Open Haven church (west front with covered walkways) .. x -6..24, z -5..32
    Paths east of the church ................................ x 24..37 (a second path with bushes)
    Open Hof car park and road, the east shops ............ x 37..58
    Raadhuisstraat (north) and the north lane ............. z -41..-30 and z -11..-5

Alpha starts in the south-west (the south street at Kerkstraat's end), Bravo in the east (the car
park), the church in between. Phase 1: a blockout: footprints, routes, enterable buildings,
objectives and cover. The library's arcade and glass box, the church's tower and the roof
climbs come next.

    python3 tools/mapgen/z_town.py
    python3 tools/mapgen/z_town.py --check      # also flood-fill the walkable floor
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

from dressing import aircon, barrels, bins, crates, pallet_stack, sandbags, shutter
from kit import MAPS_DIR, Level, door, window

OUT = MAPS_DIR / "z_town.map"

STOREY = 3.6  # floor to floor
SLAB = 0.3
WT = STOREY - SLAB  # top of a ground-floor wall
UP = 2 * STOREY - SLAB  # top of an upper-floor wall
T = 0.4  # wall thickness
CHURCH_WALL = 7.0  # the church halls are double height
CHURCH_ROOF = CHURCH_WALL + SLAB

# The playfield, and the perimeter wall standing outside it.
X0, X1 = -66.0, 58.0
Z0, Z1 = -44.0, 54.0
PERIMETER = 14.0


# ---- helpers ------------------------------------------------------------------------------


# Facade (wall, band) surfaces of the massing blocks: brick old and new, some weathered plaster, so a
# street is not one red wall. A block's pick follows its west edge, so a regenerated Map is stable and a building's floors match.
FACADES = [("brick", "plaster"), ("brick_old", "concrete"), ("plaster_white", "concrete"), ("brick", "plaster"), ("stone", "concrete"), ("brick_old", "plaster"), ("plaster_yellow", "concrete")]


def terrace(L: Level, x0: float, x1: float, z0: float, z1: float, y0: float, y1: float, brick: bool = False) -> None:
    """A solid massing block, a facade surface with a band of another at each storey line (a stack of
    blocks with equal footprints, so no two faces overlap). `brick` forces brick walls (the church,
    whose roofs are built on top). Any other block taller than a storey ends in a flat gravel roof."""
    wall, band = ("brick", "plaster") if brick else FACADES[int(abs(x0) * 1.7) % len(FACADES)]
    roof = 0.0 if brick or y1 <= STOREY + 0.5 else 0.12
    top = y1 - roof
    y = y0
    while y < top - 1e-9:
        wall_top = min(y + WT, top)
        L.block(wall, x0, x1, y, wall_top, z0, z1)
        y = wall_top
        if y < top - 1e-9:
            band_top = min(y + SLAB, top)
            L.block(band, x0, x1, y, band_top, z0, z1)
            y = band_top
    if roof:
        L.block("gravel", x0, x1, top, y1, z0, z1)


def slits(a0: float, a1: float, count: int, width: float = 0.5, low: float = 1.3, high: float = 1.8):
    """Loophole gaps (a body can't pass, shots can) spread evenly between a0 and a1."""
    step = (a1 - a0) / count
    return [(a0 + step * (i + 0.5) - width / 2, a0 + step * (i + 0.5) + width / 2, low, high) for i in range(count)]


def paint_floor(L: Level, regions, default: str = "concrete") -> None:
    """The ground as non-overlapping tiles: `regions` (surface, x0, x1, z0, z1) painted over a
    default, later ones on top, cut on every region edge and merged into runs."""
    xs = sorted({X0, X1, *(v for r in regions for v in (r[1], r[2]) if X0 < v < X1)})
    zs = sorted({Z0, Z1, *(v for r in regions for v in (r[3], r[4]) if Z0 < v < Z1)})

    def surface_at(cx: float, cz: float) -> str:
        found = default
        for surface, a, b, c, d in regions:
            if a <= cx <= b and c <= cz <= d:
                found = surface
        return found

    rows = []
    for j in range(len(zs) - 1):
        cz = (zs[j] + zs[j + 1]) / 2
        runs: list[list] = []
        for i in range(len(xs) - 1):
            surface = surface_at((xs[i] + xs[i + 1]) / 2, cz)
            if runs and runs[-1][0] == surface:
                runs[-1][2] = xs[i + 1]
            else:
                runs.append([surface, xs[i], xs[i + 1]])
        rows.append([zs[j], zs[j + 1], runs])
    merged = []
    for row in rows:
        if merged and merged[-1][2] == row[2]:
            merged[-1][1] = row[1]
        else:
            merged.append(row)
    for za, zb, runs in merged:
        for surface, xa, xb in runs:
            L.floor(surface, xa, xb, za, zb, 0.0)


def stall(L: Level, x: float, z: float, yaw: float = 0.0) -> None:
    """A market stall, 3.0 x 2.0 m: back and side panels and a canopy, a counter across most of
    the front with a 1 m gap to walk in by. Its front looks along +z at yaw 0 (radians)."""
    c, n = math.cos(yaw), math.sin(yaw)

    def part(lx: float, y: float, lz: float, sx: float, sy: float, sz: float, surface: str) -> None:
        L.turned(surface, x + lx * c + lz * n, y, z - lx * n + lz * c, sx, sy, sz, yaw)

    part(0.0, 1.15, -0.95, 3.0, 2.3, 0.1, "wood")  # back panel
    part(-1.45, 1.15, -0.1, 0.1, 2.3, 1.6, "wood")  # side panels
    part(1.45, 1.15, -0.1, 0.1, 2.3, 1.6, "wood")
    part(-0.5, 0.45, 0.65, 1.8, 0.9, 0.3, "wood")  # counter
    part(0.0, 2.35, -0.1, 3.2, 0.1, 2.4, "metal")  # canopy


def pine(L: Level, x: float, z: float, height: float = 6.0) -> None:
    """A pine: a thin solid trunk under three tiers of grass-surface foliage above head height."""
    L.block("wood", x - 0.2, x + 0.2, 0.0, height, z - 0.2, z + 0.2)
    L.block("grass", x - 1.4, x + 1.4, 3.2, 4.6, z - 1.4, z + 1.4)
    L.block("grass", x - 1.0, x + 1.0, 4.6, 5.8, z - 1.0, z + 1.0)
    L.block("grass", x - 0.6, x + 0.6, 5.8, 6.8, z - 0.6, z + 0.6)


def tree(L: Level, x: float, z: float) -> None:
    """A street tree: a trunk and a two-tier grass-surface canopy above head height."""
    L.block("wood", x - 0.25, x + 0.25, 0.0, 3.2, z - 0.25, z + 0.25)
    L.block("grass", x - 1.8, x + 1.8, 3.0, 4.6, z - 1.8, z + 1.8)
    L.block("grass", x - 1.2, x + 1.2, 4.6, 5.8, z - 1.2, z + 1.2)


def hedge(L: Level, x0: float, x1: float, z0: float, z1: float, height: float = 1.3) -> None:
    """A hedge or bush row: waist-high cover (a grass-surface block)."""
    L.block("grass", x0, x1, 0.0, height, z0, z1)


def stair(L: Level, x0: float, x1: float, z0: float, z1: float) -> None:
    """A wooden stair ramp from the ground up to the first floor, rising along +z over (x0..x1,
    z0..z1), with rails on the first floor along the hole's open east side and across its low
    end. The slab above needs a hole of the same footprint."""
    L.ramp("wood", x0, x1, z0, z1, 0.0, STOREY, "+z")
    L.block("metal", x1, x1 + 0.2, STOREY, STOREY + 1.0, z0, z1)
    L.block("metal", x0, x1 + 0.2, STOREY, STOREY + 1.0, z0 - 0.3, z0)


def shop(L: Level, x0: float, x1: float, z0: float, z1: float, upper: float = 7.2, **sides) -> None:
    """A one-storey shop room (walls with the given door and window gaps per side: north, south,
    west, east) under a roof slab and a solid brick upper storey up to `upper`."""
    L.room("brick", x0, x1, z0, z1, 0.0, WT, T, **sides)
    L.block("concrete", x0, x1, WT, STOREY, z0, z1)
    if upper > STOREY:
        terrace(L, x0, x1, z0, z1, STOREY, upper)

def parapet(L: Level, x0: float, x1: float, z0: float, z1: float, y: float, h: float = 0.9, t: float = 0.25, north=(), south=(), west=(), east=()) -> None:
    """A low concrete wall (cover) round a flat roof's edge, standing on the roof at height `y`.
    Each side may have gaps ((a, b) along it) where a climb arrives."""

    def gaps(spans):
        return [(a, b, 0.0, h) for a, b in spans]

    L.wall("concrete", "z", z0 + t / 2, x0, x1, y, y + h, t, gaps(north))
    L.wall("concrete", "z", z1 - t / 2, x0, x1, y, y + h, t, gaps(south))
    L.wall("concrete", "x", x0 + t / 2, z0 + t, z1 - t, y, y + h, t, gaps(west))
    L.wall("concrete", "x", x1 - t / 2, z0 + t, z1 - t, y, y + h, t, gaps(east))



# ---- ground, perimeter ----------------------------------------------------------------------


def ground(L: Level) -> None:
    street = "paving_brick"  # clinker streets
    streets = [
        ("asphalt", 37.0, 46.0, -5.0, 38.0),  # the car park
        (street, -37.0, -31.0, -33.0, Z1),  # Kerkstraat
        (street, X0, -5.0, -3.0, 2.0),  # Marktstraat
        (street, -38.0, -31.0, -37.0, -33.0),  # the street round the north of the square
        (street, -5.0, 1.0, Z0, -3.0),  # Torenstraat and the street along the square's east side
        (street, -5.0, X1, -41.0, -30.0),  # Raadhuisstraat
        (street, -6.0, 46.0, -11.0, -5.0),  # the north lane
        (street, 24.0, 29.0, -5.0, 32.0),  # the path along the church's east side
        (street, 32.0, 37.0, -5.0, 38.0),  # the second path (with bushes)
        ("asphalt", 46.0, 49.0, Z0, Z1),  # Open Hof
        (street, -37.0, -4.0, 40.0, 50.0),  # the south street
        (street, -16.0, 46.0, 32.0, 38.0),  # the south lane
        (street, -16.0, -6.0, -3.0, 40.0),  # the lane in front of the church (west side)
    ]
    lawns = [
        ("grass", -31.0, -5.0, -33.0, -30.5),  # Kerkplein's frame of lawn
        ("grass", -31.0, -5.0, -5.5, -3.0),
        ("grass", -31.0, -28.5, -33.0, -3.0),
        ("grass", -7.5, -5.0, -33.0, -3.0),
        ("grass", 29.0, 32.0, -5.0, 32.0),  # the garden between the east paths
        ("grass", -5.0, X1, -41.0, -39.4),  # the tree strip along the north row
    ]
    paint_floor(L, streets + lawns, default="paving")


def perimeter(L: Level) -> None:
    """Tall brick walls closing the Map."""
    r = 3.0
    L.block("brick", X0 - r, X1 + r, 0.0, PERIMETER, Z0 - r, Z0)
    L.block("brick", X0 - r, X1 + r, 0.0, PERIMETER, Z1, Z1 + r)
    L.block("brick", X0 - r, X0, 0.0, PERIMETER, Z0, Z1)
    L.block("brick", X1, X1 + r, 0.0, PERIMETER, Z0, Z1)


# ---- Kerkplein, the market square -------------------------------------------------------------


def kerkplein(L: Level) -> None:
    """The market square (x -31..-5, z -33..-3): stalls in two rows and two columns round a clear
    middle (the Flag), a market van, pines, light poles and low planters."""
    for x, z, yaw in (
        (-26.0, -27.0, 0.0), (-20.0, -27.0, 0.0), (-14.0, -27.0, 0.0),
        (-26.0, -9.0, math.pi), (-20.0, -9.0, math.pi),
        (-9.0, -18.0, math.pi / 2),
    ):
        stall(L, x, z, yaw)
    L.van(-12.0, -11.0, 0.35, "corrugated")
    for x, z in ((-29.0, -31.0), (-7.0, -31.0), (-30.0, -5.0), (-10.0, -4.5), (-18.0, -31.5)):
        pine(L, x, z)
    for x, z in ((-30.5, -32.5), (-5.5, -32.5), (-30.5, -3.5), (-5.5, -3.5)):
        L.lamp(x, z, 7.0)
        L.light(x, 6.4, z, intensity=220000.0, reach=12.0, practical=True)
    for x in (-24.0, -14.0):
        L.prop("Prop_Planter_Single", x, -4.6, 0.0, 0.0, "box")


# ---- the library (west of Kerkstraat) ------------------------------------------------------------


def library(L: Level) -> None:
    """The FlevoMeer library, x -66..-42 (+ 5 m over the arcade to -37), z 3..26: a pale concrete
    box, 11 m tall at the back, with a 5 m deep arcade on Kerkstraat's side and the dark glass box
    on thin legs over it (the first floor reaches out to x -37); thin dark bands on the upper
    concrete (the window grid); loopholes ("boekenbajes") in the concrete first floor, big windows
    in the glass box; a roof terrace over the glass box (7.2 m, parapet) that is a trick-jump up from
    Marktstraat (two crates, three ledges: `CLIMBS`)."""
    x0, x1, z0, z1 = -66.0, -42.0, 3.0, 26.0
    xe = -37.0
    s = "concrete"
    L.room(
        s, x0, x1, z0, z1, 0.0, WT, T,
        north=[window(-60.0, -58.0), door(-50.0, -47.4)],
        south=[window(-60.0, -58.0), window(-52.0, -50.0)],
        east=[door(7.0, 9.4), door(16.0, 18.4), window(21.0, 23.0)],
    )
    # The arcade: its ceiling and thin dark columns.
    L.block(s, x1, xe, WT, STOREY, z0, z1)
    for z in (4.0, 10.0, 16.0, 22.0, 25.4):
        L.block("metal", xe - 0.4, xe - 0.1, 0.0, WT, z - 0.15, z + 0.15)
    # First floor: slab with the stair hole, concrete walls with loopholes, the glass box in metal.
    L.slab(s, x0, x1, z0, z1, STOREY, SLAB, holes=[(x0 + T, x0 + T + 1.6, 6.0, 13.0)])
    stair(L, x0 + T, x0 + T + 1.6, 6.0, 13.0)
    L.wall(s, "z", z0 + T / 2, x0, x1, STOREY, UP, T, slits(x0 + 2, x1 - 2, 6))
    L.wall(s, "z", z1 - T / 2, x0, x1, STOREY, UP, T, slits(x0 + 2, x1 - 2, 5))
    L.wall(s, "x", x0 + T / 2, z0 + T, z1 - T, STOREY, UP, T)
    glass = "metal"
    L.wall(glass, "z", z0 + T / 2, x1, xe, STOREY, UP, T, [window(-41.4, -39.6), window(-38.8, -37.6)])
    L.wall(glass, "z", z1 - T / 2, x1, xe, STOREY, UP, T, [window(-41.4, -39.6), window(-38.8, -37.6)])
    L.wall(glass, "x", xe - T / 2, z0 + T, z1 - T, STOREY, UP, T, [window(5.0, 6.6), window(9.0, 10.6), window(13.0, 14.6), window(17.0, 18.6), window(21.0, 22.6)])
    # Roofs: concrete over the box, dark metal over the glass box; the box rises to 11 m at the back.
    L.block(s, x0, x1, UP, STOREY * 2, z0, z1)
    L.block(glass, x1, xe, UP, STOREY * 2, z0, z1)
    L.block(s, x0, -48.0, STOREY * 2, 11.0, z0, z1)
    # The terrace (x -48..-37, 7.2 m): a parapet round it, a gap on the north where the climb arrives.
    parapet(L, -48.0, xe, z0, z1, STOREY * 2, h=1.0, t=0.3, north=[(-41.0, -39.0)])
    # The window grid: dark bands standing proud on the upper concrete (north, south, west).
    for y in (7.8, 9.0, 10.2):
        L.block("metal", -65.0, -49.0, y, y + 0.4, z0 - 0.05, z0)
        L.block("metal", -65.0, -49.0, y, y + 0.4, z1, z1 + 0.05)
    for i in range(7):
        z = z0 + 1.5 + i * 3.0
        L.block("metal", x0 - 0.05, x0, 4.2, 10.6, z, z + 0.4)
    # The hard way up (three ledges on the glass box's north face, over Marktstraat).
    L.crate(-38.6, 1.4, 1.2)
    L.crate(-38.6, 1.4, 1.0, y=1.2)
    L.block("metal", -41.0, -39.0, 3.2, 3.4, 2.0, 3.0)
    L.block("metal", -39.0, -37.0, 4.5, 4.7, 2.0, 3.0)
    L.block("metal", -41.0, -39.0, 5.8, 6.0, 2.0, 3.0)
    # Book stacks (cover) and a desk.
    for x in (-62.0, -57.0, -52.0):
        L.block("wood", x, x + 0.5, 0.0, 1.9, 9.0, 21.0)
    L.block("wood", -48.0, -45.0, 0.0, 0.9, 12.0, 13.0)
    L.block("wood", -62.0, -58.0, STOREY, STOREY + 0.8, 18.0, 20.0)
    L.block("wood", -55.0, -50.0, STOREY, STOREY + 1.9, 8.0, 8.5)
    L.block("wood", -55.0, -50.0, STOREY, STOREY + 1.9, 15.0, 15.5)
    for x, z, y in ((-57.0, 10.0, 2.8), (-52.0, 20.0, 2.8), (-57.0, 14.0, 6.4), (-50.0, 20.0, 6.4)):
        L.light(x, y, z, intensity=200000.0, reach=9.0)


# ---- the church: Open Haven -----------------------------------------------------------------------


def church(L: Level) -> None:
    """Open Haven, x -6..24, z -5..28. Three halls under double-height roofs: the entrance hall
    (x -6..8, z 1..11), the north hall (x 8..24, z -2..11) and the smaller south hall of the parish
    (x -6..20, z 14..28, with windows on three sides), a solid block (z 11..14) between them. A 2 m
    covered walk rides the north front and, as the canopy over the entrance, the west front
    (z -4..13); no door of the church opens into either one except the entrance. The glass front,
    the tower and the hard way up to the roof (phase 2) follow."""
    brick, h = "brick", CHURCH_WALL
    # Entrance hall: the glass front on the west (a 5 m door), a door to the north hall.
    L.room(brick, -6.0, 8.0, 1.0, 11.0, 0.0, h, T, west=[(3.0, 8.0, 0.0, h)], east=[door(4.0, 7.0, 3.2)])
    # North hall: a door from the entrance hall and one to the east path.
    L.room(brick, 8.0, 24.0, -2.0, 11.0, 0.0, h, T, west=[door(4.0, 7.0, 3.2)], east=[door(3.0, 6.0)])
    # South hall (the parish): two doors (south lane, east path) and windows on the front lane, the
    # south lane and the east path: extra ways in and out.
    L.room(
        brick, -6.0, 20.0, 14.0, 28.0, 0.0, h, T,
        south=[door(8.0, 11.0), window(-2.0, -0.4), window(4.0, 5.6)],
        west=[window(16.0, 17.6), window(20.5, 22.1), window(24.0, 25.6)],
        east=[door(19.0, 22.0), window(15.5, 17.1)],
    )
    # The corner block north of the entrance hall, and the solid block between the halls.
    terrace(L, -6.0, 8.0, -2.0, 1.0, 0.0, h, brick=True)
    terrace(L, -6.0, 24.0, 11.0, 14.0, 0.0, h, brick=True)
    terrace(L, 16.0, 24.0, 28.0, 31.0, 0.0, 7.2, brick=True)  # the sacristy annex, closing the hall's south-east corner
    for x0, x1, z0, z1 in ((-6.0, 24.0, -2.0, 14.0), (-6.0, 20.0, 14.0, 28.0)):
        L.block("concrete", x0, x1, h, CHURCH_ROOF, z0, z1)
    # Covered walks (a green metal canopy on columns): north (x -6..24, z -4..-2) and west
    # (x -8..-6, z -4..13), the latter over the entrance.
    L.block("metal", -6.0, 24.0, WT, STOREY, -4.0, -2.0)
    L.block("metal", -8.0, -6.0, WT, STOREY, -4.0, 13.0)
    for x in (-5.6, 0.0, 6.0, 12.0, 18.0, 23.6):
        L.block("metal", x - 0.2, x + 0.2, 0.0, WT, -4.0, -3.6)
    for z in (-3.6, 2.0, 8.0, 12.6):
        L.block("metal", -8.0, -7.6, 0.0, WT, z - 0.2, z + 0.2)
    # Pews.
    for z in (-0.5, 2.0):
        for x0, x1 in ((10.0, 13.0), (18.0, 21.0)):
            L.block("wood", x0, x1, 0.0, 0.9, z, z + 0.5)
    for z in (17.0, 19.0, 21.0, 25.0):
        for x0, x1 in ((-3.0, 3.0), (7.0, 13.0)):
            L.block("wood", x0, x1, 0.0, 0.9, z, z + 0.5)
    for x, z in ((1.0, 6.0), (16.0, 4.0), (9.0, 22.0), (-2.0, 22.0)):
        L.light(x, 5.5, z, intensity=260000.0, reach=11.0)
    glass_front(L)
    tower(L)
    church_roof_route(L)



def glass_front(L: Level) -> None:
    """The entrance hall's glass front, x -6, z 3..8: the opening above the 3.2 m door is a lintel and
    six green mullions (the `metal` surface) with open air between them, up to the roof; shots and
    sight pass, nobody reaches it (the sill is 3.6 m up)."""
    L.block("metal", -6.0, -5.6, 3.2, 3.6, 3.0, 8.0)
    for i in range(6):
        z = 3.0 + i * (5.0 - 0.15) / 5.0
        L.block("metal", -6.0, -5.6, 3.6, CHURCH_WALL, z, z + 0.15)


def tower(L: Level) -> None:
    """The open-work tower over the entrance hall, centred (1, 6): a stepped base on the roof (two
    steps of 1.4 and 1.3 m, so the roof and the deck are each a mantle apart), four corner posts with
    rails and slender uprights, a clock plate on each face near the top, and a boat-and-cross
    weather vane at about 36 m. Sight and shots pass between the bars; bodies do not. The deck (the
    second step's top, 10 m up) is the perch, reachable only by the trick-jump route to the roof.
    Every bar differs in thickness from the ones it crosses (flicker rule)."""
    cx, cz = 1.0, 6.0
    base = CHURCH_ROOF
    L.block("concrete", cx - 2.5, cx + 2.5, base, base + 1.4, cz - 2.5, cz + 2.5)
    L.block("plaster", cx - 1.8, cx + 1.8, base + 1.4, base + 2.7, cz - 1.8, cz + 1.8)
    y0, y1 = base + 2.7, 33.0
    half, post = 0.85, 0.3
    for sx in (-1, 1):
        for sz in (-1, 1):
            px, pz = cx + sx * half, cz + sz * half
            L.block("metal", px - post / 2, px + post / 2, y0, y1, pz - post / 2, pz + post / 2)
    y = y0 + 3.0
    while y < y1 - 0.2:
        for sz in (-1, 1):  # rails along x
            L.block("metal", cx - half, cx + half, y, y + 0.16, cz + sz * half - 0.09, cz + sz * half + 0.09)
        for sx in (-1, 1):  # rails along z
            L.block("metal", cx + sx * half - 0.07, cx + sx * half + 0.07, y + 0.5, y + 0.62, cz - half, cz + half)
        y += 3.0
    for sz in (-1, 1):  # uprights at the middle of each face
        L.block("metal", cx - 0.06, cx + 0.06, y0, y1, cz + sz * half - 0.05, cz + sz * half + 0.05)
    for sx in (-1, 1):
        L.block("metal", cx + sx * half - 0.04, cx + sx * half + 0.04, y0, y1, cz - 0.06, cz + 0.06)
    inner = half - post / 2
    for sz in (-1, 1):  # clock plates between the posts
        L.block("plaster", cx - inner, cx + inner, 28.5, 30.7, cz + sz * half - 0.1, cz + sz * half + 0.1)
    for sx in (-1, 1):
        L.block("plaster", cx + sx * half - 0.12, cx + sx * half + 0.12, 28.5, 30.7, cz - inner, cz + inner)
    L.block("concrete", cx - 1.4, cx + 1.4, y1, y1 + 0.4, cz - 1.4, cz + 1.4)
    L.block("metal", cx - 0.05, cx + 0.05, y1 + 0.4, 34.8, cz - 0.05, cz + 0.05)
    L.block("wood", cx - 0.8, cx + 0.8, 34.8, 35.15, cz - 0.12, cz + 0.12)
    L.block("metal", cx - 0.03, cx + 0.03, 35.15, 36.1, cz - 0.03, cz + 0.03)
    L.block("metal", cx - 0.25, cx + 0.25, 35.65, 35.7, cz - 0.02, cz + 0.02)


def church_roof_route(L: Level) -> None:
    """The hard way up to the church roof and the tower deck: a two-crate stack in the front lane
    against the covered walk's edge, the walk's roof (3.6 m), two ledges on the glass front (5.0 and
    6.3 m), the roof (7.3 m), the tower's two steps (8.7 and 10.0 m). Every step is within the 1.6 m
    mantle (`CLIMBS`, checked by `--check`). The roof has no cover."""
    L.crate(-8.6, 10.8, 1.2)
    L.crate(-8.6, 10.8, 1.0, y=1.2)
    L.block("metal", -7.0, -6.0, 4.8, 5.0, 6.0, 9.0)
    L.block("metal", -7.0, -6.0, 6.1, 6.3, 3.0, 6.0)
    # Tall poles in front of the church (the cable net's, in the real square), with a wire between.
    for z in (0.5, 12.5):
        L.block("metal", -12.62, -12.38, 0.0, 9.0, z - 0.12, z + 0.12)
    L.block("metal", -12.53, -12.47, 8.8, 8.86, 0.5, 12.5)

# ---- the shops, west and south ----------------------------------------------------------------------


def west_blocks(L: Level) -> None:
    # Cafe De Mens, x -31..-16, z 2..13: enterable, on Marktstraat and Kerkstraat.
    shop(L, -31.0, -16.0, 2.0, 13.0, upper=STOREY, north=[door(-25.0, -22.6), window(-20.0, -18.0)], west=[door(5.0, 7.4)], east=[door(7.0, 9.4), window(3.5, 5.5)])
    L.block("wood", -29.0, -27.0, 0.0, 0.9, 4.0, 11.0)  # the bar
    L.light(-24.0, 2.8, 8.0, intensity=200000.0, reach=9.0)
    # Its flat roof (3.6 m) is a trick-jump from Kerkstraat: a two-crate stack at the wall, a parapet
    # round the roof with a gap where the stack arrives; the roof looks over Marktstraat and Kerkplein.
    parapet(L, -31.0, -16.0, 2.0, 13.0, STOREY, west=[(8.4, 10.4)])
    L.crate(-31.7, 9.4, 1.2)
    L.crate(-31.7, 9.4, 1.0, y=1.2)
    # Block A and B along Kerkstraat's east side, with an alley between them.
    terrace(L, -31.0, -16.0, 13.0, 24.0, 0.0, 7.2)
    terrace(L, -31.0, -16.0, 27.0, 40.0, 0.0, 7.2)
    # The arch over the lane at the church's south-west corner: from block B to the church hall,
    # 6 m wide between two piers.
    L.block("concrete", -16.0, -6.0, WT, 6.8, 28.0, 32.0)
    L.block("brick", -16.0, -14.0, 0.0, WT, 28.0, 32.0)
    L.block("brick", -8.0, -6.0, 0.0, WT, 28.0, 32.0)
    # North and west of Kerkplein and Kerkstraat: solid terraces.
    terrace(L, X0, -38.0, -33.0, -5.0, 0.0, 7.2)
    terrace(L, X0, -37.0, 26.0, Z1 - 4.0, 0.0, 7.2)
    terrace(L, -38.0, -6.0, Z0, -37.0, 0.0, 7.2)
    terrace(L, -30.0, -10.0, 50.0, Z1, 0.0, 7.2)


# ---- the north: Raadhuisstraat and the shops along it ------------------------------------------------


def north_shop(L: Level, x0: float, x1: float, enterable: bool = False, columns=(), **sides) -> None:
    """One shop of the row on Raadhuisstraat's south side (z -30..-11): the ground floor set back 2 m
    behind an arcade (a green metal ceiling on columns), one storey of brick above it. Enterable ones
    have a shop room (z -28..-19) with a door and a window on the street and the given side doors."""
    z_front, z_back = -28.0, -19.0
    if enterable:
        L.room("brick", x0, x1, z_front, z_back, 0.0, WT, T, north=[door(x0 + 3.0, x0 + 5.4), window(x0 + 7.0, x0 + 9.5)], **sides)
        terrace(L, x0, x1, z_back, -11.0, 0.0, WT)
        L.block("wood", x0 + 1.0, x1 - 1.0, 0.0, 1.0, z_back - 1.6, z_back - 1.0)  # the counter
        L.block("wood", x0 + 0.3, x0 + 0.9, 0.0, 2.0, z_front + 1.5, z_front + 5.5)  # shelves
        L.block("wood", x1 - 0.9, x1 - 0.3, 0.0, 2.0, z_front + 1.5, z_front + 5.5)
        L.light((x0 + x1) / 2, 2.9, (z_front + z_back) / 2, intensity=120000.0, reach=9.0)
    else:
        terrace(L, x0, x1, z_front, -11.0, 0.0, WT)
    L.block("metal", x0, x1, WT, STOREY, -30.0, z_front)  # the awning
    L.block("concrete", x0, x1, WT, STOREY, z_front, -11.0)
    terrace(L, x0, x1, -30.0, -11.0, STOREY, 7.2)
    for x in columns:
        L.block("metal", x - 0.2, x + 0.2, 0.0, WT, -29.8, -29.4)


def north_block(L: Level) -> None:
    # The north row on Raadhuisstraat.
    terrace(L, -5.0, X1, Z0, -41.0, 0.0, 7.2)
    # The shops between Raadhuisstraat and the north lane, with two alleys through; the two end
    # shops are enterable (street door, an alley door), the middle one is solid.
    north_shop(L, 1.0, 14.0, True, (1.4, 7.2, 11.0, 13.6), east=[door(-24.0, -21.6)])
    north_shop(L, 17.0, 31.0, False, (17.4, 21.0, 24.0, 27.0, 30.6))
    north_shop(L, 34.0, 46.0, True, (34.4, 40.2, 44.0, 45.6), west=[door(-24.0, -21.6)])
    # Street life: trees in the grass strip along the north row, planters, benches and a kiosk.
    for x in (4.0, 18.0, 36.0, 52.0):
        tree(L, x, -40.0)
    for x, z in ((15.5, -35.5), (32.5, -35.5), (47.5, -35.5)):
        L.prop("Prop_Planter_Single", x, z, 0.0, 0.0, "box")
    for x, z in ((21.0, -33.0), (40.0, -33.0)):
        L.block("wood", x - 0.9, x + 0.9, 0.0, 0.5, z - 0.25, z + 0.25)
    L.block("metal", 34.0, 36.4, 0.0, 2.6, -37.0, -35.0)  # the newspaper kiosk
    # The art pavilion (the "Insectenhotel") on the north lane: cover.
    L.block("wood", 12.0, 15.0, 0.0, 3.0, -9.5, -7.0)
    L.block("metal", 12.5, 14.5, 3.0, 3.2, -9.0, -7.5)


# ---- the south block -----------------------------------------------------------------------------------


def south_block(L: Level) -> None:
    # Closes the south street east of x -4: the south lane runs north of it.
    terrace(L, -4.0, 18.0, 38.0, Z1, 0.0, 7.2)
    terrace(L, 21.0, 36.0, 40.0, Z1, 0.0, 7.2)


# ---- the east: gardens, car park, Open Hof and the east shops ----------------------------------------


def east_side(L: Level) -> None:
    # The two paths east of the church: hedges between and beside them (defilade, bushes).
    hedge(L, 29.0, 32.0, -2.0, 6.0)
    hedge(L, 29.0, 32.0, 10.0, 17.0)
    hedge(L, 29.0, 32.0, 21.0, 31.0)
    hedge(L, 33.0, 36.0, 3.0, 5.0)
    hedge(L, 33.0, 35.0, 14.0, 16.0)
    hedge(L, 34.0, 37.0, 24.0, 26.0)
    # East shops along Open Hof: two enterable, the rest solid.
    terrace(L, 49.0, X1, -34.0, 0.0, 0.0, 7.2)
    shop(L, 49.0, X1, 0.0, 12.0, upper=STOREY, west=[door(4.0, 6.4), window(1.5, 3.0), window(8.5, 10.0)])
    terrace(L, 49.0, X1, 12.0, 16.0, 0.0, 7.2)
    shop(L, 49.0, X1, 16.0, 32.0, upper=STOREY, west=[door(22.0, 24.4), window(17.5, 19.5), window(27.0, 29.0)])
    terrace(L, 49.0, X1, 32.0, Z1, 0.0, 7.2)
    L.block("wood", 55.0, 57.0, 0.0, 0.9, 2.0, 10.0)
    L.block("wood", 55.0, 57.0, 0.0, 0.9, 18.0, 30.0)
    for x, z in ((53.0, 6.0), (53.0, 21.0), (53.0, 26.0)):
        L.light(x, 2.8, z, intensity=200000.0, reach=9.0)
    # Their flat roofs (3.6 m) are a trick-jump away from Open Hof: a two-crate stack against the wall,
    # and a parapet round the roof (cover) with a gap where the stack arrives.
    parapet(L, 49.0, X1, 0.0, 12.0, STOREY, west=[(10.0, 11.7)])
    parapet(L, 49.0, X1, 16.0, 32.0, STOREY, west=[(20.0, 21.6)])
    for z in (11.0, 20.8):
        L.crate(48.35, z, 1.2)
        L.crate(48.35, z, 1.0, y=1.2)


# ---- cover ---------------------------------------------------------------------------------------------


def cover(L: Level) -> None:
    # Alpha's start: the south street.
    L.jersey(-24.0, 44.0, math.pi / 2, 3.0)
    L.crate(-34.0, 48.0, 1.2)
    L.dumpster(-14.0, 48.5, 0.0)
    L.car(-28.0, 41.5, 0.0, "covered")
    # Kerkstraat.
    L.car(-34.0, -26.0, math.pi / 2)
    L.van(-33.0, -14.0, math.pi / 2, "metal")
    L.jersey(-34.0, 8.0, 0.0, 3.0)
    L.car(-33.5, 18.0, math.pi / 2, "corrugated")
    L.crate(-31.8, 31.0, 1.2)
    L.crate(-34.0, 36.0, 1.2)
    # The lane in front of the church.
    L.car(-13.0, 6.0, math.pi / 2, "covered")
    L.jersey(-11.0, 18.0, 0.0, 3.0)
    L.crate(-14.0, 24.0, 1.2)
    L.dumpster(-14.5, 36.0, 0.0)
    # The north lane.
    L.jersey(4.0, -8.0, math.pi / 2, 3.0)
    L.van(24.0, -7.5, 0.0, "metal")
    L.jersey(36.0, -8.0, math.pi / 2, 3.0)
    L.crate(10.0, -6.0, 1.2)
    # Raadhuisstraat.
    L.van(8.0, -38.0, 0.0, "corrugated")
    L.car(26.0, -37.5, 0.0, "covered")
    L.jersey(40.0, -38.0, math.pi / 2, 3.0)
    # The east paths and the car park (Bravo's start).
    L.crate(26.0, 8.0, 1.2)
    L.crate(26.5, 26.0, 1.2)
    L.jersey(34.5, 10.0, 0.0, 3.0)
    L.jersey(34.5, 30.0, 0.0, 3.0)
    for z, body in ((13.0, "metal"), (19.0, "covered"), (25.0, "metal"), (31.0, "covered"), (36.0, "corrugated")):
        L.car(40.0, z, math.pi / 2, body)
    L.van(44.0, 8.0, math.pi / 2, "metal")
    L.jersey(43.0, 22.0, 0.0, 3.0)
    L.dumpster(44.0, 34.0, 0.0)
    # Open Hof.
    L.car(47.5, -22.0, math.pi / 2, "covered")
    L.jersey(47.5, -4.0, math.pi / 2, 3.0)
    L.car(47.5, 44.0, math.pi / 2, "corrugated")
    # The south lane.
    L.jersey(8.0, 35.0, 0.0, 3.0)
    L.crate(26.0, 35.0, 1.2)
    L.dumpster(14.0, 33.5, 0.0)
    for x, z in ((-33.0, 0.0), (-33.0, 28.0), (-10.0, 20.0), (20.0, -8.0), (38.0, -1.0), (30.0, 36.0)):
        L.lamp(x, z)
        L.light(x, 5.2, z, intensity=160000.0, reach=12.0, practical=True)


# ---- street life ---------------------------------------------------------------------------------------


def street_life(L: Level) -> None:
    """A Dutch town centre in use: crates, pallets and barrels by the market stalls, benches and bins
    on the square, chairs at the cafe, and on the blank brick of the streets' walls shutters, utility
    boxes, air-conditioning units and a few barrels, bins and a generator. Civil on purpose: no
    sandbags at all, and nothing solid within 1.5 m of a door, a doorway, an alley mouth, a spawn or
    an objective. Wall fittings are decoration (no collider)."""
    market(L)
    kerkstraat(L)
    marktstraat(L)
    north_streets(L)
    east_and_south(L)


def market(L: Level) -> None:
    """Kerkplein: the stalls' stock and the square's furniture."""
    # North row of stalls (x -26, -20, -14 at z -27, backs on z -28): stock beside and behind them.
    crates(L, -23.0, -27.2, 0, stacked=2)
    crates(L, -17.0, -27.4, 0, ("props/wooden_crate", "props/military_crate"), 2)
    pallet_stack(L, -23.0, -29.8, 0, 2)
    barrels(L, -11.4, -28.6, 2, 0)
    # South row (x -26, -20 at z -9, facing north).
    crates(L, -23.0, -8.6, 2, stacked=2)
    pallet_stack(L, -23.0, -6.6, 0, 2)
    barrels(L, -29.0, -7.8, 2, 1)
    # Benches on the lawn strips, facing the square; bins by the light poles; planter boxes.
    for z in (-15.0, -21.0):
        L.model("props/bench", -29.7, z, 90.0)
    for z in (-14.0, -24.0):
        L.model("props/bench", -6.3, z, 270.0)
    bins(L, -6.9, -4.6, 0, 1, 2)
    bins(L, -29.9, -30.4, 0, 1, 2)
    for x in (-27.0, -17.0):
        L.model("props/planter", x, -4.5, 0.0)


def kerkstraat(L: Level) -> None:
    """The brick canyon of Kerkstraat (x -37..-31) and the church lane (x -16..-6) between the
    blocks A (z 13..24) and B (z 27..40), and the alley that separates them."""
    # Block A and B, west faces (Kerkstraat side).
    shutter(L, "x", -31.0, 15.0, -1, "door", graffiti=True)
    shutter(L, "x", -31.0, 19.0, -1, "window")
    L.on_wall("props/utility_box", "x", -31.0, 22.4, -1, solid=True, mounted=False)
    aircon(L, "x", -31.0, 22.4, -1)
    shutter(L, "x", -31.0, 34.0, -1, "window", graffiti=True)
    shutter(L, "x", -31.0, 37.8, -1, "door")
    L.on_wall("props/power_box", "x", -31.0, 28.8, -1, solid=True, mounted=False)
    aircon(L, "x", -31.0, 29.0, -1)
    # The cafe's west wall: units above the door's head and a gas cylinder by the corner.
    aircon(L, "x", -31.0, 3.4, -1)
    aircon(L, "x", -31.0, 11.4, -1)
    L.model("props/propane_tank", -31.4, 2.6)
    L.model("props/propane_tank", -31.45, 3.15)
    # Block A and B, east faces (church lane side).
    shutter(L, "x", -16.0, 16.5, 1, "window")
    shutter(L, "x", -16.0, 20.6, 1, "door", graffiti=True)
    L.on_wall("props/generator", "x", -16.0, 22.8, 1, solid=True, mounted=False)
    shutter(L, "x", -16.0, 36.0, 1, "window")
    shutter(L, "x", -16.0, 38.6, 1, "door")
    aircon(L, "x", -16.0, 15.0, 1)
    aircon(L, "x", -16.0, 35.0, 1)
    # The alley (z 24..27, 3 m wide): bins against block B's north face, barrels at the far end
    # against block A's south face; each leaves 2.4 m clear.
    bins(L, -28.5, 26.65, 0, 2, 2)
    barrels(L, -19.5, 24.4, 2, 0)
    # Kerkstraat's own clutter.
    barrels(L, -31.5, 12.0, 2, 1)


def marktstraat(L: Level) -> None:
    """Marktstraat (z -3..2) and the street along the square's north side."""
    # The library's north face (z 3, outward -1): clear of its door (x -50..-47.4).
    shutter(L, "z", 3.0, -55.0, -1, "window", graffiti=True)
    shutter(L, "z", 3.0, -44.0, -1, "door")
    barrels(L, -64.0, 1.6, 3, 0)
    bins(L, -57.0, 2.6, 0, 1, 1)
    # The terrace across the street (z -5, outward +1).
    shutter(L, "z", -5.0, -60.0, 1, "window")
    shutter(L, "z", -5.0, -50.0, 1, "door", graffiti=True)
    shutter(L, "z", -5.0, -44.0, 1, "door")
    aircon(L, "z", -5.0, -56.0, 1)
    L.on_wall("props/utility_box", "z", -5.0, -47.0, 1, solid=True, mounted=False)
    # The cafe's north face (z 2, outward -1): chairs on its pavement.
    L.model("props/plastic_chair", -19.0, 0.9, 190.0, solid=False)
    L.model("props/plastic_chair", -18.1, 0.6, 170.0, solid=False)
    L.model("props/plastic_chair", -28.0, 0.9, 200.0, solid=False)
    shutter(L, "z", 2.0, -28.0, -1, "window")


def north_streets(L: Level) -> None:
    """Raadhuisstraat's north row (z -41, outward +1), the kiosk, and the shops' backs on the north
    lane (z -11, outward +1)."""
    shutter(L, "z", -41.0, 9.5, 1, "door", graffiti=True)
    shutter(L, "z", -41.0, 26.0, 1, "window")
    shutter(L, "z", -41.0, 44.0, 1, "window", graffiti=True)
    aircon(L, "z", -41.0, 12.0, 1)
    aircon(L, "z", -41.0, 30.0, 1)
    aircon(L, "z", -41.0, 48.0, 1)
    L.on_wall("props/utility_box_tall", "z", -41.0, 22.0, 1, solid=True, mounted=False)
    L.on_wall("props/power_box", "z", -41.0, 55.0, 1, solid=True, mounted=False)
    shutter(L, "z", -35.0, 35.2, 1, "window")  # on the newspaper kiosk's south face
    bins(L, 7.0, -39.6, 0, 1, 1)
    barrels(L, 26.5, -40.6, 2, 0)
    for x in (23.0, 38.0):
        L.model("props/plastic_chair", x, -31.4, 20.0, solid=False)
    # The north lane's shop faces.
    shutter(L, "z", -11.0, 7.0, 1, "window")
    L.on_wall("props/utility_box", "z", -11.0, 10.5, 1, solid=True, mounted=False)
    aircon(L, "z", -11.0, 10.5, 1)
    shutter(L, "z", -11.0, 20.0, 1, "door", graffiti=True)
    barrels(L, 21.8, -10.5, 2, 0)
    shutter(L, "z", -11.0, 28.0, 1, "window")
    L.on_wall("props/power_box", "z", -11.0, 37.5, 1, solid=True, mounted=False)
    aircon(L, "z", -11.0, 40.0, 1)
    shutter(L, "z", -11.0, 43.0, 1, "door")


def east_and_south(L: Level) -> None:
    """Open Hof's shop faces, the car park and the south lane."""
    # East shops' blank terraces (x 49, outward -1).
    shutter(L, "x", 49.0, -30.0, -1, "door")
    shutter(L, "x", 49.0, -15.0, -1, "window", graffiti=True)
    aircon(L, "x", 49.0, -25.0, -1)
    aircon(L, "x", 49.0, -9.0, -1)
    shutter(L, "x", 49.0, 14.0, -1, "door", graffiti=True)
    shutter(L, "x", 49.0, 38.0, -1, "window")
    aircon(L, "x", 49.0, 42.0, -1)
    # The car park: a picnic table (a lunch spot beside the path), barrels and a bin.
    L.model("props/picnic_table", 26.5, 20.0, 0.0)  # 2.2 m across, 3.0 m long (along z): 1.4 m clear each side of the 5 m path
    barrels(L, 44.6, 38.4, 3, 0)
    bins(L, 38.0, 38.6, 0, 1, 1)
    # The south lane (z 32..38) and its north faces: the south block at z 38 (outward -1) and the
    # block at z 40.
    shutter(L, "z", 38.0, -1.0, -1, "window", graffiti=True)
    shutter(L, "z", 38.0, 3.0, -1, "door")
    L.on_wall("props/utility_box", "z", 38.0, 6.2, -1, solid=True, mounted=False)
    aircon(L, "z", 38.0, 12.5, -1)
    barrels(L, 16.4, 37.3, 2, 0)
    shutter(L, "z", 40.0, 24.0, -1, "window")
    L.on_wall("props/generator", "z", 40.0, 30.0, -1, solid=True, mounted=False)
    aircon(L, "z", 40.0, 33.0, -1)
    # The south street (Alpha's start): the terrace on its far side (z 50, outward -1).
    shutter(L, "z", 50.0, -26.0, -1, "window")
    shutter(L, "z", 50.0, -19.0, -1, "door", graffiti=True)
    aircon(L, "z", 50.0, -14.0, -1)
    pallet_stack(L, -29.5, 49.0, 0, 2)


# ---- entities ------------------------------------------------------------------------------------------

CENTRE = (0.0, 14.0)
ALPHA = [(-32.0, 46.0), (-27.0, 48.0), (-22.0, 46.0), (-17.0, 48.0), (-12.0, 46.0), (-8.0, 48.0)]
BRAVO = [(39.0, 16.0), (43.0, 20.0), (39.0, 23.0), (43.0, 27.0), (39.0, 30.0), (43.0, 33.0)]
FFA = [(-34.0, -20.0), (-34.0, 12.0), (26.5, -3.0), (35.0, 28.0), (29.0, -8.0), (-12.0, 30.0), (-18.0, -18.0), (12.0, 36.0)]
FLAGS = {"A": (-19.0, -18.0), "B": (-54.0, 14.0), "C": (53.0, 24.0)}
HQS = {
    "market": (-15.0, -30.0),
    "cafe": (-23.0, 7.0),
    "library": (-56.0, 20.0),
    "church": (16.0, 3.0),
    "east shop": (53.0, 6.0),
}
BOMBS = {"A": (1.0, 6.0), "B": (53.0, 22.0)}
AMMO = [(-12.0, -18.0), (-34.0, 22.0), (-14.0, 15.5), (27.0, 14.0), (18.0, -8.0), (36.0, -1.0)]


def entities(L: Level) -> None:
    for x, z in ALPHA:
        L.spawn("alpha", "tdm dom hq sd", x, z, CENTRE)
    for x, z in BRAVO:
        L.spawn("bravo", "tdm dom hq sd", x, z, CENTRE)
    for x, z in FFA:
        L.spawn("any", "ffa", x, z, CENTRE)
    for label, (x, z) in FLAGS.items():
        L.flag(label, x, z)
    for x, z in HQS.values():
        L.hq(x, z)
    for label, (x, z) in BOMBS.items():
        L.bomb(label, x, z)
    for x, z in AMMO:
        L.ammo_crate(x, z)


def build() -> Level:
    L = Level("Z-Town", "overcast_urban")
    ground(L)
    perimeter(L)
    kerkplein(L)
    library(L)
    church(L)
    west_blocks(L)
    north_block(L)
    south_block(L)
    east_side(L)
    cover(L)
    street_life(L)
    entities(L)
    return L


# The trick-jump routes: the (x, z) of each ledge in climbing order (a third number picks the top
# below it, for a stack). Each step may rise at most the mantle height (kit.MANTLE_MAX).
CLIMBS = {
    "church roof, tower steps": [(-11.5, 10.8), (-8.6, 10.8, 1.5), (-8.6, 10.8, 2.5), (-7.2, 10.8), (-6.5, 7.5), (-6.5, 4.5), (-3.0, 6.0, 7.5), (-0.5, 6.0, 9.0), (-0.5, 6.0, 10.5)],
    "library terrace": [(-38.6, 0.0), (-38.6, 1.4, 1.5), (-38.6, 1.4, 2.5), (-40.0, 2.5, 3.6), (-38.0, 2.5, 5.0), (-40.0, 2.5, 6.5), (-44.0, 8.0, 7.5)],
    "cafe roof": [(-33.5, 9.4), (-31.7, 9.4, 1.5), (-31.7, 9.4, 2.5), (-29.0, 9.4)],
    "east shop roof, north": [(47.0, 11.0), (48.35, 11.0, 1.5), (48.35, 11.0, 2.5), (51.0, 11.0)],
    "east shop roof, south": [(47.0, 20.8), (48.35, 20.8, 1.5), (48.35, 20.8, 2.5), (51.0, 20.8)],
}


if __name__ == "__main__":
    level = build()
    level.write(OUT)
    if "--check" in sys.argv:
        from kit import check_walkability, climb_problems, fence_conflicts, placement_problems

        starts = [ALPHA[2], BRAVO[2]]
        goals = {
            "flag A market": FLAGS["A"],
            "flag B library": FLAGS["B"],
            "flag C east shop": FLAGS["C"],
            **{f"HQ {name}": pos for name, pos in HQS.items()},
            "bomb A church hall": BOMBS["A"],
            "bomb B east shop": BOMBS["B"],
            "church north hall": (16.0, 8.0),
            "church south hall": (9.0, 24.0),
            "cafe": (-24.0, 8.0),
            "library 1F": (-50.0, STOREY, 18.0),
            "second path": (34.0, 20.0),
            "north lane": (20.0, -8.0),
            "south lane": (20.0, 35.0),
        }
        walks = check_walkability(
            level, (X0 - 1, X1 + 1, Z0 - 1, Z1 + 1), starts, goals, image=Path("/tmp/z_town_nav.ppm"), walk_range=(80.0, 140.0),
        )
        problems = placement_problems(level) + climb_problems(level, CLIMBS) + fence_conflicts(level)
        for line in problems:
            print("  PROBLEM: " + line)
        print("check: " + ("ok" if walks and not problems else "FAILED"))
        sys.exit(0 if walks and not problems else 1)
