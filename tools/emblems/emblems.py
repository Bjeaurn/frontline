"""Generates the Faction emblems: worn, embroidered cloth patches (256x256 RGBA PNG each).

Python 3 with Pillow only (no numpy). Deterministic: every random choice is seeded per Faction.

    python3 -m venv /tmp/emb && /tmp/emb/bin/pip install Pillow
    /tmp/emb/bin/python tools/emblems/emblems.py                 # all, into assets/icons/factions/
    /tmp/emb/bin/python tools/emblems/emblems.py ranger volkov   # just these
    /tmp/emb/bin/python tools/emblems/emblems.py --sheet /tmp/sheet.png   # contact sheet too

Each patch is a woven twill field with a raised merrow border (thread ribs with dashed stitch
highlights), an embroidered symbol (satin-stitch hatching, outline thread, soft emboss) and a
name tab or rocker set in the game's Rajdhani Bold. Wear: frayed edges with loose threads,
sun fade and grime. Work is done at SS x the output size and reduced at the end.

Coordinates in the layout code are final pixels (0..256, y down); a symbol is drawn in its own
100x100 box placed with `Box`.
"""

from __future__ import annotations

import argparse
import math
import random
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "crates/frontline/assets/fonts/Rajdhani-Bold.ttf"
OUT_DIR = ROOT / "crates/frontline/assets/icons/factions"

SIZE = 256
SS = 3
W = SIZE * SS

Pt = tuple[float, float]
Rgb = tuple[int, int, int]


# ---------------------------------------------------------------- geometry


def sc(pts):
    return [(x * SS, y * SS) for x, y in pts]


def bezier(p0, p1, p2, p3, n=24):
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        out.append(
            (
                u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
                u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
            )
        )
    return out


def ellipse_pts(cx, cy, rx, ry, n=96, a0=0.0, a1=360.0):
    return [
        (
            cx + rx * math.cos(math.radians(a0 + (a1 - a0) * i / n)),
            cy + ry * math.sin(math.radians(a0 + (a1 - a0) * i / n)),
        )
        for i in range(n + 1)
    ]


def sym(half):
    """A symmetric outline in a 100-wide box from its right half (top centre to bottom centre)."""
    return half + [(100 - x, y) for x, y in reversed(half)]


def round_poly(pts, r, n=7):
    """Closed polygon with every corner rounded by a quadratic curve of reach `r`."""
    out = []
    k = len(pts)
    for i in range(k):
        a, b, c = pts[i - 1], pts[i], pts[(i + 1) % k]
        la, lc = math.dist(a, b), math.dist(c, b)
        t = min(r, la * 0.5, lc * 0.5)
        p1 = (b[0] + (a[0] - b[0]) / la * t, b[1] + (a[1] - b[1]) / la * t)
        p2 = (b[0] + (c[0] - b[0]) / lc * t, b[1] + (c[1] - b[1]) / lc * t)
        for j in range(n + 1):
            s = j / n
            u = 1 - s
            out.append(
                (
                    u * u * p1[0] + 2 * u * s * b[0] + s * s * p2[0],
                    u * u * p1[1] + 2 * u * s * b[1] + s * s * p2[1],
                )
            )
    return out


def resample(pts, step):
    """Evenly spaced points along a closed polygon."""
    closed = pts + [pts[0]]
    total = sum(math.dist(closed[i], closed[i + 1]) for i in range(len(pts)))
    count = max(8, int(total / step))
    out = []
    seg, acc = 0, 0.0
    seg_len = math.dist(closed[0], closed[1])
    for i in range(count):
        d = total * i / count
        while acc + seg_len < d and seg < len(pts) - 1:
            acc += seg_len
            seg += 1
            seg_len = math.dist(closed[seg], closed[seg + 1])
        f = 0 if seg_len == 0 else (d - acc) / seg_len
        a, b = closed[seg], closed[seg + 1]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out


def with_normals(pts):
    """[(point, tangent, inward normal)] along a closed polygon."""
    area = sum(
        pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1]
        for i in range(len(pts))
    )
    flip = 1 if area > 0 else -1
    out = []
    k = len(pts)
    for i in range(k):
        a, b = pts[i - 1], pts[(i + 1) % k]
        tx, ty = b[0] - a[0], b[1] - a[1]
        n = math.hypot(tx, ty) or 1.0
        tx, ty = tx / n, ty / n
        out.append((pts[i], (tx, ty), (-ty * flip, tx * flip)))
    return out


def inset(pts, d):
    return [(p[0] + n[0] * d, p[1] + n[1] * d) for p, _, n in with_normals(pts)]


def arc_band(cx, cy, r0, r1, a0, a1, n=40):
    """Annular sector outline between radii r0 < r1 and angles a0..a1 (degrees, y down)."""
    outer = ellipse_pts(cx, cy, r1, r1, n, a0, a1)
    inner = ellipse_pts(cx, cy, r0, r0, n, a1, a0)
    return outer + inner


# ---------------------------------------------------------------- image helpers


def clamp8(v):
    return max(0, min(255, int(v)))


def shade(c, k):
    return tuple(clamp8(v * k) for v in c)


def mix(a, b, t):
    return tuple(clamp8(a[i] * (1 - t) + b[i] * t) for i in range(3))


def noise(rng, blur=0.0, w=W, h=W):
    """Deterministic grey noise, stretched to the full range."""
    img = Image.frombytes("L", (w, h), rng.randbytes(w * h))
    if blur:
        img = img.filter(ImageFilter.GaussianBlur(blur))
    lo, hi = img.getextrema()
    span = max(1, hi - lo)
    return img.point(lambda v: int((v - lo) * 255 / span))


def blank():
    return Image.new("L", (W, W), 0)


def solid(color):
    return Image.new("RGB", (W, W), color)


def erode(mask, d):
    """Mask shrunk by ~d working pixels (blur and threshold; keeps edges smooth)."""
    if d <= 0:
        return mask
    s = d / 2.05
    return mask.filter(ImageFilter.GaussianBlur(s)).point(lambda v: 255 if v >= 250 else 0)


def dilate(mask, d):
    s = d / 2.05
    return mask.filter(ImageFilter.GaussianBlur(s)).point(lambda v: 255 if v >= 5 else 0)


def smooth(mask, r=0.7):
    return mask.filter(ImageFilter.GaussianBlur(r))


def scaled_mask(mask, k):
    return mask.point(lambda v: int(v * k))


def lighten(img, mask, k):
    """Lighten `img` towards white by k where `mask` is set."""
    return Image.composite(img.point(lambda v: v + (255 - v) * k), img, mask)


