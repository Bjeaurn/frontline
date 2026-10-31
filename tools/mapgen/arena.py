"""Generates crates/frontline/assets/maps/arena.map: the greybox arena, with spawns per Team,
Domination Flags, HQ sites, Search & Destroy bomb sites and Team Deathmatch ammo crates. 96 x 96 m:
the cover is spread out (about 1.6x the original 60 m layout) rather than enlarged, with more of it
in the new space, so it takes about 20 s to walk across.

    python3 tools/mapgen/arena.py
    python3 tools/mapgen/arena.py --check      # also flood-fill the walkable floor

Coordinates are game space (metres, y up); the floor's top face is at y = 0.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

from kit import MAPS_DIR, Level

HALF = 48.0  # the playfield is x, z in -HALF..HALF
SPREAD = 1.6  # how far the original layout's cover positions moved apart
PERIMETER_WALL_HEIGHT = 4.0

OUT = MAPS_DIR / "arena.map"

# Surface folder (crates/frontline/assets/textures/<surface>/) per role of a block.
FLOOR, PERIMETER, WALL, BUILDING = "ground", "concrete", "brick", "plaster"
RAMP, COVER, CRATE, SLAB, CONTAINER = "wood", "metal", "wood", "concrete", "corrugated"


def build() -> Level:
    L = Level("Arena", "desert_dust")

    def add(surface: str, size, center) -> None:
        sx, sy, sz = size
        cx, cy, cz = center
        L.block(surface, cx - sx / 2, cx + sx / 2, cy - sy / 2, cy + sy / 2, cz - sz / 2, cz + sz / 2)

    def spread(surface: str, size, x: float, z: float, y: float | None = None) -> None:
        """A free-standing block standing on the floor at the original layout's (x, z) moved out
        by SPREAD."""
        add(surface, size, (x * SPREAD, size[1] / 2.0 if y is None else y, z * SPREAD))

    # Floor (top face at y = 0) and perimeter walls.
    L.floor(FLOOR, -HALF, HALF, -HALF, HALF, 0.0)
    wall_y = PERIMETER_WALL_HEIGHT
    L.block(PERIMETER, -HALF - 1.0, HALF + 1.0, 0.0, wall_y, -HALF - 1.0, -HALF)
    L.block(PERIMETER, -HALF - 1.0, HALF + 1.0, 0.0, wall_y, HALF, HALF + 1.0)
    L.block(PERIMETER, -HALF - 1.0, -HALF, 0.0, wall_y, -HALF, HALF)
    L.block(PERIMETER, HALF, HALF + 1.0, 0.0, wall_y, -HALF, HALF)

    # Central building: 12 x 12 m, 3 m ground storey with a walkable roof.
    # North/south: walk-in doorways. West: a 0.6 m prone-only crawl gap. East: solid, ramp to roof.
    for sign in (-1.0, 1.0):
        z = sign * 5.8
        add(BUILDING, (5.2, 3.0, 0.4), (-3.4, 1.5, z))
        add(BUILDING, (5.2, 3.0, 0.4), (3.4, 1.5, z))
        add(BUILDING, (1.6, 0.8, 0.4), (0.0, 2.6, z))
        add(BUILDING, (0.4, 3.0, 4.8), (-5.8, 1.5, sign * 3.2))
        # Roof parapet, crouch height; the east side leaves a gap where the ramp arrives.
        add(BUILDING, (12.0, 1.0, 0.3), (0.0, 3.8, sign * 5.85))
        add(BUILDING, (0.3, 1.0, 4.3), (5.85, 3.8, sign * 3.55))
    add(BUILDING, (0.4, 2.4, 1.6), (-5.8, 1.8, 0.0))
    add(BUILDING, (0.4, 3.0, 11.2), (5.8, 1.5, 0.0))
    add(BUILDING, (12.0, 0.3, 12.0), (0.0, 3.15, 0.0))
    add(BUILDING, (0.3, 1.0, 11.4), (-5.85, 3.8, 0.0))
    add(COVER, (4.0, 1.0, 0.4), (0.0, 0.5, 0.0))

    # Ramp from the east floor (x = 13) up to the roof edge (x = 6, y = 3.3).
    L.slab_ramp(RAMP, (13.0, 0.0, 0.0), (6.0, 3.3, 0.0), 2.6, 0.3)

    # West crawl barrier: a slab on two pillars leaves a 0.6 m gap, flanked by long tall walls.
    bx = -16.0 * SPREAD
    add(SLAB, (1.0, 0.6, 1.0), (bx, 0.3, -2.0))
    add(SLAB, (1.0, 0.6, 1.0), (bx, 0.3, 2.0))
    add(SLAB, (1.0, 1.0, 5.0), (bx, 1.1, 0.0))
    add(WALL, (0.5, 2.5, 12.0), (bx, 1.25, -8.5))
    add(WALL, (0.5, 2.5, 12.0), (bx, 1.25, 8.5))

    # The building's yard: an open chain-link fence (sight and shots pass, bodies do not) with a
    # gate (3 m or more) at each end of every side and a 1 m low section to vault in the middle.
    L.fence_rect(
        -15.0, 15.0, -15.0, 15.0, "link",
        north=[(-12.0, -8.0), (8.0, 12.0)],
        south=[(-12.0, -8.0), (8.0, 12.0)],
        west=[(-12.0, -8.0), (8.0, 12.0)],
        east=[(-12.0, -8.0), (8.0, 12.0)],
        lows={"north": [(-2.0, 2.0)], "south": [(-2.0, 2.0)], "west": [(-2.0, 2.0)], "east": [(-2.0, 2.0)]},
    )

    # Crouch-height (1 m) cover walls.
    spread(COVER, (4.0, 1.0, 0.4), 0.0, -14.0)
    spread(COVER, (4.0, 1.0, 0.4), 0.0, 14.0)
    spread(COVER, (0.4, 1.0, 4.0), 20.0, -8.0)
    spread(COVER, (0.4, 1.0, 4.0), 20.0, 8.0)
    spread(COVER, (3.0, 1.0, 0.6), -22.0, -10.0)
    spread(COVER, (3.0, 1.0, 0.6), -22.0, 10.0)

    # A crate against two of the containers: crate (1.2 m), then container (2.6 m), a trick-jump
    # route within the 1.6 m mantle.
    L.crate(-33.35, 28.8, 1.2)
    L.crate(33.35, -22.4, 1.2)

    # Crates, some stacked.
    for cx, cy, cz in [(10.0, 0.5, -10.0), (11.2, 0.5, -10.3), (10.6, 1.5, -10.15), (-10.0, 0.5, 10.0), (-11.2, 0.5, 10.4), (10.0, 0.5, 12.0), (-9.5, 0.5, -12.0)]:
        spread(CRATE, (1.0, 1.0, 1.0), cx, cz, cy)

    # Medium and full-height cover.
    spread(COVER, (2.0, 1.5, 2.0), -8.0, -20.0)
    spread(COVER, (2.0, 1.5, 2.0), 8.0, 20.0)
    spread(COVER, (2.0, 2.2, 2.0), 16.0, 16.0)
    spread(COVER, (2.0, 2.2, 2.0), -16.0, -16.0)
    spread(WALL, (6.0, 2.5, 0.5), 16.0, -22.0)
    spread(WALL, (6.0, 2.5, 0.5), -16.0, 22.0)
    spread(CONTAINER, (2.5, 2.6, 6.0), -22.0, 18.0)
    spread(CONTAINER, (2.5, 2.6, 6.0), 22.0, -14.0)

    # The new space between: clusters of cover turned about the centre, so both halves get them.
    with L.mirrored():
        L.crate(-30.0, -26.0, 1.2)
        L.crate(-28.7, -26.3, 1.0)
        L.crate(-29.4, -26.1, 1.0, y=1.2)
        add(COVER, (2.0, 1.5, 2.0), (-17.0, 0.75, -33.5))
        add(WALL, (6.0, 2.5, 0.5), (-24.0, 1.25, -40.0))
        add(CONTAINER, (2.5, 2.6, 6.0), (-18.0, 1.3, -14.0))
        add(COVER, (4.0, 1.0, 0.4), (-8.0, 0.5, -30.0))
        add(COVER, (3.0, 1.0, 0.6), (-34.0, 0.5, -8.0))
        add(COVER, (0.4, 1.0, 4.0), (14.0, 0.5, -24.0))
        add(CONTAINER, (6.0, 2.6, 2.5), (24.0, 1.3, -30.5))
        add(WALL, (6.0, 2.5, 0.5), (12.0, 1.25, -38.0))
        L.crate(32.0, -16.0, 1.2)
        L.crate(33.2, -16.3, 1.0)
        add(COVER, (2.0, 2.2, 2.0), (26.0, 1.1, -26.0))
        add(COVER, (3.0, 1.0, 0.6), (40.0, 0.5, 8.0))
        add(COVER, (2.0, 1.5, 2.0), (22.0, 0.75, 6.0))
        # Full-height walls across the long lines between the spawns.
        add(WALL, (0.5, 2.5, 6.0), (-38.0, 1.25, -12.0))
        add(WALL, (6.0, 2.5, 0.5), (-4.0, 1.25, -38.0))
        add(WALL, (0.5, 2.5, 6.0), (-27.0, 1.25, 22.0))
        add(WALL, (6.0, 2.5, 0.5), (16.0, 1.25, -30.0))
        add(WALL, (0.5, 2.5, 6.0), (30.0, 1.25, -6.0))
        add(WALL, (6.0, 2.5, 0.5), (-21.0, 1.25, -9.0))

    # Team Deathmatch: eight spawns either Team may use, facing the centre.
    centre = (0.0, 0.0)
    for x, z in [(-38.0, -38.0), (38.0, -38.0), (-38.0, 38.0), (38.0, 38.0), (0.0, -40.0), (0.0, 40.0), (-40.0, 0.0), (40.0, 0.0)]:
        L.spawn("any", "tdm", x, z, centre)
    # Objective modes: Alpha starts south, Bravo north.
    for team, sign in (("alpha", -1.0), ("bravo", 1.0)):
        for x, z in [(-38.0, 38.0), (-16.0, 41.0), (16.0, 41.0), (38.0, 38.0)]:
            L.spawn(team, "dom hq sd", x, sign * z, centre)

    # Domination: A by Alpha, B inside the building, C by Bravo.
    L.flag("A", 0.0, -30.0)
    L.flag("B", 0.0, -3.0)
    L.flag("C", 0.0, 30.0)
    # Headquarters: the open west and east lanes.
    L.hq(-37.0, 0.0)
    L.hq(37.0, 0.0)
    # Search & Destroy: both bomb sites on Bravo's half.
    L.bomb("A", -22.0, 27.0)
    L.bomb("B", 19.0, 27.0)
    # Team Deathmatch ammo crates (ignored by the other modes): three pairs turned 180 degrees about
    # the centre, in the west/east lane and the north/south and diagonal lanes between cover, well
    # away from the spawns. Walk over one to be resupplied.
    with L.mirrored():
        L.ammo_crate(-22.0, 0.0)
        L.ammo_crate(-12.0, -22.0)
        L.ammo_crate(19.0, -12.0)

    # Practical lights: lamp posts, the building's interior, the containers, the HQ lanes and the
    # outer Flags. `targetname` "practical" lets the game scale them by the look's
    # `practical_lights` (off by day, on at night); only a few cast shadows.
    for x, z in [(8.0, -8.0), (-8.0, 8.0), (-8.0, -8.0), (8.0, 8.0)]:
        add(COVER, (0.3, 4.0, 0.3), (x, 2.0, z))
    lamps = [
        # (x, y, z, colour, lumens, range, shadows)
        (8.0, 4.4, -8.0, "1 0.78 0.45", 500000, 22.0, True),
        (-8.0, 4.4, 8.0, "1 0.78 0.45", 500000, 22.0, True),
        (-8.0, 4.4, -8.0, "1 0.78 0.45", 500000, 22.0, False),
        (8.0, 4.4, 8.0, "1 0.78 0.45", 500000, 22.0, False),
        (0.0, 2.5, -3.0, "1 0.85 0.6", 260000, 10.0, True),
        (0.0, 2.5, 3.0, "1 0.85 0.6", 260000, 10.0, False),
        (-35.2, 3.6, 28.8, "1 0.6 0.3", 320000, 14.0, False),
        (35.2, 3.6, -22.4, "1 0.6 0.3", 320000, 14.0, False),
        (-37.0, 3.5, 0.0, "1 0.8 0.5", 380000, 16.0, False),
        (37.0, 3.5, 0.0, "1 0.8 0.5", 380000, 16.0, False),
        (0.0, 3.5, -30.0, "1 0.8 0.5", 380000, 16.0, False),
        (0.0, 3.5, 30.0, "1 0.8 0.5", 380000, 16.0, False),
        (-22.0, 3.5, 27.0, "1 0.8 0.5", 300000, 14.0, False),
        (19.0, 3.5, 27.0, "1 0.8 0.5", 300000, 14.0, False),
    ]
    for x, y, z, color, lumens, reach, shadows in lamps:
        L.light(x, y, z, color=color, intensity=float(lumens), reach=reach, practical=True, shadows=shadows)
    return L


# The trick-jump routes: the (x, z) of each ledge in climbing order (a third number picks the
# top below it). Each step may rise at most the mantle height (kit.MANTLE_MAX).
CLIMBS = {
    "west container: crate, container": [(-31.0, 28.8), (-33.35, 28.8, 1.3), (-35.2, 28.8)],
    "east container: crate, container": [(31.0, -22.4), (33.35, -22.4, 1.3), (35.2, -22.4)],
}


if __name__ == "__main__":
    level = build()
    level.write(OUT)
    if "--check" in sys.argv:
        from kit import check_walkability, climb_problems, fence_conflicts, placement_problems

        starts = [(0.0, -41.0), (0.0, 41.0)]
        goals = {
            "flag A": (0.0, -30.0),
            "flag B": (0.0, -3.0),
            "flag C": (0.0, 30.0),
            "HQ W": (-37.0, 0.0),
            "HQ E": (37.0, 0.0),
            "bomb A": (-22.0, 27.0),
            "bomb B": (19.0, 27.0),
            "roof": (0.0, 3.3, 3.0),
        }
        tour = [(0.0, -41.0), goals["flag A"], goals["HQ W"], goals["flag B"], goals["roof"], goals["HQ E"], goals["bomb B"], goals["bomb A"], goals["flag C"], (0.0, 41.0)]
        walks = check_walkability(
            level, (-HALF - 1, HALF + 1, -HALF - 1, HALF + 1), starts, goals, image=Path("/tmp/arena_nav.ppm"), tour=tour,
            route_out=Path("/tmp/arena.route"), walk_range=(110.0, 140.0),
        )
        problems = placement_problems(level) + climb_problems(level, CLIMBS) + fence_conflicts(level)
        for line in problems:
            print("  PROBLEM: " + line)
        print("check: " + ("ok" if walks and not problems else "FAILED"))
        sys.exit(0 if walks and not problems else 1)
