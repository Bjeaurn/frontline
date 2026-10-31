"""Generates crates/frontline/assets/maps/crossroads.map: Crossroads, an original urban Map in the
Call of Duty recipe: three lanes (west street, a two-storey building in the middle, east street),
sightlines of 15-40 m, about 22 s to cross at a walk (a spawn-to-spawn path of ~120 m), sized for 6-10 players. The layout is point-symmetric
(turned 180 degrees about the centre), so both Teams get the same map; Alpha starts in the north
(-z), Bravo in the south.

    python3 tools/mapgen/crossroads.py
    python3 tools/mapgen/crossroads.py --check      # also flood-fill the walkable floor

Brushes make everything players touch; Downtown City MegaKit models (models/downtown/) dress
it: facades, complete buildings on the skyline, street tiles and furniture. The kit has no
vehicles, barriers or lamp posts, so those are brush blockouts (kit.py).
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

from kit import MAPS_DIR, Level, door, window

OUT = MAPS_DIR / "crossroads.map"

STOREY = 3.6  # floor to floor
SLAB = 0.3
WALL_TOP = STOREY - SLAB  # top of a storey's walls
ROOF = 2 * STOREY  # top of the central building's roof slab

# The Map is spread out rather than scaled: the buildings keep their size, the gaps between them
# grow. The blocks sit BX further out (wide lanes), the streets and Teams' plazas are pushed out
# along z, and the open ground in between is filled with cover.
BX = 8.0
DZ = 10.0  # how much further out the streets and plazas sit than the lanes' ends
HX, HZ = 46.0, 62.0  # half extents of the playfield


def central_building(L: Level) -> None:
    """The contested two-storey building in the middle: a hall with a stair ramp up each side,
    two side rooms per floor, windows over both lanes and a fire escape to the roof on the east
    (and, turned about, the west) wall. x -10..10, z -8..8."""
    wall, slab = "plaster", "concrete"
    t = 0.4
    # Window and door gaps, along the north wall (x) and west wall (z) of each storey. Mirroring
    # builds the south and east walls.
    north_gf = [door(-1.4, 1.4), window(-8.0, -6.2), window(6.2, 8.0)]
    north_2f = [window(-1.2, 1.2), window(-8.0, -6.2), window(6.2, 8.0)]
    west_gf = [door(-5.5, -4.0), window(2.5, 4.3)]
    west_2f = [door(-6.8, -5.4), window(-4.8, -3.0), window(2.0, 3.8)]
    inner_x0, inner_x1, inner_z0, inner_z1 = -9.6, 9.6, -7.6, 7.6

    with L.mirrored():
        # Ground floor outer walls (north, west; the mirror gives south, east).
        L.wall(wall, "z", -8.0 + t / 2, -10.0, 10.0, 0.0, STOREY, t, north_gf)
        L.wall(wall, "x", -10.0 + t / 2, -8.0 + t, 8.0 - t, 0.0, STOREY, t, west_gf)
        # Upper floor.
        L.wall(wall, "z", -8.0 + t / 2, -10.0, 10.0, STOREY, ROOF - SLAB, t, north_2f)
        L.wall(wall, "x", -10.0 + t / 2, -8.0 + t, 8.0 - t, STOREY, ROOF - SLAB, t, west_2f)
        # Hall partitions: doorways to the side rooms on the side without the stair.
        L.wall(wall, "x", -3.2, inner_z0, inner_z1, 0.0, WALL_TOP, 0.3, [door(-5.3, -3.7)])
        L.wall(wall, "x", -3.2, inner_z0, inner_z1, STOREY, ROOF - SLAB, 0.3, [door(-5.3, -3.7)])
        # Divider of each side room, a doorway between its halves.
        L.wall(wall, "z", 0.0, inner_x0, -3.05, 0.0, WALL_TOP, 0.3, [door(-7.2, -5.6)])
        L.wall(wall, "z", 0.0, inner_x0, -3.05, STOREY, ROOF - SLAB, 0.3, [door(-7.2, -5.6)])

        # Stair ramp up the hall's west side, rising north to the upper floor.
        L.ramp("wood", -3.05, -1.0, 0.4, 6.4, 0.0, STOREY, "-z")
        # Rails round the stairwell on the upper floor (its other edges are walls).
        L.block("metal", -1.25, -1.0, STOREY, STOREY + 1.0, 0.4, 6.4)
        L.block("metal", -3.05, -1.25, STOREY, STOREY + 1.0, 6.15, 6.4)

        # Upper floor slab: the side rooms, and the hall around the two stairwells.
        L.block(slab, inner_x0, -3.05, STOREY - SLAB, STOREY, inner_z0, inner_z1)
        L.block(slab, -3.05, 3.05, STOREY - SLAB, STOREY, -7.6, -6.4)
        L.block(slab, -3.05, 1.0, STOREY - SLAB, STOREY, -6.4, -0.4)
    L.block(slab, -3.05, 3.05, STOREY - SLAB, STOREY, -0.4, 0.4)
    # Roof slab and parapet. The east parapet has a gap where the fire escape arrives.
    L.block(slab, -10.0, 10.0, ROOF - SLAB, ROOF, -8.0, 8.0)
    with L.mirrored():
        L.wall("concrete", "z", -8.0 + 0.15, -10.0, 10.0, ROOF, ROOF + 1.0, 0.3)
        L.wall("concrete", "x", 10.0 - 0.15, -7.7, 7.7, ROOF, ROOF + 1.0, 0.3, [door(-4.0, -1.5, 1.0)])

        # Fire escape on the east wall: a ramp up to a landing at the upper floor's door, then
        # back north up to the roof.
        L.slab_ramp("metal", (11.5, 0.0, -2.0), (11.5, STOREY, 5.0), 1.6)
        L.block("metal", 10.2, 13.9, STOREY - 0.25, STOREY, 5.0, 8.0)
        L.slab_ramp("metal", (13.3, STOREY, 5.0), (13.3, ROOF, -1.5), 1.6)
        L.block("metal", 10.0, 14.1, ROOF - 0.25, ROOF, -4.0, -1.5)
        for px, pz in ((13.7, 7.7), (10.4, 7.7), (13.7, 5.2)):
            L.block("metal", px - 0.1, px + 0.1, 0.0, STOREY - 0.25, pz - 0.1, pz + 0.1)
        L.block("metal", 13.8, 14.0, 0.0, ROOF - 0.25, -3.9, -3.7)

        # Rails on the landing and the roof landing's open sides.
        L.block("metal", 10.2, 13.9, STOREY, STOREY + 1.0, 7.75, 8.0)
        L.block("metal", 13.65, 13.9, STOREY, STOREY + 1.0, 5.1, 7.9)
        L.block("metal", 13.9, 14.15, ROOF, ROOF + 1.0, -4.0, -1.5)
        L.block("metal", 10.0, 14.15, ROOF, ROOF + 1.0, -4.2, -3.95)

        # Furniture: counters and desks, rooftop vents.
        L.block("wood", -9.6, -8.2, 0.0, 0.9, -7.6, -5.8)  # counter, NW room
        L.block("wood", -8.6, -4.0, 0.0, 0.9, 0.9, 1.5)  # counter, SW room
        L.block("wood", -8.2, -6.4, STOREY, STOREY + 0.8, 2.4, 4.0)  # desks upstairs
        L.block("wood", -9.0, -7.6, STOREY, STOREY + 0.8, -3.0, -1.4)
        L.block("wood", -6.0, -4.2, STOREY, STOREY + 0.8, -6.4, -5.2)
        L.block("metal", -7.0, -5.0, ROOF, ROOF + 1.3, -5.0, -3.0)  # rooftop vent housing
        L.block("metal", 4.0, 6.2, ROOF, ROOF + 1.0, 2.0, 3.0)
        # Concrete columns in the hall.
        L.block("concrete", -0.25, 0.25, 0.0, WALL_TOP, -2.2, -1.7)

    # Lights.
    for x, y, z in ((0, 2.9, -4.0), (-6.4, 2.9, -4.0), (-6.4, 2.9, 4.0), (0, 6.5, -4.0), (-6.4, 6.5, -2.0), (-6.4, 6.5, 4.5)):
        L.light(x, y, z, intensity=220000.0, reach=9.0)


def blocks(L: Level) -> None:
    """The east block's garage and workshop (and, turned about, the west block's): one storey
    with a roof you reach by a ramp from the north street. x 24..38, z -12..12 (before `BX`)."""
    wall = "brick"
    t = 0.4
    top = WALL_TOP
    # The lane side (west face of the east block): a wide shutter door, a door, windows.
    lane_side = [door(-9.0, -5.0, 2.8), window(-3.8, -1.8), window(1.5, 3.5), door(5.5, 7.0), window(8.5, 10.3)]
    north_side = [window(26.0, 27.8), window(32.0, 33.4), door(34.0, 35.5)]
    south_side = [door(26.5, 28.0), window(33.0, 34.8)]
    with L.mirrored():
        # East block: the mirror gives the west block.
        L.wall(wall, "x", 24.0 + t / 2, -12.0 + t, 12.0 - t, 0.0, top, t, lane_side)
        L.wall(wall, "z", -12.0 + t / 2, 24.0, 38.0, 0.0, top, t, north_side)
        L.wall(wall, "z", 12.0 - t / 2, 24.0, 38.0, 0.0, top, t, south_side)
        L.wall(wall, "x", 38.0 - t / 2, -12.0 + t, 12.0 - t, 0.0, top, t)
        L.block("concrete", 24.0, 38.0, top, STOREY, -12.0, 12.0)  # roof
        # Roof edge: a parapet on the lane side and the ends; the north end has the ramp's gap.
        L.wall("concrete", "x", 24.0 + 0.15, -12.0, 12.0, STOREY, STOREY + 1.0, 0.3)
        L.wall("concrete", "z", -12.0 + 0.15, 24.3, 37.7, STOREY, STOREY + 1.0, 0.3, [door(28.5, 31.0, 1.0)])
        L.wall("concrete", "z", 12.0 - 0.15, 24.3, 37.7, STOREY, STOREY + 1.0, 0.3)
        L.wall("concrete", "x", 38.0 - 0.15, -11.7, 11.7, STOREY, STOREY + 1.0, 0.3)
        # Roof access ramp from the north street.
        L.ramp("metal", 28.5, 31.0, -18.0, -12.0, 0.0, STOREY, "+z")
        # Interior: a divider with a doorway, a second partition.
        L.wall("plaster", "z", 0.0, 24.4, 37.6, 0.0, top, 0.3, [door(29.5, 31.5)])
        L.wall("plaster", "x", 31.0, -11.6, 0.0, 0.0, top, 0.3, [door(-8.0, -6.2)])
        # Interior cover.
        L.block("wood", 33.0, 36.0, 0.0, 1.0, -10.5, -9.5)
        L.block("metal", 25.0, 26.2, 0.0, 1.8, -11.6, -9.0)
        L.crate(28.0, -3.0, 1.2)
        L.crate(29.3, -2.6, 1.0)
        L.crate(28.6, -2.9, 1.0, y=1.2)
        L.block("wood", 34.0, 37.4, 0.0, 0.9, 3.0, 3.6)
        L.block("metal", 26.0, 28.0, 0.0, 1.4, 9.0, 11.2)
        L.block("concrete", 33.2, 33.8, 0.0, top, 6.0, 6.6)  # column
        L.light(30.0, 3.0, -6.0, intensity=200000.0, reach=9.0)
        L.light(31.0, 3.0, 6.0, intensity=200000.0, reach=9.0)
        L.light(34.5, 3.0, -6.0, intensity=200000.0, reach=9.0)


def perimeter(L: Level) -> None:
    """Tall walls closing the Map, with complete MegaKit buildings on the skyline."""
    wall = "brick"
    L.block(wall, -HX - 3, HX + 3, 0.0, 14.0, -HZ - 3, -HZ)  # north
    L.block(wall, -HX - 3, HX + 3, 0.0, 14.0, HZ, HZ + 3)  # south
    L.block(wall, -HX - 3, -HX, 0.0, 14.0, -HZ, HZ)  # west
    L.block(wall, HX, HX + 3, 0.0, 14.0, -HZ, HZ)  # east
    # Skyline: complete buildings along the north and west edges (the mirror gives the south
    # and east), fronts on the playable side.
    with L.mirrored():
        L.building("Building_Large_2", -34.0, -HZ, 0.0)
        L.building("Building_Medium_2_001", -12.0, -HZ, 0.0)
        L.building("Building_Small_1", 8.0, -HZ, 0.0)
        L.building("Building_Large_2", 32.0, -HZ, 0.0)
        L.building("Building_Small_1", -HX, -44.0, 90.0)
        L.building("Building_Large_2", -HX, -14.0, 90.0)
        L.building("Building_Medium_2_001", -HX, 14.0, 90.0)
        L.building("Building_Medium_2_001", -HX, 40.0, 90.0)


def street(L: Level) -> None:
    """Ground, asphalt tiles, road markings and street furniture."""
    L.floor("concrete", -HX, HX, -HZ, HZ, 0.0)
    asphalt = "Street_Asphalt_6x6"
    # The tile's base is its surface: 1 cm over the floor, decals 1 cm over the tile.
    for i in range(15):  # the north and south streets
        for z in (-40.0 - DZ, -34.0 - DZ, 34.0 + DZ, 40.0 + DZ):
            L.prop(asphalt, -42.0 + 6.0 * i, z, 0.0, 0.01)
    for x in (-27.0, -21.0, -15.0, 15.0, 21.0, 27.0):  # the lanes
        for j in range(14):
            L.prop(asphalt, x, -39.0 + 6.0 * j, 0.0, 0.01)
    with L.mirrored():
        for i in range(15):
            L.prop("Decal_DoubleYellow_Straight", -42.0 + 6.0 * i, -37.0 - DZ, 0.0, 0.02)
        for x in (-21.0, 21.0):
            L.prop("Decal_Crosswalk", x, -29.0 - DZ, 90.0, 0.02)
            L.prop("Decal_ArrowStraight", x, -28.0, 0.0, 0.02)
            L.prop("Decal_ArrowStraight", x, -14.0, 0.0, 0.02)
        # Street furniture.
        L.prop("Prop_ManholeCover", -8.0, -37.0 - DZ, 0.0, 0.02)
        L.prop("Prop_ManholeCover", -21.0, -3.0, 0.0, 0.02)
        L.prop("Prop_Drain", -23.4, -31.6 - DZ, 0.0, 0.02)
        L.prop("Prop_Drain", 27.4, -14.6, 0.0, 0.02)
        for x in (-42.0, -38.0, -14.0, 14.0, 38.0, 42.0):
            L.prop("Prop_Planter_Single", x, -HZ + 1.5, 0.0, 0.0, "box")
        for x in (-11.0, -9.0):
            L.prop("Prop_Bollard", x, -12.5, 0.0, 0.0, "box")


def cover(L: Level) -> None:
    """Vehicles and barriers: the cover that breaks the long sightlines. Everything here is the
    north half (the mirror makes the south half)."""
    with L.mirrored():
        # North street and Alpha's plaza, DZ further out than the lanes' ends.
        with L.shifted(0.0, -DZ):
            L.car(-40.0, -40.5, 0.0)
            L.car(-35.0, -35.5, 0.25, "corrugated")
            L.van(-24.0, -38.0, 0.0)
            L.jersey(-30.0, -32.5, 0.0, 3.0)
            L.jersey(-27.0, -32.5, 0.0, 3.0)
            L.car(-10.0, -35.0, 0.0)
            L.van(14.0, -36.0, 0.0, "metal")
            L.car(26.0, -41.0, math.pi / 2)
            L.car(34.0, -35.0, 0.0, "corrugated")
            L.jersey(40.0, -32.5, 0.0, 3.0)
            L.dumpster(43.0, -41.0, 0.0)
            # North plaza (Alpha's spawn).
            L.jersey(-9.0, -43.0, 0.0, 3.0)
            L.jersey(9.0, -43.0, 0.0, 3.0)
            L.crate(-24.0, -49.0, 1.2)
            L.crate(-22.7, -49.2, 1.0)
            L.crate(24.0, -47.0, 1.0)
            L.dumpster(-19.0, -50.5, 0.0)
            L.dumpster(18.0, -50.5, 0.0)
            L.crate(-38.0, -48.0, 1.2)
            L.jersey(-6.0, -30.0, 0.0, 3.0)
            L.jersey(6.0, -29.0, 0.0, 3.0)
        # The lanes' far ends and the central plaza in front of the hall (z -42..-30).
        L.van(-22.0, -36.0, math.pi / 2)
        L.jersey(-28.0, -40.0, 0.0, 3.0)
        L.car(-14.0, -38.0, 0.4, "corrugated")
        L.crate(-30.0, -34.0, 1.2)
        L.crate(-28.7, -34.2, 1.0)
        L.car(24.0, -37.0, math.pi / 2)
        L.jersey(16.0, -34.0, 0.0, 3.0)
        L.dumpster(30.0, -40.0, 0.0)
        L.van(20.0, -42.0, 0.0, "metal")
        L.jersey(-4.0, -36.0, 0.0, 3.0)
        L.jersey(5.0, -41.0, 0.0, 3.0)
        L.crate(0.0, -32.0, 1.2)
        # North half of the central plaza, in front of the hall.
        L.van(-5.0, -22.0, 0.0)
        L.car(7.5, -15.5, 0.0)
        L.crate(0.0, -26.0, 1.2)
        L.crate(1.3, -26.2, 1.0)
        # West lane, north end: a van across the lane, barriers at its mouth, crates and a car.
        L.van(-18.0, -19.0, math.pi / 2)
        L.jersey(-14.0, -26.0, math.pi / 2, 3.0)
        L.car(-28.0, -14.0, 0.3)
        L.crate(-30.0, -28.0, 1.2)
        L.crate(-28.7, -28.2, 1.0)
        L.dumpster(-12.0, -31.0, 0.0)
        L.jersey(-24.0, -10.5, 0.0, 3.0)
        L.jersey(-27.0, -10.5, 0.0, 3.0)
        # West lane, middle: a van across its middle, barriers, crates.
        L.van(-20.0, 2.0, 0.0)
        L.jersey(-26.0, 6.0, math.pi / 2, 3.0)
        L.jersey(-14.0, -5.5, math.pi / 2, 3.0)
        L.crate(-27.0, -6.0, 1.2)
        L.crate(-27.0, -4.8, 1.0)
        # East lane, north end (and the west lane's south end by the mirror).
        L.car(14.0, -19.0, math.pi / 2)
        L.van(26.0, -26.0, 0.0, "metal")
        L.jersey(28.0, -16.0, math.pi / 2, 3.0)
        L.dumpster(30.0, -30.0, 0.0)
        L.crate(18.0, -30.0, 1.2)
        L.crate(19.3, -30.2, 1.0)
        L.car(14.0, -27.0, 0.2, "corrugated")
        L.jersey(26.0, -9.0, 0.0, 3.0)
        L.crate(12.0, -10.0, 1.2)
        for x, z in ((-42.0, -41.0), (-22.0, -41.0), (-12.0, -18.0), (12.0, -26.0), (-30.0, -8.0), (30.0, -41.0), (-6.0, -57.0)):
            L.lamp(x, z)


def entities(L: Level) -> None:
    centre = (0.0, 0.0)
    # Team Deathmatch and the objective modes: Alpha spawns north, Bravo south.
    alpha = [(-12.0, -60.0), (-6.0, -58.0), (0.0, -60.0), (6.0, -58.0), (12.0, -60.0), (0.0, -55.0)]
    for team, sign in (("alpha", -1.0), ("bravo", 1.0)):
        for x, z in alpha:
            # Alpha faces south from the north; Bravo is the same spot turned about the centre.
            L.spawn(team, "tdm dom hq sd", x if sign < 0 else -x, z if sign < 0 else -z, centre)
    # Free-for-all (a later Game Mode): spread over the street ends and lanes.
    for x, z in [(-42.0, -40.0), (42.0, 40.0), (-42.0, 40.0), (42.0, -40.0), (-21.0, 0.0), (21.0, 0.0), (0.0, -58.0), (0.0, 58.0)]:
        L.spawn("any", "ffa", x, z, centre)
    # Domination: A on Alpha's side, B in the central hall, C on Bravo's side.
    L.flag("A", -21.0, -32.0)
    L.flag("B", 0.0, 0.0)
    L.flag("C", 21.0, 32.0)
    # Headquarters: the hall and the two far lane ends.
    L.hq(0.0, 3.0)
    L.hq(-24.0, 22.0)
    L.hq(24.0, -22.0)
    # Search & Destroy: one site in each flanking block.
    L.bomb("A", -39.0, -5.0)
    L.bomb("B", 39.0, 5.0)
    # Team Deathmatch ammo crates (ignored by the other modes): three pairs turned 180 degrees about
    # the centre, in the middle lanes between the blocks, far from both Teams' spawns.
    with L.mirrored():
        L.ammo_crate(-22.0, -2.0)
        L.ammo_crate(-30.0, -20.0)
        L.ammo_crate(6.0, 18.0)


def dress(L: Level) -> None:
    """MegaKit facades, door leaves and roof props on top of the blockout."""
    brick = ["Brick_Plain_3", "Brick_Plain_3_noWear"]
    with L.mirrored():
        with L.shifted(BX, 0.0):
            # East block: lane face and the street faces.
            lane_side = [door(-9.0, -5.0, 2.8), window(-3.8, -1.8), window(1.5, 3.5), door(5.5, 7.0), window(8.5, 10.3)]
            # Facing the lane (west): z runs along the face.
            L.facade(brick, "x", 24.0, -12.0, 12.0, lane_side, 0.0, -1)
            north_side = [window(26.0, 27.8), window(32.0, 33.4), door(34.0, 35.5)]
            L.facade(brick, "z", -12.0, 24.0, 38.0, north_side, 0.0, -1)
            # Roof props: AC units are cover-sized.
            L.prop("Prop_ACUnit", 32.0, 9.0, 0.0, STOREY, "box")
            L.prop("Prop_ACUnit", 27.0, -9.0, 180.0, STOREY, "box")
        # Central building: the hall's north face and the side faces.
        north_gf = [door(-1.4, 1.4), window(-8.0, -6.2), window(6.2, 8.0)]
        L.facade(["Trim_Plain_3", "Brick_Plain_3"], "z", -8.0, -10.0, 10.0, north_gf, 0.0, -1)
        west_gf = [door(-5.5, -4.0), window(2.5, 4.3)]
        L.facade(["Trim_Plain_3", "Brick_Plain_3"], "x", -10.0, -8.0, 8.0, west_gf, 0.0, -1)
        L.prop("Prop_ACUnit", 5.0, 1.2, 0.0, ROOF, "box")
        L.prop("Prop_ACUnit", -2.0, -6.8, 180.0, ROOF, "box")


def build() -> Level:
    L = Level("Crossroads", "overcast_urban")
    street(L)
    perimeter(L)
    central_building(L)
    with L.shifted(BX, 0.0):
        blocks(L)
    cover(L)
    dress(L)
    entities(L)
    return L


if __name__ == "__main__":
    level = build()
    level.write(OUT)
    if "--check" in sys.argv:
        from kit import check_walkability

        starts = [(0.0, -58.0), (0.0, 58.0)]
        goals = {
            "flag A": (-21.0, -32.0),
            "flag B": (0.0, 0.0),
            "flag C": (21.0, 32.0),
            "HQ hall": (0.0, 3.0),
            "HQ W": (-24.0, 22.0),
            "HQ E": (24.0, -22.0),
            "bomb A": (-39.0, -5.0),
            "bomb B": (39.0, 5.0),
            "hall 2F": (0.0, STOREY, -3.0),
            "roof": (0.0, ROOF, 0.0),
        }
        tour = [
            (0.0, -58.0), goals["flag A"], goals["HQ E"], goals["bomb B"], goals["flag B"], goals["hall 2F"],
            goals["roof"], goals["HQ W"], goals["flag C"], goals["bomb A"], (0.0, 58.0),
        ]
        check_walkability(
            level, (-HX - 1, HX + 1, -HZ - 1, HZ + 1), starts, goals, image=Path("/tmp/crossroads_nav.ppm"), tour=tour,
            route_out=Path("/tmp/crossroads.route"), walk_range=(110.0, 140.0),
        )
