"""Run inside Blender (`blender -b --python tools/assets/blender_soldier.py -- ...`): builds the
skinned soldier (`soldier.glb`) from primitives: an armature, a body and gear made of lofted rings
with explicit skin weights, per-vertex ambient occlusion, and the Faction headgear as separate
skinned meshes. `tools/assets/build_soldier.py` drives it (and makes the fabric texture first).

    blender -b --python blender_soldier.py -- --fabric fabric.png --out soldier.glb \
        [--preview /tmp/prefix] [--gun m4.glb]

Everything below is written in the game's model space (metres, +X the soldier's right, +Y up, the
soldier faces -Z, feet at the origin) and turned into Blender's (+Y forward, +Z up) only when the
Blender objects are made. `src/player/soldier.rs` drives the bones by name and has the same rest
pose numbers (hip height, hand points); its tests check the .glb against them.
"""

import argparse
import math
import os
import random
import sys

import bpy
import bmesh
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

V = Vector

# --- the skeleton -------------------------------------------------------------------------------

# Where the held rifle's origin (its receiver) is, relative to the shoulder pivot: the `gun_mount`
# empty goes here. Keep in step with blender_soldier_preview.py.
GUN_ORIGIN = V((0.10, -0.15, -0.24))
# Hand points of the M4 in gun space (`gun_rig(Carbine)` in src/weapon/rig.rs).
RIGHT_GRIP = V((0.034, -0.05, 0.052))
LEFT_HAND = V((-0.02, -0.006, -0.225))

PIVOT = V((0.0, 1.40, 0.0))  # the shoulder pivot: the `shoulders` bone
SHOULDER_X = 0.185
UPPER_ARM = 0.30
FOREARM = 0.27
HAND_LEN = 0.085  # wrist to palm centre

HIP_X = 0.10
HIP_Y = 0.93
KNEE_Y = 0.47
ANKLE_Y = 0.09


def solve_elbow(shoulder, wrist, upper, fore, pole):
    """Elbow position of a two-bone arm reaching `wrist`, bent towards `pole`."""
    reach = wrist - shoulder
    length = reach.length
    if length >= upper + fore - 1e-4:
        return shoulder + reach * (upper / (upper + fore))
    along = (upper * upper - fore * fore + length * length) / (2 * length)
    height = math.sqrt(max(upper * upper - along * along, 0.0))
    direction = reach / length
    bend = (pole - direction * pole.dot(direction)).normalized()
    return shoulder + direction * along + bend * height


def arm_points(side):
    """Shoulder, elbow, wrist and palm centre of the arm on `side` (+1 right, -1 left), holding the
    rifle: the right hand on the grip, the left under the handguard."""
    shoulder = PIVOT + V((SHOULDER_X * side, 0.0, 0.0))
    if side > 0:
        palm = PIVOT + GUN_ORIGIN + RIGHT_GRIP
        wrist = palm + V((0.0, 0.0, HAND_LEN))
    else:
        palm = PIVOT + GUN_ORIGIN + LEFT_HAND
        wrist = palm + V((-HAND_LEN, 0.0, 0.0))
    elbow = solve_elbow(shoulder, wrist, UPPER_ARM, FOREARM, V((0.5 * side, -1.0, 0.35)))
    return shoulder, elbow, wrist, palm


def build_skeleton():
    """(name, parent, head, tail) of every bone, parents first."""
    bones = [
        ("hips", None, V((0, HIP_Y, 0)), V((0, 1.03, 0))),
        ("spine", "hips", V((0, 1.03, 0)), V((0, 1.20, 0))),
        ("chest", "spine", V((0, 1.20, 0)), V((0, 1.45, 0))),
        ("neck", "chest", V((0, 1.45, 0)), V((0, 1.56, 0))),
        ("head", "neck", V((0, 1.56, 0)), V((0, 1.78, 0))),
        ("shoulders", "chest", PIVOT, PIVOT + V((0, 0, -0.08))),
    ]
    for side, tag in ((1, "r"), (-1, "l")):
        shoulder, elbow, wrist, palm = arm_points(side)
        bones += [
            (f"upper_arm_{tag}", "shoulders", shoulder, elbow),
            (f"forearm_{tag}", f"upper_arm_{tag}", elbow, wrist),
            (f"hand_{tag}", f"forearm_{tag}", wrist, palm),
        ]
    for side, tag in ((1, "r"), (-1, "l")):
        x = HIP_X * side
        bones += [
            (f"thigh_{tag}", "hips", V((x, HIP_Y, 0)), V((x, KNEE_Y, 0))),
            (f"shin_{tag}", f"thigh_{tag}", V((x, KNEE_Y, 0)), V((x, ANKLE_Y, 0))),
            (f"foot_{tag}", f"shin_{tag}", V((x, ANKLE_Y, 0)), V((x, ANKLE_Y, -0.2))),
        ]
    return bones


BONES = build_skeleton()
SIDED = {"upper_arm", "forearm", "hand", "thigh", "shin", "foot"}

# --- mesh accumulator ---------------------------------------------------------------------------

SLOTS = ["uniform", "gear", "skin", "fixed", "team"]
UNIFORM, GEAR, SKIN, FIXED, TEAM = range(5)

BOOT = (0.12, 0.10, 0.085)
GLOVE = (0.07, 0.07, 0.075)
DARK = (0.05, 0.05, 0.055)
WEBBING = (0.62, 0.62, 0.62)


