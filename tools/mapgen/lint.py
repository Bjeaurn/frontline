#!/usr/bin/env python3
"""Z-fighting linter for Valve 220 `.map` files (the generators' output, or a TrenchBroom Map).

Two brush faces z-fight (flicker) when they lie in the same plane, face the same way and
overlap in area: the depth buffer can't tell which is in front. Typical causes are a building
slab sitting flush with the ground, a wall flush with a floor edge, or a trim brush laid exactly
on another brush's face. (Faces that touch back to back, facing opposite ways, are hidden and
never fight; faces that merely share an edge have no overlap.)

    python3 tools/mapgen/lint.py crates/frontline/assets/maps/arena.map [more.map ...]
    python3 tools/mapgen/lint.py --tol 0.5 --min-area 0.01 --verbose some.map

Faces are rebuilt as polygons by clipping each brush plane against its brush's other planes,
so overlap is measured on the real faces, not their planes. Findings are grouped by plane and
surface pair with brush numbers (the `// brush N` of the file, per entity) and a rough position
in game metres (x, y, z; y up). Overlaps buried inside a third brush (a block's top under the
slab above it) and downward faces at or below the ground are not visible and are ignored (see
--include-buried, --ground-y). Flat props (street tiles and decals, from the model's
`bounds.json`) whose top surfaces coincide and overlap are reported too. Exit code 1 if any
pair is found, 2 on a read error.

Only the standard library; positions and tolerances follow the TrenchBroom convention used by
mapgen.py (1 unit = 1 inch, units -> game metres via mapgen.UNITS_PER_METRE).
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from itertools import product
from pathlib import Path

UNITS_PER_METRE = 39.37008  # keep in step with mapgen.py / crates/frontline/src/maps/mod.rs

Vec3 = tuple[float, float, float]

FACE_RE = re.compile(
    r"\(\s*([-\d.eE+]+)\s+([-\d.eE+]+)\s+([-\d.eE+]+)\s*\)\s*"
    r"\(\s*([-\d.eE+]+)\s+([-\d.eE+]+)\s+([-\d.eE+]+)\s*\)\s*"
    r"\(\s*([-\d.eE+]+)\s+([-\d.eE+]+)\s+([-\d.eE+]+)\s*\)\s*(\S+)"
)


def _sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _scale(a: Vec3, s: float) -> Vec3:
    return (a[0] * s, a[1] * s, a[2] * s)


def _lerp(a: Vec3, b: Vec3, t: float) -> Vec3:
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t)


def _unit(a: Vec3) -> Vec3:
    return _scale(a, 1.0 / math.sqrt(_dot(a, a)))


def to_game(p: Vec3) -> Vec3:
    """A point from TrenchBroom space (inches, z up) to game space (metres, y up)."""
    return (-p[1] / UNITS_PER_METRE, p[2] / UNITS_PER_METRE, -p[0] / UNITS_PER_METRE)


@dataclass
class Plane:
    normal: Vec3  # outward, unit, game space
    offset: float  # normal . p == offset on the plane, metres
    texture: str


@dataclass
class Face:
    brush: int  # index into the brush list
    plane: Plane
    polygon: list[Vec3]  # counter-clockwise about the normal, game space
    area: float


@dataclass
class Brush:
    entity: int
    number: int  # the `// brush N` of its entity
    planes: list[Plane]
    faces: list[Face]
    lo: Vec3 = (0.0, 0.0, 0.0)  # bounding box of the faces, game space
    hi: Vec3 = (0.0, 0.0, 0.0)


# ---- reading ---------------------------------------------------------------------------------


@dataclass
class Prop:
    entity: int
    model: str
    origin: Vec3  # feet, game space
    yaw: float  # radians, as `Quat::from_rotation_y`


def read_map(path: Path) -> tuple[list[Brush], list[Prop]]:
    """The brushes of a Valve 220 `.map`, planes only (faces are built by `build_faces`), and its
    `prop` entities."""
    brushes: list[Brush] = []
    props: list[Prop] = []
    keys: dict[str, str] = {}
    entity = -1
    depth = 0
    planes: list[Plane] | None = None
    number = 0
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("//"):
            m = re.match(r"//\s*entity\s+(\d+)", line)
            if m:
                entity = int(m.group(1))
            m = re.match(r"//\s*brush\s+(\d+)", line)
            if m:
                number = int(m.group(1))
            continue
        if line == "{":
            depth += 1
            if depth == 1:
                keys = {}
            if depth == 2:
                planes = []
        elif line == "}":
            if depth == 2 and planes is not None:
                brushes.append(Brush(entity, number, planes, []))
                planes = None
                number += 1
            if depth == 1 and keys.get("classname") == "prop" and "model" in keys:
                x, y, z = (float(v) for v in keys.get("origin", "0 0 0").split())
                props.append(Prop(entity, keys["model"], to_game((x, y, z)), math.radians(float(keys.get("angle", "0")))))
            depth -= 1
        elif depth == 1 and line.startswith('"'):
            m = re.match(r'"([^"]*)"\s+"([^"]*)"', line)
            if m:
                keys[m.group(1)] = m.group(2)
        elif planes is not None and line.startswith("("):
            m = FACE_RE.match(line)
            if not m:
                raise ValueError(f"{path}: unreadable face line: {line[:80]}")
            v = [float(g) for g in m.groups()[:9]]
            p1, p2, p3 = (v[0:3], v[3:6], v[6:9])
            # TrenchBroom space: outward normal = (p3 - p1) x (p2 - p1).
            n_tb = _cross(_sub(tuple(p3), tuple(p1)), _sub(tuple(p2), tuple(p1)))
            length = math.sqrt(_dot(n_tb, n_tb))
            if length == 0.0:
                continue  # a degenerate face; the engine drops it too
            n_tb = _scale(n_tb, 1.0 / length)
            normal = (-n_tb[1], n_tb[2], -n_tb[0])  # game space
            point = to_game(tuple(p1))
            planes.append(Plane(normal, _dot(normal, point), m.group(10)))
    return brushes, props


# ---- geometry --------------------------------------------------------------------------------


def _clip(poly: list[Vec3], normal: Vec3, offset: float, eps: float = 1e-9) -> list[Vec3]:
    """Sutherland-Hodgman: the part of a convex polygon with normal . p <= offset."""
    out: list[Vec3] = []
    for i, a in enumerate(poly):
        b = poly[(i + 1) % len(poly)]
        da = _dot(normal, a) - offset
        db = _dot(normal, b) - offset
        if da <= eps:
            out.append(a)
        if (da <= eps) != (db <= eps) and abs(da - db) > 0.0:
            out.append(_lerp(a, b, da / (da - db)))
    return out


def _polygon_area(poly: list[Vec3], normal: Vec3) -> float:
    if len(poly) < 3:
        return 0.0
    total = (0.0, 0.0, 0.0)
    o = poly[0]
    for i in range(1, len(poly) - 1):
        c = _cross(_sub(poly[i], o), _sub(poly[i + 1], o))
        total = (total[0] + c[0], total[1] + c[1], total[2] + c[2])
    return 0.5 * _dot(total, normal)


def build_faces(brush: Brush, index: int, eps: float = 1e-7) -> None:
    """Fills `brush.faces`: each plane's polygon, clipped by the brush's other planes."""
    for i, plane in enumerate(brush.planes):
        n = plane.normal
        helper = (0.0, 1.0, 0.0) if abs(n[1]) < 0.9 else (1.0, 0.0, 0.0)
        u = _unit(_cross(helper, n))
        v = _cross(n, u)
        centre = _scale(n, plane.offset)
        big = 1.0e4
        poly = [
            tuple(centre[k] + big * (su * u[k] + sv * v[k]) for k in range(3))
            for su, sv in ((-1, -1), (1, -1), (1, 1), (-1, 1))
        ]  # counter-clockwise about n, as u x v = n
        for j, other in enumerate(brush.planes):
            if j == i or not poly:
                continue
            if _dot(other.normal, n) > 1.0 - 1e-9 and abs(other.offset - plane.offset) < eps:
                continue  # a duplicate plane
            poly = _clip(poly, other.normal, other.offset, eps)
        area = _polygon_area(poly, n)
        if len(poly) >= 3 and area > 1e-9:
            brush.faces.append(Face(index, plane, poly, area))
    corners = [p for f in brush.faces for p in f.polygon]
    if corners:
        brush.lo = tuple(min(p[k] for p in corners) for k in range(3))
        brush.hi = tuple(max(p[k] for p in corners) for k in range(3))


def buried(brushes: list[Brush], point: Vec3, skip: tuple[int, int], depth: float) -> bool:
    """Is `point` inside some other brush? A face overlap that is buried can't be seen."""
    for i, b in enumerate(brushes):
        if i in skip:
            continue
        if any(point[k] < b.lo[k] - 1e-6 or point[k] > b.hi[k] + 1e-6 for k in range(3)):
            continue
        if all(_dot(pl.normal, point) <= pl.offset + 1e-6 for pl in b.planes):
            return True
    return False


