"""Run inside Blender: builds the first-person M4 and M16A4 (`m4.glb`, `m16.glb`) from scratch and
exports them to crates/frontline/assets/models/weapons/. Normally started by build_rifles.py:

    blender -b --python tools/assets/blender_rifles.py -- --out crates/frontline/assets/models/weapons

Original geometry written here, no input files. Everything is authored in the game's gun space
(metres, origin at the receiver, -Z forward, +Y up, +X right; see src/weapon/rig.rs) and converted
by `G` to Blender's axes so that the glTF exporter (+Y up) writes the pack's convention (+X
forward, +Y up) that `GunRig::model_transform` turns back.

A primitive is always a stack of rings (a closed solid): boxes, rounded lofts (stocks, magazine,
grip, receivers), lathe profiles (barrels, muzzle devices) and extruded outlines (rails,
handle, trigger guard). Edges sharper than `CREASE` get a 1 mm chamfer in a lighter "worn" material,
and stay hard-shaded; everything else is smooth. Each rifle's nodes are named as rig.rs expects.
"""

import argparse
import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Vector

BORE = 0.033  # height of the bore axis
RAIL_TOP = 0.060  # top of the flat-top rail (`body_top` in rig.rs)
CREASE = math.radians(35.0)  # edges sharper than this are hard and chamfered
CHAMFER = 0.0010

# Linear base colour, metallic, roughness. Phosphate and anodised metals a notch above the polymer
# so the shapes read under the flat game lighting; "wear" is the bare metal on the chamfers.
MATERIALS = {
    "receiver": ((0.030, 0.031, 0.036), 0.30, 0.55),
    "polymer": ((0.017, 0.017, 0.019), 0.0, 0.85),
    "steel": ((0.075, 0.076, 0.082), 0.45, 0.45),
    "barrel": ((0.040, 0.042, 0.040), 0.30, 0.65),
    "rubber": ((0.008, 0.008, 0.009), 0.0, 1.0),
    "wear": ((0.140, 0.140, 0.150), 0.55, 0.40),
}
ORDER = list(MATERIALS)
METAL = {"receiver", "steel", "barrel"}


def G(p):
    """Gun space (x right, y up, z back) to Blender (x forward, y left, z up): -Y up export gives +X forward."""
    return Vector((-p[2], -p[0], p[1]))


