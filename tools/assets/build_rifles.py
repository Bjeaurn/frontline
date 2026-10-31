"""Builds the first-person M4 and M16A4 models (`m4.glb`, `m16.glb`) in
crates/frontline/assets/models/weapons/ from primitives, with no input files:

    python3 tools/assets/build_rifles.py

The Styloo pack (the other guns, see import_weapons.py) has no AR-15 family rifle, so these two are
original geometry written down here: boxes, lofts and cylinders in gun space (metres, origin at the
receiver, -Z forward, +Y up, +X right; see src/weapon/rig.rs), flat-coloured (no texture, no UV;
the camo maps are planar) with four plain materials. The M4 is a collapsed-stock carbine with a
quad-rail handguard, flat-top rail and a flip-up rear sight; the M16A4 is the long rifle with a
fixed stock, a round full-length handguard and the removable carry handle.

The node names are what `rig.rs` refers to (magazine, trigger, the sights that go when an Optic is
fitted); `rig.rs` has a test that every name it uses is in the file. The model is written turned
to the pack's convention (authored along +X, +Y up), which `GunRig::model_transform` turns back.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from contextlib import contextmanager
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "crates/frontline/assets/models/weapons"

# Linear base colour, metallic, roughness. Dark, a little lighter on the metal so the shapes read
# under the flat game lighting.
MATERIALS = {
    "receiver": ((0.024, 0.025, 0.029), 0.0, 0.90),
    "polymer": ((0.016, 0.016, 0.019), 0.0, 0.95),
    "steel": ((0.060, 0.060, 0.066), 0.15, 0.70),
    "rubber": ((0.009, 0.009, 0.010), 0.0, 1.0),
}

BORE = 0.033  # height of the bore axis
RAIL_TOP = 0.060  # top of the flat-top rail (`body_top` in rig.rs)

Vec = tuple[float, float, float]


def sub(a: Vec, b: Vec) -> Vec:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def add(a: Vec, b: Vec) -> Vec:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def dot(a: Vec, b: Vec) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a: Vec, b: Vec) -> Vec:
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def unit(a: Vec) -> Vec:
    length = math.sqrt(dot(a, a)) or 1.0
    return (a[0] / length, a[1] / length, a[2] / length)


def mean(points: list[Vec]) -> Vec:
    n = len(points)
    return (sum(p[0] for p in points) / n, sum(p[1] for p in points) / n, sum(p[2] for p in points) / n)


class Part:
    """The geometry of one node: triangles with their own normals, grouped by material."""

    def __init__(self) -> None:
        self.prims: dict[str, tuple[list[Vec], list[Vec], list[int]]] = {}
        self.turn: tuple[float, float, Vec] | None = None  # (cos, sin) about X, pivot

    @contextmanager
    def tilted(self, angle: float, pivot: Vec):
        """Everything emitted inside is turned about the X axis through `pivot`."""
        self.turn = (math.cos(angle), math.sin(angle), pivot)
        try:
            yield
        finally:
            self.turn = None

    def _xf(self, p: Vec, vector: bool = False) -> Vec:
        if self.turn is None:
            return p
        c, s, pivot = self.turn
        q = p if vector else sub(p, pivot)
        r = (q[0], q[1] * c - q[2] * s, q[1] * s + q[2] * c)
        return r if vector else add(r, pivot)

    def tri(self, material: str, points: list[Vec], normals: list[Vec]) -> None:
        points = [self._xf(p) for p in points]
        normals = [self._xf(n, True) for n in normals]
        geometric = cross(sub(points[1], points[0]), sub(points[2], points[0]))
        if dot(geometric, mean(normals)) < 0:
            points = [points[0], points[2], points[1]]
            normals = [normals[0], normals[2], normals[1]]
        positions, norms, indices = self.prims.setdefault(material, ([], [], []))
        base = len(positions)
        positions += points
        norms += normals
        indices += [base, base + 1, base + 2]

    def face(self, material: str, points: list[Vec], outward: Vec) -> None:
        """A flat polygon facing `outward` (a direction, only its side matters)."""
        normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))
        if dot(normal, outward) < 0:
            normal = (-normal[0], -normal[1], -normal[2])
        for i in range(1, len(points) - 1):
            self.tri(material, [points[0], points[i], points[i + 1]], [normal] * 3)

    def solid(self, material: str, front: list[Vec], back: list[Vec], caps: tuple[bool, bool] = (True, True)) -> None:
        """A convex solid between two quads of the same winding; faces point away from its middle."""
        middle = mean(front + back)
        sides = [(front[i], front[(i + 1) % 4], back[(i + 1) % 4], back[i]) for i in range(4)]
        for quad in sides:
            self.face(material, list(quad), sub(mean(list(quad)), middle))
        if caps[0]:
            self.face(material, front, sub(mean(front), middle))
        if caps[1]:
            self.face(material, back, sub(mean(back), middle))

    def box(self, material: str, x: tuple[float, float], y: tuple[float, float], z: tuple[float, float]) -> None:
        self.loft(material, [(z[0], x[0], x[1], y[0], y[1]), (z[1], x[0], x[1], y[0], y[1])])

    def loft(self, material: str, sections: list[tuple[float, float, float, float, float]]) -> None:
        """Rectangles `(z, x0, x1, y0, y1)` joined in order along Z."""

        def corners(s: tuple[float, float, float, float, float]) -> list[Vec]:
            z, x0, x1, y0, y1 = s
            return [(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)]

        for i in range(len(sections) - 1):
            self.solid(
                material,
                corners(sections[i]),
                corners(sections[i + 1]),
                (i == 0, i == len(sections) - 2),
            )

    def tube(
        self,
        material: str,
        centre: tuple[float, float],
        z: tuple[float, float],
        outer: float,
        inner: float = 0.0,
        sides: int = 12,
        outer_end: float | None = None,
    ) -> None:
        """A cylinder (a ring when `inner` > 0) along Z from z[0] to z[1], smooth-shaded; the
        outer radius is `outer` at z[0] and `outer_end` (default the same) at z[1]."""
        cx, cy = centre
        end = outer if outer_end is None else outer_end
        rim = lambda r, zz, k: (cx + r * math.cos(2 * math.pi * k / sides), cy + r * math.sin(2 * math.pi * k / sides), zz)
        radial = lambda k: (math.cos(2 * math.pi * k / sides), math.sin(2 * math.pi * k / sides), 0.0)
        for k in range(sides):
            j = (k + 1) % sides
            a, b = rim(outer, z[0], k), rim(outer, z[0], j)
            c, d = rim(end, z[1], j), rim(end, z[1], k)
            n = [radial(k), radial(j), radial(j), radial(k)]
            self.tri(material, [a, b, c], [n[0], n[1], n[2]])
            self.tri(material, [a, c, d], [n[0], n[2], n[3]])
            if inner > 0:
                a, b = rim(inner, z[0], k), rim(inner, z[0], j)
                c, d = rim(inner, z[1], j), rim(inner, z[1], k)
                n = [tuple(-v for v in radial(k)), tuple(-v for v in radial(j))]
                self.tri(material, [a, b, c], [n[0], n[1], n[1]])
                self.tri(material, [a, c, d], [n[0], n[1], n[0]])
            for zz, r_out, sign in ((z[0], outer, -1.0), (z[1], end, 1.0)):
                r_in = inner
                p = [rim(r_out, zz, k), rim(r_out, zz, j), rim(r_in, zz, j), rim(r_in, zz, k)]
                if inner > 0:
                    self.face(material, p, (0.0, 0.0, sign))
                else:
                    self.face(material, [p[0], p[1], (cx, cy, zz)], (0.0, 0.0, sign))


def rail(part: Part, material: str, z0: float, z1: float, x: tuple[float, float], y: tuple[float, float], pitch: float = 0.0105, width: float = 0.0068) -> None:
    """Picatinny teeth along Z."""
    z = z0
    while z + width <= z1 + 1e-9:
        part.box(material, x, y, (z, z + width))
        z += pitch


def side_rail(part: Part, material: str, z0: float, z1: float, x: tuple[float, float], y: tuple[float, float], pitch: float = 0.0105, width: float = 0.0068) -> None:
    """Teeth standing out of a handguard's side (same boxes; named for the reader)."""
    rail(part, material, z0, z1, x, y, pitch, width)