def overlap(a: Face, b: Face) -> list[Vec3]:
    """The overlap of two coplanar faces (clip `a` by the side planes of `b`)."""
    n = b.plane.normal
    poly = a.polygon
    for i, p in enumerate(b.polygon):
        q = b.polygon[(i + 1) % len(b.polygon)]
        side = _cross(n, _sub(q, p))  # points into the polygon (counter-clockwise about n)
        length = math.sqrt(_dot(side, side))
        if length < 1e-12:
            continue
        side = _scale(side, 1.0 / length)
        # keep the part with side . (x - p) >= 0, i.e. (-side) . x <= -side . p
        poly = _clip(poly, _scale(side, -1.0), -_dot(side, p), 1e-12)
        if len(poly) < 3:
            return []
    return poly


# ---- finding pairs ---------------------------------------------------------------------------


@dataclass
class Finding:
    a: Face
    b: Face
    area: float
    where: Vec3


def find_pairs(brushes: list[Brush], tol: float, min_area: float, hidden: list[Finding] | None = None, ground_y: float | None = 0.0) -> list[Finding]:
    """Same-facing coplanar overlapping face pairs; `tol` is the plane distance tolerance in
    metres, `min_area` the least overlap in square metres. Overlaps whose centre is buried
    inside a third brush (a block's underside resting on the ground) can't be seen and go to
    `hidden` instead when it is given, else are dropped... unless `hidden` is None, then kept."""
    faces = [f for b in brushes for f in b.faces]
    if ground_y is not None:
        # Nobody gets under the ground: downward faces at or below it are never seen.
        faces = [f for f in faces if not (f.plane.normal[1] < -0.5 and max(p[1] for p in f.polygon) <= ground_y + 1e-6)]
    cell = 0.01
    buckets: dict[tuple[int, int, int], list[Face]] = defaultdict(list)
    for f in faces:
        key = tuple(round(c / cell) for c in f.plane.normal)
        buckets[key].append(f)
    # normal tolerance: a face tilted by `tol` over a 100 m run still counts as coplanar enough.
    normal_tol = tol / 100.0
    findings: list[Finding] = []
    seen: set[tuple[int, int, int, int]] = set()
    for f in faces:
        key = tuple(round(c / cell) for c in f.plane.normal)
        for d in product((-1, 0, 1), repeat=3):
            for g in buckets.get((key[0] + d[0], key[1] + d[1], key[2] + d[2]), ()):
                if g.brush <= f.brush:
                    continue  # each pair once; faces of one brush never overlap
                if abs(g.plane.offset - f.plane.offset) > tol:
                    continue
                if _dot(g.plane.normal, f.plane.normal) < 1.0 - normal_tol:
                    continue
                id_ = (f.brush, id(f.plane), g.brush, id(g.plane))
                if id_ in seen:
                    continue
                seen.add(id_)
                poly = overlap(f, g)
                area = _polygon_area(poly, f.plane.normal)
                if area >= min_area:
                    where = tuple(sum(p[k] for p in poly) / len(poly) for k in range(3))
                    finding = Finding(f, g, area, where)  # type: ignore[arg-type]
                    if hidden is not None and buried(brushes, tuple(where[k] + f.plane.normal[k] * 0.02 for k in range(3)), (f.brush, g.brush), 0.02):
                        hidden.append(finding)
                    else:
                        findings.append(finding)
    return findings


