"""Generates the weapon camo textures: 128x128 seamlessly tiling RGB PNGs.

Python 3 with Pillow only (no numpy). Deterministic: every random choice is seeded per camo.

    python3 -m venv /tmp/camo && /tmp/camo/bin/pip install Pillow
    /tmp/camo/bin/python tools/camos/camos.py                    # all, into assets/camos/
    /tmp/camo/bin/python tools/camos/camos.py woodland gold      # just these
    /tmp/camo/bin/python tools/camos/camos.py --sheet /tmp/camos.png   # contact sheet too

Shapes are drawn on a 3x3 canvas at every wrapped offset and the middle tile is kept, so the
edges meet. The viewmodel maps one tile onto about 12 cm of gun (`weapon::viewmodel`), so the
blobs here are a few pixels across at most. The ids match `data/progression.ron`.
"""

from __future__ import annotations

import argparse
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "crates/frontline/assets/camos"

TILE = 128

Rgb = tuple[int, int, int]


def canvas(base: Rgb) -> tuple[Image.Image, ImageDraw.ImageDraw]:
    image = Image.new("RGB", (TILE * 3, TILE * 3), base)
    return image, ImageDraw.Draw(image)


def middle(image: Image.Image) -> Image.Image:
    return image.crop((TILE, TILE, TILE * 2, TILE * 2))


def wrapped(draw_one) -> None:
    """Runs `draw_one(dx, dy)` for every offset a shape near an edge can reach into the middle."""
    for ox in (-TILE, 0, TILE):
        for oy in (-TILE, 0, TILE):
            draw_one(TILE + ox, TILE + oy)


def blob(rng: random.Random, radius: float, lumpy: float = 0.45, points: int = 14):
    """A ragged closed outline around the origin as (x, y) offsets."""
    stretch = rng.uniform(0.6, 1.6)
    turn = rng.uniform(0, math.tau)
    out = []
    for i in range(points):
        angle = math.tau * i / points
        r = radius * (1 + rng.uniform(-lumpy, lumpy))
        x, y = math.cos(angle) * r * stretch, math.sin(angle) * r / stretch
        out.append(
            (x * math.cos(turn) - y * math.sin(turn), x * math.sin(turn) + y * math.cos(turn))
        )
    return out


def splash(draw, rng: random.Random, colour: Rgb, count: int, radius: tuple[float, float]):
    for _ in range(count):
        cx, cy = rng.uniform(0, TILE), rng.uniform(0, TILE)
        shape = blob(rng, rng.uniform(*radius))

        def one(dx, dy, cx=cx, cy=cy, shape=shape):
            draw.polygon([(cx + dx + x, cy + dy + y) for x, y in shape], fill=colour)

        wrapped(one)


def grain(image: Image.Image, rng: random.Random, amount: int) -> Image.Image:
    """Fine per-pixel noise so flat colour doesn't read as plastic."""
    pixels = image.load()
    for y in range(image.height):
        for x in range(image.width):
            shift = rng.randint(-amount, amount)
            r, g, b = pixels[x, y]
            pixels[x, y] = (
                max(0, min(255, r + shift)),
                max(0, min(255, g + shift)),
                max(0, min(255, b + shift)),
            )
    return image


def woodland(rng: random.Random) -> Image.Image:
    image, draw = canvas((84, 98, 52))
    splash(draw, rng, (122, 92, 52), 26, (6, 14))
    splash(draw, rng, (52, 66, 36), 24, (5, 12))
    splash(draw, rng, (28, 30, 22), 22, (3, 8))
    return grain(middle(image), rng, 6)


def desert(rng: random.Random) -> Image.Image:
    image, draw = canvas((196, 168, 118))
    splash(draw, rng, (226, 204, 154), 26, (6, 14))
    splash(draw, rng, (150, 118, 76), 22, (5, 12))
    splash(draw, rng, (110, 86, 58), 16, (3, 8))
    return grain(middle(image), rng, 6)