def wk(side, weights):
    """Bone weights with the sided bones given their `_r`/`_l` suffix."""
    tag = "r" if side > 0 else "l"
    return {(f"{name}_{tag}" if name in SIDED else name): w for name, w in weights.items()}


def mirror(v):
    return V((-v.x, v.y, v.z))


class Ring:
    """One cross-section of a loft: a superellipse of half-widths `a` (along `side`) and `b` (along
    the other in-plane axis) round `centre`, in the plane across `axis`."""

    def __init__(self, centre, axis, a, b, weights, p=2.4, n=12, color=None, side=(1, 0, 0)):
        self.centre = V(centre)
        self.axis = V(axis).normalized()
        self.a, self.b, self.p, self.n = a, b, p, n
        self.weights = weights
        self.color = color
        self.side = V(side)


class Acc:
    """Vertices, faces, vertex colours and skin weights of one Blender object."""

    def __init__(self):
        self.verts = []
        self.cols = []
        self.weights = []
        self.faces = []
        self.slots = []

    def loft(self, rings, slot, color=(1, 1, 1), caps=(True, True), taper_ends=False):
        """A closed tube through `rings` (all the same `n`)."""
        grid = []
        for ring in rings:
            w = ring.axis
            u = ring.side - w * ring.side.dot(w)
            if u.length < 1e-4:
                u = V((0, 0, 1)) - w * w.z
            u.normalize()
            v = w.cross(u)
            row = []
            for k in range(ring.n):
                t = 2 * math.pi * k / ring.n
                c, s = math.cos(t), math.sin(t)
                e = 2.0 / ring.p
                x = math.copysign(abs(c) ** e, c) * ring.a
                y = math.copysign(abs(s) ** e, s) * ring.b
                row.append(len(self.verts))
                self.verts.append(ring.centre + u * x + v * y)
                self.cols.append(ring.color or color)
                self.weights.append(dict(ring.weights))
            grid.append(row)
        for r in range(len(grid) - 1):
            n = len(grid[r])
            for k in range(n):
                self.faces.append((grid[r][k], grid[r][(k + 1) % n], grid[r + 1][(k + 1) % n], grid[r + 1][k]))
                self.slots.append(slot)
        if caps[0]:
            self.faces.append(tuple(grid[0]))
            self.slots.append(slot)
        if caps[1]:
            self.faces.append(tuple(grid[-1]))
            self.slots.append(slot)
        return grid

    def box(self, centre, size, slot, weights, color=(1, 1, 1), round_=0.01, axis=(0, 1, 0), side=(1, 0, 0), p=4.0, n=8):
        """A rounded box `size` (x, y, z, along the global axes unless turned by `axis`/`side`:
        the long dimension is `size[1]`)."""
        sx, sy, sz = size
        w = V(axis).normalized()
        half = sy / 2
        r = min(round_, half * 0.8)
        # The ends narrow by `r` so the edges are chamfered.
        shape = [(-half, -r), (-half + r, 0.0), (half - r, 0.0), (half, -r)]
        rings = [
            Ring(V(centre) + w * off, w, sx / 2 + shrink, sz / 2 + shrink, weights, p=p, n=n, color=color, side=side)
            for off, shrink in shape
        ]
        for ring in rings:
            ring.a = max(ring.a, 0.002)
            ring.b = max(ring.b, 0.002)
        return self.loft(rings, slot)

    def add(self, other):
        base = len(self.verts)
        self.verts += other.verts
        self.cols += other.cols
        self.weights += other.weights
        self.faces += [tuple(i + base for i in f) for f in other.faces]
        self.slots += other.slots


def lerp(a, b, t):
    return a + (b - a) * t


def shade(color, f):
    return tuple(c * f for c in color)


# --- the body -----------------------------------------------------------------------------------


def torso(acc):
    spec = [
        # y, half-width, half-depth, centre z, weights
        (0.84, 0.186, 0.130, 0.000, {"hips": 1}),
        (0.92, 0.176, 0.118, 0.000, {"hips": 0.7, "spine": 0.3}),
        (1.02, 0.170, 0.110, 0.000, {"spine": 1}),
        (1.14, 0.180, 0.118, -0.004, {"spine": 0.5, "chest": 0.5}),
        (1.27, 0.196, 0.126, -0.006, {"chest": 1}),
        (1.36, 0.208, 0.118, 0.000, {"chest": 1}),
        (1.42, 0.196, 0.100, 0.004, {"chest": 1}),
        (1.46, 0.125, 0.086, 0.004, {"chest": 1}),
        (1.495, 0.074, 0.072, 0.006, {"chest": 0.5, "neck": 0.5}),
    ]
    rings = [Ring((0, y, zc), (0, 1, 0), a, b, w, p=2.4, n=14) for y, a, b, zc, w in spec]
    acc.loft(rings, UNIFORM)


