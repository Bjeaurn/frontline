"""Generates the rank insignia: one worn cloth patch per account Level (64x64 RGBA PNG each).

Reuses the patch machinery of tools/emblems/emblems.py (woven field, merrow border, satin-stitch
symbol, wear, frayed edge), so ranks and Faction emblems share a look. Pillow only. Deterministic:
every random choice is seeded per Level.

    python3 -m venv /tmp/emb && /tmp/emb/bin/pip install Pillow
    /tmp/emb/bin/python tools/ranks/ranks.py                 # all, into assets/icons/ranks/
    /tmp/emb/bin/python tools/ranks/ranks.py 1 12 30         # just these Levels
    /tmp/emb/bin/python tools/ranks/ranks.py --sheet /tmp/ranks.png   # contact sheet too

The bands (keep in step with `progression/rank.rs`): Levels 1-10 wear 1-10 chevrons on olive
drab, 11-20 wear 1-10 bars on navy, 21-30 wear 1-10 stars on oxblood. Files are named
`chevrons_NN.png`, `bars_NN.png` and `stars_NN.png`.
"""

from __future__ import annotations

import argparse
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/emblems"))

import emblems as E  # noqa: E402
from PIL import Image, ImageChops, ImageFilter  # noqa: E402

OUT_DIR = ROOT / "crates/frontline/assets/icons/ranks"
OUT_SIZE = 64
LEVELS = 30
PER_BAND = 10

# Symbol box: a 100 unit box centred on the patch, 176 px of the 256 px patch.
BOX = E.Box(128, 128, 176)

CHEVRON = dict(field=(78, 84, 52), border=(36, 40, 24), thread=(222, 204, 150))
BAR = dict(field=(38, 54, 84), border=(176, 186, 198), thread=(224, 228, 234))
STAR = dict(field=(98, 30, 30), border=(206, 164, 62), thread=(246, 208, 92))

BODY = E.round_poly([(12, 12), (244, 12), (244, 244), (12, 244)], 46)


def chevrons(n: int):
    """`n` stacked chevrons, filling the symbol box more tightly the more there are."""
    rise = 30 - 1.2 * (n - 1)
    pitch = min(24.0, (100 - rise) / (n - 0.3))
    thick = min(13.0, 0.7 * pitch)
    height = rise + thick + (n - 1) * pitch
    top = 50 - height / 2
    prims = []
    for i in range(n):
        y = top + i * pitch
        prims.append(
            E.poly([(50, y), (92, y + rise), (92, y + rise + thick), (50, y + thick), (8, y + rise + thick), (8, y + rise)])
        )
    return prims, 135


def bars(n: int):
    """`n` stacked horizontal bars."""
    pitch = min(26.0, 100 / (n + 0.4))
    thick = min(15.0, 0.7 * pitch)
    height = thick + (n - 1) * pitch
    top = 50 - height / 2
    prims = [E.poly([(8, top + i * pitch), (92, top + i * pitch), (92, top + i * pitch + thick), (8, top + i * pitch + thick)]) for i in range(n)]
    return prims, 0


STAR_ROWS = [[1], [2], [3], [2, 2], [3, 2], [3, 3], [4, 3], [4, 4], [3, 3, 3], [4, 3, 3]]


def stars(n: int):
    """`n` five-point stars in rows."""
    rows = STAR_ROWS[n - 1]
    wide = max(rows)
    cell = min(92 / wide, 92 / len(rows) / 0.92)
    ro = min(34.0, 0.6 * cell)
    prims = []
    for r, count in enumerate(rows):
        cy = 50 + (r - (len(rows) - 1) / 2) * cell * 0.92
        for c in range(count):
            cx = 50 + (c - (count - 1) / 2) * cell
            prims.append(E.poly(E.star_pts(cx, cy, ro, ro * 0.42, 5)))
    return prims, 90


BANDS = [("chevrons", CHEVRON, chevrons), ("bars", BAR, bars), ("stars", STAR, stars)]


def band(level: int):
    """(name, colours, symbol factory, count) of a Level."""
    index, count = divmod(level - 1, PER_BAND)
    return (*BANDS[index], count + 1)


def file_name(level: int) -> str:
    name, _, _, count = band(level)
    return f"{name}_{count:02}.png"


def build(level: int) -> Image.Image:
    rng = random.Random(1000 + level)
    _, look, symbol, count = band(level)
    canvas = Image.new("RGBA", (E.W, E.W), (0, 0, 0, 0))
    rgb, mask, pts = E.piece(BODY, look["field"], look["border"], rng, border=12.0)
    prims, angle = symbol(count)
    m = E.draw_prims(prims, BOX)
    rgb = E.stitch(rgb, m, look["thread"], rng, angle, E.shade(look["thread"], 0.5), 3.2)
    rgb = E.wear(rgb, mask, rng)
    alpha = E.fray(mask, rng)
    threads = E.loose_threads(pts, look["border"], rng)
    shadow_mask = E.offset(alpha, 1.5, 2.0).filter(ImageFilter.GaussianBlur(2.5 * E.SS / 2))
    shadow = Image.new("RGBA", (E.W, E.W), (8, 8, 6, 0))
    shadow.putalpha(E.scaled_mask(shadow_mask, 0.4))
    canvas = Image.alpha_composite(canvas, shadow)
    layer = rgb.convert("RGBA")
    layer.putalpha(alpha)
    canvas = Image.alpha_composite(canvas, layer)
    canvas = Image.alpha_composite(canvas, threads)
    return canvas.convert("RGBa").resize((OUT_SIZE, OUT_SIZE), Image.LANCZOS).convert("RGBA")


def contact_sheet(imgs: list[Image.Image], path: Path) -> None:
    """Ten per row at 2x, on a mid-grey table."""
    cell = OUT_SIZE * 2 + 8
    rows = (len(imgs) + PER_BAND - 1) // PER_BAND
    sheet = Image.new("RGBA", (cell * PER_BAND, cell * rows), (92, 90, 84, 255))
    for i, img in enumerate(imgs):
        big = img.resize((OUT_SIZE * 2, OUT_SIZE * 2), Image.LANCZOS)
        sheet.alpha_composite(big, ((i % PER_BAND) * cell + 4, (i // PER_BAND) * cell + 4))
    sheet.save(path)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("levels", nargs="*", type=int, help="Levels to render (default: 1-30)")
    ap.add_argument("--out", type=Path, default=OUT_DIR)
    ap.add_argument("--sheet", type=Path, help="also write a contact sheet")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    done = []
    for level in args.levels or range(1, LEVELS + 1):
        img = build(level)
        img.save(args.out / file_name(level), optimize=True)
        print(file_name(level))
        done.append(img)
    if args.sheet and done:
        contact_sheet(done, args.sheet)


if __name__ == "__main__":
    main()
