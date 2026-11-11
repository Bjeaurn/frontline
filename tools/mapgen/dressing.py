"""Street dressing for the generated Maps: small helpers that place clusters of the prop models
(models/props/, see tools/assets/import_polyhaven.py) through `Level.model`: barrels in twos and
threes, pallets loaded with bags, stacked crates, sandbag walls, tyres, bins with bags, wall
fittings. Each takes game-space metres and works inside `Level.mirrored()`.

Python 3, standard library only.
"""

from __future__ import annotations

from kit import Level

BARRELS = ("props/barrel_red", "props/barrel_rusty", "props/barrel_blue")


def _turn(dx: float, dz: float, quarters: int) -> tuple[float, float]:
    """(dx, dz) turned by `quarters` quarter turns about the origin."""
    for _ in range(quarters % 4):
        dx, dz = -dz, dx
    return dx, dz


def barrels(L: Level, x: float, z: float, count: int = 3, quarters: int = 0, kinds=BARRELS) -> None:
    """Two or three upright barrels standing close together at (x, z), the group turned
    `quarters` quarter turns. Their clip boxes (the model bounds, about 0.6 m square) just touch."""
    for i, (dx, dz) in enumerate([(0.0, 0.0), (0.66, 0.04), (0.33, 0.68)][:count]):
        dx, dz = _turn(dx, dz, quarters)
        L.model(kinds[(i + quarters) % len(kinds)], x + dx, z + dz, 90.0 * (i + quarters))


def pallet_stack(L: Level, x: float, z: float, quarters: int = 0, layers: int = 2) -> None:
    """A pallet (1.2 x 0.8 m, long axis along x unless turned) loaded with `layers` layers of
    two cement bags each (0.18 m a layer): a knee to thigh high, solid block. `quarters` turns it."""
    L.model("props/pallet", x, z, 90.0 * quarters, solid=True)
    for layer in range(layers):
        for side in (-1.0, 1.0):
            dx, dz = _turn(side * 0.25, 0.0, quarters)
            # Bags lie across the pallet; alternate layers turn a hair so the stack looks hand built.
            L.model("props/cement_bag", x + dx, z + dz, 90.0 * quarters + (4.0 if layer % 2 else -3.0), 0.144 + 0.18 * layer, solid=True, mounted=True)


def crates(L: Level, x: float, z: float, quarters: int = 0, kinds=("props/military_crate", "props/wooden_crate"), stacked: int = 2) -> None:
    """A stack of `stacked` small crates at (x, z), one on the other, each turned a little."""
    y = 0.0
    for i in range(stacked):
        kind = kinds[i % len(kinds)]
        L.model(kind, x, z, 90.0 * quarters + (6.0 if i % 2 else 0.0), y, solid=True, mounted=i > 0)
        y += L._folder_bounds(kind)[1][1]


def sandbags(L: Level, x: float, z: float, quarters: int = 0) -> None:
    """A chest-high sandbag wall (2.0 x 0.41 m, 0.95 m tall); `quarters` 1 turns it to run
    along z."""
    L.model("props/sandbags", x, z, 90.0 * quarters)


def tyres(L: Level, x: float, z: float, quarters: int = 0, count: int = 2) -> None:
    """Upright tyres standing side by side (0.6 m across, 0.17 m thick), `quarters` 0 facing
    south and north."""
    for i in range(count):
        dx, dz = _turn(0.0, 0.22 * i, quarters)
        L.model("props/tyre", x + dx, z + dz, 90.0 * quarters)


def bins(L: Level, x: float, z: float, quarters: int = 0, cans: int = 2, bags: int = 2) -> None:
    """Trash cans in a row along x (turned by `quarters`) with trash bags piled beside them:
    the cans solid (0.77 x 0.56 m, 0.91 m tall), the bags not."""
    for i in range(cans):
        dx, dz = _turn(0.85 * i, 0.0, quarters)
        L.model("props/trash_can", x + dx, z + dz, 90.0 * quarters + 4.0 * i)
    for i in range(bags):
        dx, dz = _turn(0.85 * cans + 0.1 + 0.15 * (i % 2), 0.2 * i - 0.1, quarters)
        L.model("props/trashbag", x + dx, z + dz, 37.0 * (i + 1), solid=False)


def aircon(L: Level, axis: str, c: float, along: float, outward: int, y: float = 2.4) -> None:
    """An air-conditioning unit (0.8 x 0.93 m, 0.37 deep) hung on a wall face above head height:
    `y` is its base (at least 2.4 m so it never blocks), decoration only."""
    assert y >= 2.4, "an aircon unit hangs above head height"
    L.on_wall("props/aircon", axis, c, along, outward, y)


def shutter(L: Level, axis: str, c: float, along: float, outward: int, kind: str = "door", graffiti: bool = False) -> None:
    """A roller shutter fixed to a blank wall face, 0.02 m off it, decoration only: a "door" (1.08
    x 2.4 m, standing on the ground) or a "window" (2.1 x 1.85 m on a 0.9 m sill)."""
    name = f"props/shutter_{kind}" + ("_graffiti" if graffiti else "")
    L.on_wall(name, axis, c, along, outward, 0.0 if kind == "door" else 0.9)