def legs(acc):
    for side in (1, -1):
        x = HIP_X * side
        spec = [
            # y, half-width, half-depth, centre z, weights
            (1.00, 0.108, 0.122, 0.0, {"hips": 1}),
            (0.92, 0.106, 0.120, 0.0, {"hips": 0.5, "thigh": 0.5}),
            (0.80, 0.102, 0.114, 0.0, {"thigh": 1}),
            (0.66, 0.090, 0.098, -0.002, {"thigh": 1}),
            (0.55, 0.079, 0.087, -0.004, {"thigh": 0.85, "shin": 0.15}),
            (0.47, 0.076, 0.085, -0.006, {"thigh": 0.5, "shin": 0.5}),
            (0.40, 0.073, 0.081, -0.004, {"thigh": 0.15, "shin": 0.85}),
            (0.30, 0.067, 0.073, 0.0, {"shin": 1}),
            (0.23, 0.066, 0.072, 0.0, {"shin": 1}),
            (0.205, 0.075, 0.080, 0.0, {"shin": 1}),  # blouses over the boot top
            (0.17, 0.068, 0.073, 0.0, {"shin": 0.8, "foot": 0.2}),
        ]
        rings = [
            Ring((x, y, zc), (0, 1, 0), a, b, wk(side, w), p=2.4, n=12) for y, a, b, zc, w in spec
        ]
        acc.loft(rings, UNIFORM)
        boots(acc, side)


def boots(acc, side):
    x = HIP_X * side
    shaft = [
        (0.255, 0.057, 0.063, {"shin": 1}),
        (0.20, 0.058, 0.064, {"shin": 0.7, "foot": 0.3}),
        (0.12, 0.059, 0.068, {"shin": 0.4, "foot": 0.6}),
        (0.06, 0.059, 0.072, {"foot": 1}),
    ]
    rings = [Ring((x, y, 0.0), (0, 1, 0), a, b, wk(side, w), p=2.6, n=10, color=BOOT) for y, a, b, w in shaft]
    acc.loft(rings, FIXED)
    # The foot: sections along the sole, heel to toe (axis -Z).
    foot = [
        (0.084, 0.042, 0.052, 0.058),
        (0.050, 0.050, 0.058, 0.062),
        (-0.020, 0.054, 0.054, 0.058),
        (-0.100, 0.058, 0.046, 0.052),
        (-0.170, 0.056, 0.040, 0.046),
        (-0.225, 0.044, 0.034, 0.040),
        (-0.246, 0.026, 0.024, 0.032),
    ]
    rings = [
        Ring((x, yc, z), (0, 0, -1), a, h, wk(side, {"foot": 1}), p=2.8, n=10, color=BOOT, side=(1, 0, 0))
        for z, a, h, yc in foot
    ]
    acc.loft(rings, FIXED)
    # A darker sole, a little wider than the upper.
    sole = [
        (0.090, 0.046, 0.016, 0.016),
        (-0.100, 0.061, 0.016, 0.016),
        (-0.248, 0.040, 0.014, 0.014),
    ]
    rings = [
        Ring((x, yc, z), (0, 0, -1), a, h, wk(side, {"foot": 1}), p=3.0, n=8, color=shade(BOOT, 0.4), side=(1, 0, 0))
        for z, a, h, yc in sole
    ]
    acc.loft(rings, FIXED)


def arms(acc):
    for side in (1, -1):
        shoulder, elbow, wrist, palm = arm_points(side)
        # Start inside the torso, above the shoulder joint, so the shoulder is rounded.
        start = shoulder + V((-0.03 * side, 0.025, 0.0))
        pts = [
            (start, 0.074, {"chest": 0.35, "upper_arm": 0.65}),
            (shoulder, 0.071, {"chest": 0.1, "upper_arm": 0.9}),
            (shoulder.lerp(elbow, 0.3), 0.064, {"upper_arm": 1}),
            (shoulder.lerp(elbow, 0.7), 0.057, {"upper_arm": 1}),
            (elbow.lerp(shoulder, 0.12), 0.055, {"upper_arm": 0.75, "forearm": 0.25}),
            (elbow, 0.055, {"upper_arm": 0.5, "forearm": 0.5}),
            (elbow.lerp(wrist, 0.12), 0.053, {"upper_arm": 0.25, "forearm": 0.75}),
            (elbow.lerp(wrist, 0.5), 0.047, {"forearm": 1}),
            (elbow.lerp(wrist, 0.9), 0.040, {"forearm": 0.8, "hand": 0.2}),
            (wrist + (wrist - elbow).normalized() * 0.02, 0.040, {"forearm": 0.5, "hand": 0.5}),
        ]
        rings = []
        for i, (c, r, w) in enumerate(pts):
            prev = pts[max(i - 1, 0)][0]
            nxt = pts[min(i + 1, len(pts) - 1)][0]
            axis = nxt - prev
            rings.append(Ring(c, axis, r, r * 0.95, wk(side, w), p=2.2, n=10, side=(0, 1, 0) if abs(axis.x) > 0.8 else (1, 0, 0)))
        acc.loft(rings, UNIFORM)
        glove(acc, side)
        armband(acc, side, shoulder, elbow)


def armband(acc, side, shoulder, elbow):
    axis = elbow - shoulder
    rings = []
    for t in (0.36, 0.56):
        c = shoulder.lerp(elbow, t)
        r = lerp(0.064, 0.057, t) + 0.006
        rings.append(Ring(c, axis, r, r * 0.95, wk(side, {"upper_arm": 1}), p=2.2, n=10))
    acc.loft(rings, TEAM)