def pixelated(rng: random.Random, base: Rgb, shades: list[tuple[Rgb, int, tuple[float, float]]]):
    """Digital camo: the blobs are drawn on a coarse grid, then enlarged without smoothing."""
    cells = 32
    scale = TILE // cells
    image = Image.new("RGB", (cells * 3, cells * 3), base)
    draw = ImageDraw.Draw(image)
    for colour, count, radius in shades:
        for _ in range(count):
            cx, cy = rng.uniform(0, cells), rng.uniform(0, cells)
            shape = blob(rng, rng.uniform(*radius), lumpy=0.5, points=10)
            for ox in (-cells, 0, cells):
                for oy in (-cells, 0, cells):
                    draw.polygon(
                        [(cells + cx + ox + x, cells + cy + oy + y) for x, y in shape],
                        fill=colour,
                    )
    tile = image.crop((cells, cells, cells * 2, cells * 2))
    return tile.resize((TILE, TILE), Image.NEAREST)


def urban(rng: random.Random) -> Image.Image:
    image = pixelated(
        rng,
        (128, 130, 134),
        [((164, 166, 170), 16, (1.5, 4)), ((92, 94, 100), 16, (1.5, 4)), ((58, 60, 66), 12, (1, 2.5))],
    )
    return grain(image, rng, 5)


def tiger(rng: random.Random) -> Image.Image:
    image, draw = canvas((176, 116, 48))
    splash(draw, rng, (204, 150, 70), 18, (5, 11))
    # Tapering black stripes along wavy paths; whole waves per tile so they meet at the edges.
    for stripe in range(7):
        y0 = stripe * TILE / 7 + rng.uniform(-3, 3)
        waves = rng.choice((1, 2))
        phase = rng.uniform(0, math.tau)
        amp = rng.uniform(5, 11)
        slope = rng.uniform(-0.25, 0.25)

        def one(dx, dy, y0=y0, waves=waves, phase=phase, amp=amp, slope=slope):
            top, bottom = [], []
            steps = 48
            for i in range(steps + 1):
                t = i / steps
                x = t * TILE
                width = 1.2 + 4.2 * math.sin(math.pi * t) ** 0.7
                y = y0 + amp * math.sin(math.tau * waves * t + phase) + slope * x
                top.append((x + dx, y - width + dy))
                bottom.append((x + dx, y + width + dy))
            draw.polygon(top + bottom[::-1], fill=(24, 16, 10))

        wrapped(one)
    return grain(middle(image), rng, 6)


def arctic(rng: random.Random) -> Image.Image:
    image, draw = canvas((226, 232, 238))
    splash(draw, rng, (178, 192, 208), 26, (6, 14))
    splash(draw, rng, (136, 150, 168), 20, (4, 10))
    splash(draw, rng, (246, 248, 250), 14, (4, 9))
    return grain(middle(image), rng, 4)


def gold(rng: random.Random) -> Image.Image:
    image, draw = canvas((214, 168, 52))
    splash(draw, rng, (236, 196, 84), 22, (8, 18))
    splash(draw, rng, (184, 138, 34), 18, (6, 14))
    # Fine diamond engraving.
    step = TILE // 8
    for i in range(-TILE * 3, TILE * 3, step):
        draw.line([(i, 0), (i + TILE * 3, TILE * 3)], fill=(170, 124, 28), width=1)
        draw.line([(i, TILE * 3), (i + TILE * 3, 0)], fill=(246, 214, 112), width=1)
    tile = middle(image).filter(ImageFilter.GaussianBlur(0.4))
    return grain(tile, rng, 5)


CAMOS = {
    "woodland": woodland,
    "desert": desert,
    "urban": urban,
    "tiger": tiger,
    "arctic": arctic,
    "gold": gold,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("names", nargs="*", help="camo ids (default: all)")
    parser.add_argument("--sheet", type=Path, help="also write a contact sheet (each tile 2x2)")
    args = parser.parse_args()
    names = args.names or list(CAMOS)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    tiles = []
    for name in names:
        tile = CAMOS[name](random.Random(name))
        assert tile.size == (TILE, TILE)
        tile.save(OUT_DIR / f"{name}.png", optimize=True)
        tiles.append(tile)
        print(f"wrote {OUT_DIR / (name + '.png')}")
    if args.sheet:
        sheet = Image.new("RGB", (TILE * 2 * len(tiles), TILE * 2))
        for i, tile in enumerate(tiles):
            for ox in (0, TILE):
                for oy in (0, TILE):
                    sheet.paste(tile, (i * TILE * 2 + ox, oy))
        sheet.save(args.sheet)


if __name__ == "__main__":
    main()
