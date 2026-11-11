"""Run inside Blender: renders a rifle from `m4.glb` or `m16.glb` from the left, the right, the front
three-quarter and the first-person angle (behind the stock, along the barrel) into one contact sheet,
to look at while building the models; with `--rig` it adds coloured spheres at the points rig.rs and
weapons.ron expect (muzzle, optic point, iron sight, hands, magazine, underbarrel) so the fit shows.

    blender -b --python tools/assets/blender_rifles_preview.py -- \
        --glb crates/frontline/assets/models/weapons/m4.glb --rifle m4 --out /tmp/m4_sheet.png [--rig]

Needs ImageMagick (`magick`) to join the four renders. The game's own lighting and camera differ:
this only checks shape, proportions and the rig's reference points.
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

import bpy
from mathutils import Vector

# Gun space (x right, y up, z back) points per rifle: weapons.ron `muzzle_offset`, `sight_point`,
# `iron_point`; rig.rs `left`, `left_mag`, `left_grip`, `underbarrel` and the grip (`right`).
# (name, colour, point)
POINTS = {
    "m4": [
        ("muzzle", (1, 0, 0), (0.0, 0.033, -0.500)),
        ("optic", (0, 1, 0), (0.0, 0.088, -0.07)),
        ("iron", (1, 1, 0), (0.0, 0.078, 0.045)),
        ("right hand", (0, 0.5, 1), (0.034, -0.05, 0.052)),
        ("support", (1, 0, 1), (-0.02, -0.006, -0.225)),
        ("mag hand", (0, 1, 1), (-0.034, -0.10, -0.032)),
        ("underbarrel", (1, 0.5, 0), (0.0, 0.010, -0.24)),
    ],
    "m16": [
        ("muzzle", (1, 0, 0), (0.0, 0.033, -0.580)),
        ("optic", (0, 1, 0), (0.0, 0.088, -0.07)),
        ("iron", (1, 1, 0), (0.0, 0.092, 0.089)),
        ("right hand", (0, 0.5, 1), (0.034, -0.05, 0.052)),
        ("support", (1, 0, 1), (-0.02, -0.006, -0.27)),
        ("mag hand", (0, 1, 1), (-0.034, -0.10, -0.032)),
        ("underbarrel", (1, 0.5, 0), (0.0, 0.008, -0.27)),
    ],
}


def G(p):
    """Gun space to Blender: the same mapping blender_rifles.py authors with (glTF import undoes the export)."""
    return Vector((-p[2], -p[0], p[1]))


def look(cam, target):
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :]
    parser = argparse.ArgumentParser()
    parser.add_argument("--glb", required=True)
    parser.add_argument("--rifle", choices=("m4", "m16"), required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--rig", action="store_true")
    parser.add_argument("--width", type=int, default=450)
    parser.add_argument("--view", help="render only this view (left, right, three, fp, rear_left, rear_right) instead of the sheet")
    args = parser.parse_args(argv)
    width, height = args.width, args.width * 2 // 3

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=args.glb)

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x, scene.render.resolution_y = width, height
    scene.render.film_transparent = False
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.22, 0.24, 0.27, 1)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
    scene.world = world

    for loc, energy in (((-1.0, -0.6, 1.4), 3.0), ((1.0, 0.8, 0.9), 1.2), ((-0.2, 1.0, -0.4), 1.2)):
        sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
        sun.data.energy = energy
        sun.location = Vector(loc)
        sun.rotation_euler = (Vector((0, 0, 0)) - sun.location).to_track_quat("-Z", "Y").to_euler()
        scene.collection.objects.link(sun)

    if args.rig:
        for name, colour, point in POINTS[args.rifle]:
            bpy.ops.mesh.primitive_uv_sphere_add(radius=0.0045, location=G(point), segments=12, ring_count=8)
            ball = bpy.context.object
            ball.name = f"rig_{name}"
            mat = bpy.data.materials.new(name)
            mat.use_nodes = True
            shader = mat.node_tree.nodes["Principled BSDF"]
            shader.inputs["Base Color"].default_value = (*colour, 1)
            shader.inputs["Emission Color"].default_value = (*colour, 1)
            shader.inputs["Emission Strength"].default_value = 2.0
            ball.data.materials.append(mat)

    cam_data = bpy.data.cameras.new("cam")
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.clip_start = 0.01
    cam_data.clip_end = 20

    length = 1.0 if args.rifle == "m16" else 0.9
    centre = (0.0, 0.01, -0.1 if args.rifle == "m16" else -0.07)
    views = [
        ("left", dict(ortho=length, at=(-1.0, 0.01, centre[2]), to=centre)),
        ("right", dict(ortho=length, at=(1.0, 0.01, centre[2]), to=centre)),
        ("three", dict(lens=48, at=(0.42, 0.20, -0.78), to=(0.0, 0.025, -0.12))),
        ("fp", dict(lens=30, at=(0.0, 0.12, 0.55), to=(0.0, 0.05, -0.5))),
        ("rear_left", dict(lens=60, at=(-0.30, 0.13, 0.30), to=(0.0, 0.01, -0.02))),
        ("rear_right", dict(lens=60, at=(0.30, 0.13, 0.30), to=(0.0, 0.01, -0.02))),
        ("front_left", dict(lens=60, at=(-0.30, 0.10, -0.62), to=(0.0, 0.03, -0.36))),
    ]
    if args.view:
        views = [v for v in views if v[0] == args.view]
    else:
        views = views[:4]
    tmp = Path(tempfile.mkdtemp())
    files = []
    for name, v in views:
        if "ortho" in v:
            cam_data.type = "ORTHO"
            cam_data.ortho_scale = v["ortho"]
        else:
            cam_data.type = "PERSP"
            cam_data.lens = v["lens"]
        cam.location = G(v["at"])
        look(cam, G(v["to"]))
        scene.render.filepath = str(tmp / f"{name}.png")
        bpy.ops.render.render(write_still=True)
        files.append(scene.render.filepath)
    if args.view:
        subprocess.run(["magick", files[0], args.out], check=True)
    else:
        subprocess.run(["magick", "montage", *files, "-tile", "2x2", "-geometry", "+0+0", args.out], check=True)
    for f in files:
        Path(f).unlink()
    print("SHEET", args.out)


main()