def glove(acc, side):
    """The hand in the glove round the grip (right) or under the handguard (left)."""
    shoulder, elbow, wrist, palm = arm_points(side)
    tag = "r" if side > 0 else "l"
    w = {f"hand_{tag}": 1}
    if side > 0:
        # Fingers forward (-Z), curling to the left (-X) round the grip; palm faces left.
        forward = V((0, 0, -1))
        rings = [
            Ring(palm + V((0, 0, 0.045)), forward, 0.026, 0.047, w, p=2.8, n=8, color=GLOVE, side=(1, 0, 0)),
            Ring(palm + V((0, 0, 0.0)), forward, 0.029, 0.050, w, p=2.8, n=8, color=GLOVE, side=(1, 0, 0)),
            Ring(palm + V((-0.01, 0, -0.04)), forward, 0.029, 0.050, w, p=2.8, n=8, color=GLOVE, side=(1, 0, 0)),
            Ring(palm + V((-0.03, 0, -0.06)), V((-1, 0, -1)), 0.024, 0.046, w, p=2.8, n=8, color=GLOVE, side=(0, 1, 0)),
        ]
        acc.loft(rings, FIXED)
        # Thumb up the grip.
        acc.loft(
            [
                Ring(palm + V((-0.002, 0.04, 0.02)), (0, 1, 0), 0.014, 0.014, w, p=2.4, n=8, color=GLOVE),
                Ring(palm + V((-0.01, 0.07, -0.01)), (0, 1, -0.3), 0.012, 0.012, w, p=2.4, n=8, color=GLOVE),
            ],
            FIXED,
        )
    else:
        # Palm up under the handguard, fingers to the right (+X) curling up over it.
        axis = V((1, 0, 0))
        rings = [
            Ring(palm + V((-0.045, 0, 0)), axis, 0.026, 0.047, w, p=2.8, n=8, color=GLOVE, side=(0, 0, 1)),
            Ring(palm + V((0.0, 0, 0)), axis, 0.030, 0.050, w, p=2.8, n=8, color=GLOVE, side=(0, 0, 1)),
            Ring(palm + V((0.04, 0.005, 0)), axis, 0.030, 0.050, w, p=2.8, n=8, color=GLOVE, side=(0, 0, 1)),
            Ring(palm + V((0.06, 0.025, 0)), V((1, 1, 0)), 0.024, 0.046, w, p=2.8, n=8, color=GLOVE, side=(0, 0, 1)),
        ]
        acc.loft(rings, FIXED)
        acc.loft(
            [
                Ring(palm + V((0.0, 0.0, -0.045)), (0, 0, -1), 0.014, 0.014, w, p=2.4, n=8, color=GLOVE),
                Ring(palm + V((0.01, 0.01, -0.075)), (0, 0, -1), 0.012, 0.012, w, p=2.4, n=8, color=GLOVE),
            ],
            FIXED,
        )


def head(acc):
    """Neck and head (bare), the face turned to -Z, with the facial marks."""
    neck = [
        (1.44, 0.060, 0.062, {"chest": 0.5, "neck": 0.5}),
        (1.50, 0.052, 0.056, {"neck": 1}),
        (1.56, 0.050, 0.054, {"neck": 0.5, "head": 0.5}),
        (1.585, 0.050, 0.056, {"head": 1}),
    ]
    acc.loft([Ring((0, y, 0.004), (0, 1, 0), a, b, w, p=2.4, n=10) for y, a, b, w in neck], SKIN)
    skull = [
        # y, half-width, half-depth, centre z
        (1.545, 0.040, 0.050, -0.030),
        (1.572, 0.058, 0.074, -0.022),
        (1.605, 0.071, 0.090, -0.010),
        (1.645, 0.076, 0.100, 0.000),
        (1.685, 0.077, 0.104, 0.004),
        (1.725, 0.070, 0.098, 0.006),
        (1.762, 0.052, 0.078, 0.006),
        (1.784, 0.022, 0.040, 0.006),
    ]
    acc.loft([Ring((0, y, zc), (0, 1, 0), a, b, {"head": 1}, p=2.2, n=12) for y, a, b, zc in skull], SKIN)
    # Nose, ears, brow ridge.
    acc.box((0, 1.633, -0.104), (0.024, 0.045, 0.030), SKIN, {"head": 1}, color=(0.95, 0.88, 0.85), round_=0.008)
    for side in (1, -1):
        acc.box((0.077 * side, 1.650, 0.004), (0.014, 0.05, 0.03), SKIN, {"head": 1}, color=(0.92, 0.85, 0.82), round_=0.006)
    acc.box((0, 1.672, -0.094), (0.15, 0.016, 0.022), SKIN, {"head": 1}, color=(0.93, 0.86, 0.83), round_=0.005)
    # Eyes, brows and mouth: small dark shapes laid on the face.
    for side in (1, -1):
        acc.box((0.034 * side, 1.657, -0.1005), (0.026, 0.009, 0.006), FIXED, {"head": 1}, color=(0.035, 0.03, 0.03), round_=0.002, axis=(0, 0, 1))
        acc.box((0.036 * side, 1.673, -0.1005), (0.036, 0.007, 0.006), FIXED, {"head": 1}, color=(0.09, 0.07, 0.05), round_=0.002, axis=(0, 0, 1))
    acc.box((0, 1.581, -0.0845), (0.040, 0.007, 0.006), FIXED, {"head": 1}, color=(0.34, 0.17, 0.15), round_=0.002, axis=(0, 0, 1))


