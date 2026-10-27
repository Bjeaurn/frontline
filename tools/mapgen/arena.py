"""Generates crates/frontline/assets/maps/arena.map: the greybox arena, with spawns per Team,
Domination Flags, HQ sites and Search & Destroy bomb sites.

    python3 tools/mapgen/arena.py

Coordinates are game space (metres, y up); the floor's top face is at y = 0.
"""

from __future__ import annotations

import math
from pathlib import Path

from mapgen import Map, box, box_rotated_z, point_entity

ARENA_SIZE = 60.0
PERIMETER_WALL_HEIGHT = 4.0

OUT = Path(__file__).resolve().parents[2] / "crates" / "frontline" / "assets" / "maps" / "arena.map"

# Surface folder (crates/frontline/assets/textures/<surface>/) per role of a block.
FLOOR, PERIMETER, WALL, BUILDING = "ground", "concrete", "brick", "plaster"
RAMP, COVER, CRATE, SLAB, CONTAINER = "wood", "metal", "wood", "concrete", "corrugated"


def facing_centre(x: float, z: float) -> float:
    """Yaw that faces the arena centre from (x, z)."""
    return math.atan2(x, z)


def build() -> Map:
    arena = Map(worldspawn={"name": "Arena", "look": "desert_dust"})
    add = lambda surface, size, center: arena.brushes.append(box(center, size, surface))

    # Floor (top face at y = 0) and perimeter walls.
    half = ARENA_SIZE / 2.0
    wall_y = PERIMETER_WALL_HEIGHT / 2.0
    add(FLOOR, (ARENA_SIZE, 1.0, ARENA_SIZE), (0.0, -0.5, 0.0))
    for sign in (-1.0, 1.0):
        add(PERIMETER, (ARENA_SIZE + 2.0, PERIMETER_WALL_HEIGHT, 1.0), (0.0, wall_y, sign * (half + 0.5)))
        add(PERIMETER, (1.0, PERIMETER_WALL_HEIGHT, ARENA_SIZE), (sign * (half + 0.5), wall_y, 0.0))

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

    # West crawl barrier: a slab on two pillars leaves a 0.6 m gap, flanked by tall walls.
    add(SLAB, (1.0, 1.6, 1.0), (-16.0, 0.8, -2.0))
    add(SLAB, (1.0, 1.6, 1.0), (-16.0, 0.8, 2.0))
    add(SLAB, (1.0, 1.0, 5.0), (-16.0, 1.1, 0.0))
    add(WALL, (0.5, 2.5, 8.0), (-16.0, 1.25, -6.5))
    add(WALL, (0.5, 2.5, 8.0), (-16.0, 1.25, 6.5))

    # Crouch-height (1 m) cover walls.
    add(COVER, (4.0, 1.0, 0.4), (0.0, 0.5, -14.0))
    add(COVER, (4.0, 1.0, 0.4), (0.0, 0.5, 14.0))
    add(COVER, (0.4, 1.0, 4.0), (20.0, 0.5, -8.0))
    add(COVER, (0.4, 1.0, 4.0), (20.0, 0.5, 8.0))
    add(COVER, (3.0, 1.0, 0.6), (-22.0, 0.5, -10.0))
    add(COVER, (3.0, 1.0, 0.6), (-22.0, 0.5, 10.0))

    # Crates, some stacked.
    add(CRATE, (1.0, 1.0, 1.0), (10.0, 0.5, -10.0))
    add(CRATE, (1.0, 1.0, 1.0), (11.2, 0.5, -10.3))
    add(CRATE, (1.0, 1.0, 1.0), (10.6, 1.5, -10.15))
    add(CRATE, (1.0, 1.0, 1.0), (-10.0, 0.5, 10.0))
    add(CRATE, (1.0, 1.0, 1.0), (-11.2, 0.5, 10.4))
    add(CRATE, (1.0, 1.0, 1.0), (10.0, 0.5, 12.0))
    add(CRATE, (1.0, 1.0, 1.0), (-9.5, 0.5, -12.0))

    # Medium and full-height cover.
    add(COVER, (2.0, 1.5, 2.0), (-8.0, 0.75, -20.0))
    add(COVER, (2.0, 1.5, 2.0), (8.0, 0.75, 20.0))
    add(COVER, (2.0, 2.2, 2.0), (16.0, 1.1, 16.0))
    add(COVER, (2.0, 2.2, 2.0), (-16.0, 1.1, -16.0))
    add(WALL, (6.0, 2.5, 0.5), (16.0, 1.25, -22.0))
    add(WALL, (6.0, 2.5, 0.5), (-16.0, 1.25, 22.0))
    add(CONTAINER, (2.5, 2.6, 6.0), (-22.0, 1.3, 18.0))
    add(CONTAINER, (2.5, 2.6, 6.0), (22.0, 1.3, -14.0))

    # Ramp from the east floor (x = 13) up to the roof edge (x = 6, y = 3.3): a 0.3 m plank
    # turned about z, its top face running from floor to roof edge.
    run, rise, thickness = 7.0, 3.3, 0.3
    angle = -math.atan2(rise, run)
    up = (-math.sin(angle), math.cos(angle), 0.0)
    top_mid = (6.0 + run / 2.0, rise / 2.0, 0.0)
    center = tuple(top_mid[i] - up[i] * thickness / 2.0 for i in range(3))
    arena.brushes.append(box_rotated_z(center, (math.hypot(run, rise), thickness, 2.6), angle, RAMP))

    # Team Deathmatch: eight spawns either Team may use, facing the centre.
    for x, z in [(-24, -24), (24, -24), (-24, 24), (24, 24), (0, -25), (0, 25), (-25, 0), (25, 0)]:
        arena.entities.append(
            point_entity("spawn_point", (x, 0.0, z), facing_centre(x, z), team="any", modes="tdm")
        )
    # Objective modes: Alpha starts south, Bravo north.
    for team, sign in (("alpha", -1.0), ("bravo", 1.0)):
        for x, z in [(-24.0, 24.0), (-10.0, 26.0), (10.0, 26.0), (24.0, 24.0)]:
            arena.entities.append(
                point_entity(
                    "spawn_point", (x, 0.0, sign * z), facing_centre(x, sign * z), team=team, modes="dom hq sd"
                )
            )

    # Domination: A by Alpha, B inside the building, C by Bravo.
    for label, position in (("A", (0.0, 0.0, -19.0)), ("B", (0.0, 0.0, -3.0)), ("C", (0.0, 0.0, 19.0))):
        arena.entities.append(point_entity("dom_flag", position, label=label))
    # Headquarters: the open west and east lanes.
    for position in ((-23.0, 0.0, 0.0), (23.0, 0.0, 0.0)):
        arena.entities.append(point_entity("hq_site", position))
    # Search & Destroy: both bomb sites on Bravo's half.
    for label, position in (("A", (-14.0, 0.0, 17.0)), ("B", (12.0, 0.0, 17.0))):
        arena.entities.append(point_entity("bomb_site", position, label=label))

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
        (-22.0, 3.6, 18.0, "1 0.6 0.3", 320000, 14.0, False),
        (22.0, 3.6, -14.0, "1 0.6 0.3", 320000, 14.0, False),
        (-23.0, 3.5, 0.0, "1 0.8 0.5", 380000, 16.0, False),
        (23.0, 3.5, 0.0, "1 0.8 0.5", 380000, 16.0, False),
        (0.0, 3.5, -19.0, "1 0.8 0.5", 380000, 16.0, False),
        (0.0, 3.5, 19.0, "1 0.8 0.5", 380000, 16.0, False),
    ]
    for x, y, z, color, lumens, reach, shadows in lamps:
        arena.entities.append(
            point_entity(
                "light_point",
                (x, y, z),
                targetname="practical",
                color=color,
                intensity=str(lumens),
                range=f"{reach:.1f}",
                shadows_enabled="true" if shadows else "false",
            )
        )
    return arena


if __name__ == "__main__":
    OUT.parent.mkdir(parents=True, exist_ok=True)
    build().write(str(OUT))
    print(f"Wrote {OUT}")
