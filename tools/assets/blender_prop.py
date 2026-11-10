"""Run inside Blender (`blender -b --python tools/assets/blender_prop.py -- ...`): turns a source
model (glTF) or a built-in generator into one game prop: a single mesh object with at most the
asked number of triangles, standing on the ground plane (lowest point at y 0, centred in x and z),
textures at most the asked size, exported as a .glb with JPEG textures. Optionally renders a
preview to look at.

    blender -b --python blender_prop.py -- --source in.gltf --out out.glb --tris 900 --texture 512 \
        [--material NAME] [--scale 1.0] [--turn DEG] [--preview out.png]
    blender -b --python blender_prop.py -- --build sandbags --out out.glb [--preview out.png]

`tools/assets/import_polyhaven.py` drives it; the generators live in `tools/assets/blender_builds.py`.
"""
import argparse
import math
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def parse() -> argparse.Namespace:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--source")
    parser.add_argument("--build")
    parser.add_argument("--out", required=True)
    parser.add_argument("--tris", type=int, default=1200)
    parser.add_argument("--texture", type=int, default=512)
    parser.add_argument("--material", help="keep only this material variant's textures (Poly Haven names them)")
    parser.add_argument("--scale", type=float, default=1.0)
    parser.add_argument("--turn", type=float, default=0.0, help="degrees about the vertical axis")
    parser.add_argument("--keep", help="comma-separated object name parts to keep (others are deleted); `part$` matches the end of a name")
    parser.add_argument("--drop", help="comma-separated object name parts to delete")
    parser.add_argument("--textures", help="folder with the surface texture sets (for --build)")
    parser.add_argument("--preview")
    return parser.parse_args(argv)


def triangles(obj) -> int:
    return sum(len(polygon.vertices) - 2 for polygon in obj.data.polygons)


def named(name: str, parts: str) -> bool:
    """Whether an object name has one of the comma-separated `parts` (`part$`: at its end)."""
    name = name.lower()
    return any(name.endswith(part[:-1]) if part.endswith("$") else part in name for part in parts.lower().split(","))


def join_meshes(keep: str | None, drop: str | None):
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    unwanted = [o for o in meshes if (keep and not named(o.name, keep)) or (drop and named(o.name, drop))]
    for obj in unwanted:
        bpy.data.objects.remove(obj, do_unlink=True)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        raise SystemExit("no mesh objects in the source")
    bpy.ops.object.select_all(action="DESELECT")
    for obj in meshes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    # Keep the world placement: clear parents first, then bake the transforms into the mesh.
    bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    if len(meshes) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    for other in [o for o in bpy.context.scene.objects if o is not obj and o.type != "MESH"]:
        bpy.data.objects.remove(other, do_unlink=True)
    return obj


def ground(obj, scale: float, turn: float) -> None:
    """Scales and turns the mesh, then puts its base centre on the origin (Blender is z up)."""
    obj.scale = (scale, scale, scale)
    obj.rotation_euler = (0.0, 0.0, math.radians(turn))
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    corners = [obj.matrix_world @ Vector(corner) for corner in obj.bound_box]
    low = Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
    high = Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
    obj.location = (-(low.x + high.x) / 2, -(low.y + high.y) / 2, -low.z)
    bpy.ops.object.transform_apply(location=True, rotation=False, scale=False)


def decimate(obj, target: int) -> None:
    count = triangles(obj)
    if count <= target:
        return
    modifier = obj.modifiers.new("decimate", "DECIMATE")
    modifier.ratio = target / count
    modifier.use_collapse_triangulate = True
    bpy.ops.object.modifier_apply(modifier=modifier.name)


def shrink_textures(size: int) -> None:
    for image in bpy.data.images:
        if image.size[0] > size or image.size[1] > size:
            factor = size / max(image.size)
            image.scale(max(1, round(image.size[0] * factor)), max(1, round(image.size[1] * factor)))


def render_preview(obj, path: str) -> None:
    scene = bpy.context.scene
    corners = [obj.matrix_world @ Vector(corner) for corner in obj.bound_box]
    centre = sum(corners, Vector()) / 8
    radius = max((corner - centre).length for corner in corners)
    camera_data = bpy.data.cameras.new("preview")
    camera = bpy.data.objects.new("preview", camera_data)
    scene.collection.objects.link(camera)
    direction = Vector((1.0, -1.25, 0.7)).normalized()
    camera.location = centre + direction * radius * 3.2
    camera.rotation_euler = (centre - camera.location).to_track_quat("-Z", "Y").to_euler()
    scene.camera = camera
    sun_data = bpy.data.lights.new("sun", "SUN")
    sun_data.energy = 3.5
    sun = bpy.data.objects.new("sun", sun_data)
    sun.rotation_euler = (math.radians(50), 0.0, math.radians(35))
    scene.collection.objects.link(sun)
    world = bpy.data.worlds.new("preview")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.55, 0.6, 0.7, 1.0)
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.9
    scene.world = world
    bpy.ops.mesh.primitive_plane_add(size=radius * 12, location=(centre.x, centre.y, 0.0))
    floor = bpy.context.view_layer.objects.active
    material = bpy.data.materials.new("floor")
    material.diffuse_color = (0.35, 0.35, 0.35, 1.0)
    floor.data.materials.append(material)
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 480
    scene.render.resolution_y = 480
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(floor, do_unlink=True)
    bpy.data.objects.remove(camera, do_unlink=True)
    bpy.data.objects.remove(sun, do_unlink=True)


def main() -> None:
    args = parse()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if args.build:
        import blender_builds

        obj = blender_builds.BUILDS[args.build](args.textures)
    else:
        bpy.ops.import_scene.gltf(filepath=args.source)
        obj = join_meshes(args.keep, args.drop)
    ground(obj, args.scale, args.turn)
    before = triangles(obj)
    decimate(obj, args.tris)
    shrink_textures(args.texture)
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    obj.name = os.path.splitext(os.path.basename(args.out))[0]
    bpy.ops.export_scene.gltf(
        filepath=args.out,
        export_format="GLB",
        use_selection=True,
        export_image_format="JPEG",
        export_jpeg_quality=82,
        export_apply=True,
        export_yup=True,
        export_tangents=False,
    )
    size = obj.dimensions
    print(f"PROP {obj.name}: {before} -> {triangles(obj)} tris, {size.x:.2f} x {size.y:.2f} x {size.z:.2f} m (x, depth, height)")
    if args.preview:
        render_preview(obj, args.preview)


main()