# --- gear ---------------------------------------------------------------------------------------

POUCH = 0.78
WEB = (0.82, 0.82, 0.82)


def vest(acc):
    """Plate carrier with front and back plates, shoulder straps, cummerbund, and the pouches."""
    spec = [
        (1.045, 0.190, 0.133, 0.0, {"spine": 1}),
        (1.075, 0.200, 0.143, 0.0, {"spine": 1}),
        (1.17, 0.208, 0.150, -0.004, {"spine": 0.4, "chest": 0.6}),
        (1.30, 0.208, 0.150, -0.006, {"chest": 1}),
        (1.385, 0.196, 0.138, -0.004, {"chest": 1}),
        (1.41, 0.170, 0.120, 0.0, {"chest": 1}),
    ]
    rings = [Ring((0, y, zc), (0, 1, 0), a, b, w, p=3.4, n=12) for y, a, b, zc, w in spec]
    acc.loft(rings, GEAR, color=(0.92, 0.92, 0.92))
    # Shoulder straps over the jacket's shoulders.
    for side in (1, -1):
        acc.box((0.115 * side, 1.428, 0.0), (0.06, 0.016, 0.26), GEAR, {"chest": 1}, color=(0.85, 0.85, 0.85), round_=0.006, axis=(0, 0, 1), side=(1, 0, 0))
    # Cummerbund: a belt of webbing round the waist.
    belt = [
        (0.985, 0.178, 0.124, {"hips": 0.5, "spine": 0.5}),
        (1.045, 0.182, 0.128, {"spine": 1}),
    ]
    acc.loft([Ring((0, y, 0), (0, 1, 0), a, b, w, p=3.0, n=12) for y, a, b, w in belt], GEAR, color=shade(WEB, 0.7))
    # Magazine pouches on the front.
    for x in (-0.078, 0.0, 0.078):
        acc.box((x, 1.155, -0.166), (0.07, 0.125, 0.04), GEAR, {"spine": 0.5, "chest": 0.5}, color=shade((1, 1, 1), POUCH), round_=0.01)
        acc.box((x, 1.205, -0.168), (0.072, 0.03, 0.044), GEAR, {"spine": 0.5, "chest": 0.5}, color=shade((1, 1, 1), POUCH * 0.8), round_=0.008)
    # Admin pouch on the chest and radio on the left shoulder.
    acc.box((0.0, 1.31, -0.162), (0.14, 0.07, 0.03), GEAR, {"chest": 1}, color=shade((1, 1, 1), POUCH), round_=0.008)
    acc.box((-0.19, 1.20, -0.02), (0.05, 0.14, 0.10), GEAR, {"chest": 1}, color=shade((1, 1, 1), POUCH * 0.85), round_=0.01)
    # Hip pouches and the dump pouch.
    for side in (1, -1):
        acc.box((0.19 * side, 1.0, 0.02), (0.05, 0.10, 0.11), GEAR, {"hips": 1}, color=shade((1, 1, 1), POUCH), round_=0.01)
    # Team patches on the chest (front) and back, over the plates.
    acc.box((0.0, 1.355, -0.1695), (0.15, 0.045, 0.008), TEAM, {"chest": 1}, round_=0.003, axis=(0, 1, 0))
    acc.box((0.0, 1.255, 0.1585), (0.22, 0.17, 0.010), TEAM, {"chest": 1}, round_=0.004, axis=(0, 1, 0))


def pack(acc):
    """Assault pack on the back with a rolled mat on top."""
    acc.box((0.0, 1.21, 0.185), (0.30, 0.34, 0.105), GEAR, {"chest": 1}, color=shade((1, 1, 1), 0.88), round_=0.025)
    acc.box((0.0, 1.30, 0.245), (0.22, 0.20, 0.03), GEAR, {"chest": 1}, color=shade((1, 1, 1), 0.7), round_=0.01)
    mat = [
        Ring((x, 1.415, 0.20), (1, 0, 0), 0.055, 0.055, {"chest": 1}, p=2.2, n=10, color=shade((1, 1, 1), 0.6), side=(0, 1, 0))
        for x in (-0.15, 0.15)
    ]
    acc.loft(mat, GEAR)


def leg_gear(acc):
    for side in (1, -1):
        x = HIP_X * side
        # Cargo pocket on the outside of the thigh, knee pad on the shin, holster on the right.
        acc.box((x + 0.098 * side, 0.70, -0.01), (0.04, 0.15, 0.10), UNIFORM, wk(side, {"thigh": 1}), color=(0.86, 0.86, 0.86), round_=0.008)
        acc.box((x, 0.475, -0.083), (0.10, 0.115, 0.04), FIXED, wk(side, {"shin": 1}), color=shade(BOOT, 1.6), round_=0.012)
    acc.box((HIP_X + 0.105, 0.83, 0.0), (0.04, 0.18, 0.09), FIXED, {"thigh_r": 1}, color=shade(BOOT, 1.4), round_=0.01)


def build_body():
    acc = Acc()
    torso(acc)
    legs(acc)
    arms(acc)
    head(acc)
    vest(acc)
    pack(acc)
    leg_gear(acc)
    return acc


# --- headgear -----------------------------------------------------------------------------------

HEAD_C = V((0.0, 1.665, 0.006))