class Node:
    """One glTF node: a bmesh collecting primitives."""

    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()

    def solid(self, material, rings, loop=False, bevel=CHAMFER, tilt=None):
        """Quads between consecutive rings (same point count), the ends capped unless `loop`."""
        if tilt:
            rings = [[rotate_yz(p, *tilt) for p in ring] for ring in rings]
        bm = self.bm
        verts = [[bm.verts.new(G(p)) for p in ring] for ring in rings]
        n = len(verts[0])
        faces = []
        count = len(verts) if loop else len(verts) - 1
        for k in range(count):
            a, b = verts[k], verts[(k + 1) % len(verts)]
            for i in range(n):
                j = (i + 1) % n
                faces.append(bm.faces.new((a[i], a[j], b[j], b[i])))
        if not loop:
            faces.append(bm.faces.new(verts[0]))
            faces.append(bm.faces.new(verts[-1][::-1]))
        bmesh.ops.recalc_face_normals(bm, faces=faces)
        index = ORDER.index(material)
        for face in faces:
            face.material_index = index
        if bevel > 0:
            edges = {e for f in faces for e in f.edges if len(e.link_faces) == 2}
            hard = [e for e in edges if e.calc_face_angle(0.0) > CREASE]
            if hard:
                wear = ORDER.index("wear") if material in METAL else index
                bmesh.ops.bevel(
                    bm,
                    geom=hard,
                    offset=bevel,
                    offset_type="OFFSET",
                    segments=1,
                    affect="EDGES",
                    material=wear,
                    clamp_overlap=True,
                )

    def finish(self):
        """Smooth faces, hard creases, box-projected UVs (metres) so TEXCOORD_0 exists."""
        bm = self.bm
        for face in bm.faces:
            face.smooth = True
        for edge in bm.edges:
            if len(edge.link_faces) == 2 and edge.calc_face_angle(0.0) > CREASE:
                edge.smooth = False
        layer = bm.loops.layers.uv.new("UVMap")
        for face in bm.faces:
            n = face.normal
            axis = max(range(3), key=lambda i: abs(n[i]))
            for loop in face.loops:
                p = loop.vert.co
                u, v = [(p.y, p.z), (p.x, p.z), (p.x, p.y)][axis]
                loop[layer].uv = (u, v)
        mesh = bpy.data.meshes.new(self.name)
        bm.to_mesh(mesh)
        bm.free()
        for name in ORDER:
            mesh.materials.append(bpy.data.materials[name])
        obj = bpy.data.objects.new(self.name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        return obj


def rotate_yz(p, angle, pivot):
    """Rotate about the X axis through `pivot` (y, z) so that points below it swing toward +Z."""
    dy, dz = p[1] - pivot[0], p[2] - pivot[1]
    c, s = math.cos(angle), math.sin(angle)
    return (p[0], pivot[0] + dy * c + dz * s, pivot[1] - dy * s + dz * c)


def rr(a0, a1, b0, b1, r, seg=2):
    """A rounded rectangle outline in 2D."""
    if r <= 0:
        return [(a0, b0), (a1, b0), (a1, b1), (a0, b1)]
    pts = []
    for ca, cb, start in ((a1 - r, b0 + r, -90), (a1 - r, b1 - r, 0), (a0 + r, b1 - r, 90), (a0 + r, b0 + r, 180)):
        for i in range(seg + 1):
            t = math.radians(start + 90.0 * i / seg)
            pts.append((ca + r * math.cos(t), cb + r * math.sin(t)))
    return pts


def ring_z(z, x0, x1, y0, y1, r=0.0, seg=2):
    return [(a, b, z) for a, b in rr(x0, x1, y0, y1, r, seg)]


def ring_y(y, x0, x1, z0, z1, r=0.0, seg=2):
    return [(a, y, b) for a, b in rr(x0, x1, z0, z1, r, seg)]


def circle_z(z, cx, cy, radius, n=16):
    return [(cx + radius * math.cos(2 * math.pi * i / n), cy + radius * math.sin(2 * math.pi * i / n), z) for i in range(n)]


def box(node, material, xr, yr, zr, r=0.0, tilt=None, bevel=CHAMFER):
    if min(xr[1] - xr[0], yr[1] - yr[0], zr[1] - zr[0]) < 0.008:
        bevel = 0.0  # too small for a chamfer to read: not worth the triangles
    node.solid(material, [ring_z(zr[0], xr[0], xr[1], yr[0], yr[1], r), ring_z(zr[1], xr[0], xr[1], yr[0], yr[1], r)], tilt=tilt, bevel=bevel)


def lathe(node, material, profile, cx=0.0, cy=BORE, n=16, loop=False, bevel=CHAMFER):
    """A solid of revolution about the bore axis: `profile` is (z, radius) pairs."""
    node.solid(material, [circle_z(z, cx, cy, r, n) for z, r in profile], loop=loop, bevel=bevel)


def prism(node, material, plane, poly, lo, hi, bevel=0.0):
    """An outline extruded: 'zy' extrudes along x, 'zx' along y (outline points are (z, y|x))."""
    if plane == "zy":
        rings = [[(t, a, b) for b, a in poly] for t in (lo, hi)]
    else:
        rings = [[(a, t, b) for b, a in poly] for t in (lo, hi)]
    node.solid(material, rings, bevel=bevel)


def comb(node, material, plane, z0, z1, base, gap_floor, top, lo, hi, pitch=0.0105, slot=0.0042):
    """Picatinny teeth: an outline of slots and teeth along z standing on `base`, extruded between lo and hi."""
    pts = [(z0, gap_floor)]
    z = z0
    while z + pitch <= z1 + 1e-9:
        pts += [(z + slot, gap_floor), (z + slot, top), (z + pitch, top), (z + pitch, gap_floor)]
        z += pitch
    if abs(pts[-1][0] - z1) > 1e-9:
        pts.append((z1, gap_floor))
    poly = [(z0, base), (z1, base)] + pts[::-1]
    prism(node, material, plane, poly, lo, hi)


def torus(node, material, centre, big, small, n=10, m=5):
    """A ring in the plane across X (sling loop)."""
    cx, cy, cz = centre
    rings = []
    for i in range(n):
        a = 2 * math.pi * i / n
        ring = []
        for j in range(m):
            b = 2 * math.pi * j / m
            r = big + small * math.cos(b)
            ring.append((cx + small * math.sin(b), cy + r * math.cos(a), cz + r * math.sin(a)))
        rings.append(ring)
    node.solid(material, rings, loop=True, bevel=0.0)


def cylinder_x(node, material, centre, radius, x0, x1, n=8, bevel=0.0):
    """A pin or knob along X at (y, z) = centre."""
    y, z = centre
    rings = [[(x, y + radius * math.sin(2 * math.pi * i / n), z + radius * math.cos(2 * math.pi * i / n)) for i in range(n)] for x in (x0, x1)]
    node.solid(material, rings, bevel=bevel)


def ridged(node, material, z0, z1, cx, cy, r_hi, r_lo, ridges, taper=0.0):
    """A ribbed round handguard: `ridges` flutes around the bore, narrowing by `taper` toward z0."""

    def section(z, k):
        pts = []
        for i in range(ridges):
            a = 2 * math.pi * i / ridges
            step = 2 * math.pi / ridges
            for off, r in ((0.0, r_hi), (0.45, r_hi), (0.55, r_lo), (0.95, r_lo)):
                t = a + step * off
                pts.append((cx + (r - k) * math.cos(t), cy + (r - k) * math.sin(t), z))
        return pts

    node.solid(material, [section(z0, taper), section(z1, 0.0)], bevel=0.0)


def hollow(node, material, z0, z1, cx, cy, outer, inner, n=14):
    """A ring (a peep sight, a muzzle face): a tube open at both ends."""
    lathe(node, material, [(z0, outer), (z1, outer), (z1, inner), (z0, inner)], cx, cy, n=n, loop=True, bevel=0.0007)


# ---- shared parts -------------------------------------------------------------------------------


def grip_rings():
    """The pistol grip in its own frame (before the rake): stations top to bottom with finger grooves."""
    stations = [
        # y, front z, back z, half width, corner
        (-0.010, 0.0395, 0.0805, 0.0165, 0.006),
        (-0.030, 0.0420, 0.0795, 0.0158, 0.006),
        (-0.044, 0.0408, 0.0800, 0.0156, 0.006),
        (-0.058, 0.0425, 0.0800, 0.0156, 0.006),
        (-0.072, 0.0410, 0.0805, 0.0157, 0.006),
        (-0.088, 0.0428, 0.0808, 0.0158, 0.006),
        (-0.102, 0.0405, 0.0815, 0.0162, 0.006),
        (-0.111, 0.0400, 0.0820, 0.0166, 0.006),
        (-0.116, 0.0392, 0.0825, 0.0168, 0.004),
    ]
    return [ring_y(y, -hw, hw, zf, zb, r) for y, zf, zb, hw, r in stations]


GRIP_TILT = (0.42, (-0.066, 0.060))


def receiver(body):
    """Upper and lower receiver, flat-top rail, grip, trigger guard: everything the M4 and M16A4 share
    (the magazine, trigger, sights and handle are their own nodes)."""
    # Lower receiver: the magazine well flaring at its mouth, the block over the trigger, the tang.
    body.solid("receiver", [ring_y(0.010, -0.0172, 0.0172, -0.068, 0.004, 0.003), ring_y(-0.030, -0.0176, 0.0176, -0.070, 0.0055, 0.003), ring_y(-0.052, -0.0184, 0.0184, -0.073, 0.0075, 0.004)])
    prism(body, "receiver", "zy", [(0.002, 0.010), (0.002, -0.012), (0.034, -0.016), (0.046, -0.030), (0.090, -0.026), (0.098, -0.016), (0.098, 0.010)], -0.0165, 0.0165, bevel=CHAMFER)
    # The trigger guard: a bar leaving the well and curving up into the grip.
    prism(body, "receiver", "zy", [(0.002, -0.046), (0.020, -0.053), (0.038, -0.051), (0.049, -0.041), (0.051, -0.024), (0.045, -0.024), (0.044, -0.037), (0.036, -0.045), (0.020, -0.047), (0.002, -0.041)], -0.0030, 0.0030, bevel=0.0007)
    # The pistol grip raked back at the bottom, with its finger grooves and heel.
    body.solid("polymer", grip_rings(), tilt=GRIP_TILT, bevel=0.0)
    # Pivot and takedown pins.
    cylinder_x(body, "steel", (0.002, -0.058), 0.0036, -0.0185, 0.0185)
    cylinder_x(body, "steel", (-0.001, 0.090), 0.0036, -0.0185, 0.0185)
    # Upper receiver: its parting line over the lower and the flat-top rail on it.
    body.solid(
        "receiver",
        [
            ring_z(-0.130, -0.0166, 0.0166, 0.010, 0.056, 0.008, 3),
            ring_z(-0.050, -0.0175, 0.0175, 0.010, 0.056, 0.009, 3),
            ring_z(0.060, -0.0175, 0.0175, 0.010, 0.056, 0.009, 3),
            ring_z(0.098, -0.0168, 0.0168, 0.010, 0.056, 0.007, 3),
        ],
        bevel=0.0,
    )
    comb(body, "receiver", "zy", -0.1255, 0.0975, 0.0555, 0.0575, RAIL_TOP, -0.0105, 0.0105)
    # Ejection port cover and its hinge, the shell deflector, the forward assist (right side).
    box(body, "receiver", (0.0172, 0.0186), (0.024, 0.047), (-0.040, 0.010), r=0.003)
    box(body, "steel", (0.0172, 0.0192), (0.0235, 0.0255), (-0.041, 0.009), bevel=0.0005)
    body.solid("receiver", [ring_z(0.010, 0.0172, 0.0172 + 0.0048, 0.016, 0.036, 0.002), ring_z(0.036, 0.0172, 0.0172 + 0.0006, 0.016, 0.036, 0.002)], bevel=0.0007)
    box(body, "receiver", (0.0172, 0.0222), (0.034, 0.045), (0.043, 0.066), r=0.002)
    cylinder_x(body, "steel", (0.0395, 0.0585), 0.0045, 0.0200, 0.0248)
    # The dust cover's slot, the rear of the upper's flats and the brass-catching dimples are painted
    # on as a dark sliver under the cover.
    box(body, "rubber", (0.0170, 0.0178), (0.0215, 0.0235), (-0.036, 0.006), bevel=0.0)
    # Charging handle: the stem out of the upper and its two latches.
    box(body, "steel", (-0.0075, 0.0075), (0.044, 0.054), (0.090, 0.112), bevel=0.0007)
    for side in (-1.0, 1.0):
        prism(body, "steel", "zx", [(0.104, 0.007 * side), (0.118, 0.012 * side), (0.118, 0.0215 * side), (0.109, 0.0215 * side), (0.104, 0.013 * side)], 0.043, 0.054, bevel=0.0006)
    # Magazine release (right), bolt catch and selector (left).
    cylinder_x(body, "steel", (-0.023, -0.013), 0.0048, 0.0172, 0.0210)
    box(body, "steel", (-0.0196, -0.0170), (-0.004, 0.006), (-0.014, -0.001), r=0.001, bevel=0.0007)
    cylinder_x(body, "steel", (0.001, 0.030), 0.0065, -0.0192, -0.0168)
    prism(body, "steel", "zy", [(0.026, 0.001), (0.034, 0.001), (0.043, 0.006), (0.040, 0.008), (0.032, 0.004), (0.026, 0.004)], -0.0215, -0.0192, bevel=0.0005)
    # Lower receiver's buffer tube boss at the back.
    lathe(body, "receiver", [(0.096, 0.0150), (0.102, 0.0150)], n=14)
    # Sling loop on the rear end plate.
    torus(body, "steel", (0.0, -0.0075, 0.090), 0.0055, 0.0016)


def trigger():
    node = Node("trigger")
    prism(node, "steel", "zy", [(0.0135, -0.004), (0.0215, -0.004), (0.0220, -0.017), (0.0200, -0.030), (0.0150, -0.037), (0.0125, -0.034), (0.0150, -0.024), (0.0145, -0.012)], -0.0030, 0.0030, bevel=0.0007)
    return node


def magazine():
    """A curved 30-round STANAG magazine: ribbed body, floorplate. Its top hides in the well."""
    node = Node("magazine")
    top, bottom = -0.020, -0.168

    def centre(y):
        s = (top - y) / (top - bottom)
        return -0.0315 - 0.030 * s**1.6

    def ring(y, grow=0.0):
        c = centre(y)
        return ring_y(y, -0.0122 - grow, 0.0122 + grow, c - 0.0312 - grow, c + 0.0312 + grow, 0.004, 1)

    rings = [ring(top), ring(-0.058)]
    for k in range(6):
        y = -0.062 - 0.0185 * k
        rings += [ring(y), ring(y, 0.0009), ring(y - 0.0045, 0.0009), ring(y - 0.0045)]
    rings.append(ring(bottom + 0.008))
    node.solid("polymer", rings, bevel=0.0)
    c = centre(bottom)
    node.solid("steel", [ring_y(bottom + 0.008, -0.0130, 0.0130, c - 0.0330, c + 0.0332, 0.004), ring_y(bottom - 0.002, -0.0136, 0.0136, c - 0.0345, c + 0.0345, 0.004)], bevel=0.0007)
    return node


def muzzle_device(body, tip):
    """The A2 birdcage flash hider ending at z=`tip`: a slotted cylinder with rings, bore in its face."""
    inner = 0.0045
    lathe(
        body,
        "barrel",
        [(tip, inner), (tip, 0.0086), (tip + 0.003, 0.0098), (tip + 0.047, 0.0098), (tip + 0.047, 0.0076), (tip + 0.052, 0.0076), (tip + 0.052, 0.0073), (tip + 0.047, 0.0073), (tip + 0.003, inner)],
        n=10,
        loop=True,
        bevel=0.0,
    )
    for offset in (0.0, 0.020, 0.040):
        lathe(body, "steel", [(tip + offset, 0.0098), (tip + offset, 0.0112), (tip + offset + 0.006, 0.0112), (tip + offset + 0.006, 0.0098)], n=10, loop=False, bevel=0.0)
    # The slots: dark bars across the middle section, read as the cuts.
    box(body, "rubber", (-0.0100, 0.0100), (BORE - 0.0024, BORE + 0.0024), (tip + 0.010, tip + 0.018), bevel=0.0)
    box(body, "rubber", (-0.0024, 0.0024), (BORE - 0.0100, BORE + 0.0100), (tip + 0.010, tip + 0.018), bevel=0.0)
    box(body, "rubber", (-0.0100, 0.0100), (BORE - 0.0024, BORE + 0.0024), (tip + 0.026, tip + 0.034), bevel=0.0)
    box(body, "rubber", (-0.0024, 0.0024), (BORE - 0.0100, BORE + 0.0100), (tip + 0.026, tip + 0.034), bevel=0.0)


def front_sight(name, z, base_top, post_top, base_half, sling=False):
    """A2 front sight tower with its ears and post, standing on the gas block at z."""
    node = Node(name)
    box(node, "steel", (-base_half, base_half), (BORE + 0.007, base_top), (z - 0.014, z + 0.014), r=0.002)
    box(node, "steel", (-0.0021, 0.0021), (base_top - 0.002, post_top), (z - 0.0028, z + 0.0028), bevel=0.0005)
    for side in (-1.0, 1.0):
        x = (side * 0.0050, side * 0.0068)
        box(node, "steel", (min(x), max(x)), (base_top, post_top - 0.006), (z - 0.0035, z + 0.0035), bevel=0.0005)
    return node


def rear_aperture(node, z0, z1, centre_y, base_y, outer=0.0105, inner=0.0055):
    """A rear peep: a ring on two legs, `centre_y` high."""
    hollow(node, "steel", z0, z1, 0.0, centre_y, outer, inner, n=14)
    for side in (-1.0, 1.0):
        x = (side * 0.0068, side * 0.0102)
        box(node, "steel", (min(x), max(x)), (base_y, centre_y), (z0, z1), bevel=0.0006)


# ---- the rifles ---------------------------------------------------------------------------------


def m4():
    body = Node("m4_body")
    receiver(body)
    # Quad-rail handguard, flat-topped on the receiver's rail: slim round-cornered shell with a rail on each face.
    hand = (-0.330, -0.126)
    body.solid("receiver", [ring_z(hand[0], -0.0192, 0.0192, BORE - 0.0185, BORE + 0.0185, 0.007, 3), ring_z(hand[1], -0.0192, 0.0192, BORE - 0.0185, BORE + 0.0185, 0.007, 3)], bevel=0.0)
    box(body, "steel", (-0.0210, 0.0210), (BORE - 0.0200, BORE + 0.0200), (hand[0], hand[0] + 0.005), r=0.007, bevel=0.0007)
    comb(body, "receiver", "zy", -0.326, -0.1265, BORE + 0.0170, BORE + 0.0185, RAIL_TOP, -0.0105, 0.0105)
    comb(body, "receiver", "zy", -0.300, -0.200, BORE - 0.0170, BORE - 0.0185, BORE - 0.0185 - 0.0035, -0.0105, 0.0105)
    for side in (-1.0, 1.0):
        # Side rails: teeth standing out of the shell, shorter than the top one.
        comb(body, "receiver", "zx", -0.290, -0.200, side * 0.0170, side * 0.0190, side * 0.0225, BORE - 0.0105, BORE + 0.0105)
    # Barrel, gas block, the cut for an underbarrel launcher, and the flash hider.
    lathe(body, "barrel", [(-0.500, 0.0075), (-0.386, 0.0075), (-0.386, 0.0090), (-0.372, 0.0090), (-0.372, 0.0075), (-0.330, 0.0075)], n=12, bevel=0.0)
    lathe(body, "steel", [(-0.368, 0.0108), (-0.334, 0.0108)], n=12)
    muzzle_device(body, -0.500)
    torus(body, "steel", (0.0, BORE - 0.012, -0.352), 0.0050, 0.0016)
    # Collapsed telescoping stock on its buffer tube, rubber butt pad: its own node (hidden while aimed).
    stock = Node("m4_stock")
    lathe(stock, "receiver", [(0.098, 0.0150), (0.108, 0.0150)], n=14)
    lathe(stock, "receiver", [(0.108, 0.0125), (0.222, 0.0125)], n=16, bevel=0.0)
    stations = [(0.118, 0.0045, 0.0462, 0.0138), (0.150, -0.003, 0.0462, 0.0145), (0.190, -0.012, 0.0462, 0.0152), (0.228, -0.027, 0.0455, 0.0160), (0.262, -0.040, 0.0445, 0.0165)]
    stock.solid("polymer", [ring_z(z, -hw, hw, lo, hi, 0.0065, 2) for z, lo, hi, hw in stations], bevel=0.0)
    box(stock, "steel", (-0.0055, 0.0055), (0.0035, 0.0125), (0.112, 0.138), r=0.0015, bevel=0.0006)
    stock.solid("rubber", [ring_z(0.262, -0.0172, 0.0172, -0.046, 0.0455, 0.007), ring_z(0.277, -0.0172, 0.0172, -0.046, 0.0455, 0.007)], bevel=0.0)
    box(stock, "steel", (-0.003, 0.003), (-0.046, -0.040), (0.244, 0.252), bevel=0.0005)
    torus(stock, "steel", (0.0, -0.050, 0.248), 0.0050, 0.0015)
    rear = Node("m4_rearsight")
    box(rear, "steel", (-0.0095, 0.0095), (RAIL_TOP, RAIL_TOP + 0.006), (0.030, 0.058))
    for side in (-1.0, 1.0):
        x = (side * 0.0070, side * 0.0108)
        box(rear, "steel", (min(x), max(x)), (RAIL_TOP + 0.006, 0.086), (0.0360, 0.0540), r=0.0012, bevel=0.0006)
    hollow(rear, "steel", 0.040, 0.050, 0.0, 0.078, 0.0105, 0.0055)
    box(rear, "steel", (-0.0075, 0.0075), (RAIL_TOP + 0.006, 0.073), (0.040, 0.050), bevel=0.0006)
    front = front_sight("m4_frontsight", -0.345, BORE + 0.019, 0.078, 0.0085)
    return "m4", [body, stock, magazine_named("m4_magazine"), trigger_named("m4_trigger"), rear, front]


def m16():
    body = Node("m16_body")
    receiver(body)
    # Round full-length handguard: fluted two-piece shells with the delta ring and a front cap.
    hand = (-0.400, -0.126)
    ridged(body, "polymer", hand[0], hand[1] - 0.012, 0.0, BORE, 0.0232, 0.0214, 14, taper=0.0010)
    lathe(body, "steel", [(-0.1425, 0.0225), (-0.1425, 0.0258), (-0.1285, 0.0258), (-0.1285, 0.0225)], cy=BORE, n=16, bevel=0.0)
    lathe(body, "steel", [(hand[0] - 0.006, 0.0215), (hand[0] - 0.006, 0.0242), (hand[0], 0.0242), (hand[0], 0.0215)], cy=BORE, n=16, bevel=0.0)
    lathe(body, "receiver", [(-0.140, 0.0205), (-0.126, 0.0205)], n=16)
    # The long barrel, front sight base and the A2 birdcage.
    lathe(body, "barrel", [(-0.580, 0.0078), (-0.476, 0.0078), (-0.476, 0.0115), (-0.434, 0.0115), (-0.434, 0.0078), (-0.400, 0.0078)], n=12, bevel=0.0)
    muzzle_device(body, -0.580)
    torus(body, "steel", (0.0, BORE - 0.0235, -0.4), 0.0050, 0.0016)
    # Fixed A2 stock sloping to a rubber butt plate: its own node (hidden while aimed).
    stock = Node("m16_stock")
    stations = [
        (0.098, 0.014, 0.0462, 0.0135),
        (0.150, 0.002, 0.0462, 0.0143),
        (0.220, -0.016, 0.0450, 0.0150),
        (0.290, -0.040, 0.0425, 0.0160),
        (0.355, -0.060, 0.0400, 0.0165),
    ]
    stock.solid("polymer", [ring_z(z, -hw, hw, lo, hi, 0.0065, 2) for z, lo, hi, hw in stations], bevel=0.0)
    stock.solid("rubber", [ring_z(0.355, -0.0175, 0.0175, -0.072, 0.046, 0.008), ring_z(0.371, -0.0175, 0.0175, -0.072, 0.046, 0.008)], bevel=0.0)
    box(stock, "steel", (-0.0065, 0.0065), (-0.0535, -0.0490), (0.330, 0.352), bevel=0.0005)
    cylinder_x(stock, "steel", (0.0385, 0.340), 0.0035, -0.0170, 0.0170)
    torus(stock, "steel", (0.0, -0.058, 0.318), 0.0055, 0.0016)
    # The removable carry handle: ramp up from the rail, the grip bar with the open slot beneath, the
    # rear sight housing, with the peep standing on top (the sight line runs just over the bar).
    handle = Node("m16_handle")
    prism(handle, "receiver", "zy", [(-0.108, RAIL_TOP), (-0.088, RAIL_TOP), (-0.078, 0.0675), (0.036, 0.0675), (0.036, RAIL_TOP), (0.098, RAIL_TOP), (0.098, 0.082), (-0.060, 0.082)], -0.0115, 0.0115, bevel=CHAMFER)
    box(handle, "receiver", (-0.0165, 0.0165), (RAIL_TOP, 0.082), (0.042, 0.098), r=0.002)
    box(handle, "steel", (-0.0172, 0.0172), (RAIL_TOP + 0.002, RAIL_TOP + 0.005), (0.040, 0.100), bevel=0.0005)
    rear = Node("m16_rearsight")
    rear_aperture(rear, 0.084, 0.094, 0.092, 0.082)
    cylinder_x(rear, "steel", (0.073, 0.062), 0.0065, 0.0140, 0.0210)
    cylinder_x(rear, "steel", (0.073, 0.062), 0.0055, -0.0200, -0.0140)
    front = front_sight("m16_frontsight", -0.455, BORE + 0.030, 0.092, 0.0085)
    return "m16", [body, stock, magazine_named("m16_magazine"), trigger_named("m16_trigger"), handle, rear, front]


def magazine_named(name):
    node = magazine()
    node.name = name
    return node


def trigger_named(name):
    node = trigger()
    node.name = name
    return node


# ---- export -------------------------------------------------------------------------------------


def make_materials():
    for name, (colour, metallic, roughness) in MATERIALS.items():
        mat = bpy.data.materials.new(name)
        mat.use_nodes = True
        shader = mat.node_tree.nodes["Principled BSDF"]
        shader.inputs["Base Color"].default_value = (*colour, 1.0)
        shader.inputs["Metallic"].default_value = metallic
        shader.inputs["Roughness"].default_value = roughness
        mat.diffuse_color = (*colour, 1.0)


def export(root_name, nodes, out):
    bpy.ops.object.select_all(action="DESELECT")
    root = bpy.data.objects.new(root_name, None)
    bpy.context.scene.collection.objects.link(root)
    triangles = 0
    for node in nodes:
        obj = node.finish()
        obj.parent = root
        triangles += sum(len(p.vertices) - 2 for p in obj.data.polygons)
    path = out / f"{root_name}.glb"
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        export_apply=False,
        export_yup=True,
        export_cameras=False,
        export_lights=False,
        export_texcoords=True,
        export_normals=True,
        export_materials="EXPORT",
        export_vertex_color="NONE",
        export_extras=False,
    )
    print(f"RIFLE {path.name}: {triangles} triangles, {path.stat().st_size / 1024:.0f} KB")


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--only", choices=("m4", "m16"))
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    for name, build in (("m4", m4), ("m16", m16)):
        if args.only and args.only != name:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        make_materials()
        root, nodes = build()
        export(root, nodes, args.out)


main()