def receiver(body: Part) -> None:
    """Upper and lower receiver, flat-top rail, grip, trigger guard: everything the M4 and M16A4
    share (the magazine, trigger and sights are their own nodes)."""
    # Lower: magwell, the bar over the trigger, the rear block the grip hangs from.
    body.box("receiver", (-0.0178, 0.0178), (-0.052, 0.010), (-0.068, 0.003))
    body.box("receiver", (-0.0165, 0.0165), (-0.010, 0.010), (0.004, 0.034))
    body.box("receiver", (-0.0165, 0.0165), (-0.034, 0.010), (0.034, 0.098))
    # Trigger guard: a bar from the magwell's back to the grip's front.
    body.box("receiver", (-0.003, 0.003), (-0.045, -0.040), (0.004, 0.042))
    body.box("receiver", (-0.003, 0.003), (-0.045, -0.008), (0.004, 0.0095))
    # Upper receiver and its flat-top rail.
    body.box("receiver", (-0.0175, 0.0175), (0.010, 0.056), (-0.130, 0.098))
    rail(body, "receiver", -0.1255, 0.098 - 0.0005, (-0.0105, 0.0105), (0.056, RAIL_TOP))
    # Ejection port cover, shell deflector, forward assist (right side).
    body.box("receiver", (0.0175, 0.0186), (0.024, 0.046), (-0.040, 0.012))
    body.box("receiver", (0.0175, 0.0222), (0.016, 0.034), (0.012, 0.034))
    body.box("steel", (0.0175, 0.0228), (0.030, 0.040), (0.045, 0.062))
    # Charging handle: the latch at the back of the upper.
    body.box("steel", (-0.0165, 0.0165), (0.046, 0.056), (0.098, 0.118))
    body.box("steel", (-0.0205, 0.0205), (0.043, 0.052), (0.100, 0.113))
    # Magazine release and bolt catch.
    body.box("steel", (0.0165, 0.0196), (-0.028, -0.018), (-0.020, -0.006))
    body.box("steel", (-0.0196, -0.0165), (-0.004, 0.004), (0.020, 0.032))
    # Pistol grip, raked back at the bottom.
    with body.tilted(-0.42, (0.0, -0.066, 0.060)):
        body.box("polymer", (-0.0158, 0.0158), (-0.111, -0.010), (0.0405, 0.0795))
        body.box("polymer", (-0.0168, 0.0168), (-0.116, -0.105), (0.0395, 0.0815))