def dome(acc, slot, spec, weights=None, p=2.3, n=12, color=(1, 1, 1), centre=(0, 0, 0), tilt=(0, 1, 0)):
    """A cap: rings (y, half-width, half-depth, z offset) from the rim up."""
    weights = weights or {"head": 1}
    rings = [Ring(V(centre) + V((0, y, zc)), tilt, a, b, weights, p=p, n=n, color=color) for y, a, b, zc in spec]
    return acc.loft(rings, slot)


def headgear_helmet():
    acc = Acc()
    # Tilted so the rim sits lower at the back and over the ears than over the brow.
    dome(
        acc,
        0,
        [
            (1.622, 0.106, 0.130, 0.016),
            (1.658, 0.117, 0.141, 0.012),
            (1.706, 0.113, 0.136, 0.008),
            (1.758, 0.093, 0.114, 0.005),
            (1.795, 0.056, 0.074, 0.003),
            (1.810, 0.020, 0.032, 0.0),
        ],
        p=2.6,
        tilt=(0, 1, 0.30),
    )
    return acc


def headgear_beret():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.688, 0.100, 0.115, 0.004),
            (1.700, 0.118, 0.132, 0.004),
            (1.722, 0.134, 0.142, 0.004),
            (1.742, 0.118, 0.128, 0.004),
            (1.750, 0.060, 0.070, 0.004),
        ],
        centre=(0.014, 0, 0),
        p=2.2,
    )
    return acc


def headgear_boonie():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.680, 0.205, 0.215, 0.0),
            (1.690, 0.212, 0.222, 0.0),
            (1.708, 0.150, 0.160, 0.0),
            (1.722, 0.108, 0.122, 0.004),
            (1.770, 0.104, 0.118, 0.004),
            (1.795, 0.070, 0.082, 0.004),
            (1.804, 0.028, 0.034, 0.004),
        ],
        p=2.0,
        n=14,
    )
    return acc


def headgear_cap():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.690, 0.100, 0.118, 0.006),
            (1.720, 0.108, 0.124, 0.006),
            (1.760, 0.100, 0.116, 0.006),
            (1.790, 0.066, 0.080, 0.006),
            (1.800, 0.020, 0.030, 0.006),
        ],
        p=2.2,
    )
    # Peak.
    acc.box((0.0, 1.708, -0.150), (0.15, 0.014, 0.09), 0, {"head": 1}, color=shade((1, 1, 1), 0.85), round_=0.006, axis=(0, 0, 1))
    return acc


def headgear_beanie():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.665, 0.098, 0.118, 0.004),
            (1.690, 0.105, 0.124, 0.004),
            (1.722, 0.101, 0.120, 0.004),
            (1.760, 0.090, 0.108, 0.004),
            (1.790, 0.060, 0.074, 0.004),
            (1.804, 0.018, 0.026, 0.004),
        ],
        p=2.2,
    )
    return acc


def headgear_shemagh():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.655, 0.106, 0.126, 0.012),
            (1.690, 0.114, 0.134, 0.012),
            (1.735, 0.108, 0.128, 0.010),
            (1.775, 0.086, 0.104, 0.008),
            (1.800, 0.040, 0.056, 0.006),
        ],
        p=2.3,
    )
    # Scarf round the neck with a loose end down the chest.
    acc.loft(
        [
            Ring((0, 1.50, 0.006), (0, 1, 0), 0.098, 0.100, {"neck": 1}, p=2.3, n=12),
            Ring((0, 1.455, 0.004), (0, 1, 0), 0.115, 0.108, {"neck": 0.5, "chest": 0.5}, p=2.3, n=12),
        ],
        0,
    )
    acc.box((0.04, 1.40, -0.118), (0.10, 0.17, 0.016), 0, {"chest": 1}, color=shade((1, 1, 1), 0.92), round_=0.006, axis=(0, 1, 0))
    return acc


def headgear_ushanka():
    acc = Acc()
    dome(
        acc,
        0,
        [
            (1.668, 0.112, 0.130, 0.008),
            (1.700, 0.122, 0.140, 0.008),
            (1.742, 0.116, 0.134, 0.006),
            (1.782, 0.090, 0.106, 0.004),
            (1.805, 0.040, 0.054, 0.004),
        ],
        p=2.3,
    )
    for side in (1, -1):
        acc.box((0.117 * side, 1.665, 0.014), (0.038, 0.115, 0.14), 0, {"head": 1}, color=shade((1, 1, 1), 1.2), round_=0.012)
    acc.box((0.0, 1.715, -0.128), (0.20, 0.04, 0.045), 0, {"head": 1}, color=shade((1, 1, 1), 1.25), round_=0.012, axis=(0, 0, 1))
    return acc


def mask_acc():
    """A balaclava over the lower face and the back of the head, leaving a strip for the eyes."""
    acc = Acc()
    skull = [
        (1.535, 0.044, 0.054, -0.030),
        (1.570, 0.063, 0.079, -0.022),
        (1.605, 0.075, 0.095, -0.010),
        (1.640, 0.080, 0.105, 0.000),
    ]
    rings = [Ring((0, y, zc), (0, 1, 0), a, b, {"head": 1}, p=2.2, n=12) for y, a, b, zc in skull]
    acc.loft(rings, 0, caps=(True, True))
    return acc