# ---- reporting -------------------------------------------------------------------------------


def _facing(normal: Vec3) -> str:
    axis = max(range(3), key=lambda k: abs(normal[k]))
    names = (("-x", "+x"), ("-y (down)", "+y (up)"), ("-z", "+z"))[axis]
    label = names[normal[axis] > 0]
    return label if abs(normal[axis]) > 0.9999 else f"{label}~"


def _surface(texture: str) -> str:
    return texture.split("/")[0]


REPO = Path(__file__).resolve().parents[2]
_BOUNDS: dict[str, dict] = {}


def prop_bounds(model: str) -> tuple[Vec3, Vec3] | None:
    """The (min, max) of a prop's model from its folder's `bounds.json`, or None if unknown."""
    folder, _, name = model.rpartition("/")
    if folder not in _BOUNDS:
        if folder.startswith("private://"):
            path = REPO / "content" / "private" / folder.removeprefix("private://") / "bounds.json"
        else:
            path = REPO / "crates" / "frontline" / "assets" / folder / "bounds.json"
        _BOUNDS[folder] = json.loads(path.read_text()) if path.exists() else {}
    b = _BOUNDS[folder].get(name.removesuffix(".glb"))
    return (tuple(b["min"]), tuple(b["max"])) if b else None  # type: ignore[return-value]