def trigger() -> Part:
    part = Part()
    with part.tilted(0.3, (0.0, -0.006, 0.018)):
        part.box("steel", (-0.003, 0.003), (-0.036, -0.006), (0.015, 0.021))
    return part


def magazine() -> Part:
    """A straight 30-round STANAG magazine with ribs and a floorplate. Its top hides in the well."""
    part = Part()
    part.box("polymer", (-0.0125, 0.0125), (-0.168, -0.020), (-0.063, 0.000))
    for k in range(6):
        y = -0.062 - 0.019 * k
        part.box("polymer", (-0.0132, 0.0132), (y - 0.003, y), (-0.063, 0.000))
    part.box("steel", (-0.0136, 0.0136), (-0.176, -0.168), (-0.067, 0.004))
    return part


def muzzle_device(body: Part, tip: float) -> None:
    """The A2 birdcage flash hider ending at z=`tip`: a slotted cylinder with rings."""
    body.tube("steel", (0.0, BORE), (tip, tip + 0.046), 0.0098, sides=10)
    for offset in (0.0, 0.020, 0.040):
        body.tube("steel", (0.0, BORE), (tip + offset, tip + offset + 0.006), 0.0112, sides=10)
    # The slots: dark bars across the middle section.
    body.box("rubber", (-0.0114, 0.0114), (BORE - 0.0025, BORE + 0.0025), (tip + 0.010, tip + 0.018))
    body.box("rubber", (-0.0025, 0.0025), (BORE - 0.0114, BORE + 0.0114), (tip + 0.010, tip + 0.018))


def front_sight(z: float, base_top: float, post_top: float, base_half: float) -> Part:
    """Front sight tower centred at `z` on a gas block, its post ending at `post_top`."""
    part = Part()
    part.box("steel", (-base_half, base_half), (BORE + 0.007, base_top), (z - 0.014, z + 0.014))
    part.box("steel", (-0.0023, 0.0023), (base_top, post_top), (z - 0.003, z + 0.003))
    for side in (-1.0, 1.0):
        x = (side * 0.0046, side * 0.0066)
        part.box("steel", (min(x), max(x)), (base_top, post_top - 0.006), (z - 0.003, z + 0.003))
    return part