def goggles_acc():
    acc = Acc()
    acc.box((0.0, 1.668, -0.088), (0.176, 0.050, 0.044), 0, {"head": 1}, color=(0.05, 0.05, 0.055), round_=0.012, axis=(0, 0, 1))
    acc.box((0.0, 1.668, 0.015), (0.178, 0.026, 0.20), 0, {"head": 1}, color=(0.05, 0.05, 0.055), round_=0.01, axis=(0, 0, 1))
    acc.box((0.0, 1.668, -0.1115), (0.150, 0.036, 0.004), 0, {"head": 1}, color=(0.12, 0.17, 0.2), round_=0.001, axis=(0, 0, 1))
    return acc


HEADGEAR = {
    "headgear_helmet": headgear_helmet,
    "headgear_beret": headgear_beret,
    "headgear_boonie": headgear_boonie,
    "headgear_cap": headgear_cap,
    "headgear_beanie": headgear_beanie,
    "headgear_shemagh": headgear_shemagh,
    "headgear_ushanka": headgear_ushanka,
}


def helmet_fittings():
    """The dark fittings of the helmet: NVG mount, side rails, rear pouch, as a second object."""
    acc = Acc()
    acc.box((0.0, 1.745, -0.146), (0.05, 0.05, 0.03), 0, {"head": 1}, color=DARK, round_=0.008, axis=(0, 0, 1))
    for side in (1, -1):
        acc.box((0.119 * side, 1.665, -0.01), (0.016, 0.036, 0.10), 0, {"head": 1}, color=DARK, round_=0.006, axis=(0, 0, 1))
    acc.box((0.0, 1.668, 0.138), (0.10, 0.05, 0.03), 0, {"head": 1}, color=shade(BOOT, 0.7), round_=0.008, axis=(0, 0, 1))
    return acc


# --- Blender objects ----------------------------------------------------------------------------


def to_blender(p):
    return V((p.x, -p.z, p.y))


def make_mesh_object(name, acc, armature, materials, slot_of=None):
    """A skinned mesh object from `acc`. `slot_of` maps an `Acc` slot index to a material."""
    mesh = bpy.data.meshes.new(name)
    verts = [to_blender(p) for p in acc.verts]
    mesh.from_pydata([tuple(v) for v in verts], [], [tuple(f) for f in acc.faces])
    mesh.update()
    for m in materials:
        mesh.materials.append(m)
    for poly, slot in zip(mesh.polygons, acc.slots):
        poly.material_index = slot
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    # Outward normals, per connected shell.
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()
    names = {b[0] for b in BONES}
    for bone in names:
        obj.vertex_groups.new(name=bone)
    for i, weights in enumerate(acc.weights):
        total = sum(weights.values())
        for bone, w in weights.items():
            assert bone in names, bone
            obj.vertex_groups[bone].add([i], w / total, "REPLACE")
    modifier = obj.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature
    obj.parent = armature
    return obj