def darken(img, mask, k):
    return Image.composite(img.point(lambda v: v * k), img, mask)


def offset(mask, dx, dy):
    return ImageChops.offset(mask, int(dx * SS), int(dy * SS))


# ---------------------------------------------------------------- fabric


def weave_tile(cell):
    """A plain-weave tile: alternating warp/weft threads with a rounded highlight."""
    n = cell * 2
    tile = Image.new("L", (n, n))
    px = tile.load()
    for j in range(n):
        for i in range(n):
            ci, cj = i // cell, j // cell
            u, v = (i % cell + 0.5) / cell, (j % cell + 0.5) / cell
            if (ci + cj) % 2 == 0:
                val = 128 + 46 * math.sin(math.pi * u) - 16
            else:
                val = 128 + 46 * math.sin(math.pi * v) - 16
            px[i, j] = clamp8(val)
    return tile


def tiled(tile):
    n = tile.width
    row = Image.new("L", (W + n, n))
    for x in range(0, W + n, n):
        row.paste(tile, (x, 0))
    full = Image.new("L", (W, W + n))
    for y in range(0, W + n, n):
        full.paste(row.crop((0, 0, W, n)), (0, y))
    return full.crop((0, 0, W, W))


def fabric(color, rng, cell=3, grain=0.5):
    """Woven field: weave, fibre noise and soft mottling over `color`."""
    tex = tiled(weave_tile(cell))
    fine = noise(rng, 0.5)
    tex = Image.blend(tex, fine, grain * 0.5)
    mottle = noise(rng, 14).point(lambda v: 118 + v * 22 // 255)
    tex = ImageChops.overlay(tex, mottle)
    return ImageChops.overlay(solid(color), tex.convert("RGB"))


def hatch(angle, spacing, rng, thread=2.0, jitter=0.3, seg=(10, 26)):
    """Parallel stitched threads as brightness 0..255 (gaps between threads are dark)."""
    d = int(W * 1.5)
    img = Image.new("L", (d, d), 0)
    draw = ImageDraw.Draw(img)
    y = 0.0
    while y < d:
        x = -rng.randint(0, seg[1])
        while x < d:
            length = rng.randint(*seg)
            v = int(255 * (1 - jitter * rng.random()))
            draw.line([(x, y), (x + length, y)], fill=v, width=max(1, round(thread)))
            x += length + rng.randint(0, 2)
        y += spacing
    img = img.rotate(angle, resample=Image.BICUBIC)
    o = (d - W) // 2
    return img.crop((o, o, o + W, o + W))


def thread_image(color, angle, rng, spacing=3.2, thread=2.2, jitter=0.3):
    h = hatch(angle, spacing, rng, thread, jitter)
    gain = h.point(lambda v: 118 + v * 137 // 255).convert("RGB")
    return ImageChops.multiply(solid(color), gain)


# ---------------------------------------------------------------- embroidery


def stitch(cloth, mask, color, rng, angle=45.0, outline=None, spacing=3.2, emboss=1.0):
    """Embroider `mask` onto `cloth`: cast shadow, satin-stitch fill, outline thread, rim light."""
    m = smooth(mask, 0.6)
    shadow = offset(mask, 1.6, 2.2).filter(ImageFilter.GaussianBlur(2.4 * SS / 2))
    cloth = darken(cloth, scaled_mask(shadow, 0.62 * emboss), 0.5)
    fill = thread_image(color, angle, rng, spacing, spacing * 0.7)
    cloth = Image.composite(fill, cloth, m)
    if outline is not False:
        edge = ImageChops.subtract(mask, erode(mask, 2.2 * SS / 2 + 1))
        oc = outline if outline is not None else shade(color, 0.52)
        thr = thread_image(oc, angle + 90, rng, 2.4, 1.6, 0.2)
        cloth = Image.composite(thr, cloth, smooth(edge, 0.5))
    hl = ImageChops.subtract(mask, offset(mask, 1.4, 1.8))
    cloth = lighten(cloth, scaled_mask(smooth(hl, 0.8), 0.55 * emboss), 0.5)
    lo = ImageChops.subtract(offset(mask, 1.2, 1.5), mask)
    return darken(cloth, scaled_mask(smooth(lo, 0.8), 0.0), 1.0)


# ---------------------------------------------------------------- shape primitives
# A layer is a list of primitives drawn into a mask through a Box transform.


@dataclass
class Box:
    """Maps a 100x100 symbol box onto the 256x256 emblem: centre (cx, cy), side `size`."""

    cx: float
    cy: float
    size: float

    def __call__(self, p):
        k = self.size / 100.0
        return ((self.cx + (p[0] - 50) * k) * SS, (self.cy + (p[1] - 50) * k) * SS)

    def length(self, v):
        return v * self.size / 100.0 * SS


class Absolute(Box):
    def __init__(self):
        super().__init__(128, 128, 100)

    def __call__(self, p):
        return (p[0] * SS, p[1] * SS)

    def length(self, v):
        return v * SS


def poly(pts, fill=255):
    return ("poly", pts, fill)


def ell(cx, cy, rx, ry, fill=255):
    return ("ell", (cx, cy, rx, ry), fill)


def line(pts, width, fill=255):
    return ("line", (pts, width), fill)


def draw_prims(prims, tf):
    mask = blank()
    d = ImageDraw.Draw(mask)
    for kind, arg, fill in prims:
        if kind == "poly":
            d.polygon([tf(p) for p in arg], fill=fill)
        elif kind == "ell":
            cx, cy, rx, ry = arg
            a, b = tf((cx - rx, cy - ry)), tf((cx + rx, cy + ry))
            d.ellipse([a, b], fill=fill)
        elif kind == "line":
            pts, width = arg
            w = max(1, round(tf.length(width)))
            q = [tf(p) for p in pts]
            d.line(q, fill=fill, width=w, joint="curve")
            r = w / 2
            for x, y in (q[0], q[-1]):
                d.ellipse([x - r, y - r, x + r, y + r], fill=fill)
    return mask


def star_pts(cx, cy, ro, ri, n, rot=-90.0):
    out = []
    for i in range(n * 2):
        r = ro if i % 2 == 0 else ri
        a = math.radians(rot + 180.0 * i / n)
        out.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return out


def taper_line(pts, w0, w1):
    """Outline polygon of a polyline whose width goes from w0 to w1."""
    left, right = [], []
    k = len(pts)
    for i, p in enumerate(pts):
        a, b = pts[max(0, i - 1)], pts[min(k - 1, i + 1)]
        tx, ty = b[0] - a[0], b[1] - a[1]
        n = math.hypot(tx, ty) or 1.0
        nx, ny = -ty / n, tx / n
        w = (w0 + (w1 - w0) * i / (k - 1)) / 2
        left.append((p[0] + nx * w, p[1] + ny * w))
        right.append((p[0] - nx * w, p[1] - ny * w))
    return left + right[::-1]


@dataclass
class Layer:
    color: Rgb
    prims: list
    angle: float = 45.0
    outline: Rgb | None | bool = None
    spacing: float = 3.2
    absolute: bool = False


# ---------------------------------------------------------------- symbols (100x100 box)

WOLF_HALF = [
    (50, 27),
    (58, 24),
    (63, 25),
    (73, 3),
    (85, 37),
    (95, 55),
    (84, 57),
    (93, 73),
    (80, 71),
    (71, 85),
    (62, 91),
    (56, 99),
]


def wolf_head(light, dark, mid, ox=0.0, oy=0.0, k=1.0):
    """Front-facing wolf head; returns layers in a 100 box scaled by k about its centre."""

    def T(pts):
        return [(50 + (x - 50) * k + ox, 50 + (y - 50) * k + oy) for x, y in pts]

    face = T(sym(WOLF_HALF))
    ear_l = T([(37, 25), (27, 6), (16, 38), (28, 36)])
    ear_r = T([(63, 25), (73, 6), (84, 38), (72, 36)])
    muzzle = T([(41, 62), (59, 62), (60, 82), (50, 92), (40, 82)])
    eye_l = T([(24, 46), (43, 52), (41, 57), (26, 52)])
    eye_r = [(100 - x, y) for x, y in eye_l]
    nose = T([(43, 85), (57, 85), (50, 96)])
    brow = [T([(50, 30), (50, 52)])]
    cheek_l = T([(14, 56), (30, 62), (22, 72)])
    cheek_r = [(100 - x, y) for x, y in cheek_l]
    return [
        Layer(light, [poly(face)], 75, shade(light, 0.5)),
        Layer(dark, [poly(ear_l[:3] + [ear_l[3]]), poly(ear_r)], 80, False, 2.8),
        Layer(mid, [poly(muzzle)], 100, False, 2.6),
        Layer(dark, [poly(eye_l), poly(eye_r), poly(nose), poly(cheek_l), poly(cheek_r)], 20, False, 2.4),
        Layer(dark, [line(b, 3.0) for b in brow], 90, False, 2.4),
    ]


def sym_wolf_flat(light, dark, mid, k=1.0):
    return wolf_head(light, dark, mid, k=k)


def tiger_head(face, stripe, eye, nose):
    """A snarling tiger head, front on: angular, pointed ears, spiked cheeks, narrowed eyes."""

    def both(pts):
        return [poly(pts), poly([(100 - x, y) for x, y in pts])]

    half = [
        (50, 24),
        (60, 23),
        (68, 11),
        (77, 9),
        (85, 26),
        (95, 36),
        (99, 53),
        (88, 56),
        (97, 70),
        (82, 72),
        (72, 88),
        (58, 96),
        (50, 97),
    ]
    head = sym(half)
    inner_ears = both([(69, 17), (77, 17), (80, 28), (67, 26)])
    stripes = (
        [poly([(50, 24), (45, 33), (50, 46), (55, 33)])]
        + both([(37, 26), (44, 38), (33, 36)])
        + both([(25, 31), (37, 43), (22, 41)])
        + both([(2, 49), (25, 58), (4, 62)])
        + both([(4, 65), (28, 67), (9, 76)])
        + both([(17, 77), (33, 77), (24, 86)])
    )
    brows = both([(19, 40), (46, 49), (45, 53), (22, 46)])
    eyes = both([(25, 48), (43, 56), (41, 60), (28, 55)])
    muzzle = [poly([(36, 66), (50, 58), (64, 66), (67, 81), (50, 95), (33, 81)])]
    nose_p = [poly([(42, 64), (58, 64), (50, 74)])]
    mouth = [poly([(36, 81), (50, 77), (64, 81), (58, 90), (50, 92), (42, 90)])]
    fangs = both([(54, 80), (59, 80), (57, 89)])
    return [
        Layer(face, [poly(head)], 75, shade(face, 0.55)),
        Layer(stripe, inner_ears + stripes, 70, False, 2.6),
        Layer(shade(face, 1.08), muzzle, 100, shade(face, 0.55), 2.6),
        Layer(eye, eyes, 0, stripe, 2.2),
        Layer(stripe, brows + nose_p + mouth, 40, False, 2.2),
        Layer(face, fangs, 100, stripe, 2.0),
    ]


def lion_head(mane, face, dark, mid):
    spikes = star_pts(50, 52, 50, 36, 15)
    inner_mane = star_pts(50, 52, 41, 33, 15, -80)
    ears = [ell(27, 29, 9, 9), ell(73, 29, 9, 9)]
    head = [ell(50, 54, 24, 28), poly([(34, 66), (66, 66), (58, 86), (42, 86)])]
    muzzle = [ell(50, 68, 13, 10)]
    eyes = [poly([(33, 47), (45, 50), (43, 54), (34, 52)]), poly([(67, 47), (55, 50), (57, 54), (66, 52)])]
    nose = [poly([(44, 60), (56, 60), (50, 67)])]
    mouth = [line([(50, 67), (50, 74), (43, 77)], 2.0), line([(50, 74), (57, 77)], 2.0)]
    brow = [line([(50, 30), (50, 46)], 3.0)]
    return [
        Layer(mane, [poly(spikes)], 60, shade(mane, 0.5), 3.0),
        Layer(shade(mane, 0.78), [poly(inner_mane)], 120, False, 3.0),
        Layer(mid, ears, 90, shade(mid, 0.5), 2.8),
        Layer(face, head, 80, shade(face, 0.55), 2.8),
        Layer(shade(face, 1.1), muzzle, 100, False, 2.6),
        Layer(dark, eyes + nose + mouth + brow, 20, False, 2.2),
    ]


def howling_wolf(dark, light, moon):
    """A wolf sitting in profile, head thrown back, howling at a crescent moon."""
    neck = taper_line(bezier((50, 66), (52, 54), (52, 42), (56, 28), 16), 26, 13)
    tail = taper_line(bezier((24, 86), (10, 86), (2, 76), (4, 60), 20), 11, 3)
    muzzle_up = taper_line([(58, 22), (68, 12), (78, 5)], 10, 3.5)
    muzzle_low = taper_line([(60, 29), (68, 25), (76, 18)], 7, 2.5)
    ear = [(48, 18), (51, 3), (60, 14)]
    ruff = [(44, 60), (52, 36), (60, 62), (58, 74)]
    body = [
        ell(38, 76, 22, 17),
        ell(34, 90, 18, 6),
        poly(neck),
        poly(tail),
        ell(57, 24, 9.5, 8.5),
        poly(muzzle_up),
        poly(muzzle_low),
        poly(ear),
        poly(ruff),
        line([(60, 58), (60, 92)], 9),
        ell(62, 94, 8, 3.2),
    ]
    moon_prims = [ell(87, 24, 11, 11), ell(92, 20, 10, 10, 0)]
    fur = [
        line([(34, 62), (46, 50), (52, 38)], 1.8),
        line([(26, 74), (38, 66)], 1.8),
        line([(28, 84), (40, 78)], 1.8),
    ]
    return [
        Layer(moon, moon_prims, 70, shade(moon, 0.6)),
        Layer(dark, body, 60, shade(light, 0.4), 2.8),
        Layer(light, fur, 100, False, 2.2),
    ]


def fist_gear(gear, ring_in, fist, dark):
    teeth = []
    n = 10
    for i in range(n):
        a = 360 / n * i
        w = 360 / n * 0.27
        teeth.append(
            poly(
                [
                    (50 + 38 * math.cos(math.radians(a - w * 1.5)), 50 + 38 * math.sin(math.radians(a - w * 1.5))),
                    (50 + 49 * math.cos(math.radians(a - w)), 50 + 49 * math.sin(math.radians(a - w))),
                    (50 + 49 * math.cos(math.radians(a + w)), 50 + 49 * math.sin(math.radians(a + w))),
                    (50 + 38 * math.cos(math.radians(a + w * 1.5)), 50 + 38 * math.sin(math.radians(a + w * 1.5))),
                ]
            )
        )
    gear_prims = [ell(50, 50, 41, 41)] + teeth + [ell(50, 50, 31, 31, 0)]
    fist_prims = [
        poly([(33, 52), (68, 52), (70, 76), (64, 84), (36, 84), (30, 76)]),
        ell(38, 49, 7, 7),
        ell(47, 46, 7, 7),
        ell(56, 46, 7, 7),
        ell(65, 49, 7, 7),
        poly([(40, 80), (60, 80), (60, 92), (40, 92)]),
    ]
    thumb = [poly([(30, 60), (58, 64), (60, 72), (34, 72)]), ell(32, 66, 6, 6)]
    lines = [
        line([(43, 48), (43, 58)], 1.8),
        line([(52, 46), (52, 58)], 1.8),
        line([(61, 47), (61, 58)], 1.8),
        line([(37, 58), (68, 59)], 1.8),
    ]
    return [
        Layer(gear, gear_prims, 45, shade(gear, 0.55), 3.0),
        Layer(ring_in, [ell(50, 50, 29, 29)], 100, False, 3.2),
        Layer(fist, fist_prims, 80, shade(fist, 0.5), 2.8),
        Layer(shade(fist, 0.85), thumb, 20, shade(fist, 0.5), 2.6),
        Layer(dark, lines, 0, False, 2.2),
    ]


def sabres(blade, hilt, star):
    def one(mirror):
        def m(p):
            return (100 - p[0], p[1]) if mirror else p

        curve = bezier((27, 78), (36, 58), (58, 32), (86, 6), 28)
        blade_pts = [m(p) for p in taper_line(curve, 9.0, 2.5)]
        guard = line([m((17, 76)), m((35, 83))], 4.0)
        grip = line([m((25, 82)), m((20, 94))], 5.5)
        pommel = ell(*m((19, 95)), 4.2, 4.2)
        return blade_pts, guard, grip, pommel

    b1, g1, r1, p1 = one(False)
    b2, g2, r2, p2 = one(True)
    northstar = star_pts(50, 21, 17, 4, 4)
    diag = star_pts(50, 21, 10, 3.5, 4, -45)
    return [
        Layer(blade, [poly(b1), poly(b2)], 20, shade(blade, 0.5), 2.8),
        Layer(hilt, [g1, g2, r1, r2, p1, p2], 90, shade(hilt, 0.5), 2.8),
        Layer(star, [poly(northstar), poly(diag)], 70, shade(star, 0.55), 2.6),
    ]


def harmattan_symbol(crescent, spear, swirl, steel):
    swirls = []
    for cx, cy, flip in ((14, 82, 1), (88, 78, -1), (90, 22, -1), (10, 20, 1)):
        pts = []
        for i in range(40):
            t = i / 39
            a = t * 4.2 * math.pi
            r = 1.5 + 9.5 * t
            pts.append((cx + flip * r * math.cos(a), cy + r * math.sin(a)))
        swirls.append(line(pts, 2.6))
    tail = [
        line(bezier((0, 58), (10, 50), (14, 62), (24, 56), 16), 2.4),
        line(bezier((76, 94), (86, 88), (90, 98), (100, 92), 16), 2.4),
    ]
    moon = [ell(46, 54, 34, 34), ell(61, 45, 27, 27, 0)]
    shaft = [line([(20, 98), (74, 14)], 4.6)]
    head = [poly([(79, 3), (86, 22), (79, 20), (72, 22)])]
    head = [poly(taper_line([(69, 28), (78, 0)], 15, 1.2))]
    return [
        Layer(swirl, swirls + tail, 30, False, 2.6),
        Layer(crescent, moon, 60, shade(crescent, 0.55)),
        Layer(spear, shaft, 90, shade(spear, 0.5), 2.6),
        Layer(steel, head, 90, shade(steel, 0.5), 2.4),
    ]


def tianlang_symbol(sun, wolf_light, wolf_dark, wolf_mid, mountain, snow):
    massif = [(0, 100), (12, 76), (23, 82), (37, 63), (50, 46), (63, 63), (77, 82), (88, 74), (100, 100)]
    lit = [(50, 46), (63, 63), (77, 82), (88, 74), (100, 100), (58, 100)]
    snowcap = [(50, 46), (59, 58), (53, 56), (50, 64), (45, 56), (40, 58)]
    snow_l = [(37, 63), (43, 70), (38, 69), (33, 74), (29, 70)]
    layers = [
        Layer(sun, [ell(50, 36, 34, 34)], 60, shade(sun, 0.55)),
        Layer(mountain, [poly(massif)], 115, shade(mountain, 0.5), 3.0),
        Layer(shade(mountain, 1.25), [poly(lit)], 65, False, 3.0),
        Layer(snow, [poly(snowcap), poly(snow_l)], 70, shade(snow, 0.55), 2.6),
    ]
    layers += wolf_head(wolf_light, wolf_dark, wolf_mid, oy=-27, k=0.66)
    return layers


def volkov_symbol(star, wolf_light, wolf_dark, wolf_mid):
    layers = [Layer(star, [poly(star_pts(50, 53, 54, 22, 5))], 70, shade(star, 0.5), 3.0)]
    layers += wolf_head(wolf_light, wolf_dark, wolf_mid, oy=2, k=0.78)
    return layers


def ranger_symbol(chevron):
    prims = []
    for oy in (0, 24, 48):
        prims.append(poly([(50, 4 + oy), (92, 34 + oy), (92, 47 + oy), (50, 17 + oy), (8, 47 + oy), (8, 34 + oy)]))
    return [Layer(chevron, prims, 135, shade(chevron, 0.45), 3.0)]


# ---------------------------------------------------------------- pieces


def mask_of(pts):
    m = blank()
    ImageDraw.Draw(m).polygon(sc(pts), fill=255)
    return m


def piece(pts, field_color, border_color, rng, border=8.0, cell=3):
    """A cloth piece: woven field with a raised merrow border. Returns (rgb, mask)."""
    pts = resample(pts, 1.0)
    mask = mask_of(pts)
    cloth = fabric(field_color, rng, cell)
    if border <= 0:  # cut edge: no merrow, just the woven field
        return cloth, mask, pts
    bw = border * SS
    inner = erode(mask, bw)
    band = ImageChops.subtract(mask, inner)

    # Border: base thread, ribs across the band, dashed highlight along it.
    ring = fabric(shade(border_color, 0.62), rng, cell)
    layer = ImageDraw.Draw(ring)
    sp = resample(pts, 1.0)
    step = 2.2 * SS
    rib = resample(pts, 1.4)
    walk = with_normals(resample(pts, step / SS))
    for i, (p, t, n) in enumerate(walk):
        k = rng.uniform(0.82, 1.12)
        c = shade(border_color, k)
        a = (p[0] * SS + n[0] * bw * 0.04 - t[0] * bw * 0.32, p[1] * SS + n[1] * bw * 0.04 - t[1] * bw * 0.32)
        b = (p[0] * SS + n[0] * bw * 0.98 + t[0] * bw * 0.32, p[1] * SS + n[1] * bw * 0.98 + t[1] * bw * 0.32)
        layer.line([a, b], fill=c, width=max(2, round(1.7 * SS)))
        # shadowed gap between ribs
        g0 = (a[0] + t[0] * step * 0.55, a[1] + t[1] * step * 0.55)
        g1 = (b[0] + t[0] * step * 0.55, b[1] + t[1] * step * 0.55)
        layer.line([g0, g1], fill=shade(border_color, 0.45), width=max(1, round(0.8 * SS)))
    dash = resample(pts, 1.0)
    walk2 = with_normals(resample(pts, 1.0))
    pos = 0.0
    on = True
    for p, t, n in walk2:
        pos += 1.0
        period = 7.0 if on else 5.0
        if pos >= period:
            pos = 0.0
            on = not on
        if on:
            q = (p[0] * SS + n[0] * bw * 0.52, p[1] * SS + n[1] * bw * 0.52)
            layer.ellipse([q[0] - 1.3 * SS / 2, q[1] - 1.3 * SS / 2, q[0] + 1.3 * SS / 2, q[1] + 1.3 * SS / 2], fill=shade(border_color, 1.35))
    # Inner edge stitch line where the border meets the field.
    ring_edge = ImageChops.subtract(inner, erode(inner, 1.4 * SS))
    ring = Image.composite(solid(shade(border_color, 0.4)), ring, smooth(ring_edge, 0.6))

    out = Image.composite(ring, cloth, band)
    # Raised border: light on the top-left edge, shadow cast into the field.
    cast = smooth(ImageChops.multiply(offset(band, 1.8, 2.2), inner), 2.6)
    out = darken(out, scaled_mask(cast, 0.55), 0.55)
    hl = ImageChops.subtract(band, offset(band, 1.4, 1.8))
    out = lighten(out, scaled_mask(smooth(hl, 0.9), 0.5), 0.45)
    return out, mask, pts


@dataclass
class Shape:
    """Patch silhouette: body outline, optional name tab outline, and where the name goes.

    `name` is (target, kind, params): target "tab" or "body"; kind "rect" (params: one rect per
    text line) or "arc" (params: cx, cy, r, mid_deg, outward, max_arc_deg, size)."""

    body: list
    tab: list | None
    name: tuple


def rosette(cx, cy, r, depth, bumps, n=360):
    """A circle with scalloped edge: `bumps` round lobes `depth` deep."""
    out = []
    for i in range(n):
        t = 2 * math.pi * i / n
        rr = r - depth * (1 - abs(math.cos(bumps * t / 2)))
        out.append((cx + rr * math.cos(t), cy + rr * math.sin(t)))
    return out


def shape_def(name):
    if name == "shield":  # heater shield, straight tab below
        top = [(30, 14), (226, 14)]
        right = bezier((226, 14), (226, 108), (190, 164), (128, 190), 30)
        left = bezier((128, 190), (66, 164), (30, 108), (30, 14), 30)
        body = round_poly(top + right[1:] + left[1:-1], 5)
        tab = round_poly([(34, 198), (222, 198), (222, 240), (34, 240)], 7)
        return Shape(body, tab, ("tab", "rect", [(44, 205, 212, 233)]))
    if name == "circle":  # circle, curved rocker below
        circle = ellipse_pts(128, 100, 86, 86, 120)[:-1]
        band = round_poly(arc_band(128, 100, 98, 127, 32, 148, 60), 6)
        return Shape(circle, band, ("tab", "arc", (128, 100, 112.5, 90, False, 104, 20)))
    if name == "arrow":  # arrowhead pentagon pointing down, curved rocker above
        body = round_poly([(36, 64), (220, 64), (220, 150), (128, 246), (36, 150)], 9)
        band = round_poly(arc_band(128, 320, 262, 290, -112.5, -67.5, 60), 6)
        return Shape(body, band, ("tab", "arc", (128, 320, 276, -90, True, 40, 21)))
    if name == "diamond":  # diamond, wide straight tab below
        body = round_poly([(128, 4), (232, 98), (128, 204), (24, 98)], 10)
        tab = round_poly([(50, 212), (206, 212), (206, 246), (50, 246)], 7)
        return Shape(body, tab, ("tab", "rect", [(58, 217, 198, 241)]))
    if name == "banner":  # wide landscape rectangle, no tab, name inside beside the symbol
        body = round_poly([(8, 48), (248, 48), (248, 198), (8, 198)], 10)
        return Shape(body, None, ("body", "rect", [(142, 78, 238, 116), (142, 128, 238, 166)]))
    if name == "octagon":  # chamfered portrait plate, name in a strip at the bottom
        pts = [(64, 10), (192, 10), (236, 52), (236, 172), (192, 214), (64, 214), (20, 172), (20, 52)]
        return Shape(round_poly(pts, 8), None, ("body", "rect", [(72, 180, 184, 202)]))
    if name == "arch":  # tombstone arch, flat bottom
        pts = ellipse_pts(128, 94, 90, 90, 48, 180, 360) + [(218, 236), (38, 236)]
        return Shape(round_poly(pts, 10), None, ("body", "rect", [(60, 186, 196, 212)]))
    if name == "rosette":  # scalloped sunburst, name on an arc inside
        return Shape(rosette(128, 128, 100, 11, 10), None, ("body", "arc", (128, 128, 67, 90, False, 110, 17)))
    # "swallowtail": tall pennant with a V cut in the bottom, name across the top
    body = round_poly([(26, 8), (230, 8), (230, 238), (128, 198), (26, 238)], 6)
    return Shape(body, None, ("body", "rect", [(50, 24, 206, 48)]))


@dataclass
class Spec:
    id: str
    text: str
    shape: str
    field: Rgb
    border: Rgb
    tab_field: Rgb
    tab_border: Rgb
    text_color: Rgb
    symbol: object  # callable -> layers
    box: tuple = (128, 98, 118)
    seed: int = 1
    bw: float = 9.0  # merrow width in px; 0 = cut edge (no merrow)
    tab_bw: float = 5.0
    inner_line: Rgb | None = None  # second stitch line inside the border
    inner_d: float = 15.0
    dashed: bool = False  # inner line as a running stitch


def text_mask_straight(text, rect, rng):
    x0, y0, x1, y1 = rect
    width, height = (x1 - x0) * SS, (y1 - y0) * SS
    size = int(height * 0.9)
    while size > 8:
        font = ImageFont.truetype(str(FONT), size)
        track = size * 0.07
        length = sum(font.getlength(c) + track for c in text) - track
        asc, desc = font.getmetrics()
        if length <= width and (asc - desc * 0.2) <= height:
            break
        size -= 1
    m = blank()
    d = ImageDraw.Draw(m)
    x = x0 * SS + (width - length) / 2
    cap = font.getbbox("H")
    y = y0 * SS + (height - (cap[3] - cap[1])) / 2 - cap[1]
    for c in text:
        d.text((x, y), c, font=font, fill=255)
        x += font.getlength(c) + track
    return m


def text_mask_arc(text, cx, cy, r, mid_deg, outward, max_arc_deg, size_final):
    """Text along an arc about (cx, cy) with baseline-centre radius `r` (final px).

    outward=False: glyph tops point at the centre (reads left to right along the bottom);
    outward=True: tops point away (reads left to right along the top)."""
    size = int(size_final * SS)
    while size > 8:
        font = ImageFont.truetype(str(FONT), size)
        track = size * 0.07
        advances = [font.getlength(c) + track for c in text]
        total = sum(advances) - track
        if math.degrees(total / (r * SS)) <= max_arc_deg:
            break
        size -= 1
    cap = font.getbbox("H")
    cap_h = cap[3] - cap[1]
    m = blank()
    ang = math.radians(mid_deg)
    # Walk left to right: angle decreases along the bottom, increases along the top.
    direction = 1 if outward else -1
    start = ang - direction * (total / (r * SS)) / 2
    pos = 0.0
    for c, adv in zip(text, advances):
        a = start + direction * (pos + (adv - track) / 2) / (r * SS)
        gw = int(adv + size)
        glyph = Image.new("L", (gw * 2, size * 3), 0)
        gd = ImageDraw.Draw(glyph)
        gd.text((gw, size * 1.5), c, font=font, fill=255, anchor="ms")
        gd.rectangle([0, 0, 0, 0], fill=0)
        # anchor "ms": baseline at (gw, 1.5*size); shift so the cap-height middle is the centre
        shift = cap_h / 2
        rot = -(math.degrees(a) + 90) if outward else (90 - math.degrees(a))
        glyph = glyph.rotate(rot, resample=Image.BICUBIC, center=(gw, size * 1.5 - shift))
        px = cx * SS + r * SS * math.cos(a)
        py = cy * SS + r * SS * math.sin(a)
        m.paste(255, (int(px - gw), int(py - (size * 1.5 - shift))), glyph)
        pos += adv
    return m


def name_mask(text, name):
    _, kind, p = name
    if kind == "arc":
        return text_mask_arc(text.replace("\n", " "), *p)
    lines = text.split("\n") if len(p) > 1 else [text.replace("\n", " ")]
    m = blank()
    for line_text, rect in zip(lines, p):
        m = ImageChops.lighter(m, text_mask_straight(line_text, rect, None))
    return m


def build(spec: Spec, rng: random.Random):
    shape = shape_def(spec.shape)
    canvas = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    tm = name_mask(spec.text, shape.name)
    name_style = dict(angle=90, outline=shade(spec.text_color, 0.55), spacing=2.4, emboss=0.8)

    pieces = []
    tab_piece = None
    if shape.tab:
        tab_rgb, tab_mask, tab_poly = piece(shape.tab, spec.tab_field, spec.tab_border, rng, border=spec.tab_bw)
        tab_rgb = stitch(tab_rgb, tm, spec.text_color, rng, **name_style)
        tab_piece = (tab_rgb, tab_mask, tab_poly, spec.tab_border)

    body_rgb, body_mask, body_poly = piece(shape.body, spec.field, spec.border, rng, border=spec.bw)
    if spec.inner_line:
        pts = [(x * SS, y * SS) for x, y in inset(resample(body_poly, 1.5), spec.inner_d)]
        ring = _dashed(pts, 1.8 * SS) if spec.dashed else _stroke(pts, 1.6 * SS)
        body_rgb = stitch(body_rgb, ring, spec.inner_line, rng, angle=0, outline=False, emboss=0.5)

    box = Box(*spec.box)
    for layer in spec.symbol():
        tf = Absolute() if layer.absolute else box
        m = draw_prims(layer.prims, tf)
        body_rgb = stitch(body_rgb, m, layer.color, rng, layer.angle, layer.outline, layer.spacing)
    if shape.name[0] == "body":
        body_rgb = stitch(body_rgb, tm, spec.text_color, rng, **name_style)

    pieces = [(body_rgb, body_mask, body_poly, spec.border)]
    if tab_piece:
        pieces.append(tab_piece)
    for rgb, mask, pts, border in pieces:
        rgb = wear(rgb, mask, rng)
        alpha = fray(mask, rng)
        threads = loose_threads(pts, border, rng)
        # Drop shadow of the piece on the surface below.
        sh = offset(alpha, 1.5, 2.0).filter(ImageFilter.GaussianBlur(2.5 * SS / 2))
        shadow = Image.new("RGBA", (W, W), (8, 8, 6, 0))
        shadow.putalpha(scaled_mask(sh, 0.4))
        canvas = Image.alpha_composite(canvas, shadow)
        layer = rgb.convert("RGBA")
        layer.putalpha(alpha)
        canvas = Image.alpha_composite(canvas, layer)
        canvas = Image.alpha_composite(canvas, threads)

    small = canvas.convert("RGBa").resize((SIZE, SIZE), Image.LANCZOS).convert("RGBA")
    return small


def _stroke(pts, width):
    m = blank()
    d = ImageDraw.Draw(m)
    d.line(pts + [pts[0]], fill=255, width=max(1, round(width)), joint="curve")
    return m


def _dashed(pts, width, on=4, off=3):
    """A running stitch along a closed polyline: `on` points drawn, `off` skipped."""
    m = blank()
    d = ImageDraw.Draw(m)
    i = 0
    while i < len(pts):
        seg = pts[i : i + on + 1]
        if len(seg) > 1:
            d.line(seg, fill=255, width=max(1, round(width)))
        i += on + off
    return m


# ---------------------------------------------------------------- wear


def wear(rgb, mask, rng):
    """Sun fade, desaturation, grime blotches and scuffs, within the piece."""
    rgb = ImageEnhance.Color(rgb).enhance(0.8)
    # Faded patches (bleached by sun): lighter, greyer.
    fade = noise(rng, 38).point(lambda v: max(0, v - 120) * 2).filter(ImageFilter.GaussianBlur(10))
    bleach = mix((210, 200, 180), (170, 170, 165), 0.3)
    rgb = Image.composite(Image.blend(rgb, solid(bleach), 0.22), rgb, scaled_mask(fade, 0.6))
    # Grime: dirty brown blotches that sink into the weave.
    dirt = noise(rng, 22).point(lambda v: max(0, v - 128) * 3).filter(ImageFilter.GaussianBlur(8))
    rgb = Image.composite(ImageChops.multiply(rgb, solid((150, 132, 108))), rgb, scaled_mask(dirt, 0.8))
    # Edge wear: darker, dirtier towards the rim.
    rim = ImageChops.subtract(mask, erode(mask, 14 * SS)).filter(ImageFilter.GaussianBlur(6 * SS))
    rgb = Image.composite(ImageChops.multiply(rgb, solid((178, 164, 140))), rgb, scaled_mask(rim, 0.55))
    # Scuffs: short pale scratches.
    scuffs = Image.new("L", (W, W), 0)
    d = ImageDraw.Draw(scuffs)
    for _ in range(46):
        x, y = rng.uniform(40, W - 40), rng.uniform(40, W - 40)
        a = rng.uniform(0, math.pi)
        ln = rng.uniform(5, 22)
        d.line([(x, y), (x + math.cos(a) * ln, y + math.sin(a) * ln)], fill=rng.randint(60, 140), width=1)
    scuffs = ImageChops.multiply(scuffs, mask)
    rgb = lighten(rgb, scaled_mask(scuffs, 0.32), 0.7)
    # Overall dulling.
    return ImageEnhance.Brightness(rgb).enhance(0.96)


def fray(mask, rng):
    """Ragged alpha: the edge wobbles by a pixel or two, with the odd chewed notch."""
    soft = mask.filter(ImageFilter.GaussianBlur(1.6 * SS))
    jitter = noise(rng, 0.9).point(lambda v: 128 + (v - 128) * 1.1)
    wobble = noise(rng, 5).point(lambda v: 128 + (v - 128) * 0.9)
    a = ImageChops.add(soft, jitter, 1.0, -128)
    a = ImageChops.add(a, wobble, 1.0, -128)
    a = a.point(lambda v: 0 if v < 120 else 255 if v > 136 else (v - 120) * 255 // 16)
    return ImageChops.multiply(a, dilate(mask, 2.5 * SS)).filter(ImageFilter.GaussianBlur(0.4))


def loose_threads(pts, color, rng):
    layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    walk = with_normals(resample(pts, 1.0))
    for p, t, n in walk:
        if rng.random() > 0.07:
            continue
        ln = rng.uniform(3, 9) * SS / 2
        bend = rng.uniform(-0.8, 0.8)
        out = (-n[0] + t[0] * bend, -n[1] + t[1] * bend)
        x0, y0 = p[0] * SS - n[0] * 1.2 * SS, p[1] * SS - n[1] * 1.2 * SS
        mid = (x0 + out[0] * ln * 0.5 + t[0] * rng.uniform(-3, 3), y0 + out[1] * ln * 0.5)
        end = (x0 + out[0] * ln + t[0] * rng.uniform(-5, 5), y0 + out[1] * ln)
        c = shade(color, rng.uniform(0.7, 1.2)) + (rng.randint(110, 200),)
        d.line([(x0, y0), mid, end], fill=c, width=max(1, round(0.9 * SS / 1.5)))
    return layer


# ---------------------------------------------------------------- the roster

SPECS = [
    # TAN: heater shield + straight tab, thick dark merrow.
    Spec(
        "ranger",
        "TASK FORCE RANGER",
        "shield",
        field=(188, 166, 118),
        border=(82, 62, 38),
        tab_field=(70, 52, 32),
        tab_border=(196, 174, 128),
        text_color=(244, 228, 184),
        symbol=lambda: ranger_symbol((60, 44, 26)),
        box=(128, 90, 112),
        seed=11,
        bw=10.0,
        tab_bw=5.0,
    ),
    # OCHRE: circle + rocker, thin merrow.
    Spec(
        "harmattan",
        "HARMATTAN LEGION",
        "circle",
        field=(212, 148, 34),
        border=(104, 50, 22),
        tab_field=(112, 52, 26),
        tab_border=(224, 168, 62),
        text_color=(250, 232, 184),
        symbol=lambda: harmattan_symbol((244, 230, 186), (72, 40, 22), (160, 92, 22), (222, 222, 214)),
        box=(128, 100, 124),
        seed=22,
        bw=4.0,
        tab_bw=3.5,
    ),
    # NAVY: arrowhead + rocker above, double stitch.
    Spec(
        "northern_sabre",
        "NORTHERN SABRE",
        "arrow",
        field=(30, 46, 86),
        border=(190, 200, 210),
        tab_field=(188, 200, 214),
        tab_border=(30, 46, 86),
        text_color=(26, 40, 78),
        symbol=lambda: sabres((222, 228, 234), (214, 170, 74), (246, 244, 232)),
        box=(128, 140, 112),
        seed=33,
        bw=4.5,
        tab_bw=4.0,
        inner_line=(176, 188, 202),
        inner_d=12.0,
    ),
    # OLIVE: diamond + wide tab, thick red merrow.
    Spec(
        "volkov",
        "VOLKOV BATTALION",
        "diamond",
        field=(100, 102, 50),
        border=(170, 38, 32),
        tab_field=(124, 32, 28),
        tab_border=(100, 102, 50),
        text_color=(226, 220, 176),
        symbol=lambda: volkov_symbol((170, 38, 32), (190, 182, 158), (36, 34, 26), (216, 208, 184)),
        box=(128, 106, 108),
        seed=44,
        bw=8.0,
        tab_bw=5.0,
    ),
    # CRIMSON: wide rectangle, no tab, cut edge with a black running stitch.
    Spec(
        "crimson_vanguard",
        "CRIMSON\nVANGUARD",
        "banner",
        field=(152, 28, 36),
        border=(30, 26, 26),
        tab_field=(152, 28, 36),
        tab_border=(30, 26, 26),
        text_color=(240, 226, 208),
        symbol=lambda: fist_gear((26, 24, 24), (118, 24, 30), (226, 214, 196), (40, 28, 28)),
        box=(78, 123, 116),
        seed=99,
        bw=0.0,
        inner_line=(30, 26, 26),
        inner_d=7.0,
        dashed=True,
    ),
    # DARK GREEN: chamfered plate, thin white merrow.
    Spec(
        "baekho",
        "BAEKHO RECON",
        "octagon",
        field=(24, 64, 46),
        border=(214, 216, 204),
        tab_field=(24, 64, 46),
        tab_border=(214, 216, 204),
        text_color=(228, 230, 218),
        symbol=lambda: tiger_head((222, 222, 210), (24, 28, 24), (234, 150, 40), (24, 28, 24)),
        box=(128, 96, 130),
        seed=55,
        bw=4.0,
    ),
    # SNOW WHITE: tombstone arch, red double stitch.
    Spec(
        "tianlang",
        "TIANLANG GROUP",
        "arch",
        field=(226, 228, 224),
        border=(170, 40, 38),
        tab_field=(226, 228, 224),
        tab_border=(170, 40, 38),
        text_color=(150, 36, 34),
        symbol=lambda: tianlang_symbol((176, 42, 40), (64, 72, 88), (238, 240, 240), (132, 142, 156), (88, 104, 126), (236, 240, 242)),
        box=(128, 106, 120),
        seed=66,
        bw=4.5,
        inner_line=(170, 40, 38),
        inner_d=12.0,
    ),
    # BLACK: scalloped rosette, thick gold merrow.
    Spec(
        "simba",
        "SIMBA BRIGADE",
        "rosette",
        field=(30, 28, 26),
        border=(208, 164, 56),
        tab_field=(30, 28, 26),
        tab_border=(208, 164, 56),
        text_color=(232, 196, 100),
        symbol=lambda: lion_head((212, 160, 50), (230, 194, 112), (38, 28, 20), (178, 124, 40)),
        box=(128, 112, 110),
        seed=77,
        bw=10.0,
    ),
    # BROWN: swallowtail pennant, cut edge with a cream running stitch.
    Spec(
        "steppe_wolves",
        "STEPPE WOLVES",
        "swallowtail",
        field=(138, 92, 56),
        border=(70, 48, 30),
        tab_field=(138, 92, 56),
        tab_border=(70, 48, 30),
        text_color=(52, 32, 20),
        symbol=lambda: howling_wolf((34, 24, 18), (206, 176, 124), (238, 224, 184)),
        box=(128, 128, 134),
        seed=88,
        bw=0.0,
        inner_line=(236, 218, 176),
        inner_d=8.0,
        dashed=True,
    ),
]


def contact_sheet(imgs, path):
    """9-up sheet: 128 px row, 32 px row, 24 px row (score bar), 32 px row zoomed 3x (nearest)."""
    pad, cell = 8, 136
    bg = (30, 33, 36, 255)
    n = len(imgs)
    h = pad + 128 + pad + 32 + pad + 24 + pad + 96 + pad
    sheet = Image.new("RGBA", (n * cell + pad, h), bg)

    def shrink(img, s):
        return img.convert("RGBa").resize((s, s), Image.LANCZOS).convert("RGBA")

    y1 = pad
    y2 = y1 + 128 + pad
    y3 = y2 + 32 + pad
    y4 = y3 + 24 + pad
    for i, img in enumerate(imgs):
        x = pad + i * cell
        sheet.alpha_composite(shrink(img, 128), (x, y1))
        s32 = shrink(img, 32)
        sheet.alpha_composite(s32, (x, y2))
        sheet.alpha_composite(shrink(img, 24), (x, y3))
        sheet.alpha_composite(s32.resize((96, 96), Image.NEAREST), (x, y4))
    sheet.convert("RGB").save(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ids", nargs="*", help="Faction ids to render (default: all)")
    ap.add_argument("--out", type=Path, default=OUT_DIR)
    ap.add_argument("--sheet", type=Path, help="also write a 9-up contact sheet (128, 32, 24 px and 32 px zoomed)")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    done = []
    for spec in SPECS:
        if args.ids and spec.id not in args.ids:
            continue
        img = build(spec, random.Random(spec.seed))
        img.save(args.out / f"{spec.id}.png", optimize=True)
        print(f"{spec.id}.png")
        done.append(img)
    if args.sheet and done:
        contact_sheet(done, args.sheet)


if __name__ == "__main__":
    main()