def flat_prop_pairs(props: list[Prop], tol: float, min_area: float) -> list[tuple[Prop, Prop, float, Vec3]]:
    """Pairs of flat props (street tiles, decals: under 5 cm tall) whose top surfaces lie in the
    same plane and overlap, like two asphalt tiles laid over each other: they flicker too."""
    flats = []
    for prop in props:
        bounds = prop_bounds(prop.model)
        if bounds is None or bounds[1][1] - bounds[0][1] > 0.05:
            continue
        (x0, _, z0), (x1, y1, z1) = bounds
        c, n = math.cos(prop.yaw), math.sin(prop.yaw)
        y = prop.origin[1] + y1
        corners = [(prop.origin[0] + x * c + z * n, y, prop.origin[2] - x * n + z * c) for x, z in ((x0, z0), (x1, z0), (x1, z1), (x0, z1))]
        if _polygon_area(corners, (0.0, 1.0, 0.0)) < 0:
            corners.reverse()
        up = Plane((0.0, 1.0, 0.0), y, prop.model)
        flats.append((prop, Face(-1, up, corners, abs(_polygon_area(corners, (0.0, 1.0, 0.0))))))
    found = []
    for i, (pa, fa) in enumerate(flats):
        for pb, fb in flats[i + 1 :]:
            if abs(fa.plane.offset - fb.plane.offset) > tol:
                continue
            poly = overlap(fa, fb)
            area = _polygon_area(poly, (0.0, 1.0, 0.0))
            if area >= min_area:
                found.append((pa, pb, area, tuple(sum(p[k] for p in poly) / len(poly) for k in range(3))))
    return found


def box(b: Brush) -> str:
    return f"x {b.lo[0]:.2f}..{b.hi[0]:.2f} y {b.lo[1]:.2f}..{b.hi[1]:.2f} z {b.lo[2]:.2f}..{b.hi[2]:.2f}"