def make_armature():
    arm_data = bpy.data.armatures.new("soldier")
    arm = bpy.data.objects.new("soldier", arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    for name, parent, head, tail in BONES:
        bone = arm_data.edit_bones.new(name)
        bone.head = to_blender(head)
        bone.tail = to_blender(tail)
        # Roll so every bone's local frame is the same as the model's, as far as bones can.
        bone.roll = 0.0
        if parent:
            bone.parent = arm_data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    return arm


def vertex_ao(occluders, targets, rays=24, distance=0.22, strength=0.85):
    """Per-vertex ambient occlusion of each of `targets` against all `occluders` together."""
    bm_all = bmesh.new()
    for obj in occluders:
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        bmesh.ops.triangulate(bm, faces=bm.faces)
        bm.transform(obj.matrix_world)
        mesh = bpy.data.meshes.new("ao")
        bm.to_mesh(mesh)
        bm.free()
        bm_all.from_mesh(mesh)
        bpy.data.meshes.remove(mesh)
    tree = BVHTree.FromBMesh(bm_all)
    bm_all.free()
    rng = random.Random(7)
    # Cosine-weighted hemisphere directions about +Z, rotated onto each normal.
    dirs = []
    for _ in range(rays):
        u1, u2 = rng.random(), rng.random()
        r = math.sqrt(u1)
        phi = 2 * math.pi * u2
        dirs.append(V((r * math.cos(phi), r * math.sin(phi), math.sqrt(max(0.0, 1 - u1)))))
    for obj in targets:
        mesh = obj.data
        ao = []
        for v in mesh.vertices:
            n = (obj.matrix_world.to_3x3() @ v.normal).normalized()
            origin = obj.matrix_world @ v.co + n * 0.004
            t = n.cross(V((0, 0, 1)) if abs(n.z) < 0.9 else V((1, 0, 0))).normalized()
            b = n.cross(t)
            hit = 0.0
            for d in dirs:
                w = t * d.x + b * d.y + n * d.z
                result = tree.ray_cast(origin, w, distance)
                if result[0] is not None:
                    hit += 1.0 - result[3] / distance * 0.6
            ao.append(1.0 - strength * hit / rays)
        yield obj, ao


def paint_colors(obj, acc, ao):
    """Vertex colours: the pre-set colour times the occlusion."""
    mesh = obj.data
    layer = mesh.color_attributes.new("Color", "FLOAT_COLOR", "POINT")
    for i, v in enumerate(mesh.vertices):
        r, g, b = acc.cols[i]
        f = ao[i]
        layer.data[i].color = (r * f, g * f, b * f, 1.0)


def make_materials(fabric):
    image = bpy.data.images.load(fabric)
    image.colorspace_settings.name = "sRGB"
    defaults = {
        "uniform": ((0.36, 0.38, 0.27), True),
        "gear": ((0.38, 0.33, 0.24), True),
        "skin": ((0.78, 0.60, 0.48), True),
        "fixed": ((1.0, 1.0, 1.0), False),
        "team": ((0.55, 0.55, 0.55), False),
        "headgear": ((0.40, 0.36, 0.26), True),
        "mask": ((0.1, 0.11, 0.08), True),
    }
    made = {}
    for name, (color, textured) in defaults.items():
        mat = bpy.data.materials.new(name)
        mat.use_nodes = True
        nodes = mat.node_tree.nodes
        bsdf = nodes["Principled BSDF"]
        srgb = tuple(c ** 2.2 for c in color)
        bsdf.inputs["Base Color"].default_value = (*srgb, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.9
        if textured:
            tex = nodes.new("ShaderNodeTexImage")
            tex.image = image
            mix = nodes.new("ShaderNodeMix")
            mix.data_type = "RGBA"
            mix.blend_type = "MULTIPLY"
            mix.inputs[0].default_value = 1.0
            mix.inputs[7].default_value = (*srgb, 1.0)
            mat.node_tree.links.new(tex.outputs["Color"], mix.inputs[6])
            mat.node_tree.links.new(mix.outputs[2], bsdf.inputs["Base Color"])
        made[name] = mat
    return made


def box_uvs(obj, scale=1 / 0.5):
    """Per-face box-projected UVs (constant texel density, no seams to see in a fabric noise)."""
    mesh = obj.data
    uv = mesh.uv_layers.new(name="UVMap")
    for poly in mesh.polygons:
        n = poly.normal
        ax = max(range(3), key=lambda i: abs(n[i]))
        for li in poly.loop_indices:
            co = mesh.vertices[mesh.loops[li].vertex_index].co
            if ax == 0:
                uv.data[li].uv = (co.y * scale, co.z * scale)
            elif ax == 1:
                uv.data[li].uv = (co.x * scale, co.z * scale)
            else:
                uv.data[li].uv = (co.x * scale, co.y * scale)


def smooth(obj, angle=math.radians(38)):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.shade_smooth_by_angle(angle=angle)


def triangles(obj):
    return sum(len(p.vertices) - 2 for p in obj.data.polygons)


def build(fabric):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    arm = make_armature()
    mats = make_materials(fabric)
    body_acc = build_body()
    body_slots = [mats[n] for n in SLOTS]
    parts = [("soldier_body", body_acc, body_slots)]

    def single(acc, mat_names):
        return [mats[n] for n in mat_names]

    for name, make in HEADGEAR.items():
        acc = make()
        slots = ["headgear"]
        if name == "headgear_helmet":
            fit = helmet_fittings()
            acc.add(fit)
            slots = ["headgear", "fixed"]
            # Fittings are slot 0 in their own Acc; move them to the second material.
            n_fit = len(fit.faces)
            for i in range(len(acc.slots) - n_fit, len(acc.slots)):
                acc.slots[i] = 1
        parts.append((name, acc, single(acc, slots)))
    mask = mask_acc()
    parts.append(("mask", mask, single(mask, ["mask"])))
    goggles = goggles_acc()
    parts.append(("goggles", goggles, single(goggles, ["fixed"])))

    objs = []
    for name, acc, mat_list in parts:
        obj = make_mesh_object(name, acc, arm, mat_list)
        objs.append((obj, acc))
    body = objs[0][0]
    for obj, acc in objs:
        occluders = [body] if obj is body else [body, obj]
        (_, ao), = vertex_ao(occluders, [obj])
        paint_colors(obj, acc, ao)
        box_uvs(obj)
        smooth(obj)
    return arm, [o for o, _ in objs]


def add_gun_mount(arm):
    """An empty on the `shoulders` bone where the held rifle's origin goes."""
    bpy.context.view_layer.update()
    mount = bpy.data.objects.new("gun_mount", None)
    bpy.context.scene.collection.objects.link(mount)
    mount.parent = arm
    mount.parent_type = "BONE"
    mount.parent_bone = "shoulders"
    mount.matrix_world = Matrix.Translation(to_blender(PIVOT + GUN_ORIGIN))
    return mount


def export(path, objects, arm):
    bpy.ops.object.select_all(action="DESELECT")
    arm.select_set(True)
    add_gun_mount(arm).select_set(True)
    for o in objects:
        o.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_skins=True,
        export_animations=False,
        export_yup=True,
        export_vertex_color="ACTIVE",
        export_normals=True,
        export_image_format="AUTO",
        export_cameras=False,
        export_lights=False,
    )


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--fabric", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--preview")
    parser.add_argument("--gun")
    args = parser.parse_args(argv)
    arm, objects = build(args.fabric)
    total = sum(triangles(o) for o in objects)
    body = triangles(objects[0])
    print(f"SOLDIER triangles: body {body}, all {total}")
    for o in objects:
        print(f"  {o.name}: {triangles(o)}")
    export(args.out, objects, arm)
    print("SOLDIER wrote", args.out)


main()