def rear_aperture(part: Part, z: tuple[float, float], centre_y: float, base_y: float) -> None:
    """A rear peep: a ring on two legs, `centre_y` high."""
    part.tube("steel", (0.0, centre_y), z, 0.0105, 0.0055, sides=14)
    for side in (-1.0, 1.0):
        x = (side * 0.0070, side * 0.0105)
        part.box("steel", (min(x), max(x)), (base_y, centre_y), z)


def m4() -> dict[str, Part]:
    body = Part()
    receiver(body)
    # Quad-rail handguard, flat-topped on the receiver's rail.
    hand = (-0.330, -0.126)
    body.box("receiver", (-0.0195, 0.0195), (BORE - 0.0185, BORE + 0.0185), hand)
    body.box("steel", (-0.0215, 0.0215), (BORE - 0.0205, BORE + 0.0205), (hand[0], hand[0] + 0.004))
    rail(body, "receiver", -0.326, -0.1265, (-0.0105, 0.0105), (BORE + 0.0185, RAIL_TOP))
    rail(body, "receiver", -0.326, -0.1265, (-0.0105, 0.0105), (BORE - 0.0185 - 0.0035, BORE - 0.0185))
    for side in (-1.0, 1.0):
        x = (side * 0.0195, side * (0.0195 + 0.0035))
        side_rail(body, "receiver", -0.326, -0.1265, (min(x), max(x)), (BORE - 0.0105, BORE + 0.0105))
    # Barrel, gas block and the flash hider.
    body.tube("steel", (0.0, BORE), (-0.500, -0.330), 0.0075, sides=12)
    body.tube("steel", (0.0, BORE), (-0.360, -0.330), 0.0115, sides=12)
    muzzle_device(body, -0.500)
    # Collapsed telescoping stock on its buffer tube, rubber butt pad: its own node (hidden
    # while aimed).
    stock = Part()
    stock.tube("steel", (0.0, BORE), (0.098, 0.104), 0.0150, sides=12)
    stock.tube("polymer", (0.0, BORE), (0.104, 0.220), 0.0125, sides=12)
    stock.loft(
        "polymer",
        [
            (0.118, -0.0140, 0.0140, 0.0045, 0.0455),
            (0.190, -0.0150, 0.0150, -0.010, 0.0455),
            (0.262, -0.0165, 0.0165, -0.040, 0.0440),
        ],
    )
    stock.box("steel", (-0.006, 0.006), (-0.0035, 0.0045), (0.112, 0.134))
    stock.box("rubber", (-0.0175, 0.0175), (-0.046, 0.047), (0.262, 0.276))
    rear = Part()
    rear.box("steel", (-0.0095, 0.0095), (RAIL_TOP, RAIL_TOP + 0.006), (0.030, 0.058))
    rear_aperture(rear, (0.040, 0.050), 0.078, RAIL_TOP + 0.006)
    front = front_sight(-0.345, BORE + 0.019, 0.078, 0.0085)
    return {
        "m4_body": body,
        "m4_stock": stock,
        "m4_magazine": magazine(),
        "m4_trigger": trigger(),
        "m4_rearsight": rear,
        "m4_frontsight": front,
    }


def m16() -> dict[str, Part]:
    body = Part()
    receiver(body)
    # Round full-length handguard with its heat-shield ribs and the delta ring.
    hand = (-0.400, -0.126)
    body.tube("polymer", (0.0, BORE), hand, 0.0232, sides=12)
    z = hand[0] + 0.012
    while z < hand[1] - 0.02:
        body.tube("polymer", (0.0, BORE), (z, z + 0.004), 0.0244, sides=12)
        z += 0.034
    body.tube("steel", (0.0, BORE), (-0.1425, -0.1285), 0.0255, sides=12)
    body.tube("steel", (0.0, BORE), (hand[0] - 0.005, hand[0]), 0.0245, sides=12)
    # The long barrel, gas block and flash hider.
    body.tube("steel", (0.0, BORE), (-0.580, -0.400), 0.0078, sides=12)
    body.tube("steel", (0.0, BORE), (-0.470, -0.440), 0.0115, sides=12)
    muzzle_device(body, -0.580)
    # Fixed A2 stock, solid, sloping to a rubber butt plate: its own node (hidden while aimed).
    stock = Part()
    stock.loft(
        "polymer",
        [
            (0.098, -0.0135, 0.0135, 0.014, 0.046),
            (0.200, -0.0150, 0.0150, -0.008, 0.046),
            (0.355, -0.0165, 0.0165, -0.060, 0.040),
        ],
    )
    stock.box("steel", (-0.007, 0.007), (0.039, 0.0405), (0.250, 0.300))
    stock.box("rubber", (-0.0175, 0.0175), (-0.072, 0.046), (0.355, 0.370))
    # The removable carry handle: a slope up from the rail to the grip bar and a rear sight
    # block, with the peep standing on top of the block (the sight line runs just over the bar).
    handle = Part()
    handle.loft(
        "receiver",
        [
            (-0.100, -0.012, 0.012, RAIL_TOP, 0.066),
            (-0.078, -0.011, 0.011, 0.068, 0.082),
            (0.040, -0.011, 0.011, 0.068, 0.082),
        ],
    )
    handle.box("receiver", (-0.0165, 0.0165), (RAIL_TOP, 0.082), (0.040, 0.098))
    rear = Part()
    rear_aperture(rear, (0.084, 0.094), 0.092, 0.082)
    front = front_sight(-0.455, BORE + 0.030, 0.092, 0.0085)
    return {
        "m16_body": body,
        "m16_stock": stock,
        "m16_magazine": magazine(),
        "m16_trigger": trigger(),
        "m16_handle": handle,
        "m16_rearsight": rear,
        "m16_frontsight": front,
    }


