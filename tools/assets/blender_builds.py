"""Props built from scratch in Blender (imported by `blender_prop.py --build NAME`): simple shapes
dressed with the game's own CC0 surface textures, for the things no CC0 pack had. Each builder
takes the folder of surface texture sets (crates/frontline/assets/textures) and returns one mesh
object in metres, z up; `blender_prop.py` grounds, decimates and exports it.
"""
import math
import os
import random

import bmesh
import bpy


def surface_material(name: str, textures: str, surface: str, tint=(1.0, 1.0, 1.0)):
    """A Principled material from a surface's colour, normal and roughness maps, tinted."""
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    shader = nodes["Principled BSDF"]

    def image(file: str, colour: bool):
        node = nodes.new("ShaderNodeTexImage")
        node.image = bpy.data.images.load(os.path.join(textures, surface, file))
        if not colour:
            node.image.colorspace_settings.name = "Non-Color"
        return node

    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    links.new(image("color.jpg", True).outputs["Color"], mix.inputs["A"])
    mix.inputs["B"].default_value = (*tint, 1.0)
    links.new(mix.outputs["Result"], shader.inputs["Base Color"])
    normal = nodes.new("ShaderNodeNormalMap")
    links.new(image("normal.jpg", False).outputs["Color"], normal.inputs["Color"])
    links.new(normal.outputs["Normal"], shader.inputs["Normal"])
    links.new(image("roughness.jpg", False).outputs["Color"], shader.inputs["Roughness"])
    shader.inputs["Metallic"].default_value = 0.0
    return material


def flat_material(name: str, colour, roughness: float = 0.8):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    shader = material.node_tree.nodes["Principled BSDF"]
    shader.inputs["Base Color"].default_value = (*colour, 1.0)
    shader.inputs["Roughness"].default_value = roughness
    return material


