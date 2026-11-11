"""Run inside Blender: renders the rest pose of `soldier.glb` with the rifle in its hands from the
front, side, back and head close-up to PNGs, to look at while building the model.

    blender -b --python tools/assets/blender_soldier_preview.py -- --glb soldier.glb \
        --gun crates/frontline/assets/models/weapons/m4.glb --out /tmp/prefix [--tint 0.36,0.38,0.27]

Writes `<out>_front.png`, `_side.png`, `_back.png`, `_three.png` and `_head.png`.
The game draws the soldier with the Faction's tints and its own lighting: this only checks shape,
proportions, joints and where the rifle sits.
"""

import argparse
import math
import sys

import bpy
from mathutils import Matrix, Vector

V = Vector

# Where the rifle is in model space: the shoulder pivot plus GUN_ORIGIN, then turned a quarter about
# Y like `GunRig::model_transform`. Keep in step with blender_soldier.py and soldier.rs.
GUN_ORIGIN = V((0.10, -0.15, -0.24))
PIVOT = V((0.0, 1.40, 0.0))

C = Matrix(((1, 0, 0, 0), (0, 0, -1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))


def bevy_to_blender(m):
    return C @ m @ C.inverted()


def look_at(obj, target):
    direction = target - obj.location
    obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :]
    parser = argparse.ArgumentParser()
    parser.add_argument("--glb", required=True)
    parser.add_argument("--gun")
    parser.add_argument("--out", required=True)
    parser.add_argument("--size", type=int, default=720)
    args = parser.parse_args(argv)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=args.glb)
    if args.gun:
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=args.gun)
        new = [o for o in bpy.data.objects if o not in before]
        roots = [o for o in new if o.parent is None]
        place = Matrix.Translation(PIVOT + GUN_ORIGIN) @ Matrix.Rotation(math.pi / 2, 4, "Y")
        for root in roots:
            root.matrix_world = bevy_to_blender(place) @ root.matrix_world

    # Vertex colours into the materials.
    for obj in bpy.data.objects:
        if obj.type != "MESH" or not obj.data.color_attributes:
            continue
        layer = obj.data.color_attributes[0].name
        for slot in obj.material_slots:
            mat = slot.material
            if not mat or not mat.use_nodes:
                continue
            nodes, links = mat.node_tree.nodes, mat.node_tree.links
            bsdf = nodes.get("Principled BSDF")
            if bsdf is None:
                continue
            color = nodes.new("ShaderNodeVertexColor")
            color.layer_name = layer
            mix = nodes.new("ShaderNodeMix")
            mix.data_type = "RGBA"
            mix.blend_type = "MULTIPLY"
            mix.inputs[0].default_value = 1.0
            base = bsdf.inputs["Base Color"]
            if base.links:
                links.new(base.links[0].from_socket, mix.inputs[6])
            else:
                mix.inputs[6].default_value = base.default_value
            links.new(color.outputs["Color"], mix.inputs[7])
            links.new(mix.outputs[2], base)

    # Hide the headgear variants but the helmet.
    for obj in bpy.data.objects:
        if obj.name.startswith("headgear_") and obj.name != "headgear_helmet":
            obj.hide_render = True
        if obj.name in ("mask",):
            obj.hide_render = True

    scene = bpy.context.scene
    try:
        scene.render.engine = "BLENDER_EEVEE"
    except TypeError:
        scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = args.size
    scene.render.resolution_y = int(args.size * 1.25)
    scene.render.film_transparent = False
    scene.view_settings.view_transform = "Standard"
    world = bpy.data.worlds.new("w")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.42, 0.48, 0.55, 1)
    bg.inputs[1].default_value = 1.0

    ground = bpy.data.objects.new("ground", bpy.data.meshes.new("ground"))
    bpy.context.scene.collection.objects.link(ground)
    import bmesh

    bm = bmesh.new()
    bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=3.0)
    bm.to_mesh(ground.data)
    bm.free()
    gm = bpy.data.materials.new("ground")
    gm.diffuse_color = (0.30, 0.28, 0.24, 1)
    gm.use_nodes = True
    gm.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.20, 0.19, 0.16, 1)
    ground.data.materials.append(gm)

    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 3.2
    sun.rotation_euler = (math.radians(50), 0, math.radians(35))
    bpy.context.scene.collection.objects.link(sun)

    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.type = "ORTHO"
    bpy.context.scene.collection.objects.link(cam)
    scene.camera = cam

    def shoot(name, position, target, scale):
        # Positions are in model space (y up, -z forward): convert.
        cam.location = V((position[0], -position[2], position[1]))
        look_at(cam, V((target[0], -target[2], target[1])))
        cam.data.ortho_scale = scale
        scene.render.filepath = f"{args.out}_{name}.png"
        bpy.ops.render.render(write_still=True)

    mid = (0.0, 0.92, 0.0)
    shoot("front", (0.0, 0.92, -6.0), mid, 2.3)
    shoot("side", (6.0, 0.92, 0.0), mid, 2.3)
    shoot("back", (0.0, 0.92, 6.0), mid, 2.3)
    shoot("three", (4.2, 1.6, -4.2), mid, 2.3)
    shoot("head", (1.6, 1.8, -4.2), (0.0, 1.55, 0.0), 0.75)


main()