def lint(path: Path, tol: float, min_area: float, verbose: bool, limit: int, include_buried: bool, ground_y: float | None) -> int:
    brushes, props = read_map(path)
    for i, brush in enumerate(brushes):
        build_faces(brush, i)
    hidden: list[Finding] = []
    findings = find_pairs(brushes, tol, min_area, None if include_buried else hidden, ground_y)
    if hidden:
        print(f"{path}: ({len(hidden)} coplanar pair(s) buried inside other brushes ignored, e.g. undersides on the ground; --include-buried lists them)")
    label = lambda f: f"#{brushes[f.brush].number}" + (f"(e{brushes[f.brush].entity})" if brushes[f.brush].entity else "")
    total_faces = sum(len(b.faces) for b in brushes)
    flat = flat_prop_pairs(props, tol, 0.01)
    for a, b, area, w in flat:
        print(f"  prop overlap: {a.model.rpartition('/')[2]} x {b.model.rpartition('/')[2]}  {area:.2f} m2 at ({w[0]:.1f}, {w[1]:.2f}, {w[2]:.1f}), same height")
    if not findings:
        print(f"{path}: {'clean' if not flat else str(len(flat)) + ' flat prop overlap(s)'} ({len(brushes)} brushes, {total_faces} faces, {len(props)} props)")
        return len(flat)
    groups: dict[tuple, list[Finding]] = defaultdict(list)
    for fd in findings:
        n = fd.a.plane.normal
        offset = round(fd.a.plane.offset, 2)
        pair = tuple(sorted((_surface(fd.a.plane.texture), _surface(fd.b.plane.texture))))
        groups[(_facing(n), offset, pair)].append(fd)
    area = sum(fd.area for fd in findings)
    print(f"{path}: {len(findings) + len(flat)} z-fighting pair(s) in {len(groups)} plane group(s), {area:.1f} m2 overlapping ({len(brushes)} brushes, {total_faces} faces)")
    ordered = sorted(groups.items(), key=lambda kv: -len(kv[1]))
    for (facing, offset, pair), items in ordered[: limit if not verbose else None]:
        xs = [fd.where for fd in items]
        lo = tuple(min(p[k] for p in xs) for k in range(3))
        hi = tuple(max(p[k] for p in xs) for k in range(3))
        extent = f"x {lo[0]:.1f}..{hi[0]:.1f}, y {lo[1]:.2f}..{hi[1]:.2f}, z {lo[2]:.1f}..{hi[2]:.1f}"
        print(f"  {len(items):4d} x {facing:9s} plane {offset:+8.2f} m  {'/'.join(pair):22s} {extent}")
        shown = items if verbose else items[:3]
        for fd in shown:
            w = fd.where
            boxes = f" [{box(brushes[fd.a.brush])} | {box(brushes[fd.b.brush])}]" if verbose else ""
            print(f"        {label(fd.a)} x {label(fd.b)}  {fd.area:.2f} m2 at ({w[0]:.1f}, {w[1]:.2f}, {w[2]:.1f}){boxes}")
        if len(items) > len(shown):
            print(f"        ... and {len(items) - len(shown)} more")
    if len(ordered) > limit and not verbose:
        print(f"  ... and {len(ordered) - limit} more plane group(s) (use --verbose)")
    return len(findings) + len(flat)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("maps", nargs="+", type=Path)
    parser.add_argument("--tol", type=float, default=0.01, help="plane distance tolerance in TrenchBroom units (inches), default 0.01")
    parser.add_argument("--min-area", type=float, default=1e-4, help="least overlap in square metres, default 0.0001")
    parser.add_argument("--limit", type=int, default=25, help="plane groups to list per map, default 25")
    parser.add_argument("--include-buried", action="store_true", help="also report overlaps hidden inside a third brush")
    parser.add_argument("--ground-y", type=float, default=0.0, help="downward faces at or below this height (metres) are ignored, nobody gets under the ground; default 0")
    parser.add_argument("--verbose", action="store_true", help="list every pair")
    args = parser.parse_args()
    bad = 0
    for path in args.maps:
        try:
            bad += lint(path, args.tol / UNITS_PER_METRE, args.min_area, args.verbose, args.limit, args.include_buried, args.ground_y)
        except (OSError, ValueError) as error:
            print(f"{path}: {error}", file=sys.stderr)
            sys.exit(2)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