def box(size, location, material, bevel: float = 0.0, taper: float = 1.0):
    """A box of `size` (x, y, z) centred on `location`; `taper` scales its bottom face in x and
    y (a skip narrows towards the ground)."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=location)
    obj = bpy.context.view_layer.objects.active
    obj.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if taper != 1.0:
        for vertex in obj.data.vertices:
            if vertex.co.z < 0.0:
                vertex.co.x *= taper
                vertex.co.y *= taper
    if bevel > 0.0:
        modifier = obj.modifiers.new("bevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    obj.data.materials.append(material)
    return obj


def cylinder(radius: float, depth: float, location, rotation, material, vertices: int = 12):
    bpy.ops.mesh.primitive_cylinder_add(radius=radius, depth=depth, vertices=vertices, location=location, rotation=rotation)
    obj = bpy.context.view_layer.objects.active
    obj.data.materials.append(material)
    return obj


def join(objects, tile: float = 1.0):
    """Joins the objects into one, box-projects its UVs at `tile` metres per repeat and shades it
    smooth where faces meet at a shallow angle."""
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.cube_project(cube_size=tile, scale_to_bounds=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    bpy.ops.object.shade_auto_smooth(angle=math.radians(40))
    return obj


def dumpster(textures: str):
    """A steel skip with two lids, 2.0 x 1.2 m and 1.3 m tall."""
    green = surface_material("dumpster_body", textures, "metal_green")
    dark = flat_material("dumpster_lid", (0.05, 0.055, 0.06), 0.6)
    steel = surface_material("dumpster_steel", textures, "metal_rust")
    parts = [
        box((2.0, 1.2, 1.0), (0.0, 0.0, 0.62), green, bevel=0.015, taper=0.9),
        box((2.06, 1.26, 0.06), (0.0, 0.0, 1.13), green, bevel=0.01),  # the rim
        box((0.12, 0.5, 0.16), (-1.03, 0.0, 0.8), steel, bevel=0.01),  # the lifting sleeves
        box((0.12, 0.5, 0.16), (1.03, 0.0, 0.8), steel, bevel=0.01),
    ]
    for x in (-0.5, 0.5):
        lid = box((0.98, 1.24, 0.04), (x, 0.0, 1.21), dark, bevel=0.008)
        lid.rotation_euler = (math.radians(7.0), 0.0, 0.0)
        parts.append(lid)
    for x in (-0.8, 0.8):
        for y in (-0.45, 0.45):
            parts.append(cylinder(0.09, 0.06, (x, y, 0.09), (0.0, math.radians(90.0), 0.0), dark))
    return join(parts, tile=2.0)


def pallet(textures: str):
    """A wooden pallet, 1.2 x 0.8 m and 0.144 m tall."""
    wood = surface_material("pallet_wood", textures, "wood", tint=(0.8, 0.74, 0.66))
    parts = []
    for i in range(5):  # top boards
        parts.append(box((1.2, 0.1, 0.022), (0.0, -0.35 + i * 0.175, 0.133), wood))
    for x in (-0.55, 0.0, 0.55):  # blocks under three bearers
        parts.append(box((0.1, 0.8, 0.022), (x, 0.0, 0.111), wood))
        for y in (-0.35, 0.0, 0.35):
            parts.append(box((0.1, 0.1, 0.078), (x, y, 0.061), wood))
    for y in (-0.35, 0.0, 0.35):  # bottom boards
        parts.append(box((1.2, 0.1, 0.022), (0.0, y, 0.011), wood))
    return join(parts, tile=1.0)


def pillow(size, location, turn: float, material, rng, radius: float = 0.42):
    """A sandbag: a box of `size` (x length, y depth, z height) with rounded edges, a belly that
    bulges out in the middle of its height and ends that taper in plan, a little lumpy."""
    bpy.ops.mesh.primitive_cube_add(size=2.0)
    bag = bpy.context.view_layer.objects.active
    mesh = bmesh.new()
    mesh.from_mesh(bag.data)
    bmesh.ops.subdivide_edges(mesh, edges=mesh.edges[:], cuts=3, use_grid_fill=True)
    for vertex in mesh.verts:
        x, y, z = vertex.co
        inner = [min(max(c, -(1.0 - radius)), 1.0 - radius) for c in (x, y, z)]
        delta = [c - i for c, i in zip((x, y, z), inner)]
        length = math.sqrt(sum(d * d for d in delta))
        if length > 1e-9:
            x, y, z = (i + d / length * radius for i, d in zip(inner, delta))
        belly = 1.0 + 0.10 * (1.0 - z * z)  # fatter in the middle of the height
        taper = 1.0 - 0.18 * x * x  # ends pinch in plan
        dome = 1.0 + 0.05 * (1.0 - x * x) * (1.0 - y * y) * (1.0 if z > 0 else 0.0)  # the filled top
        vertex.co = (
            x * (1.0 + rng.uniform(-0.012, 0.012)),
            y * belly * taper + rng.uniform(-0.015, 0.015),
            z * dome + rng.uniform(-0.015, 0.015),
        )
    mesh.to_mesh(bag.data)
    mesh.free()
    bag.scale = (size[0] / 2, size[1] / 2, size[2] / 2)
    bag.rotation_euler = (0.0, 0.0, turn)
    bag.location = location
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    bag.data.materials.append(material)
    return bag


def sandbags(textures: str):
    """A sandbag wall: pillow bags (0.62 x 0.34 x 0.19 m, each turned a little) stacked in a
    running bond five rows high, about 2.0 m long, 0.4 m deep and 0.95 m tall: chest-high cover.
    Bags are darker burlap than the sand around, with gaps between them."""
    tints = [(0.52, 0.45, 0.32), (0.46, 0.40, 0.29), (0.56, 0.49, 0.36)]
    sacks = [surface_material(f"sandbag_{i}", textures, "sand", tint=tint) for i, tint in enumerate(tints)]
    rng = random.Random(11)
    length, depth, height = 0.62, 0.32, 0.18
    pitch = length + 0.035  # a visible seam between neighbours
    parts = []
    for row in range(5):
        z = height / 2 + row * (height + 0.012)
        # Even rows: three bags. Odd rows: two bags offset by half a pitch, and a half bag at each end.
        if row % 2 == 0:
            cells = [(-pitch, length), (0.0, length), (pitch, length)]
        else:
            cells = [(-pitch / 2, length), (pitch / 2, length), (-0.835, length / 2), (0.835, length / 2)]
        for x, bag_length in cells:
            parts.append(
                pillow(
                    (bag_length, depth + rng.uniform(-0.02, 0.03), height),
                    (x + rng.uniform(-0.012, 0.012), rng.uniform(-0.025, 0.025), z),
                    rng.uniform(-0.07, 0.07),
                    rng.choice(sacks),
                    rng,
                )
            )
    return join(parts, tile=0.8)


BUILDS = {"dumpster": dumpster, "pallet": pallet, "sandbags": sandbags}