def to_model(p: Vec) -> Vec:
    """Gun space (-Z forward, +X right) to the pack's convention (+X forward, +Z right)."""
    return (-p[2], p[1], p[0])


def write_glb(path: Path, root: str, parts: dict[str, Part]) -> None:
    binary = bytearray()
    views: list[dict] = []
    accessors: list[dict] = []
    meshes: list[dict] = []
    nodes: list[dict] = []
    materials = list(MATERIALS)

    def view(data: bytes, target: int) -> int:
        binary.extend(b"\0" * (-len(binary) % 4))
        views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data), "target": target})
        binary.extend(data)
        return len(views) - 1

    def accessor(index: int, component: int, count: int, kind: str, bounds: tuple[list[float], list[float]] | None = None) -> int:
        entry = {"bufferView": index, "componentType": component, "count": count, "type": kind}
        if bounds:
            entry["min"], entry["max"] = bounds
        accessors.append(entry)
        return len(accessors) - 1

    for name, part in parts.items():
        primitives = []
        for material, (positions, normals, indices) in part.prims.items():
            model_positions = [to_model(p) for p in positions]
            model_normals = [to_model(n) for n in normals]
            low = [min(p[i] for p in model_positions) for i in range(3)]
            high = [max(p[i] for p in model_positions) for i in range(3)]
            flat = lambda vs: b"".join(struct.pack("<3f", *v) for v in vs)
            position = accessor(view(flat(model_positions), 34962), 5126, len(positions), "VEC3", (low, high))
            normal = accessor(view(flat(model_normals), 34962), 5126, len(normals), "VEC3")
            wide = len(positions) > 65535
            packed = struct.pack(f"<{len(indices)}{'I' if wide else 'H'}", *indices)
            index = accessor(view(packed, 34963), 5125 if wide else 5123, len(indices), "SCALAR")
            primitives.append(
                {
                    "attributes": {"POSITION": position, "NORMAL": normal},
                    "indices": index,
                    "material": materials.index(material),
                }
            )
        meshes.append({"name": name, "primitives": primitives})
        nodes.append({"name": name, "mesh": len(meshes) - 1})
    nodes.append({"name": root, "children": list(range(len(parts)))})
    document = {
        "asset": {"version": "2.0", "generator": "tools/assets/build_rifles.py"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [len(nodes) - 1]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [
            {
                "name": name,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [*colour, 1.0],
                    "metallicFactor": metallic,
                    "roughnessFactor": roughness,
                },
            }
            for name, (colour, metallic, roughness) in MATERIALS.items()
        ],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary.extend(b"\0" * (-len(binary) % 4))
    total = 12 + 8 + len(text) + 8 + len(binary)
    path.write_bytes(
        struct.pack("<4sII", b"glTF", 2, total)
        + struct.pack("<I4s", len(text), b"JSON")
        + text
        + struct.pack("<I4s", len(binary), b"BIN\0")
        + bytes(binary)
    )
    print(f"{path.name} {total / 1024:.0f} KB")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, default=OUT)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    write_glb(args.out / "m4.glb", "m4", m4())
    write_glb(args.out / "m16.glb", "m16", m16())


if __name__ == "__main__":
    main()
